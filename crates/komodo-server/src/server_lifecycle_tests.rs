use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{SigningKey, pkcs8::EncodePrivateKey};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use komodo_api::Client;
use komodo_mcp::KomodoMcp;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, method, path},
};

use super::{build_router, tests::settings};

struct Fixture {
    upstream: MockServer,
    router: Router,
    token: String,
    cancellation: CancellationToken,
}

impl Fixture {
    async fn new(timeout: Duration) -> Self {
        let upstream = MockServer::start().await;
        let signing = SigningKey::from_bytes(&[7; 32]);
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"keys":[{
                "kty":"OKP", "use":"sig", "crv":"Ed25519", "kid":"test-key", "alg":"EdDSA",
                "x":URL_SAFE_NO_PAD.encode(signing.verifying_key().as_bytes())
            }]})))
            .mount(&upstream)
            .await;
        let mut config = settings();
        config.identity.jwks_url = Url::parse(&format!("{}/jwks", upstream.uri())).unwrap();
        config.identity.actor = "gateway.test".into();
        config.request_timeout = timeout;
        config.max_concurrent_requests = 1;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some("test-key".into());
        let token = encode(
            &header,
            &json!({"sub":"test-user", "iss":"https://gateway.test", "aud":"komodo",
                "iat":now, "exp":now+240, "groups":[], "act":{"sub":"gateway.test"}}),
            &EncodingKey::from_ed_der(signing.to_pkcs8_der().unwrap().as_bytes()),
        )
        .unwrap();
        let make_client = |credentials| {
            Arc::new(
                Client::new(
                    Url::parse(&upstream.uri()).unwrap(),
                    credentials,
                    Duration::from_secs(3),
                )
                .unwrap(),
            )
        };
        let cancellation = CancellationToken::new();
        let router = build_router(
            &config,
            KomodoMcp::new(
                make_client(config.read_credentials.clone()),
                make_client(config.admin_credentials.clone()),
            ),
            &cancellation,
        )
        .unwrap();
        Self {
            upstream,
            router,
            token,
            cancellation,
        }
    }

    fn request(&self, tool: &str, arguments: &Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("authorization", "Bearer 0123456789abcdef0123456789abcdef")
            .header("x-mcp-identity", &self.token)
            .header("mcp-protocol-version", "2025-11-25")
            .body(Body::from(
                json!({"jsonrpc":"2.0", "id":1,"method":"tools/call",
                "params":{"name":tool,"arguments":arguments}})
                .to_string(),
            ))
            .unwrap()
    }

    async fn stacks(&self, delay: Duration) {
        Mock::given(method("POST")).and(body_partial_json(json!({"type":"ListStacks"})))
            .respond_with(ResponseTemplate::new(200).set_delay(delay).set_body_json(json!([
                {"id":"stack-1", "name":"stack", "type":"Stack", "info":{"state":"running","services":[]}}
            ]))).expect(1).mount(&self.upstream).await;
    }

    async fn wait_for(&self, kind: &str) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if self
                    .requests()
                    .await
                    .iter()
                    .any(|body| body["type"] == kind)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("upstream submission observed");
    }

    async fn requests(&self) -> Vec<Value> {
        self.upstream
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.method == "POST")
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .collect()
    }
}

#[tokio::test]
async fn timeout_during_resolution_never_submits_an_action_configuration_or_file_write() {
    for (tool, arguments) in [
        ("stacks.stop", json!({"selector":"stack"})),
        (
            "stacks.environment.write",
            json!({"selector":"stack","environment":"synthetic"}),
        ),
        (
            "stacks.file.write",
            json!({"selector":"stack","file_path":"compose.yaml","contents":"synthetic"}),
        ),
    ] {
        let fixture = Fixture::new(Duration::from_millis(500)).await;
        fixture.stacks(Duration::from_millis(900)).await;
        let response = fixture
            .router
            .clone()
            .oneshot(fixture.request(tool, &arguments))
            .await
            .unwrap();
        assert!(
            response.status() == StatusCode::REQUEST_TIMEOUT || response.status() == StatusCode::OK
        );
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        assert!(std::str::from_utf8(&body).unwrap().contains("reconcile"));
        tokio::time::sleep(Duration::from_millis(600)).await;
        let requests = fixture.requests().await;
        assert_eq!(requests.len(), 1, "no work after the resolution read");
        assert_eq!(requests[0]["type"], "ListStacks");
    }
}

#[tokio::test]
async fn dropped_http_future_cancels_resolution_and_new_calls_wait_for_handler_cleanup() {
    let fixture = Fixture::new(Duration::from_secs(3)).await;
    fixture.stacks(Duration::from_millis(900)).await;
    let task = tokio::spawn(
        fixture
            .router
            .clone()
            .oneshot(fixture.request("stacks.stop", &json!({"selector":"stack"}))),
    );
    fixture.wait_for("ListStacks").await;
    let overloaded = fixture
        .router
        .clone()
        .oneshot(fixture.request("system.status", &json!({})))
        .await
        .unwrap();
    assert_eq!(overloaded.status(), StatusCode::SERVICE_UNAVAILABLE);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        fixture.requests().await.len(),
        1,
        "cancelled request cannot submit later"
    );
    let next = fixture
        .router
        .clone()
        .oneshot(fixture.request("system.status", &json!({})))
        .await
        .unwrap();
    assert_eq!(
        next.status(),
        StatusCode::OK,
        "admission recovered after cancellation"
    );
}

#[tokio::test]
async fn cancellation_after_submission_keeps_the_outcome_uncertain_and_does_not_retry() {
    let fixture = Fixture::new(Duration::from_millis(500)).await;
    fixture.stacks(Duration::ZERO).await;
    Mock::given(method("POST")).and(body_partial_json(json!({"type":"StopStack"})))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(900))
            .set_body_json(json!({"id":"operation-1","operation":"StopStack","status":"Complete","success":true})))
        .expect(1).mount(&fixture.upstream).await;
    let response = fixture
        .router
        .clone()
        .oneshot(fixture.request("stacks.stop", &json!({"selector":"stack"})))
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert!(std::str::from_utf8(&body).unwrap().contains("reconcile"));
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        fixture
            .requests()
            .await
            .iter()
            .filter(|body| body["type"] == "StopStack")
            .count(),
        1
    );
}

#[tokio::test]
async fn shutdown_cancels_work_and_readiness_without_disabling_liveness() {
    let fixture = Fixture::new(Duration::from_secs(3)).await;
    fixture.stacks(Duration::from_millis(900)).await;
    let task = tokio::spawn(
        fixture
            .router
            .clone()
            .oneshot(fixture.request("stacks.stop", &json!({"selector":"stack"}))),
    );
    fixture.wait_for("ListStacks").await;
    fixture.cancellation.cancel();
    let _response = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    for (uri, expected) in [
        ("/readyz", StatusCode::SERVICE_UNAVAILABLE),
        ("/healthz", StatusCode::OK),
    ] {
        let response = fixture
            .router
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let refused = fixture
        .router
        .clone()
        .oneshot(fixture.request("system.status", &json!({})))
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(fixture.requests().await.len(), 1);
}
