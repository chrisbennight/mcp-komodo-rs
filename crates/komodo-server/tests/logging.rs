use std::{net::TcpListener, process::Stdio, time::Duration};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{SigningKey, pkcs8::EncodePrivateKey};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::{Value, json};
use tokio::{io::AsyncReadExt, process::Command, task::JoinHandle};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, method, path},
};

const BEARER: &str = "synthetic-bearer-for-http-log-test";
const RESOURCE: &str = "synthetic-resource-log-sentinel";
const CONTENT: &str = "synthetic-environment-log-sentinel";
const WRITE: &str = "synthetic-write-log-sentinel";
const UPSTREAM_ERROR: &str = "synthetic-upstream-error-sentinel";
const CLIENT: &str = "synthetic-client-log-sentinel";
const URI_MARKER: &str = "synthetic-uri-log-sentinel";
const TOKEN: &str = "synthetic-webhook-token-sentinel";
const FAILED_TOKEN: &str = "synthetic-rejected-token-sentinel";

fn history_toml() -> String {
    format!(
        "[[stack]]\nname = \"ordinary-history-stack\"\n[stack.config]\nwebhook_secret = \"{TOKEN}\"\nenvironment = \"password=ordinary-text\"\nproject_name = \"10.20.0.1\"\n\n[[alerter]]\n[alerter.config.endpoint]\ntype = \"Custom\"\n[alerter.config.endpoint.params]\nurl = \"https://operator:{TOKEN}@alerts.example.test:not-a-port/public%2Froute\"\n"
    )
}

fn capture(reader: impl tokio::io::AsyncRead + Unpin + Send + 'static) -> JoinHandle<Vec<u8>> {
    tokio::spawn(async move {
        let mut output = Vec::new();
        reader
            .take(1024 * 1024)
            .read_to_end(&mut output)
            .await
            .unwrap();
        output
    })
}

async fn fake_upstream() -> (MockServer, String) {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            json!({"type":"UpdateStack","params":{"config":{"webhook_secret":FAILED_TOKEN}}}),
        ))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({
            "error":format!("Cannot store webhook token {FAILED_TOKEN}"),
            "trace":["Core storage unavailable at 10.20.0.1"]
        })))
        .with_priority(1)
        .mount(&upstream)
        .await;
    let signing = SigningKey::from_bytes(&[7; 32]);
    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"keys":[{
            "kty":"OKP", "use":"sig", "crv":"Ed25519", "kid":"test-key", "alg":"EdDSA",
            "x":URL_SAFE_NO_PAD.encode(signing.verifying_key().as_bytes())
        }]})))
        .mount(&upstream)
        .await;
    for (kind, result) in [
        (
            "ListStacks",
            json!([{"id":"stack-id", "name":RESOURCE, "type":"Stack",
            "info":{"state":"running", "services":[]}}]),
        ),
        (
            "GetStack",
            json!({"config":{"environment":CONTENT,"webhook_secret":TOKEN}}),
        ),
        ("UpdateStack", json!({"config":{"environment":WRITE}})),
        (
            "GetUpdate",
            json!({"_id":{"$oid":"history-document"},"operation":"UpdateStack",
            "start_ts":1,"status":"Complete","success":true,"prev_toml":"[[stack]]\nname = \"before\"\n",
            "current_toml":history_toml()}),
        ),
    ] {
        Mock::given(method("POST"))
            .and(body_partial_json(json!({"type":kind})))
            .respond_with(ResponseTemplate::new(200).set_body_json(result))
            .mount(&upstream)
            .await;
    }
    Mock::given(method("POST"))
        .and(body_partial_json(json!({"type":"GetVersion"})))
        .respond_with(ResponseTemplate::new(500).set_body_string(UPSTREAM_ERROR))
        .mount(&upstream)
        .await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut header = Header::new(Algorithm::EdDSA);
    header.kid = Some("test-key".into());
    let key = signing.to_pkcs8_der().unwrap();
    let identity = encode(
        &header,
        &json!({"sub":"synthetic-user", "iss":"https://gateway.test", "aud":"komodo",
            "iat":now, "exp":now+240, "groups":[], "act":{"sub":"gateway.test"}}),
        &EncodingKey::from_ed_der(key.as_bytes()),
    )
    .unwrap();
    (upstream, identity)
}

async fn request(
    client: &reqwest::Client,
    url: &str,
    identity: &str,
    method: &str,
    params: Value,
) -> Value {
    let response = client
        .post(url)
        .header("accept", "application/json, text/event-stream")
        .header("authorization", format!("Bearer {BEARER}"))
        .header("x-mcp-identity", identity)
        .header("mcp-protocol-version", "2025-11-25")
        .json(&json!({"jsonrpc":"2.0", "id":CLIENT, "method":method, "params":params}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    response.json().await.unwrap()
}

async fn exercise_tools(client: &reqwest::Client, url: &str, identity: &str) {
    let initialized = request(
        client,
        url,
        identity,
        "initialize",
        json!({
            "protocolVersion":"2025-11-25", "capabilities":{},
            "clientInfo":{"name":CLIENT, "version":"1"}
        }),
    )
    .await;
    assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
    let read = request(
        client,
        url,
        identity,
        "tools/call",
        json!({
            "name":"stacks.environment.read", "arguments":{"selector":RESOURCE}
        }),
    )
    .await;
    assert_eq!(read["result"]["structuredContent"]["environment"], CONTENT);
    let write = request(
        client,
        url,
        identity,
        "tools/call",
        json!({
            "name":"stacks.environment.write", "arguments":{"selector":RESOURCE,"environment":WRITE}
        }),
    )
    .await;
    assert!(write["result"]["structuredContent"].is_object());
    let failure = request(
        client,
        url,
        identity,
        "tools/call",
        json!({
            "name":"system.status", "arguments":{}
        }),
    )
    .await;
    assert!(failure["error"].is_object());
    assert_eq!(
        failure["error"]["data"]["upstream"]["diagnostic"]["error"],
        UPSTREAM_ERROR
    );
    assert_eq!(failure["error"]["data"]["upstream"]["httpStatus"], 500);
    assert_eq!(
        failure["error"]["data"]["upstream"]["request"],
        "GetVersion"
    );
    assert_eq!(failure["error"]["data"]["upstream"]["untrusted"], true);
}

async fn exercise_files(client: &reqwest::Client, url: &str, identity: &str) -> Vec<String> {
    let meta = json!({"io.modelcontextprotocol/clientCapabilities":{
        "files":{"upload":true,"download":true,"transports":["https", "http"]}
    }});
    let upload = request(
        client,
        url,
        identity,
        "files/authorizeUpload",
        json!({"_meta":meta,"name":"token.txt","mimeType":"text/plain"}),
    )
    .await;
    let grant = upload["result"]["upload"]["headers"]["Authorization"]
        .as_str()
        .unwrap();
    assert_eq!(
        client
            .put(upload["result"]["upload"]["url"].as_str().unwrap())
            .header("authorization", grant)
            .body(TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    let written = request(
        client,
        url,
        identity,
        "tools/call",
        json!({
            "name":"stacks.webhook.secret.write","arguments":{"selector":RESOURCE,
                "secret":{"source":"custom","file":upload["result"]["file"]}}
        }),
    )
    .await;
    assert!(written["result"]["structuredContent"].is_object());
    assert!(!written.to_string().contains(TOKEN));
    let read = request(
        client,
        url,
        identity,
        "tools/call",
        json!({
            "name":"stacks.webhook.secret.read","arguments":{"selector":RESOURCE}
        }),
    )
    .await;
    assert!(!read.to_string().contains(TOKEN));
    let download = request(
        client,
        url,
        identity,
        "files/authorizeDownload",
        json!({"_meta":meta,"uri":read["result"]["structuredContent"]["file"]["uri"]}),
    )
    .await;
    let download_grant = download["result"]["download"]["headers"]["Authorization"]
        .as_str()
        .unwrap();
    let response = client
        .get(download["result"]["download"]["url"].as_str().unwrap())
        .header("authorization", download_grant)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), TOKEN);
    let mut grants = vec![grant.to_owned(), download_grant.to_owned()];
    grants.extend(exercise_credential_failure(client, url, identity, meta.clone()).await);
    grants.push(exercise_history_files(client, url, identity, meta).await);
    grants
}

async fn exercise_history_files(
    client: &reqwest::Client,
    url: &str,
    identity: &str,
    meta: Value,
) -> String {
    let history = request(
        client,
        url,
        identity,
        "tools/call",
        json!({
        "name":"operations.status", "arguments":{"operation_id":"history-document"}
        }),
    )
    .await;
    assert!(!history.to_string().contains(TOKEN));
    assert!(history.get("error").is_none(), "{history}");
    let status = &history["result"]["structuredContent"];
    assert_eq!(
        status["previousConfiguration"]["toml"],
        "[[stack]]\nname = \"before\"\n"
    );
    let current = &status["currentConfiguration"];
    assert!(
        current["toml"]
            .as_str()
            .unwrap()
            .contains("password=ordinary-text")
    );
    assert!(current["toml"].as_str().unwrap().contains("10.20.0.1"));
    assert_eq!(
        current["credentialFields"],
        json!([
            "stack[0].config.webhook_secret",
            "alerter[0].config.endpoint.params.url"
        ])
    );
    assert!(
        current["toml"]
            .as_str()
            .unwrap()
            .contains("alerts.example.test:not-a-port/public%2Froute")
    );
    let download = request(
        client,
        url,
        identity,
        "files/authorizeDownload",
        json!({
            "_meta":meta,"uri":current["originalFile"]["uri"]
        }),
    )
    .await;
    let grant = download["result"]["download"]["headers"]["Authorization"]
        .as_str()
        .unwrap();
    let response = client
        .get(download["result"]["download"]["url"].as_str().unwrap())
        .header("authorization", grant)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), history_toml());
    grant.to_owned()
}

async fn exercise_credential_failure(
    client: &reqwest::Client,
    url: &str,
    identity: &str,
    meta: Value,
) -> Vec<String> {
    let mut grants = Vec::new();
    let failed_upload = request(
        client,
        url,
        identity,
        "files/authorizeUpload",
        json!({"_meta":meta,"name":"rejected-token.txt","mimeType":"text/plain"}),
    )
    .await;
    let failed_grant = failed_upload["result"]["upload"]["headers"]["Authorization"]
        .as_str()
        .unwrap();
    assert_eq!(
        client
            .put(failed_upload["result"]["upload"]["url"].as_str().unwrap())
            .header("authorization", failed_grant)
            .body(FAILED_TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    grants.push(failed_grant.to_owned());
    let failure = request(
        client,
        url,
        identity,
        "tools/call",
        json!({
            "name":"stacks.webhook.secret.write", "arguments":{"selector":RESOURCE,
                "secret":{"source":"custom","file":failed_upload["result"]["file"]}}
        }),
    )
    .await;
    assert!(!failure.to_string().contains(FAILED_TOKEN));
    assert_eq!(failure["result"]["isError"], true);
    assert_eq!(
        failure["result"]["_meta"]["io.modelcontextprotocol/trust-annotations"]["untrusted"],
        true
    );
    let detail = &failure["result"]["structuredContent"]["data"];
    assert_eq!(detail["outcome"], "unknown");
    assert_eq!(detail["retrySafe"], false);
    assert_eq!(detail["reconcileWith"], "stacks.webhook.secret.read");
    assert_eq!(detail["upstream"]["httpStatus"], 503);
    assert_eq!(
        detail["upstream"]["diagnostic"]["trace"][0],
        "Core storage unavailable at 10.20.0.1"
    );
    assert_eq!(detail["upstream"]["credentialValuesPresent"], true);
    let original = request(
        client,
        url,
        identity,
        "files/authorizeDownload",
        json!({"_meta":meta,"uri":detail["upstream"]["diagnosticFile"]["uri"]}),
    )
    .await;
    let original_grant = original["result"]["download"]["headers"]["Authorization"]
        .as_str()
        .unwrap();
    let body = client
        .get(original["result"]["download"]["url"].as_str().unwrap())
        .header("authorization", original_grant)
        .send()
        .await
        .unwrap();
    assert_eq!(body.status(), 200);
    assert_eq!(
        body.json::<Value>().await.unwrap(),
        json!({
            "error":format!("Cannot store webhook token {FAILED_TOKEN}"),
            "trace":["Core storage unavailable at 10.20.0.1"]
        })
    );
    grants.push(original_grant.to_owned());
    grants
}

fn assert_diagnostics(bytes: &[u8], filter: &str, identity: &str) {
    let output = std::str::from_utf8(bytes).unwrap();
    for marker in [
        BEARER,
        RESOURCE,
        CONTENT,
        WRITE,
        UPSTREAM_ERROR,
        CLIENT,
        URI_MARKER,
        TOKEN,
        FAILED_TOKEN,
        "synthetic-read-secret",
        "synthetic-admin-secret",
        identity,
    ] {
        assert!(
            !output.contains(marker),
            "protected content reached diagnostics with {filter}"
        );
    }
    let events: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for event in &events {
        assert_eq!(event["target"], komodo_server::diagnostics::TARGET);
        let fields = event["fields"].as_object().unwrap();
        assert!(
            fields
                .keys()
                .all(|name| ["message", "status", "duration_ms"].contains(&name.as_str()))
        );
    }
    if filter.contains("debug") || filter == "trace" || filter.starts_with("trace,") {
        assert!(
            events
                .iter()
                .any(|event| event["fields"]["message"] == "HTTP request completed")
        );
        assert!(events.iter().any(|event| event["fields"]["status"] == 200));
    }
    if filter == "info" {
        assert!(
            events
                .iter()
                .any(|event| event["fields"]["message"] == "Komodo MCP listening")
        );
    }
    if ["off", "error", "warn", "off,rmcp=trace"].contains(&filter) {
        assert!(events.is_empty());
    }
}

#[tokio::test]
async fn actual_http_binary_preserves_tool_content_and_excludes_it_from_every_log_setting() {
    let (upstream, identity) = fake_upstream().await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    for filter in [
        "off",
        "error",
        "warn",
        "info",
        "debug",
        "trace",
        "trace,rmcp=trace,tower_http=trace,reqwest=trace",
        "off,rmcp=trace",
        "off,komodo_server::diagnostics=debug",
    ] {
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reservation.local_addr().unwrap().port();
        drop(reservation);
        let mut child = Command::new(env!("CARGO_BIN_EXE_komodo-mcp-rs"))
            .env_clear()
            .env("KOMODO_MCP_HOST", "127.0.0.1")
            .env("KOMODO_MCP_PORT", port.to_string())
            .env("KOMODO_MCP_UPSTREAM_URL", upstream.uri())
            .env("KOMODO_MCP_READ_API_KEY", "synthetic-read-key")
            .env("KOMODO_MCP_READ_API_SECRET", "synthetic-read-secret")
            .env("KOMODO_MCP_ADMIN_API_KEY", "synthetic-admin-key")
            .env("KOMODO_MCP_ADMIN_API_SECRET", "synthetic-admin-secret")
            .env("KOMODO_MCP_GATEWAY_BEARER_CURRENT", BEARER)
            .env(
                "KOMODO_MCP_IDENTITY_JWKS_URL",
                format!("{}/jwks", upstream.uri()),
            )
            .env("KOMODO_MCP_IDENTITY_ISSUER", "https://gateway.test")
            .env("KOMODO_MCP_IDENTITY_ACTOR", "gateway.test")
            .env("KOMODO_MCP_ALLOWED_HOSTS", format!("127.0.0.1:{port}"))
            .env("KOMODO_MCP_LOG_LEVEL", filter)
            .env("RUST_LOG", "trace")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdout = capture(child.stdout.take().unwrap());
        let stderr = capture(child.stderr.take().unwrap());
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "binary exited during startup"
                );
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
        .expect("HTTP listener ready");
        exercise_tools(
            &client,
            &format!("http://127.0.0.1:{port}/mcp?audit={URI_MARKER}"),
            &identity,
        )
        .await;
        let grants =
            exercise_files(&client, &format!("http://127.0.0.1:{port}/mcp"), &identity).await;
        child.start_kill().unwrap();
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(
            stdout.await.unwrap().is_empty(),
            "HTTP diagnostics belong on stderr"
        );
        let diagnostics = stderr.await.unwrap();
        for grant in grants {
            assert!(!std::str::from_utf8(&diagnostics).unwrap().contains(&grant));
        }
        assert_diagnostics(&diagnostics, filter, &identity);
    }
}
