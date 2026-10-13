//! Keep admission ownership alive for detached MCP handler work.

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use axum::{
    body::{Body, Bytes, HttpBody},
    extract::{Request, State},
    http::{StatusCode, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use http_body::{Frame, SizeHint};
use komodo_mcp::{KomodoMcp, execution::ExecutionBudget};
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, CustomRequest, CustomResult, ListToolsResult,
        PaginatedRequestParams, ServerInfo,
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

pub(crate) fn request_budget(request: &Request) -> Option<ExecutionBudget> {
    request
        .extensions()
        .get::<RequestLease>()
        .map(|lease| lease.budget.clone())
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
    let cancel_on_drop = cancellation.clone().drop_guard();
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
        response = next.run(request) => {
            if response.body().is_end_stream() { return response; }
            let (parts, inner) = response.into_parts();
            let budget = lease.budget.clone();
            Response::from_parts(parts, Body::new(LifetimeBody {
                inner, _lease: lease, _cancel_on_drop: cancel_on_drop,
                ended: Box::pin(async move { budget.ended().await; }),
            }))
        },
    }
}

struct LifetimeBody {
    inner: Body,
    _lease: RequestLease,
    _cancel_on_drop: tokio_util::sync::DropGuard,
    ended: Pin<Box<dyn Future<Output = ()> + Send>>,
}

impl HttpBody for LifetimeBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        if self.ended.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Some(Err(axum::Error::new(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "response request lifetime ended",
            )))));
        }
        Pin::new(&mut self.inner).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }
    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[derive(Clone)]
pub(crate) struct ScopedMcp {
    pub(crate) handler: KomodoMcp,
    pub(crate) files: Arc<crate::files::TransferFiles>,
}

impl ServerHandler for ScopedMcp {
    fn get_info(&self) -> ServerInfo {
        self.handler.get_info()
    }

    async fn list_tools(
        &self,
        params: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.handler.list_tools(params, context).await
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
        let owner = context
            .extensions
            .get::<Parts>()
            .and_then(|parts| parts.extensions.get::<crate::auth::IdentityPrincipal>())
            .map(|principal| principal.subject.clone())
            .ok_or_else(|| ErrorData::internal_error("verified file owner unavailable", None))?;
        self.handler
            .clone()
            .with_execution_budget(lease.budget.clone())
            .with_credential_files(owner, self.files.clone())
            .call_tool(params, context)
            .await
    }

    async fn on_custom_request(
        &self,
        request: CustomRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CustomResult, ErrorData> {
        let direction = match request.method.as_str() {
            "files/authorizeUpload" => "upload",
            "files/authorizeDownload" => "download",
            _ => {
                return Err(ErrorData::new(
                    rmcp::model::ErrorCode::METHOD_NOT_FOUND,
                    "unsupported native file method",
                    None,
                ));
            }
        };
        if !crate::files::supports_http(&context.meta, direction) {
            return Err(ErrorData::invalid_params(
                "native file authorization requires declared HTTP transfer support",
                None,
            ));
        }
        let parts = context
            .extensions
            .get::<Parts>()
            .ok_or_else(|| ErrorData::internal_error("file request context unavailable", None))?;
        let principal = parts
            .extensions
            .get::<crate::auth::IdentityPrincipal>()
            .ok_or_else(|| ErrorData::internal_error("verified file owner unavailable", None))?;
        let lease = parts
            .extensions
            .get::<RequestLease>()
            .ok_or_else(|| ErrorData::internal_error("request lifetime unavailable", None))?;
        lease.budget.check()?;
        // Host was verified by ingress authentication. The service itself serves HTTP;
        // no forwarded header or model-supplied URL selects the transfer origin.
        let host = parts
            .headers
            .get(axum::http::header::HOST)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ErrorData::internal_error("file transfer origin unavailable", None))?;
        let base = url::Url::parse(&format!("http://{host}/"))
            .map_err(|_| ErrorData::internal_error("file transfer origin unavailable", None))?;
        let value = if direction == "upload" {
            let params = request
                .params_as::<crate::files::UploadParams>()
                .map_err(|_| ErrorData::invalid_params("invalid upload metadata", None))?
                .ok_or_else(|| ErrorData::invalid_params("missing upload metadata", None))?;
            self.files
                .authorize_upload(&principal.subject, params, &base)?
        } else {
            let params = request
                .params_as::<crate::files::DownloadParams>()
                .map_err(|_| ErrorData::invalid_params("invalid download metadata", None))?
                .ok_or_else(|| ErrorData::invalid_params("missing download metadata", None))?;
            self.files
                .authorize_download(&principal.subject, &params, &base)?
        };
        Ok(CustomResult::new(value))
    }
}

#[cfg(test)]
mod tests {
    use axum::{Extension, Router, body::Body, middleware, routing::post};
    use tokio::sync::Notify;
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn response_body_retains_admission_and_stops_at_its_deadline() {
        let limits = RequestLimits::new(1, Duration::from_millis(50), CancellationToken::new());
        let router = Router::new()
            .route("/", post(|| async { "bounded response" }))
            .layer(middleware::from_fn_with_state(limits, admit));
        let request = || {
            Request::builder()
                .method("POST")
                .uri("/")
                .body(Body::empty())
                .unwrap()
        };
        let response = router.clone().oneshot(request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            router.clone().oneshot(request()).await.unwrap().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(
            axum::body::to_bytes(response.into_body(), 1024)
                .await
                .is_err()
        );
        assert_eq!(
            router.oneshot(request()).await.unwrap().status(),
            StatusCode::OK
        );
    }

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
