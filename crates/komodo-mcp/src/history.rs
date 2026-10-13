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

impl KomodoMcp {
    pub(super) fn normalize_history_operation(
        &self,
        mut item: OperationItem,
    ) -> Result<OperationStatus, McpError> {
        // Parse both documents before admitting any native output file.
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
        let mut status = normalize_operation(item);
        status.previous_configuration =
            previous.map(|doc| doc.publish(self, "operation-before.toml"));
        status.current_configuration = current.map(|doc| doc.publish(self, "operation-after.toml"));
        Ok(status)
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
    let Ok(mut url) = Url::parse(original) else {
        return;
    };
    let mut changed = false;
    if url.password().is_some_and(|password| !password.is_empty()) {
        let _ = url.set_password(Some("credential-in-original-file"));
        changed = true;
    }
    let parts = url
        .path_segments()
        .map(|parts| parts.map(str::to_owned).collect::<Vec<_>>())
        .unwrap_or_default();
    let credential_index = match kind.as_deref() {
        Some("Slack") => parts
            .iter()
            .position(|part| part == "services")
            .map(|index| index + 3),
        Some("Discord") => parts
            .iter()
            .position(|part| part == "webhooks")
            .map(|index| index + 2),
        _ => None,
    };
    if let Some(index) =
        credential_index.filter(|index| parts.get(*index).is_some_and(|part| !part.is_empty()))
        && let Ok(mut path) = url.path_segments_mut()
    {
        path.clear()
            .extend(parts.iter().enumerate().map(|(i, part)| {
                if i == index {
                    "credential-in-original-file"
                } else {
                    part.as_str()
                }
            }));
        changed = true;
    }
    if kind.as_deref() == Some("Pushover") {
        let pairs = url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        if pairs
            .iter()
            .any(|(key, value)| matches!(key.as_str(), "token" | "user") && !value.is_empty())
        {
            let mut query = url.query_pairs_mut();
            query.clear().extend_pairs(pairs.iter().map(|(key, value)| {
                (
                    key.as_str(),
                    if matches!(key.as_str(), "token" | "user") && !value.is_empty() {
                        "credential-in-original-file"
                    } else {
                        value.as_str()
                    },
                )
            }));
            changed = true;
        }
    }
    if changed {
        replace_value(item, url.into());
        fields.push(format!("{path}.endpoint.params.url"));
    }
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
