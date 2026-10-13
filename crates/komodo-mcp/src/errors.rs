//! MCP diagnostic projection and uncertain mutation reconciliation.

use komodo_api::ApiError;
use rmcp::model::ErrorCode;
use serde_json::json;

use super::{KomodoMcp, McpError};

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct CredentialFailureData {
    upstream: UpstreamDiagnostic,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_safe: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation_id_available: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reconcile_with: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    operator_verification_required: Option<bool>,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct UpstreamDiagnostic {
    http_status: u16,
    request: String,
    diagnostic: Option<komodo_api::errors::Diagnostic>,
    diagnostic_unavailable: Option<String>,
    untrusted: bool,
    credential_values_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic_file: Option<super::files::FileValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic_file_unavailable: Option<String>,
}

impl KomodoMcp {
    pub(super) fn api_error(&self, mut error: ApiError) -> McpError {
        let message = error.to_string();
        let ApiError::Upstream(failure) = &mut error else {
            return McpError::internal_error(message, None);
        };
        let mut data = json!({
            "upstream": {"httpStatus":failure.status, "request":failure.request,
                "diagnostic":failure.diagnostic, "diagnosticUnavailable":failure.diagnostic_unavailable,
                "credentialValuesPresent":failure.credential_values_present, "untrusted":true}
        });
        if let Some(bytes) = failure.take_credential_document() {
            if let Some((owner, files)) = &self.credential_files {
                match files.publish(owner, "komodo-diagnostic.txt", "text/plain", bytes) {
                    Ok(file) => data["upstream"]["diagnosticFile"] = json!(file),
                    Err(_) => {
                        data["upstream"]["diagnosticFileUnavailable"] =
                            json!("native file capacity unavailable");
                    }
                }
            } else {
                data["upstream"]["diagnosticFileUnavailable"] =
                    json!("native file transfer required");
            }
        }
        McpError::internal_error(message, Some(data))
    }

    pub(super) fn mutation_error(
        &self,
        error: ApiError,
        target: &str,
        read_tool: &'static str,
    ) -> McpError {
        let uncertain = error.mutation_outcome_unknown();
        let mut result = self.api_error(error);
        if uncertain {
            result.message =
                "Komodo mutation outcome is unknown; reconcile before any new write".into();
            let data = result.data.get_or_insert_with(|| json!({}));
            for (key, value) in json!({
                "outcome":"unknown", "target":target, "retrySafe":false,
                "operationIdAvailable":false, "reconcileWith":read_tool,
                "operatorVerificationRequired":true
            })
            .as_object()
            .expect("fixed reconciliation object")
            {
                data[key] = value.clone();
            }
        }
        result
    }

    pub(super) fn operation_lookup_error(&self, error: ApiError) -> McpError {
        let missing = error.resource_not_found();
        let mut result = self.api_error(error);
        if missing {
            result.code = ErrorCode::INVALID_PARAMS;
            result.message = "operation not found".into();
        }
        result
    }
}
