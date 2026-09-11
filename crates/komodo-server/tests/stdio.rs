use std::{process::Stdio, time::Duration};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

async fn reply(reader: &mut BufReader<tokio::process::ChildStdout>) -> Value {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
        .await
        .expect("reply deadline")
        .expect("read reply");
    serde_json::from_str(&line).expect("stdout contains only MCP JSON")
}

#[tokio::test]
async fn actual_binary_initializes_lists_and_calls_status_without_gateway_or_admin_secrets() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/read"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"version":"2.1.2"})))
        .expect(1)
        .mount(&upstream)
        .await;
    let mut child = Command::new(env!("CARGO_BIN_EXE_komodo-mcp-rs"))
        .arg("--stdio")
        .env_clear()
        .env("KOMODO_MCP_READ_API_KEY", "synthetic-read-key")
        .env("KOMODO_MCP_READ_API_SECRET", "synthetic-read-secret")
        .env("KOMODO_MCP_UPSTREAM_URL", upstream.uri())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("start binary");
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-11-25","capabilities":{},
        "clientInfo":{"name":"integration-test","version":"1"}}});
    input
        .write_all(format!("{initialize}\n").as_bytes())
        .await
        .unwrap();
    assert_eq!(
        reply(&mut output).await["result"]["protocolVersion"],
        "2025-11-25"
    );
    input.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n").await.unwrap();
    let catalog = reply(&mut output).await;
    let tools = catalog["result"]["tools"].as_array().expect("tools");
    assert!(tools.iter().any(|tool| tool["name"] == "system.status"));
    for tool in tools {
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
    }
    for (id, name) in [
        (3, "system.status"),
        (4, "stacks.stop"),
        (5, "stacks.environment.read"),
    ] {
        let call = json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":name,"arguments":{}}});
        input
            .write_all(format!("{call}\n").as_bytes())
            .await
            .unwrap();
        let response = reply(&mut output).await;
        if id == 3 {
            assert_eq!(response["result"]["structuredContent"]["reachable"], true);
            assert_eq!(response["result"]["structuredContent"]["version"], "2.1.2");
        } else {
            assert_eq!(response["error"]["code"], -32601);
        }
    }
    drop(input);
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success());
    let mut diagnostics = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut diagnostics)
        .await
        .unwrap();
    assert!(diagnostics.is_empty());
}

#[tokio::test]
async fn invalid_initialize_never_echoes_peer_data_in_errors() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_komodo-mcp-rs"))
        .arg("--stdio")
        .env_clear()
        .env("KOMODO_MCP_READ_API_KEY", "synthetic-key")
        .env("KOMODO_MCP_READ_API_SECRET", "synthetic-secret")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":\"private-sentinel\",\"method\":\"tools/list\"}\n")
        .await
        .unwrap();
    let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("MCP initialization failed"));
    assert!(!stderr.contains("private-sentinel"));
}

#[tokio::test]
async fn idle_open_stdin_does_not_prevent_exit_after_initialization_timeout() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_komodo-mcp-rs"))
        .arg("--stdio")
        .env_clear()
        .env("KOMODO_MCP_READ_API_KEY", "synthetic-key")
        .env("KOMODO_MCP_READ_API_SECRET", "synthetic-secret")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let input = child.stdin.take().unwrap();
    let status = tokio::time::timeout(Duration::from_secs(35), child.wait())
        .await
        .expect("the process must exit even while its stdin remains open")
        .unwrap();
    assert!(!status.success());
    drop(input);
    let mut diagnostics = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut diagnostics)
        .await
        .unwrap();
    assert!(diagnostics.contains("MCP initialization timed out"));
}
