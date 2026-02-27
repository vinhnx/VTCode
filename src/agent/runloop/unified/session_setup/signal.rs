use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::state::{CtrlCSignal, CtrlCState};
use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

pub(crate) fn spawn_signal_handler(
    ctrl_c_state: Arc<CtrlCState>,
    ctrl_c_notify: Arc<Notify>,
    async_mcp_manager: Option<Arc<AsyncMcpManager>>,
    cancel_token: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
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
    })
}

fn emergency_terminal_cleanup() {
    let _ = vtcode_tui::panic_hook::restore_tui();
}
