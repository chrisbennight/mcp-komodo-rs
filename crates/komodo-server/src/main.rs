use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use anyhow::{Context, Result};
use clap::Parser;
use komodo_api::{Client, KomodoApi};
use komodo_mcp::KomodoMcp;
use komodo_server::{config::Settings, diagnostics, gateway_manifest, server::build_router};
use rmcp::ServiceExt;
use tokio::{net::TcpListener, signal};
use tokio_util::sync::CancellationToken;
use tracing::info;

#[derive(Debug, Parser)]
#[command(version, about = "Security-bounded Komodo MCP server")]
#[group(multiple = false)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Clap models mutually exclusive command-line flags as booleans"
)]
struct Args {
    /// Serve only ordinary status tools over local stdin/stdout, without gateway or admin credentials.
    #[arg(long, conflicts_with_all = ["healthcheck", "emit_gateway_manifest", "emit_tools_json"])]
    stdio: bool,
    /// Check only the local liveness endpoint and exit.
    #[arg(long)]
    healthcheck: bool,
    /// Print the gateway manifest generated from the tool registry and exit.
    #[arg(long)]
    emit_gateway_manifest: bool,
    /// Print the full standard MCP tools/list catalog without runtime credentials.
    #[arg(long, conflicts_with_all = ["healthcheck", "emit_gateway_manifest"])]
    emit_tools_json: bool,
}

fn main() -> Result<()> {
    let runtime = tokio::runtime::Runtime::new().context("create async runtime")?;
    let result = runtime.block_on(run());
    // Tokio stdin uses a blocking read that cannot be cancelled. Once the
    // service has ended, that read must not keep this executable alive.
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    result
}

async fn run() -> Result<()> {
    let args = Args::parse();
    if args.emit_tools_json {
        serde_json::to_writer(std::io::stdout().lock(), &KomodoMcp::list_tools_payload())
            .context("write tool catalog")?;
        return Ok(());
    }
    if args.emit_gateway_manifest {
        print!("{}", gateway_manifest());
        return Ok(());
    }
    if args.healthcheck {
        return healthcheck().await;
    }
    if args.stdio {
        return serve_stdio().await;
    }

    let settings = Settings::from_env().context("invalid server configuration")?;
    diagnostics::init(&settings.log_level).map_err(|_| anyhow::anyhow!("invalid log filter"))?;

    let read: Arc<dyn KomodoApi> = Arc::new(Client::new(
        settings.upstream_url.clone(),
        settings.read_credentials.clone(),
        settings.upstream_timeout,
    )?);
    let admin: Arc<dyn KomodoApi> = Arc::new(Client::new(
        settings.upstream_url.clone(),
        settings.admin_credentials.clone(),
        settings.upstream_timeout,
    )?);
    let handler = KomodoMcp::new(read, admin);
    let cancellation = CancellationToken::new();
    let router = build_router(&settings, handler, &cancellation)?;
    let address: SocketAddr = format!("{}:{}", settings.host, settings.port)
        .parse()
        .context("invalid listen address")?;
    let listener = TcpListener::bind(address).await.context("bind listener")?;
    info!(target: diagnostics::TARGET, "Komodo MCP listening");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown(cancellation))
        .await
        .context("serve MCP")
}

async fn serve_stdio() -> Result<()> {
    let settings = komodo_server::config::ReadSettings::from_env()
        .map_err(|_| anyhow::anyhow!("invalid status-only connection configuration"))?;
    let read = Arc::new(
        Client::new(
            settings.upstream_url,
            settings.credentials,
            settings.upstream_timeout,
        )
        .map_err(|_| anyhow::anyhow!("invalid status-only upstream configuration"))?,
    );
    // No tracing subscriber is installed: SDK diagnostics may include peer input.
    // stdout belongs exclusively to the MCP protocol in this mode.
    let transport =
        komodo_server::stdio::BoundedStdio::new(tokio::io::stdin(), tokio::io::stdout());
    let service = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        KomodoMcp::status_only(read).serve(transport),
    )
    .await
    .map_err(|_| anyhow::anyhow!("MCP initialization timed out"))?
    .map_err(|_| anyhow::anyhow!("MCP initialization failed"))?;
    service
        .waiting()
        .await
        .map_err(|_| anyhow::anyhow!("MCP service failed"))?;
    Ok(())
}

async fn shutdown(cancellation: CancellationToken) {
    let _ = signal::ctrl_c().await;
    cancellation.cancel();
}

async fn healthcheck() -> Result<()> {
    let settings = Settings::listener_from_env().context("invalid listener configuration")?;
    let host = settings
        .host
        .parse::<IpAddr>()
        .ok()
        .filter(IpAddr::is_loopback)
        .map_or("127.0.0.1".to_owned(), |address| address.to_string());
    let url = format!("http://{host}:{}/healthz", settings.port);
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .context("build health client")?
        .get(url)
        .send()
        .await
        .context("health request")?;
    anyhow::ensure!(
        response.status().is_success(),
        "health endpoint is not ready"
    );
    Ok(())
}
