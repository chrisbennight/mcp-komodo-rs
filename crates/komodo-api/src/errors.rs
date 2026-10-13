//! Bounded upstream diagnostics with an explicit known-credential boundary.

use std::fmt;

use zeroize::{Zeroize, Zeroizing};

use crate::ReportedError;

/// Facts returned by an unsuccessful named Core request.
#[derive(Clone, PartialEq, Eq)]
pub struct UpstreamFailure {
    pub status: u16,
    pub request: &'static str,
    pub diagnostic: Option<ReportedError>,
    original: Option<Zeroizing<Vec<u8>>>,
}

impl fmt::Debug for UpstreamFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpstreamFailure")
            .field("status", &self.status)
            .field("request", &self.request)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for UpstreamFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.status {
            401 => "Komodo authentication failed",
            403 => "Komodo denied the operation",
            404 => "the requested Komodo resource was not found",
            500..=599 => "Komodo is temporarily unavailable",
            _ => "Komodo rejected the named request",
        })
    }
}

impl UpstreamFailure {
    pub(crate) fn from_response(
        status: u16,
        request: &'static str,
        bytes: Vec<u8>,
        credentials: &[&str],
    ) -> Self {
        let mut diagnostic = if bytes.is_empty() {
            None
        } else {
            serde_json::from_slice::<ReportedError>(&bytes)
                .ok()
                .or_else(|| {
                    String::from_utf8(bytes.clone())
                        .ok()
                        .map(|error| ReportedError {
                            error,
                            trace: Vec::new(),
                        })
                })
        };
        let mut credential_present = false;
        if let Some(diagnostic) = &mut diagnostic {
            for text in std::iter::once(&mut diagnostic.error).chain(&mut diagnostic.trace) {
                let mut remaining = text.as_str();
                let mut replacement = String::new();
                let mut changed = false;
                while let Some((offset, credential)) = credentials
                    .iter()
                    .filter(|value| !value.is_empty())
                    .filter_map(|credential| {
                        remaining
                            .find(credential)
                            .map(|offset| (offset, *credential))
                    })
                    .min_by_key(|(offset, credential)| {
                        (*offset, std::cmp::Reverse(credential.len()))
                    })
                {
                    replacement.push_str(&remaining[..offset]);
                    remaining = &remaining[offset + credential.len()..];
                    credential_present = true;
                    changed = true;
                }
                if !changed {
                    continue;
                }
                replacement.push_str(remaining);
                text.zeroize();
                *text = replacement;
            }
        }
        Self {
            status,
            request,
            diagnostic,
            original: credential_present.then(|| Zeroizing::new(bytes)),
        }
    }

    /// Exact original response when it contains an explicitly known literal credential.
    /// Its bytes must only be published through the native file-transfer boundary.
    pub fn take_credential_document(&mut self) -> Option<Zeroizing<Vec<u8>>> {
        self.original.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn diagnostics_preserve_ordinary_text_and_only_transfer_known_literal_values() {
        let ordinary = json!({"error":"Cannot bind 10.20.0.1; password=ordinary-log-text",
            "trace":["secret reference gateway-token is unavailable", "retry service api"]});
        let mut failure = UpstreamFailure::from_response(
            409,
            "GetVersion",
            serde_json::to_vec(&ordinary).unwrap(),
            &["synthetic-explicit-key", "synthetic-explicit-token"],
        );
        assert_eq!(
            serde_json::to_value(failure.diagnostic.as_ref().unwrap()).unwrap(),
            ordinary
        );
        assert!(failure.take_credential_document().is_none());

        let bytes = serde_json::to_vec(&json!({"error":"x".repeat(4096), "trace":[]})).unwrap();
        let mut short = UpstreamFailure::from_response(400, "UpdateStack", bytes.clone(), &["x"]);
        assert_eq!(short.diagnostic.as_ref().unwrap().error, "");
        assert!(
            serde_json::to_vec(short.diagnostic.as_ref().unwrap())
                .unwrap()
                .len()
                <= bytes.len()
        );
        assert_eq!(*short.take_credential_document().unwrap(), bytes);

        let original =
            serde_json::to_vec(&json!({"error":"Token rejected: synthetic-explicit-token",
            "trace":["Address 10.20.0.1 could not store synthetic-explicit-key"]}))
            .unwrap();
        let mut failure = UpstreamFailure::from_response(
            503,
            "UpdateStack",
            original.clone(),
            &["synthetic-explicit-key", "synthetic-explicit-token"],
        );
        let diagnostic = serde_json::to_value(failure.diagnostic.as_ref().unwrap())
            .unwrap()
            .to_string();
        assert!(diagnostic.contains("10.20.0.1"));
        assert!(!diagnostic.contains("synthetic-explicit-key"));
        assert!(!diagnostic.contains("synthetic-explicit-token"));
        assert!(!format!("{failure:?}").contains("synthetic-explicit"));
        assert!(!failure.to_string().contains("synthetic-explicit"));
        assert_eq!(*failure.take_credential_document().unwrap(), original);
        assert!(failure.take_credential_document().is_none());
    }
}
