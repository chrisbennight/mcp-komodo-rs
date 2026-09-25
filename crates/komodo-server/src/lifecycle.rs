//! Keep admission ownership alive for detached MCP handler work.

use std::{sync::Arc, time::Duration};

use axum::{
    extract::{Request, State},
    http::{StatusCode, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use komodo_mcp::{KomodoMcp, execution::ExecutionBudget};
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams, ServerInfo,
    },
    service::RequestContext,
};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(crate) struct RequestLimits {
    permits: Arc<Semaphore>,
    timeout: Duration,
    shutdown: CancellationToken,
}

impl RequestLimits {
    pub(crate) fn new(limit: usize, timeout: Duration, shutdown: CancellationToken) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(limit)),
            timeout,
            shutdown,
        }
    }
}

#[derive(Clone)]
struct RequestLease {
    _permit: Arc<OwnedSemaphorePermit>,
    budget: ExecutionBudget,
}

pub(crate) async fn admit(
    State(limits): State<RequestLimits>,
    mut request: Request,
    next: Next,
) -> Response {
    let deadline = Instant::now() + limits.timeout;
    if limits.shutdown.is_cancelled() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    // Refuse excess work rather than building an unbounded in-memory queue.
    let Ok(permit) = Arc::clone(&limits.permits).try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let cancellation = limits.shutdown.child_token();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    let lease = RequestLease {
        _permit: Arc::new(permit),
        budget: ExecutionBudget::new(deadline, cancellation.clone()),
    };
    request.extensions_mut().insert(lease.clone());
    // The SDK copies HTTP parts into the detached request context. Its lease
    // and this transport lease share ownership until both have finished.
    tokio::select! {
        biased;
        () = cancellation.cancelled() => (
            StatusCode::SERVICE_UNAVAILABLE,
            "request ended; reconcile any submitted mutation before another write",
        ).into_response(),
        () = tokio::time::sleep_until(deadline) => (
            StatusCode::REQUEST_TIMEOUT,
            "request deadline exceeded; reconcile any submitted mutation before another write",
        ).into_response(),
        response = next.run(request) => response,
    }
}

#[derive(Clone)]
pub(crate) struct ScopedMcp(pub(crate) KomodoMcp);

impl ServerHandler for ScopedMcp {
    fn get_info(&self) -> ServerInfo {
        self.0.get_info()
    }

    async fn list_tools(
        &self,
        params: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.0.list_tools(params, context).await
    }

    async fn call_tool(
        &self,
        params: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let lease = context
            .extensions
            .get::<Parts>()
            .and_then(|parts| parts.extensions.get::<RequestLease>())
            .cloned()
            .ok_or_else(|| ErrorData::internal_error("request lifetime unavailable", None))?;
        self.0
            .clone()
            .with_execution_budget(lease.budget.clone())
            .call_tool(params, context)
            .await
    }
}

#[cfg(test)]
mod tests {
    use axum::{Extension, Router, body::Body, middleware, routing::post};
    use tokio::sync::Notify;
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn detached_work_retains_admission_after_http_timeout() {
        let limits = RequestLimits::new(1, Duration::from_millis(50), CancellationToken::new());
        let release = Arc::new(Notify::new());
        let released = Arc::new(Notify::new());
        let route_release = Arc::clone(&release);
        let route_released = Arc::clone(&released);
        let router = Router::new()
            .route(
                "/hold",
                post(move |Extension(lease): Extension<RequestLease>| {
                    let release = Arc::clone(&route_release);
                    let released = Arc::clone(&route_released);
                    async move {
                        tokio::spawn(async move {
                            release.notified().await;
                            drop(lease);
                            released.notify_one();
                        });
                        std::future::pending::<StatusCode>().await
                    }
                }),
            )
            .route("/ok", post(|| async { StatusCode::NO_CONTENT }))
            .layer(middleware::from_fn_with_state(limits, admit));
        let request = |uri| {
            Request::builder()
                .method("POST")
                .uri(uri)
                .body(Body::empty())
                .unwrap()
        };
        assert_eq!(
            router
                .clone()
                .oneshot(request("/hold"))
                .await
                .unwrap()
                .status(),
            StatusCode::REQUEST_TIMEOUT
        );
        assert_eq!(
            router
                .clone()
                .oneshot(request("/ok"))
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(1), released.notified())
            .await
            .unwrap();
        assert_eq!(
            router.oneshot(request("/ok")).await.unwrap().status(),
            StatusCode::NO_CONTENT
        );
    }
}
