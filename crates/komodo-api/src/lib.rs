//! Bounded, typed access to the Komodo Core API.

use std::{fmt, future::Future, pin::Pin, sync::Arc, time::Duration};

use reqwest::{Client as HttpClient, StatusCode, redirect::Policy};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use thiserror::Error;
use url::{Host, Url};
use zeroize::Zeroizing;

const MAXIMUM_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAXIMUM_TIMEOUT: Duration = Duration::from_mins(2);

/// Credentials used for one least-privileged Komodo service user.
#[derive(Clone)]
pub struct Credentials {
    key: Zeroizing<String>,
    secret: Zeroizing<String>,
}

impl Credentials {
    /// Build credentials while rejecting empty values.
    ///
    /// # Errors
    ///
    /// Returns an error when either credential is empty or contains a newline.
    pub fn new(key: String, secret: String) -> Result<Self, ApiError> {
        Self::from_protected(Zeroizing::new(key), Zeroizing::new(secret))
    }

    /// Build credentials from allocations that are already protected against
    /// plaintext drops during multi-file configuration loading.
    ///
    /// # Errors
    ///
    /// Returns an error when either credential is empty or contains a newline.
    pub fn from_protected(
        key: Zeroizing<String>,
        secret: Zeroizing<String>,
    ) -> Result<Self, ApiError> {
        validate_secret("API key", &key)?;
        validate_secret("API secret", &secret)?;
        Ok(Self { key, secret })
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Credentials([REDACTED])")
    }
}

fn validate_secret(label: &'static str, value: &str) -> Result<(), ApiError> {
    if value.is_empty() || value.bytes().any(|byte| matches!(byte, b'\r' | b'\n')) {
        return Err(ApiError::InvalidConfiguration(label));
    }
    Ok(())
}

/// A safe error vocabulary that never includes upstream response bodies.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ApiError {
    #[error("invalid Komodo client configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("Komodo authentication failed")]
    Authentication,
    #[error("Komodo denied the operation")]
    Forbidden,
    #[error("the requested Komodo resource was not found")]
    NotFound,
    #[error("Komodo is temporarily unavailable")]
    Unavailable,
    #[error("Komodo returned an incompatible response")]
    IncompatibleResponse,
    #[error("Komodo response exceeded the configured bound")]
    ResponseTooLarge,
}

/// Minimal common resource metadata returned by list endpoints.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ResourceListItem<I> {
    pub id: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub name: String,
    pub info: I,
}

/// Server fields explicitly allowed through the MCP normalization boundary.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ServerInfo {
    pub state: String,
    pub version: Option<String>,
}

/// Stack fields explicitly allowed through the MCP normalization boundary.
///
/// Compose contents, environment, and remote file bodies are never retained;
/// only bounded state and diagnostic signals cross this boundary.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StackInfo {
    pub state: String,
    /// Short status text Komodo relays from Docker, when present.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub services: Vec<StackService>,
    #[serde(default)]
    pub project_missing: bool,
    /// Expected Compose or additional files absent from the stack's source.
    #[serde(default)]
    pub missing_files: Vec<String>,
    /// Commit hash of the currently deployed definition, for repo-backed stacks.
    #[serde(default)]
    pub deployed_hash: Option<String>,
    /// Latest available commit hash, for repo-backed stacks.
    #[serde(default)]
    pub latest_hash: Option<String>,
}

/// Bounded per-service diagnostic signals. The service name, image reference,
/// and update signal are retained; environment, ports, and commands are not.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StackService {
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub update_available: bool,
}

/// Deployment fields explicitly allowed through the MCP normalization boundary.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DeploymentInfo {
    pub state: String,
    #[serde(default)]
    pub update_available: bool,
}

/// Build fields explicitly allowed through the MCP normalization boundary.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct BuildInfo {
    pub state: String,
    #[serde(default)]
    pub last_built_at: i64,
}

/// Repository fields explicitly allowed through the MCP normalization boundary.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RepoInfo {
    pub state: String,
    #[serde(default)]
    pub last_pulled_at: i64,
    #[serde(default)]
    pub last_built_at: i64,
}

/// Minimal operation record; command output, config snapshots, and operator data are discarded.
///
/// Komodo serializes the update identifier as a bare string in list responses
/// and as an extended-JSON `{ "$oid": ... }` object elsewhere, so the id accepts
/// either form under both the `id` and `_id` keys.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct OperationItem {
    #[serde(alias = "_id", deserialize_with = "deserialize_object_id")]
    pub id: String,
    pub operation: String,
    pub start_ts: i64,
    pub success: bool,
    pub status: String,
    /// Resource metadata only; no operator identity or configuration snapshot.
    #[serde(default)]
    pub target: Option<OperationTarget>,
}

/// Allowlisted resource correlation carried by Komodo update lists and details.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct OperationTarget {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Deserialize)]
struct ListUpdatesResponse {
    updates: Vec<OperationItem>,
    next_page: Option<u32>,
}

/// One bounded page of operation metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationPage {
    pub operations: Vec<OperationItem>,
    pub next_page: Option<u32>,
}

/// A minimal receipt returned after an accepted mutation.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MutationReceipt {
    #[serde(rename = "_id", deserialize_with = "deserialize_object_id")]
    pub operation_id: String,
    pub operation: String,
    #[serde(rename = "start_ts")]
    pub start_ts: i64,
    pub success: bool,
    pub status: String,
}

fn deserialize_object_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(id) => Ok(id),
        Value::Object(object) => object
            .get("$oid")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| serde::de::Error::custom("invalid operation id")),
        _ => Err(serde::de::Error::custom("invalid operation id")),
    }
}

/// A Komodo pre/post-deploy command.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SystemCommand {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub command: String,
}

/// A partial `UpdateStack` configuration: only the set fields are serialized, so
/// Komodo merges them into the stack without clobbering unset fields. The
/// gateway builds this from typed intents; no arbitrary configuration is
/// accepted here.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct StackConfigPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_contents: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_deploy: Option<SystemCommand>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_deploy: Option<SystemCommand>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_on_host: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_paths: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_force_deploy: Option<bool>,
}

/// Acknowledges a write whose upstream response echoes configuration we must not
/// retain. Deserializing into this empty record discards the entire body, so a
/// just-written secret or file body never lands in a field.
#[derive(Debug, Deserialize)]
struct WriteAck {}

/// Bounded stack configuration from `GetStack`. Raw Compose, environment,
/// commands, and the webhook secret are read through their own governed tools;
/// callers that only need config shape do not receive those payloads here.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct StackConfigDetail {
    #[serde(default)]
    pub file_contents: String,
    #[serde(default)]
    pub environment: String,
    #[serde(default)]
    pub files_on_host: bool,
    #[serde(default)]
    pub run_directory: String,
    #[serde(default)]
    pub file_paths: Vec<String>,
    #[serde(default)]
    pub env_file_path: String,
    #[serde(default)]
    pub pre_deploy: SystemCommand,
    #[serde(default)]
    pub post_deploy: SystemCommand,
    #[serde(default)]
    pub webhook_enabled: bool,
    #[serde(default)]
    pub webhook_secret: String,
    #[serde(default)]
    pub webhook_force_deploy: bool,
}

/// One Compose or additional file: its path and full contents.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ComposeFile {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub contents: String,
}

/// Deployed and latest Compose content exposed by `GetStack`'s stack info.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct StackComposeInfo {
    #[serde(default)]
    pub deployed_contents: Option<Vec<ComposeFile>>,
    #[serde(default)]
    pub remote_contents: Option<Vec<ComposeFile>>,
}

/// Full stack detail from `GetStack`, bounded to config and Compose content.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct StackDetail {
    #[serde(default)]
    pub config: StackConfigDetail,
    #[serde(default)]
    pub info: StackComposeInfo,
}

/// One command-execution log record. Its content is caller-sensitive and
/// untrusted, and must never reach MCP or gateway logs, errors, or metrics.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct Log {
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub start_ts: i64,
    #[serde(default)]
    pub end_ts: i64,
}

#[derive(Debug, Deserialize)]
struct UpdateLogsResponse {
    #[serde(default)]
    logs: Vec<Log>,
}

/// Allowlisted Komodo execution intents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    DeployStack,
    RestartStack,
    StopStack,
    DeployDeployment,
    RestartDeployment,
    RunBuild,
    CancelBuild,
    PullRepo,
}

impl Action {
    fn variant(self) -> &'static str {
        match self {
            Self::DeployStack => "DeployStack",
            Self::RestartStack => "RestartStack",
            Self::StopStack => "StopStack",
            Self::DeployDeployment => "Deploy",
            Self::RestartDeployment => "RestartDeployment",
            Self::RunBuild => "RunBuild",
            Self::CancelBuild => "CancelBuild",
            Self::PullRepo => "PullRepo",
        }
    }

    fn parameter(self) -> &'static str {
        match self {
            Self::DeployStack | Self::RestartStack | Self::StopStack => "stack",
            Self::DeployDeployment | Self::RestartDeployment => "deployment",
            Self::RunBuild | Self::CancelBuild => "build",
            Self::PullRepo => "repo",
        }
    }
}

/// Narrow interface consumed by the MCP layer and implemented by test fakes.
/// Sendable future returned by the object-safe API boundary.
pub type ApiFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ApiError>> + Send + 'a>>;

pub trait KomodoApi: Send + Sync {
    fn version(&self) -> ApiFuture<'_, String>;
    fn servers(&self) -> ApiFuture<'_, Vec<ResourceListItem<ServerInfo>>>;
    fn stacks(&self) -> ApiFuture<'_, Vec<ResourceListItem<StackInfo>>>;
    fn deployments(&self) -> ApiFuture<'_, Vec<ResourceListItem<DeploymentInfo>>>;
    fn builds(&self) -> ApiFuture<'_, Vec<ResourceListItem<BuildInfo>>>;
    fn repos(&self) -> ApiFuture<'_, Vec<ResourceListItem<RepoInfo>>>;
    fn operations(&self, page: u32) -> ApiFuture<'_, OperationPage>;
    fn update<'a>(&'a self, operation_id: &'a str) -> ApiFuture<'a, OperationItem>;
    fn stack_detail<'a>(&'a self, selector: &'a str) -> ApiFuture<'a, StackDetail>;
    fn stack_log<'a>(
        &'a self,
        selector: &'a str,
        services: &'a [String],
        tail: u64,
        timestamps: bool,
    ) -> ApiFuture<'a, Log>;
    fn update_logs<'a>(&'a self, operation_id: &'a str) -> ApiFuture<'a, Vec<Log>>;
    fn update_stack<'a>(&'a self, id: &'a str, patch: &'a StackConfigPatch) -> ApiFuture<'a, ()>;
    fn write_stack_file<'a>(
        &'a self,
        selector: &'a str,
        file_path: &'a str,
        contents: &'a str,
    ) -> ApiFuture<'a, MutationReceipt>;
    fn execute<'a>(&'a self, action: Action, selector: &'a str) -> ApiFuture<'a, MutationReceipt>;
}

/// Hardened Komodo HTTP client. Reads retry once; mutations are never replayed.
#[derive(Clone)]
pub struct Client {
    http: HttpClient,
    base_url: Url,
    credentials: Arc<Credentials>,
}

impl fmt::Debug for Client {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Client")
            .field("base_url", &self.base_url)
            .field("credentials", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl Client {
    /// Construct a redirect-free, proxy-free client with bounded timing.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe URLs, invalid timeout bounds, or HTTP client construction.
    pub fn new(
        base_url: Url,
        credentials: Credentials,
        timeout: Duration,
    ) -> Result<Self, ApiError> {
        validate_base_url(&base_url)?;
        if timeout.is_zero() || timeout > MAXIMUM_TIMEOUT {
            return Err(ApiError::InvalidConfiguration("request timeout"));
        }
        let http = HttpClient::builder()
            .timeout(timeout)
            .redirect(Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| ApiError::InvalidConfiguration("HTTP client"))?;
        Ok(Self {
            http,
            base_url,
            credentials: Arc::new(credentials),
        })
    }

    async fn read<T: DeserializeOwned>(&self, variant: &'static str) -> Result<T, ApiError> {
        self.post("read", variant, json!({ "query": {} }), true)
            .await
    }

    async fn post<T: DeserializeOwned>(
        &self,
        family: &'static str,
        variant: &'static str,
        params: Value,
        retry_read: bool,
    ) -> Result<T, ApiError> {
        match self.post_once(family, variant, &params).await {
            Err(ApiError::Unavailable) if retry_read => {
                self.post_once(family, variant, &params).await
            }
            result => result,
        }
    }

    async fn post_once<T: DeserializeOwned>(
        &self,
        family: &'static str,
        variant: &'static str,
        params: &Value,
    ) -> Result<T, ApiError> {
        let endpoint = self
            .base_url
            .join(family)
            .map_err(|_| ApiError::InvalidConfiguration("base URL"))?;
        let mut response = self
            .http
            .post(endpoint)
            .header("x-api-key", self.credentials.key.as_str())
            .header("x-api-secret", self.credentials.secret.as_str())
            .json(&json!({ "type": variant, "params": params }))
            .send()
            .await
            .map_err(|_| ApiError::Unavailable)?;

        match response.status() {
            status if status.is_success() => {}
            StatusCode::UNAUTHORIZED => return Err(ApiError::Authentication),
            StatusCode::FORBIDDEN => return Err(ApiError::Forbidden),
            StatusCode::NOT_FOUND => return Err(ApiError::NotFound),
            status if status.is_server_error() => return Err(ApiError::Unavailable),
            _ => return Err(ApiError::IncompatibleResponse),
        }

        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| ApiError::Unavailable)? {
            if bytes.len().saturating_add(chunk.len()) > MAXIMUM_RESPONSE_BYTES {
                return Err(ApiError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| ApiError::IncompatibleResponse)
    }
}

impl KomodoApi for Client {
    fn version(&self) -> ApiFuture<'_, String> {
        Box::pin(async move {
            #[derive(Deserialize)]
            struct Response {
                version: String,
            }
            self.post::<Response>("read", "GetVersion", json!({}), true)
                .await
                .map(|response| response.version)
        })
    }

    fn servers(&self) -> ApiFuture<'_, Vec<ResourceListItem<ServerInfo>>> {
        Box::pin(async move { self.read("ListServers").await })
    }

    fn stacks(&self) -> ApiFuture<'_, Vec<ResourceListItem<StackInfo>>> {
        Box::pin(async move { self.read("ListStacks").await })
    }

    fn deployments(&self) -> ApiFuture<'_, Vec<ResourceListItem<DeploymentInfo>>> {
        Box::pin(async move { self.read("ListDeployments").await })
    }

    fn builds(&self) -> ApiFuture<'_, Vec<ResourceListItem<BuildInfo>>> {
        Box::pin(async move { self.read("ListBuilds").await })
    }

    fn repos(&self) -> ApiFuture<'_, Vec<ResourceListItem<RepoInfo>>> {
        Box::pin(async move { self.read("ListRepos").await })
    }

    fn operations(&self, page: u32) -> ApiFuture<'_, OperationPage> {
        Box::pin(async move {
            self.post::<ListUpdatesResponse>(
                "read",
                "ListUpdates",
                json!({ "query": null, "page": page }),
                true,
            )
            .await
            .map(|response| OperationPage {
                operations: response.updates,
                next_page: response.next_page,
            })
        })
    }

    fn update<'a>(&'a self, operation_id: &'a str) -> ApiFuture<'a, OperationItem> {
        Box::pin(async move {
            self.post::<OperationItem>("read", "GetUpdate", json!({ "id": operation_id }), true)
                .await
        })
    }

    fn stack_detail<'a>(&'a self, selector: &'a str) -> ApiFuture<'a, StackDetail> {
        Box::pin(async move {
            self.post::<StackDetail>("read", "GetStack", json!({ "stack": selector }), true)
                .await
        })
    }

    fn stack_log<'a>(
        &'a self,
        selector: &'a str,
        services: &'a [String],
        tail: u64,
        timestamps: bool,
    ) -> ApiFuture<'a, Log> {
        Box::pin(async move {
            self.post::<Log>(
                "read",
                "GetStackLog",
                json!({
                    "stack": selector,
                    "services": services,
                    "tail": tail,
                    "timestamps": timestamps,
                }),
                true,
            )
            .await
        })
    }

    fn update_logs<'a>(&'a self, operation_id: &'a str) -> ApiFuture<'a, Vec<Log>> {
        Box::pin(async move {
            self.post::<UpdateLogsResponse>(
                "read",
                "GetUpdate",
                json!({ "id": operation_id }),
                true,
            )
            .await
            .map(|response| response.logs)
        })
    }

    fn update_stack<'a>(&'a self, id: &'a str, patch: &'a StackConfigPatch) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            // `UpdateStack` echoes the full stack, including any secret we just
            // wrote; discard the whole body into `WriteAck` so it never lands in
            // a field. Config writes are never retried.
            self.post::<WriteAck>(
                "write",
                "UpdateStack",
                json!({ "id": id, "config": patch }),
                false,
            )
            .await
            .map(|_| ())
        })
    }

    fn write_stack_file<'a>(
        &'a self,
        selector: &'a str,
        file_path: &'a str,
        contents: &'a str,
    ) -> ApiFuture<'a, MutationReceipt> {
        Box::pin(async move {
            self.post::<MutationReceipt>(
                "write",
                "WriteStackFileContents",
                json!({
                    "stack": selector,
                    "file_path": file_path,
                    "contents": contents,
                }),
                false,
            )
            .await
        })
    }

    fn execute<'a>(&'a self, action: Action, selector: &'a str) -> ApiFuture<'a, MutationReceipt> {
        Box::pin(async move {
            let mut params = serde_json::Map::new();
            params.insert(
                action.parameter().to_owned(),
                Value::String(selector.to_owned()),
            );
            if matches!(action, Action::DeployStack | Action::StopStack) {
                params.insert("services".to_owned(), Value::Array(Vec::new()));
                params.insert("stop_time".to_owned(), Value::Null);
            } else if action == Action::DeployDeployment {
                params.insert("stop_signal".to_owned(), Value::Null);
                params.insert("stop_time".to_owned(), Value::Null);
            }
            self.post("execute", action.variant(), Value::Object(params), false)
                .await
        })
    }
}

fn validate_base_url(url: &Url) -> Result<(), ApiError> {
    if url.cannot_be_a_base()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(ApiError::InvalidConfiguration("base URL"));
    }
    if url.scheme() == "https" {
        return Ok(());
    }
    if url.scheme() != "http" {
        return Err(ApiError::InvalidConfiguration("base URL scheme"));
    }
    let private = match url.host() {
        Some(Host::Ipv4(address)) => address.is_private() || address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback() || address.is_unique_local(),
        Some(Host::Domain(domain)) => domain == "localhost" || !domain.contains('.'),
        None => false,
    };
    private
        .then_some(())
        .ok_or(ApiError::InvalidConfiguration("plaintext Komodo address"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;
    use url::Url;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, header, method, path},
    };

    use super::{
        Action, ApiError, Client, Credentials, KomodoApi, MAXIMUM_RESPONSE_BYTES, MutationReceipt,
        OperationItem, StackConfigPatch, SystemCommand,
    };

    fn client(server: &MockServer) -> Client {
        Client::new(
            Url::parse(&format!("{}/", server.uri())).expect("wiremock URL"),
            Credentials::new("read-key".into(), "read-secret".into()).expect("credentials"),
            Duration::from_secs(2),
        )
        .expect("client")
    }

    #[tokio::test]
    async fn list_servers_uses_the_typed_envelope_and_discards_sensitive_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/read"))
            .and(header("x-api-key", "read-key"))
            .and(header("x-api-secret", "read-secret"))
            .and(body_json(json!({
                "type": "ListServers",
                "params": { "query": {} }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
                "id": "server-1",
                "type": "Server",
                "name": "mini",
                "config": { "address": "SENTINEL_PRIVATE_ADDRESS" },
                "info": {
                    "state": "Ok",
                    "version": "1.18.4",
                    "public_key": "SENTINEL_PUBLIC_KEY"
                }
            }])))
            .expect(1)
            .mount(&server)
            .await;

        let result = client(&server).servers().await.expect("server list");
        assert_eq!(result[0].name, "mini");
        assert_eq!(result[0].info.state, "Ok");
        let normalized = format!("{result:?}");
        assert!(!normalized.contains("SENTINEL"));
    }

    #[tokio::test]
    async fn mutation_is_sent_once_and_returns_only_a_receipt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/execute"))
            .and(body_json(json!({
                "type": "RestartStack",
                "params": { "stack": "gateway" }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "_id": { "$oid": "operation-1" },
                "operation": "RestartStack",
                "start_ts": 42,
                "success": true,
                "status": "Complete",
                "logs": [{ "stdout": "SENTINEL_SECRET" }],
                "target": {"type":"Stack", "id":"stack-1", "configuration":"SENTINEL_TARGET"},
                "current_toml": "SENTINEL_CONFIG"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let receipt = client(&server)
            .execute(Action::RestartStack, "gateway")
            .await
            .expect("mutation receipt");
        assert_eq!(receipt.operation_id, "operation-1");
        assert!(!format!("{receipt:?}").contains("SENTINEL"));
    }

    #[tokio::test]
    async fn stack_stop_uses_the_typed_envelope_and_returns_only_a_receipt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/execute"))
            .and(body_json(json!({
                "type": "StopStack",
                "params": { "stack": "stack-1", "services": [], "stop_time": null }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "_id": { "$oid": "stop-operation" },
                "operation": "StopStack",
                "start_ts": 42,
                "success": false,
                "status": "InProgress",
                "logs": [{ "stdout": "SENTINEL_SECRET" }],
                "current_toml": "SENTINEL_CONFIG"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let receipt = client(&server)
            .execute(Action::StopStack, "stack-1")
            .await
            .expect("stop receipt");
        assert_eq!(
            serde_json::to_value(receipt).expect("serialized receipt"),
            json!({
                "_id": "stop-operation",
                "operation": "StopStack",
                "start_ts": 42,
                "success": false,
                "status": "InProgress"
            })
        );
    }

    #[tokio::test]
    async fn get_update_reads_one_operation_by_stable_id_and_discards_sensitive_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/read"))
            .and(body_json(json!({
                "type": "GetUpdate",
                "params": { "id": "op-42" }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "_id": { "$oid": "op-42" },
                "operation": "DeployStack",
                "target": {"type":"Stack", "id":"stack-1", "configuration":"SENTINEL_TARGET"},
                "start_ts": 99,
                "success": true,
                "status": "Complete",
                "logs": [{ "stdout": "SENTINEL_SECRET" }],
                "current_toml": "SENTINEL_CONFIG",
                "operator": "SENTINEL_USER"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let operation = client(&server).update("op-42").await.expect("update");
        assert_eq!(operation.id, "op-42");
        assert_eq!(operation.operation, "DeployStack");
        assert!(operation.success);
        assert_eq!(operation.status, "Complete");
        let target = operation.target.as_ref().expect("resource target");
        assert_eq!(target.kind, "Stack");
        assert_eq!(target.id, "stack-1");
        assert!(!format!("{operation:?}").contains("SENTINEL"));
    }

    #[test]
    fn operation_ids_accept_both_bare_strings_and_extended_json_object_ids() {
        let base = json!({
            "operation": "DeployStack",
            "start_ts": 1,
            "success": true,
            "status": "Complete"
        });
        for (id_key, id_value, expected) in [
            ("id", json!("plain-id"), "plain-id"),
            ("_id", json!({ "$oid": "object-id" }), "object-id"),
        ] {
            let mut value = base.clone();
            value[id_key] = id_value;
            let item: OperationItem = serde_json::from_value(value).expect("operation item");
            assert_eq!(item.id, expected);
        }
    }

    #[tokio::test]
    async fn get_stack_returns_bounded_config_and_compose_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/read"))
            .and(body_json(json!({
                "type": "GetStack",
                "params": { "stack": "gateway" }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "_id": { "$oid": "stack-1" },
                "name": "gateway",
                "config": {
                    "file_contents": "services:\n  api: {}\n",
                    "environment": "TOKEN=sentinel-env\n",
                    "files_on_host": false,
                    "run_directory": "stacks/gateway",
                    "file_paths": ["compose.yaml"],
                    "env_file_path": ".env",
                    "pre_deploy": { "path": "stacks/gateway", "command": "echo pre" },
                    "post_deploy": { "path": "", "command": "" },
                    "webhook_enabled": true,
                    "webhook_secret": "sentinel-hook",
                    "webhook_force_deploy": false
                },
                "info": {
                    "deployed_contents": [{ "path": "compose.yaml", "contents": "deployed" }],
                    "remote_contents": [{
                        "path": "compose.yaml",
                        "contents": "latest",
                        "services": ["api"],
                        "requires": "Redeploy"
                    }]
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let detail = client(&server)
            .stack_detail("gateway")
            .await
            .expect("stack");
        assert_eq!(detail.config.run_directory, "stacks/gateway");
        assert_eq!(detail.config.webhook_secret, "sentinel-hook");
        assert_eq!(detail.config.pre_deploy.command, "echo pre");
        assert_eq!(
            detail.info.deployed_contents.expect("deployed")[0].contents,
            "deployed"
        );
        assert_eq!(
            detail.info.remote_contents.expect("remote")[0].contents,
            "latest"
        );
    }

    #[tokio::test]
    async fn get_stack_log_sends_a_bounded_query_and_returns_command_output() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/read"))
            .and(body_json(json!({
                "type": "GetStackLog",
                "params": {
                    "stack": "gateway",
                    "services": ["api"],
                    "tail": 200,
                    "timestamps": true
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "stage": "get log",
                "command": "docker compose logs",
                "stdout": "line-1\nline-2",
                "stderr": "",
                "success": true,
                "start_ts": 1,
                "end_ts": 2
            })))
            .expect(1)
            .mount(&server)
            .await;

        let log = client(&server)
            .stack_log("gateway", &["api".to_owned()], 200, true)
            .await
            .expect("stack log");
        assert_eq!(log.stdout, "line-1\nline-2");
        assert!(log.success);
    }

    #[tokio::test]
    async fn get_update_logs_projects_only_log_records_from_the_update() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/read"))
            .and(body_json(json!({
                "type": "GetUpdate",
                "params": { "id": "op-1" }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "_id": { "$oid": "op-1" },
                "operation": "DeployStack",
                "start_ts": 1,
                "success": true,
                "status": "Complete",
                "current_toml": "SENTINEL_CONFIG",
                "logs": [
                    { "stage": "deploy", "command": "up", "stdout": "ok", "stderr": "",
                      "success": true, "start_ts": 1, "end_ts": 2 }
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let logs = client(&server)
            .update_logs("op-1")
            .await
            .expect("update logs");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].stage, "deploy");
        assert!(!format!("{logs:?}").contains("SENTINEL"));
    }

    #[tokio::test]
    async fn update_stack_sends_only_set_fields_and_discards_the_echoed_config() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/write"))
            .and(body_json(json!({
                "type": "UpdateStack",
                "params": {
                    "id": "stack-1",
                    "config": { "environment": "TOKEN=sentinel-env\n" }
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "_id": { "$oid": "stack-1" },
                "config": { "environment": "TOKEN=sentinel-env\n", "webhook_secret": "SENTINEL_HOOK" }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let patch = StackConfigPatch {
            environment: Some("TOKEN=sentinel-env\n".into()),
            ..StackConfigPatch::default()
        };
        client(&server)
            .update_stack("stack-1", &patch)
            .await
            .expect("update stack");
    }

    #[tokio::test]
    async fn update_stack_sends_a_webhook_secret_and_discards_the_echoed_value() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/write"))
            .and(body_json(json!({
                "type": "UpdateStack",
                "params": {
                    "id": "stack-1",
                    "config": { "webhook_secret": "sentinel-hook" }
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "_id": { "$oid": "stack-1" },
                "config": { "webhook_secret": "sentinel-hook" }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let patch = StackConfigPatch {
            webhook_secret: Some("sentinel-hook".into()),
            ..StackConfigPatch::default()
        };
        client(&server)
            .update_stack("stack-1", &patch)
            .await
            .expect("update stack");
    }

    #[tokio::test]
    async fn update_stack_sends_exactly_the_set_webhook_flags() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/write"))
            .and(body_json(json!({
                "type": "UpdateStack",
                "params": {
                    "id": "stack-1",
                    "config": { "webhook_enabled": true, "webhook_force_deploy": false }
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "_id": { "$oid": "stack-1" },
                "config": {}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let patch = StackConfigPatch {
            webhook_enabled: Some(true),
            webhook_force_deploy: Some(false),
            ..StackConfigPatch::default()
        };
        client(&server)
            .update_stack("stack-1", &patch)
            .await
            .expect("update stack");
    }

    #[tokio::test]
    async fn write_stack_file_returns_a_receipt_without_the_written_contents() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/write"))
            .and(body_json(json!({
                "type": "WriteStackFileContents",
                "params": {
                    "stack": "gateway",
                    "file_path": "compose.yaml",
                    "contents": "SENTINEL_BODY"
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "_id": { "$oid": "op-9" },
                "operation": "WriteStackFileContents",
                "start_ts": 5,
                "success": true,
                "status": "Complete",
                "logs": [{ "stdout": "SENTINEL_BODY" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let receipt = client(&server)
            .write_stack_file("gateway", "compose.yaml", "SENTINEL_BODY")
            .await
            .expect("write file receipt");
        assert_eq!(receipt.operation_id, "op-9");
        assert!(!format!("{receipt:?}").contains("SENTINEL"));
    }

    #[tokio::test]
    async fn config_writes_are_never_replayed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/write"))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&server)
            .await;
        let patch = StackConfigPatch {
            pre_deploy: Some(SystemCommand {
                path: String::new(),
                command: "echo hi".into(),
            }),
            ..StackConfigPatch::default()
        };
        assert_eq!(
            client(&server)
                .update_stack("stack-1", &patch)
                .await
                .expect_err("unavailable write"),
            ApiError::Unavailable
        );
    }

    #[tokio::test]
    async fn unavailable_mutations_are_never_replayed() {
        for action in [Action::RestartStack, Action::StopStack] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/execute"))
                .respond_with(ResponseTemplate::new(503))
                .expect(1)
                .mount(&server)
                .await;

            assert_eq!(
                client(&server)
                    .execute(action, "gateway")
                    .await
                    .expect_err("unavailable mutation"),
                ApiError::Unavailable
            );
        }
    }

    #[test]
    fn configuration_rejects_public_plaintext_and_redacts_credentials() {
        let credentials = Credentials::new("key".into(), "secret".into()).expect("credentials");
        assert_eq!(format!("{credentials:?}"), "Credentials([REDACTED])");
        let result = Client::new(
            Url::parse("http://komodo.example.com/").expect("URL"),
            credentials,
            Duration::from_secs(2),
        );
        assert_eq!(
            result.expect_err("public plaintext rejected"),
            ApiError::InvalidConfiguration("plaintext Komodo address")
        );
    }

    #[test]
    fn credentials_reject_empty_or_line_bearing_values() {
        for (key, secret) in [
            ("", "secret"),
            ("key", ""),
            ("key\nsecond-line", "secret"),
            ("key", "secret\rsecond-line"),
        ] {
            assert!(Credentials::new(key.into(), secret.into()).is_err());
        }
        assert!(Credentials::new("key with spaces".into(), "secret".into()).is_ok());
    }

    #[test]
    fn operation_receipts_accept_only_supported_object_ids() {
        let base = json!({
            "operation": "RestartStack",
            "start_ts": 42,
            "success": true,
            "status": "Complete"
        });
        for (operation_id, expected) in [
            (json!("string-id"), "string-id"),
            (json!({ "$oid": "object-id" }), "object-id"),
        ] {
            let mut value = base.clone();
            value["_id"] = operation_id;
            let receipt: MutationReceipt = serde_json::from_value(value).expect("receipt");
            assert_eq!(receipt.operation_id, expected);
        }
        for operation_id in [json!(null), json!(7), json!({}), json!({ "$oid": 7 })] {
            let mut value = base.clone();
            value["_id"] = operation_id;
            assert!(serde_json::from_value::<MutationReceipt>(value).is_err());
        }
    }

    #[test]
    fn base_url_and_timeout_policy_is_exact() {
        let cases = [
            ("https://komodo.example.com/", true),
            ("http://core:9120/", true),
            ("http://localhost:9120/", true),
            ("http://127.0.0.1:9120/", true),
            ("http://10.0.0.8:9120/", true),
            ("http://[::1]:9120/", true),
            ("http://[fd00::1]:9120/", true),
            ("http://komodo.example.com/", false),
            ("ftp://komodo-core/", false),
            ("https://user@komodo.example.com/", false),
            ("https://komodo.example.com/?query=yes", false),
            ("https://komodo.example.com/#fragment", false),
            ("https://komodo.example.com/api/", false),
        ];
        for (raw_url, accepted) in cases {
            let result = Client::new(
                Url::parse(raw_url).expect("URL"),
                Credentials::new("key".into(), "secret".into()).expect("credentials"),
                Duration::from_secs(2),
            );
            assert_eq!(result.is_ok(), accepted, "unexpected policy for {raw_url}");
        }

        for timeout in [Duration::ZERO, Duration::from_secs(121)] {
            let result = Client::new(
                Url::parse("https://komodo.example.com/").expect("URL"),
                Credentials::new("key".into(), "secret".into()).expect("credentials"),
                timeout,
            );
            assert_eq!(
                result.expect_err("timeout rejected"),
                ApiError::InvalidConfiguration("request timeout")
            );
        }
    }

    #[test]
    fn client_debug_never_contains_credentials() {
        let client = Client::new(
            Url::parse("https://komodo.example.com/").expect("URL"),
            Credentials::new("SENTINEL_KEY".into(), "SENTINEL_SECRET".into()).expect("credentials"),
            Duration::from_secs(2),
        )
        .expect("client");
        let debug = format!("{client:?}");
        assert!(!debug.contains("SENTINEL_KEY"));
        assert!(!debug.contains("SENTINEL_SECRET"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn reads_retry_one_unavailable_response_then_stop() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/read"))
            .respond_with(ResponseTemplate::new(503))
            .with_priority(1)
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/read"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "version": "2.1.2" })))
            .with_priority(2)
            .expect(1)
            .mount(&server)
            .await;

        assert_eq!(client(&server).version().await.expect("version"), "2.1.2");
    }

    #[tokio::test]
    async fn response_status_and_body_failures_map_to_the_safe_vocabulary() {
        for (status, expected, expected_calls) in [
            (401, ApiError::Authentication, 1),
            (403, ApiError::Forbidden, 1),
            (404, ApiError::NotFound, 1),
            (409, ApiError::IncompatibleResponse, 1),
            (500, ApiError::Unavailable, 2),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/read"))
                .respond_with(
                    ResponseTemplate::new(status).set_body_string("SENTINEL_UPSTREAM_BODY"),
                )
                .expect(expected_calls)
                .mount(&server)
                .await;
            let error = client(&server).version().await.expect_err("safe error");
            assert_eq!(error, expected);
            assert!(!error.to_string().contains("SENTINEL"));
        }

        let invalid_json = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not-json"))
            .expect(1)
            .mount(&invalid_json)
            .await;
        assert_eq!(
            client(&invalid_json)
                .version()
                .await
                .expect_err("invalid JSON"),
            ApiError::IncompatibleResponse
        );

        let oversized = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 2 * 1024 * 1024 + 1]),
            )
            .expect(1)
            .mount(&oversized)
            .await;
        assert_eq!(
            client(&oversized)
                .version()
                .await
                .expect_err("oversized response"),
            ApiError::ResponseTooLarge
        );

        let exact_bound = MockServer::start().await;
        let mut bounded_body = br#"{"version":"bounded"}"#.to_vec();
        bounded_body.resize(MAXIMUM_RESPONSE_BYTES, b' ');
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bounded_body))
            .expect(1)
            .mount(&exact_bound)
            .await;
        assert_eq!(
            client(&exact_bound)
                .version()
                .await
                .expect("bounded response"),
            "bounded"
        );
    }

    #[test]
    fn every_action_has_a_stable_upstream_variant_and_selector_parameter() {
        let cases = [
            (Action::DeployStack, "DeployStack", "stack"),
            (Action::RestartStack, "RestartStack", "stack"),
            (Action::StopStack, "StopStack", "stack"),
            (Action::DeployDeployment, "Deploy", "deployment"),
            (Action::RestartDeployment, "RestartDeployment", "deployment"),
            (Action::RunBuild, "RunBuild", "build"),
            (Action::CancelBuild, "CancelBuild", "build"),
            (Action::PullRepo, "PullRepo", "repo"),
        ];
        for (action, variant, parameter) in cases {
            assert_eq!(action.variant(), variant);
            assert_eq!(action.parameter(), parameter);
        }
    }
}
