//! Bounded upstream diagnostics with an explicit known-credential boundary.

use std::fmt;

use zeroize::{Zeroize, Zeroizing};

use crate::ReportedError;

/// Text segments remain separate across an explicitly removed credential.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum DiagnosticText {
    Complete(String),
    Segments { segments: Vec<String> },
}

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct Diagnostic {
    pub error: DiagnosticText,
    pub trace: Vec<DiagnosticText>,
}

/// Facts returned by an unsuccessful named Core request.
#[derive(Clone, PartialEq, Eq)]
pub struct UpstreamFailure {
    pub status: u16,
    pub request: &'static str,
    pub diagnostic: Option<Diagnostic>,
    pub diagnostic_unavailable: Option<&'static str>,
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
        let diagnostic = if bytes.is_empty() {
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
        let mut fragmented = false;
        let diagnostic = diagnostic.map(|diagnostic| Diagnostic {
            error: project_text(
                diagnostic.error,
                credentials,
                &mut credential_present,
                &mut fragmented,
            ),
            trace: diagnostic
                .trace
                .into_iter()
                .map(|text| {
                    project_text(text, credentials, &mut credential_present, &mut fragmented)
                })
                .collect(),
        });
        Self {
            status,
            request,
            diagnostic: (!fragmented).then_some(diagnostic).flatten(),
            diagnostic_unavailable: fragmented.then_some(
                "credential-separated text exceeds inline segment bound; use original file",
            ),
            original: credential_present.then(|| Zeroizing::new(bytes)),
        }
    }

    pub(crate) fn without_body(status: u16, request: &'static str, reason: &'static str) -> Self {
        Self {
            status,
            request,
            diagnostic: None,
            diagnostic_unavailable: Some(reason),
            original: None,
        }
    }

    /// Exact original response when it contains an explicitly known literal credential.
    /// Its bytes must only be published through the native file-transfer boundary.
    pub fn take_credential_document(&mut self) -> Option<Zeroizing<Vec<u8>>> {
        self.original.take()
    }
}

fn project_text(
    mut text: String,
    credentials: &[&str],
    present: &mut bool,
    fragmented: &mut bool,
) -> DiagnosticText {
    // Cache each next match, including absence: each credential searches disjoint
    // suffixes, so a short repeated value cannot repeatedly scan absent values.
    let mut matches: Vec<_> = credentials
        .iter()
        .filter(|value| !value.is_empty())
        .map(|value| (*value, text.find(value)))
        .collect();
    let mut cursor = 0;
    let mut segments = Vec::new();
    let mut changed = false;
    while let Some((start, length)) = matches
        .iter()
        .filter_map(|(value, position)| position.map(|start| (start, value.len())))
        .min_by_key(|(start, length)| (*start, std::cmp::Reverse(*length)))
    {
        if start > cursor {
            if segments.len() == 256 {
                *fragmented = true;
                break;
            }
            segments.push(text[cursor..start].to_owned());
        }
        changed = true;
        *present = true;
        cursor = start + length;
        for (value, position) in &mut matches {
            if position.is_some_and(|start| start < cursor) {
                *position = text[cursor..].find(*value).map(|offset| cursor + offset);
            }
        }
    }
    if !changed {
        return DiagnosticText::Complete(text);
    }
    if cursor < text.len() && !*fragmented {
        if segments.len() == 256 {
            *fragmented = true;
        } else {
            segments.push(text[cursor..].to_owned());
        }
    }
    text.zeroize();
    DiagnosticText::Segments { segments }
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
        assert_eq!(
            serde_json::to_value(&short.diagnostic.as_ref().unwrap().error).unwrap(),
            json!({"segments":[]})
        );
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

    #[test]
    fn projection_cannot_join_fragments_into_a_credential_and_work_is_bounded() {
        let mut failure = UpstreamFailure::from_response(
            400,
            "UpdateStack",
            br#"{"error":"apiTOKENkey","trace":[]}"#.to_vec(),
            &["apikey", "TOKEN"],
        );
        assert_eq!(
            serde_json::to_value(failure.diagnostic.take().unwrap()).unwrap(),
            json!({"error":{"segments":["api", "key"]},"trace":[]})
        );
        assert!(failure.take_credential_document().is_some());
        let text = "x".repeat(2 * 1024 * 1024 - 100);
        let start = std::time::Instant::now();
        let failure = UpstreamFailure::from_response(
            400,
            "UpdateStack",
            serde_json::to_vec(&json!({"error":text,"trace":[]})).unwrap(),
            &["absent-api-key", "absent-api-secret", "x"],
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(
            serde_json::to_value(failure.diagnostic.unwrap()).unwrap()["error"],
            json!({"segments":[]})
        );
        let mut fragmented = UpstreamFailure::from_response(
            400,
            "UpdateStack",
            serde_json::to_vec(&json!({"error":"ordinaryx".repeat(1000),"trace":[]})).unwrap(),
            &["x"],
        );
        assert!(fragmented.diagnostic.is_none());
        assert!(fragmented.diagnostic_unavailable.is_some());
        assert!(fragmented.take_credential_document().is_some());
    }

    #[test]
    fn received_status_preserves_mutation_and_lookup_outcomes_without_a_body() {
        for status in [401, 403, 404, 409, 500, 503] {
            let error = crate::ApiError::Upstream(Box::new(UpstreamFailure::without_body(
                status,
                "GetUpdate",
                "response body interrupted",
            )));
            assert_eq!(error.resource_not_found(), status == 404);
            assert_eq!(
                error.mutation_outcome_unknown(),
                !matches!(status, 401 | 403 | 404)
            );
        }
    }
}
