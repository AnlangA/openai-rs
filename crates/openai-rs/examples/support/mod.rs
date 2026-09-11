//! Shared logging configuration for executable SDK examples.

use std::io;

/// Installs a stderr subscriber using `RUST_LOG`, with WARN as the default level.
///
/// # Errors
///
/// Returns an error if a global tracing subscriber has already been installed.
pub fn init_logging() -> io::Result<()> {
    // Applications own their subscribers. The SDK only emits tracing events.
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::WARN.into())
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .with_ansi(false)
        .try_init()
        .map_err(io::Error::other)
}
