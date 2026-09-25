#![cfg(unix)]

use std::{net::TcpListener, process::Stdio, time::Duration};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{SigningKey, pkcs8::EncodePrivateKey};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::{Value, json};
use tokio::{
    io::AsyncWriteExt,
    net::TcpStream,
    process::{Child, Command},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, method, path},
};

const BEARER: &str = "synthetic-bearer-for-shutdown-test";

struct Fixture {
    upstream: MockServer,
    child: Child,
    port: u16,
    identity: String,
}

impl Fixture {
    async fn new() -> Self {
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
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"type":"ListStacks"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(900))
                    .set_body_json(json!([{"id":"stack-1", "name":"stack", "type":"Stack",
                    "info":{"state":"running", "services":[]}}])),
            )
            .mount(&upstream)
            .await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some("test-key".into());
        let identity = encode(
            &header,
            &json!({"sub":"test-user", "iss":"https://gateway.test", "aud":"komodo",
                "iat":now, "exp":now+240, "groups":[], "act":{"sub":"gateway.test"}}),
            &EncodingKey::from_ed_der(signing.to_pkcs8_der().unwrap().as_bytes()),
        )
        .unwrap();
        let (child, port) = start_binary(&upstream).await;
        Self {
            upstream,
            child,
            port,
            identity,
        }
    }

    async fn active_request(&self) -> TcpStream {
        let mut socket = TcpStream::connect(("127.0.0.1", self.port)).await.unwrap();
        let body = json!({"jsonrpc":"2.0", "id":1,"method":"tools/call",
            "params":{"name":"stacks.stop", "arguments":{"selector":"stack-1"}}})
        .to_string();
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nAuthorization: Bearer {BEARER}\r\nX-Mcp-Identity: {}\r\nMcp-Protocol-Version: 2025-11-25\r\nContent-Length: {}\r\n\r\n{body}",
            self.identity,
            body.len()
        );
        socket.write_all(request.as_bytes()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if self
                    .request_kinds()
                    .await
                    .iter()
                    .any(|kind| kind == "ListStacks")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("authenticated request reached resource resolution");
        socket
    }

    async fn request_kinds(&self) -> Vec<String> {
        self.upstream
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.method == "POST")
            .map(|request| {
                serde_json::from_slice::<Value>(&request.body).unwrap()["type"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    async fn signal_and_wait(&mut self, signal: &str) {
        let signalled = Command::new("kill")
            .arg(signal)
            .arg(self.child.id().unwrap().to_string())
            .status()
            .await
            .unwrap();
        assert!(signalled.success());
        let status = tokio::time::timeout(Duration::from_secs(7), self.child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(
            status.success(),
            "{signal} must exit through bounded graceful shutdown"
        );
    }
}

#[tokio::test]
async fn sigterm_stops_the_http_binary_cleanly_within_the_drain_bound() {
    Fixture::new().await.signal_and_wait("-TERM").await;
}

#[tokio::test]
async fn real_client_disconnect_cancels_resolution_and_releases_admission() {
    let mut fixture = Fixture::new().await;
    let socket = fixture.active_request().await;
    drop(socket);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        fixture.request_kinds().await,
        ["ListStacks"],
        "disconnect must prevent a late write"
    );
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{}/mcp", fixture.port))
        .header("host", "localhost")
        .header("accept", "application/json, text/event-stream")
        .header("authorization", format!("Bearer {BEARER}"))
        .header("x-mcp-identity", &fixture.identity)
        .header("mcp-protocol-version", "2025-11-25")
        .json(&json!({"jsonrpc":"2.0", "id":2,"method":"tools/list"}))
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "admission must recover after disconnect"
    );
    fixture.signal_and_wait("-TERM").await;
}

#[tokio::test]
async fn termination_signals_cancel_active_binary_requests_without_late_writes() {
    for signal in ["-TERM", "-INT"] {
        let mut fixture = Fixture::new().await;
        let socket = fixture.active_request().await;
        fixture.signal_and_wait(signal).await;
        assert_eq!(
            fixture.request_kinds().await,
            ["ListStacks"],
            "shutdown must not submit a late write"
        );
        drop(socket);
    }
}

async fn start_binary(upstream: &MockServer) -> (Child, u16) {
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let mut child = Command::new(env!("CARGO_BIN_EXE_komodo-mcp-rs"))
        .env_clear()
        .env("KOMODO_MCP_HOST", "127.0.0.1")
        .env("KOMODO_MCP_PORT", port.to_string())
        .env("KOMODO_MCP_UPSTREAM_URL", upstream.uri())
        .env("KOMODO_MCP_READ_API_KEY", "synthetic-read")
        .env("KOMODO_MCP_READ_API_SECRET", "synthetic-read-secret")
        .env("KOMODO_MCP_ADMIN_API_KEY", "synthetic-admin")
        .env("KOMODO_MCP_ADMIN_API_SECRET", "synthetic-admin-secret")
        .env("KOMODO_MCP_GATEWAY_BEARER_CURRENT", BEARER)
        .env(
            "KOMODO_MCP_IDENTITY_JWKS_URL",
            format!("{}/jwks", upstream.uri()),
        )
        .env("KOMODO_MCP_IDENTITY_ISSUER", "https://gateway.test")
        .env("KOMODO_MCP_IDENTITY_ACTOR", "gateway.test")
        .env("KOMODO_MCP_ALLOWED_HOSTS", "localhost")
        .env("KOMODO_MCP_REQUEST_TIMEOUT_SECONDS", "10")
        .env("KOMODO_MCP_MAX_CONCURRENT_REQUESTS", "1")
        .env("KOMODO_MCP_LOG_LEVEL", "off")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            assert!(child.try_wait().unwrap().is_none());
            if tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("listener ready");
    (child, port)
}
