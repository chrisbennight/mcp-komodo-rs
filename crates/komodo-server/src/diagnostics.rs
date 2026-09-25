//! Payload-free service diagnostics, isolated from dependency tracing.

use tracing_subscriber::{
    EnvFilter, filter::filter_fn, layer::SubscriberExt, util::SubscriberInitExt,
};

/// Only events using this target may reach the executable's diagnostic output.
/// Fields at these call sites must contain fixed messages or operational numbers,
/// never request headers, URLs, tool arguments, upstream content, or errors.
pub const TARGET: &str = "komodo_server::diagnostics";

/// Install JSON diagnostics on stderr with an unconditional target boundary.
///
/// The configured filter can narrow these events, but cannot enable dependency
/// events or spans, whose debug fields can contain complete protocol payloads.
///
/// # Errors
///
/// Returns an error if the configured tracing expression is invalid.
pub fn init(log_filter: &str) -> Result<(), tracing_subscriber::filter::ParseError> {
    let configured = EnvFilter::try_new(log_filter)?;
    tracing_subscriber::registry()
        .with(filter_fn(|metadata| metadata.target() == TARGET))
        .with(configured)
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(std::io::stderr),
        )
        .init();
    Ok(())
}
