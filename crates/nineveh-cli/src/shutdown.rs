//! Waiting for the operator, or the supervisor, to ask us to stop.
//!
//! Ctrl-C is what a person sends; SIGTERM is what systemd, Docker and every process
//! supervisor send. Handling only the first means a deploy waits out the stop timeout
//! and is then killed, losing whatever was in flight. Nothing is corrupted by that —
//! commits are atomic and a restart resumes from the cursor — but it's a
//! several-second pause on every restart for no reason.

use anyhow::{Context, Result};
use tracing::info;

/// Resolve when the process is asked to stop, and say what asked.
///
/// # Errors
///
/// If the signal handlers can't be installed.
pub(crate) async fn requested() -> Result<()> {
    let signal = wait().await?;
    info!("{signal} received");
    Ok(())
}

#[cfg(unix)]
async fn wait() -> Result<&'static str> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate()).context("listening for SIGTERM")?;
    let mut interrupt = signal(SignalKind::interrupt()).context("listening for SIGINT")?;
    Ok(tokio::select! {
        _ = terminate.recv() => "SIGTERM",
        _ = interrupt.recv() => "Ctrl-C",
    })
}

#[cfg(not(unix))]
async fn wait() -> Result<&'static str> {
    tokio::signal::ctrl_c()
        .await
        .context("waiting for Ctrl-C")?;
    Ok("Ctrl-C")
}
