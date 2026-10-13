//! MCP diagnostic projection and uncertain mutation reconciliation.

use komodo_api::ApiError;
use rmcp::model::ErrorCode;
use serde_json::json;

use super::{KomodoMcp, McpError};

impl KomodoMcp {
    pub(super) fn api_error(&self, mut error: ApiError) -> McpError {
        let message = error.to_string();
        let ApiError::Upstream(failure) = &mut error else {
            return McpError::internal_error(message, None);
        };
        let mut data = json!({
            "upstream": {"httpStatus":failure.status, "request":failure.request,
                "diagnostic":failure.diagnostic, "untrusted":true}
        });
        if let Some(bytes) = failure.take_credential_document() {
            data["upstream"]["credentialValuesPresent"] = json!(true);
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
        let uncertain = matches!(
            &error,
            ApiError::Unavailable | ApiError::IncompatibleResponse | ApiError::ResponseTooLarge
        ) || matches!(&error, ApiError::Upstream(failure) if !matches!(failure.status, 401 | 403 | 404));
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
        let missing = matches!(&error, ApiError::NotFound)
            || matches!(&error, ApiError::Upstream(failure) if failure.status == 404);
        let mut result = self.api_error(error);
        if missing {
            result.code = ErrorCode::INVALID_PARAMS;
            result.message = "operation not found".into();
        }
        result
    }
}
