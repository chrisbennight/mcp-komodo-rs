#![cfg(unix)]

use std::{net::TcpListener, process::Stdio, time::Duration};

use tokio::process::Command;
use wiremock::MockServer;

#[tokio::test]
async fn sigterm_stops_the_http_binary_cleanly_within_the_drain_bound() {
    let upstream = MockServer::start().await;
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
        .env(
            "KOMODO_MCP_GATEWAY_BEARER_CURRENT",
            "synthetic-bearer-for-shutdown-test",
        )
        .env(
            "KOMODO_MCP_IDENTITY_JWKS_URL",
            format!("{}/jwks", upstream.uri()),
        )
        .env("KOMODO_MCP_IDENTITY_ISSUER", "https://gateway.test")
        .env("KOMODO_MCP_IDENTITY_ACTOR", "gateway.test")
        .env("KOMODO_MCP_ALLOWED_HOSTS", "localhost")
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
    let signalled = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().unwrap().to_string())
        .status()
        .await
        .unwrap();
    assert!(signalled.success());
    let status = tokio::time::timeout(Duration::from_secs(7), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(
        status.success(),
        "SIGTERM must use graceful shutdown, not signal termination"
    );
}
