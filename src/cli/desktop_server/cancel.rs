use super::*;
use crate::agent::runloop::unified::stop_requests;

/// Wait for a starting runloop to accept turn cancellation, then fire.
/// Detached so the cancel ack never blocks on runloop startup.
pub(super) fn spawn_late_stop_signal(
    state: std::sync::Arc<Mutex<ServerState>>,
    stop: StopHandleSlot,
    input: mpsc::UnboundedSender<InlineEvent>,
    session_id: String,
    turn_id: String,
) {
    tokio::spawn(async move {
        tokio::select! {
            _ = stop_requests::wait_for_host_turns_ready(&stop) => {
                let state = state.lock().await;
                if cancel_is_current(&state, &session_id, &turn_id) {
                    stop_requests::fire_stop_signal(&stop);
                }
            }
            _ = input.closed() => {}
        }
    });
}

pub(super) fn cancel_is_current(state: &ServerState, session_id: &str, turn_id: &str) -> bool {
    state.session_id.as_deref() == Some(session_id)
        && state.active_turn_id.as_deref() == Some(turn_id)
        && state.cancel_requested_turn.as_deref() == Some(turn_id)
        && state.runtime.is_some()
}

/// Re-assert a cancel signal after `delay` when the same turn is still
/// active and still cancel-requested. Closes the Submit-reset race for
/// single presses; the gate keeps a late task from touching a later turn.
pub(super) fn spawn_cancel_refire(
    state: std::sync::Arc<Mutex<ServerState>>,
    stop: StopHandleSlot,
    session_id: String,
    turn_id: String,
    delay: Duration,
) {
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let state = state.lock().await;
        if cancel_is_current(&state, &session_id, &turn_id) {
            stop_requests::fire_stop_signal(&stop);
        }
    });
}
