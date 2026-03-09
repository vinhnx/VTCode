use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::state::{CtrlCSignal, CtrlCState};
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
                    let signal = ctrl_c_state.register_signal();
                    ctrl_c_notify.notify_waiters();

                    if let Some(mcp_manager) = &async_mcp_manager
                        && let Err(e) = mcp_manager.shutdown().await
                    {
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

                    if matches!(signal, CtrlCSignal::Exit) {
                        emergency_terminal_cleanup();
                        break;
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
}
