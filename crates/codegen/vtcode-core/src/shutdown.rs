//! Shared shutdown signal helpers.

use std::io::Result as IoResult;

/// Wait for a process shutdown signal.
///
/// On Unix this treats `SIGTERM` the same as `Ctrl+C` so long-lived services can
/// drain gracefully when stopped by process supervisors.
pub async fn shutdown_signal() -> IoResult<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = signal(SignalKind::terminate())?;
        tokio::select! {
            ctrl_c_result = tokio::signal::ctrl_c() => ctrl_c_result,
            _ = terminate.recv() => Ok(()),
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}

/// Wait for a shutdown signal and log listener errors consistently.
pub async fn shutdown_signal_logged(context: &'static str) {
    if let Err(err) = shutdown_signal().await {
        tracing::warn!("Failed to listen for {context} shutdown signal: {err}");
    }
}
