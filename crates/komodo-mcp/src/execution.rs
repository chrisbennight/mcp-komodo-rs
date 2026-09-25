//! A request deadline shared by transport and tool execution.

use rmcp::ErrorData;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// Cancellation and deadline for one invocation, including resource resolution.
#[derive(Clone)]
pub struct ExecutionBudget {
    deadline: Instant,
    cancellation: CancellationToken,
}

impl ExecutionBudget {
    /// Use the ingress deadline rather than starting a new handler timeout.
    #[must_use]
    pub fn new(deadline: Instant, cancellation: CancellationToken) -> Self {
        Self {
            deadline,
            cancellation,
        }
    }

    /// Refuse new work once the caller's lifetime has ended.
    ///
    /// # Errors
    ///
    /// Returns a content-free cancellation error on cancellation or expiry.
    pub fn check(&self) -> Result<(), ErrorData> {
        if self.cancellation.is_cancelled() || Instant::now() >= self.deadline {
            return Err(ended());
        }
        Ok(())
    }

    pub(crate) async fn ended(&self) {
        tokio::select! {
            () = self.cancellation.cancelled() => {},
            () = tokio::time::sleep_until(self.deadline) => {},
        }
    }
}

pub(crate) fn ended() -> ErrorData {
    ErrorData::internal_error(
        "request cancelled or deadline exceeded; a submitted mutation may have taken effect; reconcile with its documented read before another write",
        Some(serde_json::json!({
            "outcome": "unknown",
            "retrySafe": false,
            "operatorVerificationRequired": true
        })),
    )
}
