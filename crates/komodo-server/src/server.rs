use std::sync::Arc;

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde::Serialize;
use tokio_util::sync::CancellationToken;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::{timeout::TimeoutLayer, trace::TraceLayer};

use komodo_mcp::KomodoMcp;

use crate::{
    auth::{IdentityVerifier, IngressAuth, require_gateway},
    config::Settings,
};

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

/// Compose the independent health endpoint and authenticated MCP service.
///
/// # Errors
///
/// Returns an error when the identity verifier cannot be constructed safely.
pub fn build_router(
    settings: &Settings,
    handler: KomodoMcp,
    cancellation: &CancellationToken,
) -> Result<Router, crate::auth::AuthConfigError> {
    let verifier = IdentityVerifier::new(settings.identity.clone())?;
    let auth = IngressAuth::new(
        Arc::clone(&settings.bearers),
        verifier,
        settings.allowed_hosts.clone(),
        settings.allowed_origins.clone(),
    );
    let allowed_hosts = settings.allowed_hosts.clone();
    let allowed_origins = settings.allowed_origins.clone();
    let service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_cancellation_token(cancellation.child_token())
            .with_allowed_hosts(allowed_hosts)
            .with_allowed_origins(allowed_origins)
            .with_stateful_mode(false)
            .with_json_response(true),
    );

    let mcp = Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(
            settings.max_body_bytes,
            enforce_body_limit,
        ))
        .layer(middleware::from_fn_with_state(auth, require_gateway))
        .layer(ConcurrencyLimitLayer::new(settings.max_concurrent_requests))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            settings.request_timeout,
        ));

    Ok(Router::new()
        .route("/healthz", get(healthz))
        .merge(mcp)
        .layer(TraceLayer::new_for_http()))
}

async fn enforce_body_limit(State(limit): State<usize>, request: Request, next: Next) -> Response {
    let (parts, body) = request.into_parts();
    let Ok(bytes) = to_bytes(body, limit).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

async fn healthz() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "ok",
            version: env!("CARGO_PKG_VERSION"),
        }),
    )
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
        middleware,
        routing::post,
    };
    use komodo_api::{
        Action, ApiError, ApiFuture, BuildInfo, Credentials, DeploymentInfo, KomodoApi, Log,
        MutationReceipt, OperationItem, OperationPage, RepoInfo, ResourceListItem, ServerInfo,
        StackConfigPatch, StackDetail, StackInfo,
    };
    use komodo_mcp::KomodoMcp;
    use tokio_util::sync::CancellationToken;
    use tower::ServiceExt;
    use url::Url;

    use crate::{
        auth::{GatewayBearers, IdentityVerifierSettings},
        config::Settings,
        server::{build_router, enforce_body_limit},
    };

    struct UnavailableApi;

    impl KomodoApi for UnavailableApi {
        fn version(&self) -> ApiFuture<'_, String> {
            Box::pin(async { Err(ApiError::Unavailable) })
        }

        fn servers(&self) -> ApiFuture<'_, Vec<ResourceListItem<ServerInfo>>> {
            Box::pin(async { Err(ApiError::Unavailable) })
        }

        fn stacks(&self) -> ApiFuture<'_, Vec<ResourceListItem<StackInfo>>> {
            Box::pin(async { Err(ApiError::Unavailable) })
        }

        fn deployments(&self) -> ApiFuture<'_, Vec<ResourceListItem<DeploymentInfo>>> {
            Box::pin(async { Err(ApiError::Unavailable) })
        }

        fn builds(&self) -> ApiFuture<'_, Vec<ResourceListItem<BuildInfo>>> {
            Box::pin(async { Err(ApiError::Unavailable) })
        }

        fn repos(&self) -> ApiFuture<'_, Vec<ResourceListItem<RepoInfo>>> {
            Box::pin(async { Err(ApiError::Unavailable) })
        }

        fn operations(&self, _page: u32) -> ApiFuture<'_, OperationPage> {
            Box::pin(async { Err(ApiError::Unavailable) })
        }

        fn update<'a>(&'a self, _operation_id: &'a str) -> ApiFuture<'a, OperationItem> {
            Box::pin(async { Err(ApiError::Unavailable) })
        }

        fn stack_detail<'a>(&'a self, _selector: &'a str) -> ApiFuture<'a, StackDetail> {
            Box::pin(async { Err(ApiError::Unavailable) })
        }

        fn stack_log<'a>(
            &'a self,
            _selector: &'a str,
            _services: &'a [String],
            _tail: u64,
            _timestamps: bool,
        ) -> ApiFuture<'a, Log> {
            Box::pin(async { Err(ApiError::Unavailable) })
        }

        fn update_logs<'a>(&'a self, _operation_id: &'a str) -> ApiFuture<'a, Vec<Log>> {
            Box::pin(async { Err(ApiError::Unavailable) })
        }

        fn update_stack<'a>(
            &'a self,
            _id: &'a str,
            _patch: &'a StackConfigPatch,
        ) -> ApiFuture<'a, ()> {
            Box::pin(async { Err(ApiError::Unavailable) })
        }

        fn write_stack_file<'a>(
            &'a self,
            _selector: &'a str,
            _file_path: &'a str,
            _contents: &'a str,
        ) -> ApiFuture<'a, MutationReceipt> {
            Box::pin(async { Err(ApiError::Unavailable) })
        }

        fn execute<'a>(
            &'a self,
            _action: Action,
            _selector: &'a str,
        ) -> ApiFuture<'a, MutationReceipt> {
            Box::pin(async { Err(ApiError::Unavailable) })
        }
    }

    fn settings() -> Settings {
        Settings {
            host: "127.0.0.1".into(),
            port: 8000,
            log_level: "info".into(),
            upstream_url: Url::parse("http://core:9120/").expect("upstream URL"),
            upstream_timeout: Duration::from_secs(2),
            read_credentials: Credentials::new("read".into(), "secret".into())
                .expect("read credentials"),
            admin_credentials: Credentials::new("admin".into(), "secret".into())
                .expect("admin credentials"),
            allowed_hosts: vec!["localhost".into()],
            allowed_origins: Vec::new(),
            request_timeout: Duration::from_secs(2),
            max_concurrent_requests: 4,
            max_body_bytes: 16 * 1024,
            bearers: Arc::new(
                GatewayBearers::new("0123456789abcdef0123456789abcdef".into(), None)
                    .expect("bearer"),
            ),
            identity: IdentityVerifierSettings {
                jwks_url: Url::parse("http://127.0.0.1:65534/jwks").expect("JWKS URL"),
                issuer: "https://gateway.test".into(),
                actor: "gateway.example.com".into(),
                request_timeout: Duration::from_secs(1),
                cache_ttl: Duration::from_mins(1),
            },
        }
    }

    #[tokio::test]
    async fn liveness_is_independent_and_mcp_fails_closed_without_credentials() {
        let api: Arc<dyn KomodoApi> = Arc::new(UnavailableApi);
        let cancellation = CancellationToken::new();
        let router = build_router(
            &settings(),
            KomodoMcp::new(Arc::clone(&api), api),
            &cancellation,
        )
        .expect("router");

        let health = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("health request"),
            )
            .await
            .expect("health response");
        assert_eq!(health.status(), StatusCode::OK);
        let health_body = to_bytes(health.into_body(), 1024)
            .await
            .expect("health body");
        let health_json: serde_json::Value =
            serde_json::from_slice(&health_body).expect("health JSON");
        assert_eq!(
            health_json,
            serde_json::json!({
                "status": "ok",
                "version": env!("CARGO_PKG_VERSION")
            })
        );

        let mcp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header(header::HOST, "localhost")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .expect("MCP request"),
            )
            .await
            .expect("MCP response");
        assert_eq!(mcp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn body_limit_rejects_oversized_requests_before_the_handler() {
        let router = Router::new()
            .route("/", post(|| async { StatusCode::NO_CONTENT }))
            .layer(middleware::from_fn_with_state(4_usize, enforce_body_limit));

        for (body, expected) in [
            (Body::from("1234"), StatusCode::NO_CONTENT),
            (Body::from("12345"), StatusCode::PAYLOAD_TOO_LARGE),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/")
                        .body(body)
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), expected);
        }
    }

    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "Keep the complete HTTP boundary scenario visible in one test"
    )]
    async fn authenticated_http_transport_reaches_only_the_read_upstream_for_status() {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
        use ed25519_dalek::{SigningKey, pkcs8::EncodePrivateKey};
        use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
        use serde_json::json;
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{body_json, header as matches_header, method, path},
        };

        let upstream = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/read"))
            .and(matches_header("x-api-key", "read"))
            .and(body_json(json!({"type":"GetVersion","params":{}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"version":"2.1.2"})))
            .expect(1)
            .mount(&upstream)
            .await;
        let jwks = MockServer::start().await;
        let signing = SigningKey::from_bytes(&[7; 32]);
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"keys":[{
                "kty":"OKP", "use":"sig", "crv":"Ed25519", "kid":"test-key", "alg":"EdDSA",
                "x":URL_SAFE_NO_PAD.encode(signing.verifying_key().as_bytes())
            }]})))
            .mount(&jwks)
            .await;
        let mut config = settings();
        config.identity.jwks_url = Url::parse(&format!("{}/jwks", jwks.uri())).unwrap();
        config.identity.actor = "gateway.test".into();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let claims = json!({"sub":"test-user", "iss":"https://gateway.test", "aud":"komodo",
            "iat":now, "exp":now+60, "groups":[], "act":{"sub":"gateway.test"}});
        let mut token_header = Header::new(Algorithm::EdDSA);
        token_header.kid = Some("test-key".into());
        let key = signing.to_pkcs8_der().unwrap();
        let token = encode(
            &token_header,
            &claims,
            &EncodingKey::from_ed_der(key.as_bytes()),
        )
        .unwrap();
        let read = komodo_api::Client::new(
            Url::parse(&upstream.uri()).unwrap(),
            config.read_credentials.clone(),
            Duration::from_secs(2),
        )
        .unwrap();
        let router = build_router(
            &config,
            KomodoMcp::new(Arc::new(read), Arc::new(UnavailableApi)),
            &CancellationToken::new(),
        )
        .unwrap();
        for (bearer, identity, expected) in [
            ("wrong", token.as_str(), StatusCode::UNAUTHORIZED),
            (
                "0123456789abcdef0123456789abcdef",
                "invalid",
                StatusCode::UNAUTHORIZED,
            ),
            (
                "0123456789abcdef0123456789abcdef",
                token.as_str(),
                StatusCode::OK,
            ),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/mcp")
                        .header(header::HOST, "localhost")
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(header::ACCEPT, "application/json, text/event-stream")
                        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                        .header("X-MCP-Identity", identity)
                        .header("MCP-Protocol-Version", "2025-11-25")
                        .body(Body::from(
                            json!({"jsonrpc":"2.0", "id":1,"method":"tools/call",
                    "params":{"name":"system.status","arguments":{}}})
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
            if expected == StatusCode::OK {
                let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(
                    body["result"]["structuredContent"],
                    json!({"version":"2.1.2","reachable":true})
                );
            }
        }
    }
}
