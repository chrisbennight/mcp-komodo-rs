//! Bounded upstream diagnostics with an explicit known-credential boundary.

use std::fmt;

use aho_corasick::{AhoCorasick, MatchKind};
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
    pub credential_values_present: Option<bool>,
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
        let mut unavailable = None;
        let diagnostic = if bytes.is_empty() {
            unavailable = Some("upstream response body is empty");
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
        if diagnostic.is_none() && unavailable.is_none() {
            unavailable = Some("upstream response body has unsupported encoding");
        }
        let inspected = diagnostic.is_some();
        let Ok(matcher) = AhoCorasick::builder()
            .match_kind(MatchKind::LeftmostLongest)
            .build(credentials.iter().filter(|value| !value.is_empty()))
        else {
            return Self {
                status,
                request,
                diagnostic: None,
                diagnostic_unavailable: Some(
                    "credential projection unavailable; use original file",
                ),
                credential_values_present: None,
                original: Some(Zeroizing::new(bytes)),
            };
        };
        let mut credential_present = false;
        let mut fragmented = false;
        let diagnostic = diagnostic.map(|diagnostic| Diagnostic {
            error: project_text(
                diagnostic.error,
                &matcher,
                &mut credential_present,
                &mut fragmented,
            ),
            trace: diagnostic
                .trace
                .into_iter()
                .map(|text| project_text(text, &matcher, &mut credential_present, &mut fragmented))
                .collect(),
        });
        Self {
            status,
            request,
            diagnostic: (!fragmented).then_some(diagnostic).flatten(),
            diagnostic_unavailable: fragmented
                .then_some(
                    "credential-separated text exceeds inline segment bound; use original file",
                )
                .or(unavailable),
            original: credential_present.then(|| Zeroizing::new(bytes)),
            credential_values_present: inspected.then_some(credential_present),
        }
    }

    pub(crate) fn without_body(status: u16, request: &'static str, reason: &'static str) -> Self {
        Self {
            status,
            request,
            diagnostic: None,
            diagnostic_unavailable: Some(reason),
            credential_values_present: None,
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
    matcher: &AhoCorasick,
    present: &mut bool,
    fragmented: &mut bool,
) -> DiagnosticText {
    let mut cursor = 0;
    let mut segments = Vec::new();
    let mut changed = false;
    // Leftmost-longest matching preserves credential-free text segments without
    // rebuilding or rescanning long overlapping patterns after each match.
    for found in matcher.find_iter(text.as_str()) {
        let start = found.start();
        if start > cursor {
            if segments.len() == 256 {
                *fragmented = true;
                break;
            }
            segments.push(text[cursor..start].to_owned());
        }
        changed = true;
        *present = true;
        cursor = found.end();
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
            serde_json::to_value(failure.diagnostic.as_ref().unwrap()).unwrap()["error"],
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

    #[test]
    fn overlapping_periodic_credentials_have_bounded_work_and_no_inline_value() {
        let long = "ba".repeat(131_072);
        let text = "ab".repeat(1_000_000);
        assert!(text[1..].starts_with(&long));
        let body = serde_json::to_vec(&json!({"error":text,"trace":[]})).unwrap();
        let start = std::time::Instant::now();
        let mut failure =
            UpstreamFailure::from_response(400, "UpdateStack", body.clone(), &["ab", &long]);
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(
            serde_json::to_value(failure.diagnostic.as_ref().unwrap()).unwrap()["error"],
            json!({"segments":[]})
        );
        assert_eq!(*failure.take_credential_document().unwrap(), body);
        for (bytes, reason) in [
            (vec![], "upstream response body is empty"),
            (vec![255], "upstream response body has unsupported encoding"),
        ] {
            let failure =
                UpstreamFailure::from_response(400, "GetVersion", bytes, &["actual-literal-value"]);
            assert!(failure.diagnostic.is_none());
            assert_eq!(failure.diagnostic_unavailable, Some(reason));
        }
    }
}
