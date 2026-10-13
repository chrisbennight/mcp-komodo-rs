//! Ordinary stack configuration and Compose provenance reported by Core.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stack settings beyond the original content and webhook projections.
/// Missing upstream fields remain null rather than becoming guessed defaults.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct StackConfigurationFacts {
    pub swarm_id: Option<String>,
    pub server_id: Option<String>,
    pub links: Option<Vec<String>>,
    pub project_name: Option<String>,
    pub auto_pull: Option<bool>,
    pub run_build: Option<bool>,
    pub poll_for_updates: Option<bool>,
    pub auto_update: Option<bool>,
    pub auto_update_all_services: Option<bool>,
    pub auto_update_skip_services: Option<Vec<String>>,
    pub destroy_before_deploy: Option<bool>,
    pub skip_secret_interp: Option<bool>,
    pub linked_repo: Option<String>,
    pub git_provider: Option<String>,
    pub git_https: Option<bool>,
    pub git_account: Option<String>,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub clone_path: Option<String>,
    pub reclone: Option<bool>,
    pub additional_env_files: Option<Vec<AdditionalEnvFile>>,
    pub config_files: Option<Vec<StackFileDependency>>,
    pub send_alerts: Option<bool>,
    pub registry_provider: Option<String>,
    pub registry_account: Option<String>,
    pub extra_args: Option<Vec<String>>,
    pub build_extra_args: Option<Vec<String>>,
    pub compose_cmd_wrapper: Option<String>,
    pub compose_cmd_wrapper_include: Option<Vec<String>>,
    pub ignore_services: Option<Vec<String>>,
}

/// Core accepts both legacy paths and structured environment-file settings.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(untagged)]
pub enum AdditionalEnvFile {
    Path(String),
    Settings { path: String, track: Option<bool> },
}

/// Core accepts both legacy paths and structured file dependencies.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(untagged)]
pub enum StackFileDependency {
    Path(String),
    Settings {
        path: String,
        services: Option<Vec<String>>,
        requires: Option<String>,
    },
}

/// Service identity retained with cached or deployed Compose contents.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct StackServiceNames {
    pub service_name: Option<String>,
    pub container_name: Option<String>,
    pub image: Option<String>,
    pub image_digest: Option<String>,
}

/// Stack cache and deployment facts accompanying the selected file page.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct StackComposeProvenance {
    pub missing_files: Option<Vec<String>>,
    pub deployed_project_name: Option<String>,
    pub deployed_hash: Option<String>,
    pub deployed_message: Option<String>,
    pub deployed_services: Option<Vec<StackServiceNames>>,
    pub deployed_config: Option<String>,
    pub latest_services: Option<Vec<StackServiceNames>>,
    pub remote_errors: Option<Vec<super::ComposeFile>>,
    pub latest_hash: Option<String>,
    pub latest_message: Option<String>,
}
