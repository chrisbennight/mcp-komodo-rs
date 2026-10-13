//! Operation configuration history and explicit literal credential fields.

use komodo_api::OperationItem;
use schemars::JsonSchema;
use serde::Serialize;
use toml_edit::{DocumentMut, Item, TableLike};
use url::Url;
use zeroize::Zeroizing;

use super::{KomodoMcp, McpError, OperationStatus, files::FileValue, normalize_operation};

/// Ordinary history text with the original credential-bearing document available natively.
#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct HistoryDocument {
    /// Exact original TOML when no literal credential fields are present;
    /// otherwise a projection that replaces only those credential values.
    toml: String,
    /// Typed fields whose literal values are carried by the original file.
    credential_fields: Vec<String>,
    /// Native metadata for the exact original credential-bearing document.
    original_file: Option<FileValue>,
    /// Why a credential-bearing original cannot currently be transferred.
    original_file_unavailable: Option<&'static str>,
}

struct PreparedDocument {
    toml: String,
    credential_fields: Vec<String>,
    original: Option<Zeroizing<Vec<u8>>>,
}

impl PreparedDocument {
    fn parse(original: String) -> Result<Self, McpError> {
        let mut document = original.parse::<DocumentMut>().map_err(|error| {
            McpError::internal_error("Core history document is not valid TOML", Some(serde_json::json!({
                "byteRange":error.span().map(|span| serde_json::json!({"start":span.start,"end":span.end}))
            })))
        })?;
        let mut fields = Vec::new();
        for (kind, collection) in document.iter_mut() {
            let kind = kind.get();
            match kind {
                "server" | "servers" | "builder" | "builders" | "stack" | "stacks" | "repo"
                | "repos" | "build" | "builds" | "resource_sync" | "resource_syncs"
                | "procedure" | "procedures" | "action" | "actions" | "alerter" | "alerters" => {
                    project_collection(kind, collection, &mut fields);
                }
                _ => {}
            }
        }
        if fields.is_empty() {
            Ok(Self {
                toml: original,
                credential_fields: fields,
                original: None,
            })
        } else {
            Ok(Self {
                toml: document.to_string(),
                credential_fields: fields,
                original: Some(Zeroizing::new(original.into_bytes())),
            })
        }
    }

    fn publish(self, handler: &KomodoMcp, name: &'static str) -> HistoryDocument {
        let mut result = HistoryDocument {
            toml: self.toml,
            credential_fields: self.credential_fields,
            original_file: None,
            original_file_unavailable: None,
        };
        if let Some(bytes) = self.original {
            if let Some((owner, files)) = &handler.credential_files {
                match files.publish(owner, name, "application/toml", bytes) {
                    Ok(file) => result.original_file = Some(file),
                    Err(_) => {
                        result.original_file_unavailable = Some("native file capacity unavailable");
                    }
                }
            } else {
                result.original_file_unavailable = Some("native file transfer required");
            }
        }
        result
    }
}

pub(super) struct PreparedOperation {
    item: OperationItem,
    previous: Option<PreparedDocument>,
    current: Option<PreparedDocument>,
}

impl PreparedOperation {
    pub(super) fn parse(mut item: OperationItem) -> Result<Self, McpError> {
        let previous = item
            .prev_toml
            .take()
            .map(PreparedDocument::parse)
            .transpose()?;
        let current = item
            .current_toml
            .take()
            .map(PreparedDocument::parse)
            .transpose()?;
        Ok(Self {
            item,
            previous,
            current,
        })
    }

    pub(super) fn publish(self, handler: &KomodoMcp) -> OperationStatus {
        let mut status = normalize_operation(self.item);
        status.previous_configuration = self
            .previous
            .map(|doc| doc.publish(handler, "operation-before.toml"));
        status.current_configuration = self
            .current
            .map(|doc| doc.publish(handler, "operation-after.toml"));
        status
    }
}

impl KomodoMcp {
    pub(super) fn normalize_history_operation(
        &self,
        item: OperationItem,
    ) -> Result<OperationStatus, McpError> {
        Ok(PreparedOperation::parse(item)?.publish(self))
    }
}

fn project_collection(kind: &str, collection: &mut Item, fields: &mut Vec<String>) {
    if let Some(tables) = collection.as_array_of_tables_mut() {
        for (index, table) in tables.iter_mut().enumerate() {
            project_resource(kind, table, &format!("{kind}[{index}].config"), fields);
        }
    } else if let Some(array) = collection.as_array_mut() {
        for (index, value) in array.iter_mut().enumerate() {
            if let Some(table) = value.as_inline_table_mut() {
                project_resource(kind, table, &format!("{kind}[{index}].config"), fields);
            }
        }
    } else if let Some(table) = collection.as_table_like_mut() {
        project_resource(kind, table, &format!("{kind}.config"), fields);
    }
}

fn project_resource(
    kind: &str,
    resource: &mut dyn TableLike,
    path: &str,
    fields: &mut Vec<String>,
) {
    let Some(config) = resource.get_mut("config").and_then(Item::as_table_like_mut) else {
        return;
    };
    match kind {
        "server" | "servers" => replace_literal(config, "passkey", path, fields),
        "builder" | "builders" => {
            if config.get("type").and_then(Item::as_str) == Some("Url")
                && let Some(params) = config.get_mut("params").and_then(Item::as_table_like_mut)
            {
                replace_literal(params, "passkey", &format!("{path}.params"), fields);
            }
        }
        "alerter" | "alerters" => project_alerter(config, path, fields),
        _ => replace_literal(config, "webhook_secret", path, fields),
    }
}

fn replace_literal(table: &mut dyn TableLike, key: &str, path: &str, fields: &mut Vec<String>) {
    if let Some(item) = table.get_mut(key)
        && item.as_str().is_some_and(|value| !value.is_empty())
    {
        replace_value(item, "[credential in originalFile]".into());
        fields.push(format!("{path}.{key}"));
    }
}

fn replace_value(item: &mut Item, value: String) {
    let mut replacement = toml_edit::Value::from(value);
    if let Some(old) = item.as_value() {
        *replacement.decor_mut() = old.decor().clone();
    }
    *item = Item::Value(replacement);
}

fn project_alerter(config: &mut dyn TableLike, path: &str, fields: &mut Vec<String>) {
    let Some(endpoint) = config.get_mut("endpoint").and_then(Item::as_table_like_mut) else {
        return;
    };
    let kind = endpoint
        .get("type")
        .and_then(Item::as_str)
        .map(str::to_owned);
    let Some(item) = endpoint
        .get_mut("params")
        .and_then(Item::as_table_like_mut)
        .and_then(|params| params.get_mut("url"))
    else {
        return;
    };
    let Some(original) = item.as_str() else {
        return;
    };
    if let Some(projected) = project_endpoint_url(original, kind.as_deref()) {
        replace_value(item, projected);
        fields.push(format!("{path}.endpoint.params.url"));
    }
}

const URL_CREDENTIAL: &str = "credential-in-original-file";

fn project_endpoint_url(original: &str, kind: Option<&str>) -> Option<String> {
    project_uri_components(original, kind).or_else(|| {
        // WHATWG URL parsing also accepts surrounding whitespace and special-scheme
        // slash variants. Inspect those recognized credentials without changing
        // an ordinary endpoint that has no credential component.
        let normalized = Url::parse(original).ok()?;
        project_uri_components(normalized.as_str(), kind)
    })
}

fn project_uri_components(original: &str, kind: Option<&str>) -> Option<String> {
    // URI components identify credentials independently of host or port validity.
    // Preserve each other component verbatim, including its percent encoding.
    let prefix_len = if original.starts_with("//") {
        2
    } else if let Some((scheme, _)) = original.split_once("://") {
        if !scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            || !scheme
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"+-.".contains(&c))
        {
            return None;
        }
        scheme.len() + 3
    } else {
        return None;
    };
    let rest = &original[prefix_len..];
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let projected_authority = authority.rsplit_once('@').and_then(|(userinfo, host)| {
        userinfo.split_once(':').and_then(|(username, password)| {
            (!password.is_empty()).then(|| format!("{username}:{URL_CREDENTIAL}@{host}"))
        })
    });
    let (resource, fragment) = tail.split_at(tail.find('#').unwrap_or(tail.len()));
    let (path, query) = resource
        .split_once('?')
        .map_or((resource, None), |(p, q)| (p, Some(q)));
    let projected_path = project_endpoint_path(path, kind);
    let projected_query = query.and_then(|query| project_endpoint_query(query, kind));
    if projected_authority.is_none() && projected_path.is_none() && projected_query.is_none() {
        return None;
    }
    let mut result = format!(
        "{}{}{}",
        &original[..prefix_len],
        projected_authority.as_deref().unwrap_or(authority),
        projected_path.as_deref().unwrap_or(path)
    );
    if let Some(query) = query {
        result.push('?');
        result.push_str(projected_query.as_deref().unwrap_or(query));
    }
    result.push_str(fragment);
    Some(result)
}

fn project_endpoint_path(path: &str, kind: Option<&str>) -> Option<String> {
    let mut parts = path.split('/').collect::<Vec<_>>();
    let credential_index = match kind {
        Some("Slack") => parts
            .iter()
            .position(|part| *part == "services")
            .map(|index| index + 3),
        Some("Discord") => parts
            .iter()
            .position(|part| *part == "webhooks")
            .map(|index| index + 2),
        _ => None,
    };
    let index =
        credential_index.filter(|index| parts.get(*index).is_some_and(|part| !part.is_empty()))?;
    parts[index] = URL_CREDENTIAL;
    Some(parts.join("/"))
}

fn project_endpoint_query(query: &str, kind: Option<&str>) -> Option<String> {
    if kind != Some("Pushover") {
        return None;
    }
    let mut changed = false;
    let parts = query
        .split('&')
        .map(|pair| {
            let credential = url::form_urlencoded::parse(pair.as_bytes())
                .next()
                .is_some_and(|(key, value)| {
                    matches!(key.as_ref(), "token" | "user") && !value.is_empty()
                });
            if credential {
                changed = true;
                format!(
                    "{}={URL_CREDENTIAL}",
                    pair.split_once('=').expect("nonempty query value").0
                )
            } else {
                pair.to_owned()
            }
        })
        .collect::<Vec<_>>();
    changed.then(|| parts.join("&"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_documents_and_empty_inheritance_remain_exact() {
        let source = "# ordinary comment\n[[stack]]\nname = 'api'\n[stack.config]\nwebhook_secret = ''\nenvironment = 'PASSWORD=ordinary-document-text'\nfile_contents = 'token: ordinary Compose text'\n\n[[builder]]\nname = 'builder'\n[builder.config]\ntype = 'Server'\n[builder.config.params]\nsecrets = ['secret-name-reference']\n";
        let result = PreparedDocument::parse(source.into()).unwrap();
        assert_eq!(result.toml, source);
        assert!(result.credential_fields.is_empty());
        assert!(result.original.is_none());
        let empty = PreparedDocument::parse(String::new()).unwrap();
        assert_eq!(empty.toml, "");
        assert!(empty.original.is_none());
    }

    #[test]
    fn every_documented_direct_credential_field_has_an_exact_native_original() {
        for kind in [
            "server",
            "servers",
            "stack",
            "stacks",
            "repo",
            "repos",
            "build",
            "builds",
            "resource_sync",
            "resource_syncs",
            "procedure",
            "procedures",
            "action",
            "actions",
        ] {
            let key = if matches!(kind, "server" | "servers") {
                "passkey"
            } else {
                "webhook_secret"
            };
            let source = format!(
                "[[{kind}]]\nname = 'instance'\n[{kind}.config]\n{key} = 'synthetic-literal-value'\nordinary = 'preserved'\n"
            );
            let result = PreparedDocument::parse(source.clone()).unwrap();
            assert!(!result.toml.contains("synthetic-literal-value"), "{kind}");
            assert!(result.toml.contains("ordinary = 'preserved'"));
            assert_eq!(
                result.credential_fields,
                [format!("{kind}[0].config.{key}")]
            );
            assert_eq!(*result.original.unwrap(), source.into_bytes());
        }
        for kind in ["builder", "builders"] {
            let source = format!(
                "[[{kind}]]\nname = 'builder'\n[{kind}.config]\ntype = 'Url'\n[{kind}.config.params]\npasskey = 'synthetic-literal-value'\naddress = 'https://periphery.example.test:8120'\nperiphery_public_key = 'public-key'\n"
            );
            let result = PreparedDocument::parse(source.clone()).unwrap();
            assert!(!result.toml.contains("synthetic-literal-value"));
            assert!(result.toml.contains("https://periphery.example.test:8120"));
            assert!(result.toml.contains("public-key"));
            assert_eq!(*result.original.unwrap(), source.into_bytes());
        }
        let source = "servers = [{name='one', config={passkey='synthetic-literal-value', address='https://periphery.example.test'}}]";
        let result = PreparedDocument::parse(source.into()).unwrap();
        assert!(!result.toml.contains("synthetic-literal-value"));
        assert!(result.toml.contains("https://periphery.example.test"));
        assert_eq!(*result.original.unwrap(), source.as_bytes());
    }

    #[test]
    fn alerter_credentials_preserve_endpoint_identifiers_and_ordinary_parameters() {
        for (kind, url, ordinary) in [
            (
                "Slack",
                "https://hooks.slack.com/services/TPUBLIC/BPUBLIC/synthetic-url-token?wait=true",
                "TPUBLIC/BPUBLIC",
            ),
            (
                "Discord",
                "https://discord.com/api/v10/webhooks/123456/synthetic-url-token?thread_id=42",
                "webhooks/123456",
            ),
            (
                "Pushover",
                "https://api.pushover.net/1/messages.json?token=synthetic-url-token&user=synthetic-user-key&priority=1",
                "messages.json",
            ),
            (
                "Custom",
                "https://operator:synthetic-url-token@alerts.example.test/public-route?priority=1",
                "operator:",
            ),
        ] {
            let source = format!(
                "[[alerter]]\nname = 'notifications'\n[alerter.config.endpoint]\ntype = '{kind}'\n[alerter.config.endpoint.params]\nurl = '{url}'\n"
            );
            let result = PreparedDocument::parse(source.clone()).unwrap();
            assert!(!result.toml.contains("synthetic-url-token"));
            assert!(!result.toml.contains("synthetic-user-key"));
            assert!(result.toml.contains(ordinary));
            let expected_parameter = match kind {
                "Slack" => "wait=true",
                "Discord" => "thread_id=42",
                _ => "priority=1",
            };
            assert!(result.toml.contains(expected_parameter));
            assert_eq!(
                result.credential_fields,
                ["alerter[0].config.endpoint.params.url"]
            );
            assert_eq!(*result.original.unwrap(), source.into_bytes());
        }
        for kind in ["Custom", "Ntfy"] {
            let source = format!(
                "[[alerter]]\nname = 'ordinary'\n[alerter.config.endpoint]\ntype = '{kind}'\n[alerter.config.endpoint.params]\nurl = 'https://alerts.example.test/secret-name?token=ordinary-reference'\n"
            );
            let result = PreparedDocument::parse(source.clone()).unwrap();
            assert_eq!(result.toml, source);
            assert!(result.original.is_none());
        }
    }

    #[test]
    fn webhook_projection_preserves_percent_encoded_ordinary_path_segments() {
        for (kind, url, ordinary) in [
            (
                "Slack",
                "https://hooks.slack.com/services/TPUBLIC%2FNAME/BPUBLIC%20NAME/synthetic-url-token",
                "TPUBLIC%2FNAME/BPUBLIC%20NAME",
            ),
            (
                "Discord",
                "https://discord.com/api/webhooks/public%20identifier/synthetic-url-token/extra%2Fpath",
                "webhooks/public%20identifier",
            ),
        ] {
            let source = format!(
                "[[alerter]]\n[alerter.config.endpoint]\ntype = '{kind}'\n[alerter.config.endpoint.params]\nurl = '{url}'\n"
            );
            let result = PreparedDocument::parse(source.clone()).unwrap();
            assert!(result.toml.contains(ordinary));
            assert!(!result.toml.contains("%25"));
            assert!(!result.toml.contains("synthetic-url-token"));
            if kind == "Discord" {
                assert!(result.toml.contains("extra%2Fpath"));
            }
            assert_eq!(*result.original.unwrap(), source.into_bytes());
        }
    }

    #[test]
    fn endpoint_credentials_do_not_depend_on_host_or_port_validation() {
        for (kind, url, projected) in [
            (
                "Custom",
                "https://operator:synthetic-password@alerts.example.test:not-a-port/public%2Froute?priority=%31#ordinary",
                "https://operator:credential-in-original-file@alerts.example.test:not-a-port/public%2Froute?priority=%31#ordinary",
            ),
            (
                "Slack",
                "https://operator:synthetic-password@hooks.slack.com:bad/services/TPUBLIC%2FNAME/BPUBLIC%20NAME/synthetic-token?wait=true",
                "https://operator:credential-in-original-file@hooks.slack.com:bad/services/TPUBLIC%2FNAME/BPUBLIC%20NAME/credential-in-original-file?wait=true",
            ),
            (
                "Discord",
                "https://discord.com:bad/api/webhooks/public%20id/synthetic-token/extra%2Fpath",
                "https://discord.com:bad/api/webhooks/public%20id/credential-in-original-file/extra%2Fpath",
            ),
            (
                "Pushover",
                "https://operator:synthetic-password@api.pushover.net:bad/messages?tok%65n=synthetic-token&user=synthetic-user&priority=%31&empty=&token=#ordinary",
                "https://operator:credential-in-original-file@api.pushover.net:bad/messages?tok%65n=credential-in-original-file&user=credential-in-original-file&priority=%31&empty=&token=#ordinary",
            ),
            (
                "Custom",
                "//operator:synthetic-password@[invalid-ipv6]/route",
                "//operator:credential-in-original-file@[invalid-ipv6]/route",
            ),
            (
                "Custom",
                "//operator:synthetic-password@alerts.example.test:bad/public%2Froute?next=https://docs.example.test#source=https://ordinary.example.test",
                "//operator:credential-in-original-file@alerts.example.test:bad/public%2Froute?next=https://docs.example.test#source=https://ordinary.example.test",
            ),
        ] {
            let source = format!(
                "[[alerter]]\n[alerter.config.endpoint]\ntype = '{kind}'\n[alerter.config.endpoint.params]\nurl = '{url}'\n"
            );
            let result = PreparedDocument::parse(source.clone()).unwrap();
            let document = result.toml.parse::<DocumentMut>().unwrap();
            assert_eq!(
                document["alerter"][0]["config"]["endpoint"]["params"]["url"].as_str(),
                Some(projected)
            );
            assert_eq!(*result.original.unwrap(), source.into_bytes());
        }
        for url in [
            "https://operator@alerts.example.test:not-a-port/public%2Froute?token=ordinary-reference",
            "not an endpoint: ordinary-text",
            "https://operator:@[invalid-ipv6]/route",
        ] {
            let source = format!(
                "[[alerter]]\n[alerter.config.endpoint]\ntype = 'Custom'\n[alerter.config.endpoint.params]\nurl = '{url}'\n"
            );
            let result = PreparedDocument::parse(source.clone()).unwrap();
            assert_eq!(result.toml, source);
            assert!(result.original.is_none());
        }
    }

    #[test]
    fn standard_url_parser_forms_keep_passwords_in_the_native_original() {
        for url in [
            " \thttps://operator:synthetic-password@alerts.example.test/public%2Froute?priority=1 ",
            "https:/operator:synthetic-password@alerts.example.test/public%2Froute?priority=1",
            r"https:\\operator:synthetic-password@alerts.example.test/public%2Froute?priority=1",
        ] {
            assert_eq!(
                Url::parse(url).unwrap().password(),
                Some("synthetic-password")
            );
            let source = format!(
                "[[alerter]]\n[alerter.config.endpoint]\ntype = 'Custom'\n[alerter.config.endpoint.params]\nurl = {}\n",
                serde_json::to_string(url).unwrap()
            );
            let result = PreparedDocument::parse(source.clone()).unwrap();
            assert!(!result.toml.contains("synthetic-password"));
            assert!(
                result
                    .toml
                    .contains("alerts.example.test/public%2Froute?priority=1")
            );
            assert_eq!(*result.original.unwrap(), source.into_bytes());
        }
        let source = "[[alerter]]\n[alerter.config.endpoint]\ntype = 'Custom'\n[alerter.config.endpoint.params]\nurl = '  https:/alerts.example.test/public%2Froute?token=ordinary-reference  '\n";
        let result = PreparedDocument::parse(source.into()).unwrap();
        assert_eq!(result.toml, source);
        assert!(result.original.is_none());
    }

    #[test]
    fn malformed_toml_is_an_explicit_format_failure_without_source_values() {
        let error = PreparedDocument::parse(
            "[[stack]]\n[stack.config]\nwebhook_secret = 'synthetic-private-sentinel".into(),
        )
        .err()
        .unwrap();
        assert_eq!(error.message, "Core history document is not valid TOML");
        assert!(!format!("{error:?}").contains("synthetic-private-sentinel"));
    }
}
