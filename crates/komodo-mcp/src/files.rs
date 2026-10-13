//! Model-visible file metadata and the credential byte-transfer boundary.

use rmcp::ErrorData;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// Native MCP file metadata; credential contents are absent.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileValue {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<FileDigest>,
}

/// Optional integrity metadata supplied by the native transfer client.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileDigest {
    pub algorithm: String,
    pub value: String,
}

/// A request-bound credential file service supplied by the transport.
/// Ownership is the verified principal's subject, never a model argument.
pub trait CredentialFiles: Send + Sync {
    /// Publish literal credential bytes for an authorized native download.
    ///
    /// # Errors
    /// Returns an error when the bounded transfer store cannot admit the file.
    fn publish(
        &self,
        owner: &str,
        name: &'static str,
        mime_type: &'static str,
        bytes: Zeroizing<Vec<u8>>,
    ) -> Result<FileValue, ErrorData>;

    /// Consume one completed upload owned by the request's verified principal.
    ///
    /// # Errors
    /// Returns an error for an unavailable, expired, incomplete, or foreign file.
    fn take(&self, owner: &str, uri: &str) -> Result<Zeroizing<Vec<u8>>, ErrorData>;
}
