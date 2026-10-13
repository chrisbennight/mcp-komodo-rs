use std::{
    future::IntoFuture,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use clap::Parser;
use komodo_api::{Client, KomodoApi};
use komodo_mcp::{KomodoMcp, ToolProfile};
use komodo_server::{
    config::Settings, diagnostics, gateway_manifest_for_profile, server::build_router,
};
use rmcp::ServiceExt;
use tokio::{net::TcpListener, signal};
use tokio_util::sync::CancellationToken;
use tracing::info;

#[derive(Debug, Parser)]
#[command(version, about = "Security-bounded Komodo MCP server")]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Clap models mutually exclusive command-line flags as booleans"
)]
struct Args {
    /// Serve the selected capability profile over local stdin/stdout without gateway settings.
    #[arg(long, conflicts_with_all = ["healthcheck", "emit_gateway_manifest", "emit_tools_json"])]
    stdio: bool,
    /// Check only the local liveness endpoint and exit.
    #[arg(long, conflicts_with_all = ["emit_gateway_manifest", "emit_tools_json"])]
    healthcheck: bool,
    /// Print the gateway manifest generated from the tool registry and exit.
    #[arg(long)]
    emit_gateway_manifest: bool,
    /// Print the full standard MCP tools/list catalog without runtime credentials.
    #[arg(long, conflicts_with_all = ["healthcheck", "emit_gateway_manifest"])]
    emit_tools_json: bool,
    /// Print source MCP declarations for deployment validation, without credentials.
    #[arg(long, conflicts_with_all = ["stdio", "healthcheck", "emit_gateway_manifest", "emit_tools_json"])]
    emit_gateway_contract_json: bool,
    /// Process/export capabilities: status, read-only, operations, or full. Authorization remains separate.
    #[arg(long, default_value = "full", conflicts_with = "healthcheck")]
    tool_profile: ToolProfile,
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
    if args.emit_gateway_contract_json {
        serde_json::to_writer(
            std::io::stdout().lock(),
            &komodo_server::gateway_contract_for_profile(args.tool_profile),
        )
        .context("write gateway contract")?;
        return Ok(());
    }
    if args.emit_tools_json {
        serde_json::to_writer(
            std::io::stdout().lock(),
            &KomodoMcp::list_tools_for_profile(args.tool_profile),
        )
        .context("write tool catalog")?;
        return Ok(());
    }
    if args.emit_gateway_manifest {
        print!("{}", gateway_manifest_for_profile(args.tool_profile));
        return Ok(());
    }
    if args.healthcheck {
        return healthcheck().await;
    }
    if args.stdio {
        return serve_stdio(args.tool_profile).await;
    }

    let settings = match args.tool_profile {
        ToolProfile::Status | ToolProfile::ReadOnly => Settings::read_only_from_env(),
        ToolProfile::Operations | ToolProfile::Full => Settings::from_env(),
    }
    .context("invalid server configuration")?;
    diagnostics::init(&settings.log_level).map_err(|_| anyhow::anyhow!("invalid log filter"))?;

    let read: Arc<dyn KomodoApi> = Arc::new(Client::new(
        settings.upstream_url.clone(),
        settings.read_credentials.clone(),
        settings.upstream_timeout,
    )?);
    let handler = match &settings.admin_credentials {
        Some(credentials) => KomodoMcp::new(
            read,
            Arc::new(Client::new(
                settings.upstream_url.clone(),
                credentials.clone(),
                settings.upstream_timeout,
            )?),
        ),
        None => KomodoMcp::read_only(read),
    }
    .with_tool_profile(args.tool_profile);
    let cancellation = CancellationToken::new();
    let router = build_router(&settings, handler, &cancellation)?;
    let address: SocketAddr = format!("{}:{}", settings.host, settings.port)
        .parse()
        .context("invalid listen address")?;
    let listener = TcpListener::bind(address).await.context("bind listener")?;
    info!(target: diagnostics::TARGET, "Komodo MCP listening");
    let server = axum::serve(listener, router)
        .with_graceful_shutdown(cancellation.clone().cancelled_owned())
        .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result.context("serve MCP"),
        result = shutdown_signal() => {
            cancellation.cancel();
            result.context("receive shutdown signal")?;
            tokio::time::timeout(Duration::from_secs(5), &mut server)
                .await
                .context("HTTP shutdown deadline exceeded")?
                .context("drain MCP")
        }
    }
}

async fn serve_stdio(profile: ToolProfile) -> Result<()> {
    let settings = komodo_server::config::ReadSettings::from_env()
        .map_err(|_| anyhow::anyhow!("invalid upstream connection configuration"))?;
    let administrative_credentials = match profile {
        ToolProfile::Status | ToolProfile::ReadOnly => None,
        ToolProfile::Operations | ToolProfile::Full => {
            komodo_server::config::administrative_credentials_from_env()
                .map_err(|_| anyhow::anyhow!("invalid administrative credential configuration"))?
        }
    };
    let upstream_url = settings.upstream_url.clone();
    let read = Arc::new(
        Client::new(
            settings.upstream_url,
            settings.credentials,
            settings.upstream_timeout,
        )
        .map_err(|_| anyhow::anyhow!("invalid upstream configuration"))?,
    );
    let handler = match administrative_credentials {
        Some(credentials) => KomodoMcp::new(
            read,
            Arc::new(
                Client::new(upstream_url, credentials, settings.upstream_timeout).map_err(
                    |_| anyhow::anyhow!("invalid administrative upstream configuration"),
                )?,
            ),
        ),
        None => KomodoMcp::read_only(read),
    }
    .with_tool_profile(profile);
    // No tracing subscriber is installed: SDK diagnostics may include peer input.
    // stdout belongs exclusively to the MCP protocol in this mode.
    let transport =
        komodo_server::stdio::BoundedStdio::new(tokio::io::stdin(), tokio::io::stdout());
    let service =
        tokio::time::timeout(std::time::Duration::from_secs(30), handler.serve(transport))
            .await
            .map_err(|_| anyhow::anyhow!("MCP initialization timed out"))?
            .map_err(|_| anyhow::anyhow!("MCP initialization failed"))?;
    service
        .waiting()
        .await
        .map_err(|_| anyhow::anyhow!("MCP service failed"))?;
    Ok(())
}

async fn shutdown_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate = signal::unix::signal(signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    signal::ctrl_c().await
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
