use std::sync::Arc;

use tokio::sync::Notify;

use crate::agent::runloop::unified::state::{CtrlCSignal, CtrlCState};

/// Request a local stop by setting the Ctrl+C state and notifying waiters.
///
/// # Priority Guarantee
///
/// This function is called from both the signal handler (SIGINT) and the TUI
/// interrupt handler (Ctrl+C key in raw mode). It ensures that:
///
/// 1. The CtrlCState is atomically set to CancelRequested or ExitRequested
/// 2. At least one waiter is notified (using notify_one to store a permit)
/// 3. The notification is not lost even if no task is currently waiting
///
/// # Why notify_one instead of notify_waiters?
///
/// `notify_waiters()` only wakes tasks that are CURRENTLY waiting. If no task
/// is waiting, the notification is lost entirely. This causes Ctrl+C to be
/// unresponsive during tool execution and agent thinking when no task is
/// polling the notification.
///
/// `notify_one()` stores a permit even when no task is waiting. The next task
/// that calls `.notified()` will immediately receive the notification. This
/// ensures Ctrl+C is always responsive.
pub(crate) fn request_local_stop(ctrl_c_state: &Arc<CtrlCState>, ctrl_c_notify: &Arc<Notify>) -> CtrlCSignal {
    let signal = ctrl_c_state.register_signal();
    // Use notify_one instead of notify_waiters to store a permit.
    // This ensures the notification is not lost when no task is waiting.
    ctrl_c_notify.notify_one();
    signal
}
