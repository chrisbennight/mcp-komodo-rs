use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use anyhow::{Context, Result};
use clap::Parser;
use komodo_api::{Client, KomodoApi};
use komodo_mcp::KomodoMcp;
use komodo_server::{config::Settings, gateway_manifest, server::build_router};
use tokio::{net::TcpListener, signal};
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(version, about = "Security-bounded Komodo MCP server")]
struct Args {
    /// Check only the local liveness endpoint and exit.
    #[arg(long)]
    healthcheck: bool,
    /// Print the gateway manifest generated from the tool registry and exit.
    #[arg(long)]
    emit_gateway_manifest: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if args.emit_gateway_manifest {
        print!("{}", gateway_manifest());
        return Ok(());
    }
    if args.healthcheck {
        return healthcheck().await;
    }

    let settings = Settings::from_env().context("invalid server configuration")?;
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::try_new(&settings.log_level).context("invalid log filter")?)
        .init();

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
    info!(%address, "Komodo MCP listening");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown(cancellation))
        .await
        .context("serve MCP")
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
