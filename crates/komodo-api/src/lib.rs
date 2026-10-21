//! Bounded, typed access to the Komodo Core API.

pub mod errors;
pub mod inventory;
pub mod networks;
pub mod operations;

use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use reqwest::{Client as HttpClient, redirect::Policy};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use thiserror::Error;
use url::{Host, Url};
use zeroize::Zeroizing;

pub mod stacks;

const MAXIMUM_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAXIMUM_TIMEOUT: Duration = Duration::from_mins(2);
const CORE_VERSION_CACHE_TTL: Duration = Duration::from_mins(1);

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

/// Local failures and typed upstream diagnostics. Display and Debug never
/// contain response values; callers obtain ordinary diagnostic facts separately.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ApiError {
    #[error("{0}")]
    Upstream(Box<errors::UpstreamFailure>),
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
    #[error("inventory pagination requires Komodo Core 2.3 or later; use page zero on older Core")]
    InventoryPaginationUnsupported,
    #[error("resource name lookup exceeded its bounded search; use the exact resource id")]
    InventorySelectorTooBroad,
}

impl ApiError {
    /// Whether a submitted mutation needs reconciliation before another write.
    #[must_use]
    pub fn mutation_outcome_unknown(&self) -> bool {
        matches!(
            self,
            Self::Unavailable | Self::IncompatibleResponse | Self::ResponseTooLarge
        ) || matches!(self, Self::Upstream(failure) if !matches!(failure.status, 401 | 403 | 404))
    }

    /// Whether the upstream explicitly reported that the requested resource is absent.
    #[must_use]
    pub fn resource_not_found(&self) -> bool {
        matches!(self, Self::NotFound)
            || matches!(self, Self::Upstream(failure) if failure.status == 404)
    }
}

/// Common resource metadata returned by list endpoints.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ResourceListItem<I> {
    pub id: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub name: String,
    #[serde(default)]
    pub template: Option<bool>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    pub info: I,
}

/// Server list facts reported by Core. Public keys and addresses are metadata.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq)]
pub struct ServerInfo {
    pub state: String,
    pub version: Option<String>,
    #[serde(default)]
    pub err: Option<ReportedError>,
    #[serde(default)]
    pub stats: Option<SystemStats>,
    #[serde(default)]
    pub alerting_thresholds: Option<AlertingThresholds>,
    #[serde(default)]
    pub core_count: Option<u32>,
    #[serde(default)]
    pub logical_core_count: Option<u32>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub external_address: Option<String>,
    #[serde(default)]
    pub public_ip: Option<String>,
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default)]
    pub attempted_public_key: Option<String>,
    #[serde(default)]
    pub send_unreachable_alerts: Option<bool>,
    #[serde(default)]
    pub send_cpu_alerts: Option<bool>,
    #[serde(default)]
    pub send_mem_alerts: Option<bool>,
    #[serde(default)]
    pub send_disk_alerts: Option<bool>,
    #[serde(default)]
    pub send_version_mismatch_alerts: Option<bool>,
    #[serde(default)]
    pub terminals_disabled: Option<bool>,
    #[serde(default)]
    pub container_terminals_disabled: Option<bool>,
}

/// Core's structured diagnostic, retained as data rather than executable text.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ReportedError {
    pub error: String,
    #[serde(default)]
    pub trace: Vec<String>,
}

/// Server alert thresholds, expressed as percentages.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq)]
pub struct AlertingThresholds {
    pub cpu_warning: f64,
    pub cpu_critical: f64,
    pub mem_warning: f64,
    pub mem_critical: f64,
    pub disk_warning: f64,
    pub disk_critical: f64,
}

/// Core's current minimal server statistics; absent on older Core versions.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq)]
pub struct SystemStats {
    pub cpu_perc: f64,
    pub load_average: LoadAverage,
    pub mem_free_gb: f64,
    pub mem_used_gb: f64,
    pub mem_total_gb: f64,
    pub mem_buff_cache_gb: f64,
    pub mem_zfs_arc_gb: f64,
    pub swap_total_gb: f64,
    pub swap_used_gb: f64,
    pub disk_total_gb: f64,
    pub disk_used_gb: f64,
    pub network_ingress_bytes: f64,
    pub network_egress_bytes: f64,
    pub polling_rate: String,
    pub refresh_ts: i64,
    pub refresh_list_ts: i64,
}

/// One-, five-, and fifteen-minute system load averages.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq)]
pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

/// Stack list facts, including source and deployment provenance.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct StackInfo {
    pub state: String,
    #[serde(default)]
    pub swarm_id: Option<String>,
    #[serde(default)]
    pub swarm_name: Option<String>,
    #[serde(default)]
    pub server_id: Option<String>,
    #[serde(default)]
    pub server_name: Option<String>,
    #[serde(default)]
    pub files_on_host: Option<bool>,
    #[serde(default)]
    pub file_contents: Option<bool>,
    #[serde(default)]
    pub linked_repo: Option<String>,
    #[serde(default)]
    pub linked_repo_name: Option<String>,
    #[serde(default)]
    pub git_provider: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub repo_link: Option<String>,
    #[serde(default)]
    pub auto_update_all_services: Option<bool>,
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

/// Per-service list facts reported by Core.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct StackService {
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub update_available: bool,
}

/// Deployment list facts reported by Core.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct DeploymentInfo {
    pub state: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub custom_name: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub swarm_id: Option<String>,
    #[serde(default)]
    pub swarm_name: Option<String>,
    #[serde(default)]
    pub server_id: Option<String>,
    #[serde(default)]
    pub server_name: Option<String>,
    #[serde(default)]
    pub build_id: Option<String>,
    #[serde(default)]
    pub update_available: bool,
}

/// Build list facts reported by Core.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct BuildInfo {
    pub state: String,
    #[serde(default)]
    pub version: Option<ResourceVersion>,
    #[serde(default)]
    pub builder_id: Option<String>,
    #[serde(default)]
    pub files_on_host: Option<bool>,
    #[serde(default)]
    pub dockerfile_contents: Option<bool>,
    #[serde(default)]
    pub linked_repo: Option<String>,
    #[serde(default)]
    pub linked_repo_name: Option<String>,
    #[serde(default)]
    pub git_provider: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub repo_link: Option<String>,
    #[serde(default)]
    pub built_hash: Option<String>,
    #[serde(default)]
    pub latest_hash: Option<String>,
    #[serde(default)]
    pub image_registry_domain: Option<String>,
    #[serde(default)]
    pub last_built_at: i64,
}

/// Repository list facts reported by Core.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct RepoInfo {
    pub state: String,
    #[serde(default)]
    pub server_id: Option<String>,
    #[serde(default)]
    pub server_name: Option<String>,
    #[serde(default)]
    pub builder_id: Option<String>,
    #[serde(default)]
    pub git_provider: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub repo_link: Option<String>,
    #[serde(default)]
    pub cloned_hash: Option<String>,
    #[serde(default)]
    pub cloned_message: Option<String>,
    #[serde(default)]
    pub built_hash: Option<String>,
    #[serde(default)]
    pub latest_hash: Option<String>,
    #[serde(default)]
    pub last_pulled_at: i64,
    #[serde(default)]
    pub last_built_at: i64,
}

/// Numeric version associated with a build or operation.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ResourceVersion {
    pub major: i32,
    pub minor: i32,
    pub patch: i32,
}

/// Operation facts returned by list and detail requests.
///
/// Komodo serializes the update identifier as a bare string in list responses
/// and as an extended-JSON `{ "$oid": ... }` object elsewhere, so the id accepts
/// either form under both the `id` and `_id` keys.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct OperationItem {
    #[serde(alias = "_id", deserialize_with = "deserialize_object_id")]
    pub id: String,
    pub operation: String,
    pub start_ts: i64,
    pub success: bool,
    pub status: String,
    #[serde(default)]
    pub operator: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub end_ts: Option<i64>,
    #[serde(default)]
    pub version: Option<ResourceVersion>,
    #[serde(default)]
    pub commit_hash: Option<String>,
    #[serde(default)]
    pub other_data: Option<String>,
    #[serde(default)]
    pub prev_toml: Option<String>,
    #[serde(default)]
    pub current_toml: Option<String>,
    /// Resource correlation reported by Core, including future resource kinds.
    #[serde(default)]
    pub target: Option<OperationTarget>,
}

/// Resource correlation carried by Komodo update lists and details.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
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
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct SystemCommand {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub command: String,
    /// Whether Core executes the command using its shell mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_mode: Option<bool>,
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

/// Typed stack configuration from `GetStack`. The literal webhook token has a
/// dedicated native-file read; all ordinary configuration is serializable.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct StackConfigDetail {
    #[serde(flatten)]
    pub facts: stacks::StackConfigurationFacts,
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
    #[serde(default, skip_serializing)]
    #[schemars(skip)]
    pub webhook_secret: String,
    #[serde(default)]
    pub webhook_force_deploy: bool,
}

/// One Compose or additional file: its path and full contents.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ComposeFile {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub contents: String,
    /// Services depending on a remote file; null when Core did not report it.
    pub services: Option<Vec<String>>,
    /// Core's required action when the remote file changes.
    pub requires: Option<String>,
}

/// Deployed and latest Compose content exposed by `GetStack`'s stack info.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct StackComposeInfo {
    #[serde(flatten)]
    pub provenance: stacks::StackComposeProvenance,
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
/// untrusted, and must never reach MCP or gateway logs or metrics.
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
    fn networks<'a>(&'a self, server: &'a str) -> ApiFuture<'a, Vec<networks::NetworkListItem>>;
    fn network<'a>(&'a self, server: &'a str, network: &'a str)
    -> ApiFuture<'a, networks::Network>;
    fn stacks(&self) -> ApiFuture<'_, Vec<ResourceListItem<StackInfo>>>;
    fn deployments(&self) -> ApiFuture<'_, Vec<ResourceListItem<DeploymentInfo>>>;
    fn builds(&self) -> ApiFuture<'_, Vec<ResourceListItem<BuildInfo>>>;
    fn repos(&self) -> ApiFuture<'_, Vec<ResourceListItem<RepoInfo>>>;
    fn servers_inventory<'a>(
        &'a self,
        request: inventory::InventoryRequest<'a>,
    ) -> ApiFuture<'a, inventory::InventoryPage<ServerInfo>> {
        Box::pin(
            async move { inventory::InventoryPage::from_legacy(self.servers().await?, request) },
        )
    }
    fn stacks_inventory<'a>(
        &'a self,
        request: inventory::InventoryRequest<'a>,
    ) -> ApiFuture<'a, inventory::InventoryPage<StackInfo>> {
        Box::pin(
            async move { inventory::InventoryPage::from_legacy(self.stacks().await?, request) },
        )
    }
    fn deployments_inventory<'a>(
        &'a self,
        request: inventory::InventoryRequest<'a>,
    ) -> ApiFuture<'a, inventory::InventoryPage<DeploymentInfo>> {
        Box::pin(async move {
            inventory::InventoryPage::from_legacy(self.deployments().await?, request)
        })
    }
    fn builds_inventory<'a>(
        &'a self,
        request: inventory::InventoryRequest<'a>,
    ) -> ApiFuture<'a, inventory::InventoryPage<BuildInfo>> {
        Box::pin(
            async move { inventory::InventoryPage::from_legacy(self.builds().await?, request) },
        )
    }
    fn repos_inventory<'a>(
        &'a self,
        request: inventory::InventoryRequest<'a>,
    ) -> ApiFuture<'a, inventory::InventoryPage<RepoInfo>> {
        Box::pin(async move { inventory::InventoryPage::from_legacy(self.repos().await?, request) })
    }
    fn operations(&self, page: u32) -> ApiFuture<'_, OperationPage>;
    fn operations_query<'a>(
        &'a self,
        page: u32,
        query: &'a operations::OperationQuery,
    ) -> ApiFuture<'a, OperationPage> {
        Box::pin(async move {
            let mut page = self.operations(page).await?;
            page.operations.retain(|item| query.matches(item));
            Ok(page)
        })
    }
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
    core_version: Arc<tokio::sync::Mutex<Option<CachedCoreVersion>>>,
}

struct CachedCoreVersion {
    at_least_2_3: bool,
    expires_at: Instant,
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
            core_version: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    async fn read<T: DeserializeOwned>(&self, variant: &'static str) -> Result<T, ApiError> {
        // Compatibility path for Core releases without inventory pagination.
        self.post("read", variant, json!({ "query": {}, "limit": 0 }), true)
            .await
    }

    async fn core_2_3_or_later(&self) -> Result<bool, ApiError> {
        let mut cached = self.core_version.lock().await;
        if let Some(version) = cached.as_ref().filter(|v| v.expires_at > Instant::now()) {
            return Ok(version.at_least_2_3);
        }
        let at_least_2_3 = networks::uses_current_names(&self.version().await?)
            .ok_or(ApiError::IncompatibleResponse)?;
        *cached = Some(CachedCoreVersion {
            at_least_2_3,
            expires_at: Instant::now() + CORE_VERSION_CACHE_TTL,
        });
        Ok(at_least_2_3)
    }

    async fn post<T: DeserializeOwned>(
        &self,
        family: &'static str,
        variant: &'static str,
        params: Value,
        retry_read: bool,
    ) -> Result<T, ApiError> {
        match self.post_once(family, variant, &params).await {
            Err(error)
                if retry_read
                    && (matches!(&error, ApiError::Unavailable)
                        || matches!(&error, ApiError::Upstream(failure) if matches!(failure.status, 500..=599))) =>
            {
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

        let status = response.status();
        let mut bytes = Vec::new();
        loop {
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(_) if !status.is_success() => {
                    return Err(ApiError::Upstream(Box::new(
                        errors::UpstreamFailure::without_body(
                            status.as_u16(),
                            variant,
                            "response body interrupted",
                        ),
                    )));
                }
                Err(_) => return Err(ApiError::Unavailable),
            };
            if bytes.len().saturating_add(chunk.len()) > MAXIMUM_RESPONSE_BYTES {
                if !status.is_success() {
                    return Err(ApiError::Upstream(Box::new(
                        errors::UpstreamFailure::without_body(
                            status.as_u16(),
                            variant,
                            "response body exceeds byte bound",
                        ),
                    )));
                }
                return Err(ApiError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            let mut credentials = vec![
                self.credentials.key.as_str(),
                self.credentials.secret.as_str(),
            ];
            if variant == "UpdateStack"
                && let Some(token) = params
                    .get("config")
                    .and_then(|config| config.get("webhook_secret"))
                    .and_then(Value::as_str)
            {
                credentials.push(token);
            }
            return Err(ApiError::Upstream(Box::new(
                errors::UpstreamFailure::from_response(
                    status.as_u16(),
                    variant,
                    bytes,
                    &credentials,
                ),
            )));
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

    fn networks<'a>(&'a self, server: &'a str) -> ApiFuture<'a, Vec<networks::NetworkListItem>> {
        Box::pin(async move {
            let variant = if self.core_2_3_or_later().await? {
                "ListNetworks"
            } else {
                "ListDockerNetworks"
            };
            self.post("read", variant, json!({"server":server}), true)
                .await
        })
    }

    fn network<'a>(
        &'a self,
        server: &'a str,
        network: &'a str,
    ) -> ApiFuture<'a, networks::Network> {
        Box::pin(async move {
            let variant = if self.core_2_3_or_later().await? {
                "InspectNetwork"
            } else {
                "InspectDockerNetwork"
            };
            self.post(
                "read",
                variant,
                json!({"server":server,"network":network}),
                true,
            )
            .await
        })
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

    fn servers_inventory<'a>(
        &'a self,
        request: inventory::InventoryRequest<'a>,
    ) -> ApiFuture<'a, inventory::InventoryPage<ServerInfo>> {
        Box::pin(async move { self.inventory("ListServers", request).await })
    }
    fn stacks_inventory<'a>(
        &'a self,
        request: inventory::InventoryRequest<'a>,
    ) -> ApiFuture<'a, inventory::InventoryPage<StackInfo>> {
        Box::pin(async move { self.inventory("ListStacks", request).await })
    }
    fn deployments_inventory<'a>(
        &'a self,
        request: inventory::InventoryRequest<'a>,
    ) -> ApiFuture<'a, inventory::InventoryPage<DeploymentInfo>> {
        Box::pin(async move { self.inventory("ListDeployments", request).await })
    }
    fn builds_inventory<'a>(
        &'a self,
        request: inventory::InventoryRequest<'a>,
    ) -> ApiFuture<'a, inventory::InventoryPage<BuildInfo>> {
        Box::pin(async move { self.inventory("ListBuilds", request).await })
    }
    fn repos_inventory<'a>(
        &'a self,
        request: inventory::InventoryRequest<'a>,
    ) -> ApiFuture<'a, inventory::InventoryPage<RepoInfo>> {
        Box::pin(async move { self.inventory("ListRepos", request).await })
    }
    fn operations(&self, page: u32) -> ApiFuture<'_, OperationPage> {
        Box::pin(async move {
            self.operations_query(page, &operations::OperationQuery::default())
                .await
        })
    }

    fn operations_query<'a>(
        &'a self,
        page: u32,
        query: &'a operations::OperationQuery,
    ) -> ApiFuture<'a, OperationPage> {
        Box::pin(async move {
            self.post::<ListUpdatesResponse>(
                "read",
                "ListUpdates",
                json!({ "query": query.upstream_query(), "page": page }),
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

    fn server_stats_fixture() -> serde_json::Value {
        json!({
            "cpu_perc":12.5, "load_average":{"one":1.5,"five":1.0,"fifteen":0.5},
            "mem_free_gb":2.0, "mem_used_gb":4.0, "mem_total_gb":8.0,
            "mem_buff_cache_gb":2.0, "mem_zfs_arc_gb":0.0,
            "swap_total_gb":1.0, "swap_used_gb":0.0,
            "disk_total_gb":100.0, "disk_used_gb":25.0,
            "network_ingress_bytes":1000.0, "network_egress_bytes":500.0,
            "polling_rate":"1-sec", "refresh_ts":100, "refresh_list_ts":90
        })
    }

    #[test]
    fn new_resource_facts_distinguish_omission_from_reported_empty_values() {
        use super::{BuildInfo, DeploymentInfo, RepoInfo, ResourceListItem, ServerInfo, StackInfo};
        fn check<T: serde::de::DeserializeOwned + serde::Serialize>(facts: &serde_json::Value) {
            let absent: T = serde_json::from_value(json!({"state":"Unknown"})).unwrap();
            let absent = serde_json::to_value(absent).unwrap();
            let mut reported = facts.clone();
            reported["state"] = json!("Unknown");
            let reported: T = serde_json::from_value(reported).unwrap();
            let reported = serde_json::to_value(reported).unwrap();
            for (name, value) in facts.as_object().unwrap() {
                assert!(
                    absent[name].is_null(),
                    "{name} must preserve upstream omission"
                );
                assert_eq!(
                    &reported[name], value,
                    "{name} must preserve reported values"
                );
            }
        }
        check::<ServerInfo>(&json!({"region":"", "send_unreachable_alerts":false,
            "send_cpu_alerts":false,"send_mem_alerts":false,"send_disk_alerts":false,
            "send_version_mismatch_alerts":false,"terminals_disabled":false,
            "container_terminals_disabled":false}));
        check::<StackInfo>(&json!({"swarm_id":"","swarm_name":"","server_id":"",
            "server_name":"","files_on_host":false,"file_contents":false,
            "linked_repo":"","linked_repo_name":"","git_provider":"","repo":"",
            "branch":"","repo_link":"","auto_update_all_services":false}));
        check::<DeploymentInfo>(&json!({"custom_name":"","image":"","swarm_id":"",
            "swarm_name":"","server_id":"","server_name":""}));
        check::<BuildInfo>(&json!({"builder_id":"","files_on_host":false,
            "dockerfile_contents":false,"linked_repo":"","linked_repo_name":"",
            "git_provider":"","repo":"","branch":"","repo_link":""}));
        check::<RepoInfo>(&json!({"server_id":"","server_name":"","builder_id":"",
            "git_provider":"","repo":"","branch":"","repo_link":""}));
        let absent: ResourceListItem<ServerInfo> = serde_json::from_value(json!({
            "id":"s","name":"server","type":"Server","info":{"state":"Unknown"}
        }))
        .unwrap();
        assert_eq!(absent.template, None);
        assert_eq!(absent.tags, None);
        let reported: ResourceListItem<ServerInfo> = serde_json::from_value(json!({
            "id":"s","name":"server","type":"Server","template":false,"tags":[],
            "info":{"state":"Unknown"}
        }))
        .unwrap();
        assert_eq!(reported.template, Some(false));
        assert_eq!(reported.tags, Some(Vec::new()));
    }

    #[tokio::test]
    async fn list_servers_preserves_addresses_public_keys_and_resource_metadata() {
        let server = MockServer::start().await;
        let mut resources = (0..60)
            .map(|index| {
                json!({
                    "id": format!("server-{index}"),
                    "type": "Server",
                    "name": format!("server-{index:03}"),
                    "info": { "state": "Ok", "version": "1.18.4" }
                })
            })
            .collect::<Vec<_>>();
        resources.push(json!({
            "id": "server-last",
            "type": "Server",
            "name": "z-target-server",
            "template": true,
            "tags": ["tag-production", "tag-east"],
            "config": { "address": "SENTINEL_PRIVATE_ADDRESS" },
            "info": {
                "state": "Ok",
                "version": "1.18.4",
                "address": "https://10.23.4.5:8120",
                "external_address": "https://server.example.test",
                "public_ip": "192.0.2.15",
                "region": "east",
                "public_key": "SENTINEL_PUBLIC_KEY",
                "core_count": 4,
                "logical_core_count": 8,
                "err": {"error":"dial address did not respond", "trace":["server reachability"]},
                "alerting_thresholds": {
                    "cpu_warning":80.0, "cpu_critical":95.0,
                    "mem_warning":80.0, "mem_critical":95.0,
                    "disk_warning":80.0, "disk_critical":95.0
                },
                "stats": server_stats_fixture()
            }
        }));
        Mock::given(method("POST"))
            .and(path("/read"))
            .and(header("x-api-key", "read-key"))
            .and(header("x-api-secret", "read-secret"))
            .respond_with(move |request: &wiremock::Request| {
                let payload: serde_json::Value = request.body_json().expect("read envelope");
                assert_eq!(payload["type"], "ListServers");
                assert_eq!(payload["params"]["query"], json!({}));
                let visible = if payload["params"]["limit"] == 0 {
                    &resources[..]
                } else {
                    &resources[..50]
                };
                ResponseTemplate::new(200).set_body_json(visible)
            })
            .expect(1)
            .mount(&server)
            .await;

        let result = client(&server).servers().await.expect("server list");
        assert_eq!(result.len(), 61);
        let target = result
            .iter()
            .find(|item| item.name == "z-target-server")
            .expect("target beyond the default resource page");
        assert_eq!(target.id, "server-last");
        assert_eq!(target.info.state, "Ok");
        assert_eq!(
            target.info.address.as_deref(),
            Some("https://10.23.4.5:8120")
        );
        assert_eq!(
            target.info.external_address.as_deref(),
            Some("https://server.example.test")
        );
        assert_eq!(target.info.public_ip.as_deref(), Some("192.0.2.15"));
        assert_eq!(
            target.info.public_key.as_deref(),
            Some("SENTINEL_PUBLIC_KEY")
        );
        assert_eq!(target.info.region.as_deref(), Some("east"));
        assert_eq!(target.template, Some(true));
        assert_eq!(
            target.tags.as_ref().unwrap(),
            &["tag-production", "tag-east"]
        );
        assert_eq!(target.info.core_count, Some(4));
        assert_eq!(target.info.logical_core_count, Some(8));
        let stats = target.info.stats.as_ref().expect("Core statistics");
        assert_eq!(serde_json::to_value(stats).unwrap(), server_stats_fixture());
        assert_eq!(
            serde_json::to_value(target.info.alerting_thresholds.as_ref().unwrap()).unwrap()["cpu_critical"],
            json!(95.0)
        );
        assert_eq!(
            target.info.err.as_ref().unwrap().error,
            "dial address did not respond"
        );
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
    async fn get_update_preserves_author_and_configuration_documents_by_stable_id() {
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
                "logs": [{ "stdout": "separate command log" }],
                "current_toml": "SENTINEL_CONFIG",
                "prev_toml": "previous configuration document",
                "operator": "SENTINEL_USER",
                "end_ts": 100,
                "version": {"major": 2, "minor": 3, "patch": 3},
                "commit_hash": "deployed-commit",
                "other_data": "operation-specific facts"
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
        assert_eq!(operation.operator.as_deref(), Some("SENTINEL_USER"));
        assert_eq!(operation.current_toml.as_deref(), Some("SENTINEL_CONFIG"));
        assert_eq!(
            operation.prev_toml.as_deref(),
            Some("previous configuration document")
        );
        assert_eq!(operation.end_ts, Some(100));
        assert_eq!(operation.version.expect("reported version").patch, 3);
        assert_eq!(operation.commit_hash.as_deref(), Some("deployed-commit"));
        assert_eq!(
            operation.other_data.as_deref(),
            Some("operation-specific facts")
        );
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
                    "pre_deploy": { "path": "stacks/gateway", "command": "echo pre", "shell_mode": true },
                    "post_deploy": { "path": "", "command": "" },
                    "webhook_enabled": true,
                    "webhook_secret": "sentinel-hook",
                    "webhook_force_deploy": false,
                    "auto_pull": false,
                    "repo": "operator/services",
                    "registry_account": "registry-reference"
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
        assert_eq!(detail.config.pre_deploy.shell_mode, Some(true));
        assert_eq!(detail.config.facts.auto_pull, Some(false));
        assert_eq!(
            detail.config.facts.repo.as_deref(),
            Some("operator/services")
        );
        assert_eq!(
            detail.config.facts.registry_account.as_deref(),
            Some("registry-reference")
        );
        assert_eq!(
            detail.info.deployed_contents.expect("deployed")[0].contents,
            "deployed"
        );
        let remote = detail.info.remote_contents.expect("remote");
        assert_eq!(remote[0].contents, "latest");
        assert_eq!(remote[0].services.as_ref().unwrap(), &["api"]);
        assert_eq!(remote[0].requires.as_deref(), Some("Redeploy"));
    }

    #[tokio::test]
    async fn command_replacements_preserve_explicit_shell_mode_and_core_defaults() {
        for shell_mode in [Some(true), Some(false), None] {
            let server = MockServer::start().await;
            let mut command = json!({"path":"stacks/api", "command":"printf data"});
            if let Some(value) = shell_mode {
                command["shell_mode"] = json!(value);
            }
            Mock::given(method("POST"))
                .and(path("/write"))
                .and(body_json(json!({"type":"UpdateStack","params":{
                    "id":"stack-1", "config":{"pre_deploy":command}
                }})))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                .expect(1)
                .mount(&server)
                .await;
            client(&server)
                .update_stack(
                    "stack-1",
                    &StackConfigPatch {
                        pre_deploy: Some(SystemCommand {
                            path: "stacks/api".into(),
                            command: "printf data".into(),
                            shell_mode,
                        }),
                        ..StackConfigPatch::default()
                    },
                )
                .await
                .expect("typed command replacement");
        }
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
                shell_mode: None,
            }),
            ..StackConfigPatch::default()
        };
        assert!(matches!(
            client(&server)
                .update_stack("stack-1", &patch)
                .await
                .expect_err("unavailable write"),
            ApiError::Upstream(failure) if failure.status == 503 && failure.request == "UpdateStack"
        ));
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

            assert!(matches!(
                client(&server)
                    .execute(action, "gateway")
                    .await
                    .expect_err("unavailable mutation"),
                ApiError::Upstream(failure) if failure.status == 503
            ));
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
    async fn response_failures_preserve_status_named_request_and_bounded_diagnostics() {
        for (status, expected_calls) in [(401, 1), (403, 1), (404, 1), (409, 1), (500, 2)] {
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
            assert!(!error.to_string().contains("SENTINEL"));
            let ApiError::Upstream(mut failure) = error else {
                panic!("typed upstream failure")
            };
            assert_eq!(failure.status, status);
            assert_eq!(failure.request, "GetVersion");
            assert_eq!(
                serde_json::to_value(&failure.diagnostic.as_ref().unwrap().error).unwrap(),
                json!("SENTINEL_UPSTREAM_BODY")
            );
            assert!(failure.take_credential_document().is_none());
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

        for status in [401, 403, 404] {
            let rejected = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status).set_body_bytes(vec![
                    b'x';
                    MAXIMUM_RESPONSE_BYTES
                        + 1
                ]))
                .expect(1)
                .mount(&rejected)
                .await;
            let ApiError::Upstream(failure) = client(&rejected).version().await.unwrap_err() else {
                panic!("typed failure")
            };
            assert_eq!(failure.status, status);
            assert_eq!(failure.request, "GetVersion");
            assert_eq!(
                failure.diagnostic_unavailable,
                Some("response body exceeds byte bound")
            );
        }

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

    #[tokio::test]
    async fn interrupted_failure_bodies_keep_received_status_and_request() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for status in [401, 403, 404] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 4096];
                assert!(stream.read(&mut request).await.unwrap() > 0);
                stream.write_all(format!("HTTP/1.1 {status} Rejected\r\nContent-Length: 100\r\nConnection: close\r\n\r\npartial").as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            });
            let client = Client::new(
                Url::parse(&format!("http://{address}/")).unwrap(),
                Credentials::new("read-key".into(), "read-secret".into()).unwrap(),
                Duration::from_secs(2),
            )
            .unwrap();
            let ApiError::Upstream(failure) = client.version().await.unwrap_err() else {
                panic!("typed failure")
            };
            assert_eq!(failure.status, status);
            assert_eq!(failure.request, "GetVersion");
            assert_eq!(
                failure.diagnostic_unavailable,
                Some("response body interrupted")
            );
            server.await.unwrap();
        }
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
