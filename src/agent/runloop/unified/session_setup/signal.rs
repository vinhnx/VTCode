use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::state::{CtrlCSignal, CtrlCState};
use crate::agent::runloop::unified::stop_requests::request_local_stop;
use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use vtcode_core::notifications::set_global_terminal_focused;

pub(crate) struct SignalHandlerGuard {
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl SignalHandlerGuard {
    fn new(handle: tokio::task::JoinHandle<()>) -> Self {
        Self {
            handle: Some(handle),
        }
    }
}

impl Drop for SignalHandlerGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

pub(crate) fn spawn_signal_handler(
    ctrl_c_state: Arc<CtrlCState>,
    ctrl_c_notify: Arc<Notify>,
    async_mcp_manager: Option<Arc<AsyncMcpManager>>,
    cancel_token: CancellationToken,
) -> SignalHandlerGuard {
    let handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = vtcode_core::shutdown::shutdown_signal() => {
                    let signal = request_local_stop(&ctrl_c_state, &ctrl_c_notify);

                    if matches!(signal, CtrlCSignal::Exit) {
                        // Fire-and-forget MCP shutdown with a tight timeout so
                        // the process exits immediately on double Ctrl+C.
                        if let Some(mcp_manager) = &async_mcp_manager {
                            let mcp = Arc::clone(mcp_manager);
                            tokio::spawn(async move {
                                let _ = tokio::time::timeout(
                                    std::time::Duration::from_millis(500),
                                    mcp.shutdown(),
                                ).await;
                            });
                        }
                        emergency_terminal_cleanup();
                        break;
                    }

                    // Cancel path: attempt MCP shutdown with a timeout so the
                    // signal handler stays responsive to a second Ctrl+C.
                    if let Some(mcp_manager) = &async_mcp_manager {
                        if let Err(e) = tokio::time::timeout(
                            std::time::Duration::from_secs(2),
                            mcp_manager.shutdown(),
                        ).await.unwrap_or(Ok(())) {
                            let error_msg = e.to_string();
                            if error_msg.contains("EPIPE")
                                || error_msg.contains("Broken pipe")
                                || error_msg.contains("write EPIPE")
                            {
                                tracing::debug!(
                                    "MCP client shutdown encountered pipe errors during interrupt (normal): {}",
                                    e
                                );
                            } else {
                                tracing::warn!("Failed to shutdown MCP client on interrupt: {}", e);
                            }
                        }
                    }
                }
                _ = cancel_token.cancelled() => {
                    break;
                }
            }
        }
    });
    SignalHandlerGuard::new(handle)
}

fn emergency_terminal_cleanup() {
    set_global_terminal_focused(false);
    let _ = vtcode_tui::panic_hook::restore_tui();
    vtcode_commons::trace_flush::flush_trace_log();
    std::process::exit(130);
}
