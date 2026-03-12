use std::sync::Arc;

use tokio::sync::Notify;

use crate::agent::runloop::unified::state::{CtrlCSignal, CtrlCState};

pub(crate) fn request_local_stop(
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> CtrlCSignal {
    let signal = ctrl_c_state.register_signal();
    ctrl_c_notify.notify_waiters();
    signal
}
