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
        Self { handle: Some(handle) }
    }
}

impl Drop for SignalHandlerGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

/// Spawn a signal handler task that listens for SIGINT and SIGTERM.
///
/// # Priority Guarantees
///
/// This signal handler is the highest priority component in the system.
/// It ensures that:
///
/// 1. **SIGINT (Ctrl+C) is always processed immediately** - The handler runs
///    on its own Tokio task and cannot be blocked by other operations.
///
/// 2. **First Ctrl+C cancels current operation** - Calls `request_local_stop()`
///    which transitions `CtrlCState` to `CancelRequested` and notifies waiters.
///
/// 3. **Second Ctrl+C exits the program** - If a second Ctrl+C arrives within
///    1 second, the handler calls `emergency_terminal_cleanup()` which:
///    - Restores the terminal to a usable state
///    - Flushes trace logs
///    - Calls `std::process::exit(130)` to immediately terminate the process
///
/// 4. **Emergency exit bypasses all other operations** - The `std::process::exit(130)`
///    call bypasses Rust's drop logic and async runtime shutdown, ensuring the
///    program exits immediately even if other tasks are blocked.
///
/// 5. **No signal masking** - SIGINT is never blocked or masked, ensuring the
///    OS can always deliver the signal to this handler.
///
/// 6. **MCP shutdown has tight timeout** - On double Ctrl+C, MCP shutdown is
///    fire-and-forget with a 500ms timeout to prevent blocking the exit.
///
/// # Signal Flow
///
/// 1. OS delivers SIGINT → Tokio runtime wakes up the signal handler task
/// 2. Signal handler calls `request_local_stop()` → `CtrlCState::register_signal()`
/// 3. If `CtrlCSignal::Exit` is returned → fire-and-forget MCP shutdown (500ms timeout)
/// 4. Call `emergency_terminal_cleanup()` → `restore_tui()` → `std::process::exit(130)`
///
/// # Emergency Terminal Cleanup
///
/// The `emergency_terminal_cleanup()` function ensures the terminal is left in
/// a usable state even on emergency exit. It:
/// - Disables terminal focus tracking
/// - Restores the TUI to its original state
/// - Flushes trace logs
/// - Terminates the process with exit code 130 (standard SIGINT exit code)
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

                    // Cancel path: fire-and-forget MCP shutdown so the signal
                    // handler loop immediately continues and can process a
                    // second Ctrl+C without delay.  The exit path (double
                    // Ctrl+C) also uses fire-and-forget, so this is consistent.
                    if let Some(mcp_manager) = &async_mcp_manager {
                        let mcp = Arc::clone(mcp_manager);
                        tokio::spawn(async move {
                            let _ = tokio::time::timeout(
                                std::time::Duration::from_secs(2),
                                mcp.shutdown(),
                            ).await;
                        });
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
    let _ = vtcode_ui::tui::panic_hook::restore_tui();
    vtcode_commons::trace_flush::flush_trace_log();
    std::process::exit(130);
}
