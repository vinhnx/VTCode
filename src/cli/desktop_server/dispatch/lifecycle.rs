//! Session lifecycle for request dispatch: cooperative runloop stops, session
//! shutdown, and idle configuration restarts that preserve the thread.

use super::*;
use crate::agent::runloop::unified::stop_requests;

/// Bound the cooperative stop wait on the session shutdown path. A turn stop
/// itself waits for readiness independently of this timeout.
const STOP_HANDLE_WAIT: Duration = Duration::from_secs(2);
const STOP_HANDLE_POLL: Duration = Duration::from_millis(10);

/// Fire the runloop's out-of-band stop signal, waiting briefly when the
/// runtime is still publishing its handles after shell setup.
async fn signal_runloop_stop(stop: &StopHandleSlot) {
    signal_runloop_stop_with_timeout(stop, STOP_HANDLE_WAIT).await;
}

pub async fn signal_runloop_stop_with_timeout(stop: &StopHandleSlot, wait: Duration) {
    if stop_requests::fire_stop_signal(stop) {
        return;
    }
    let _ = timeout(wait, async {
        loop {
            tokio::time::sleep(STOP_HANDLE_POLL).await;
            if stop_requests::fire_stop_signal(stop) {
                return;
            }
        }
    })
    .await;
}

pub async fn shutdown_current_session(context: &ServiceContext, cancel_active: bool) -> Result<()> {
    let (session_id, input, stop, active_turn, should_send_cancel, mut runtime) = {
        let mut state = context.state.lock().await;
        let active_turn = state.active_turn_id.clone();
        if active_turn.is_some() && !cancel_active {
            bail!("cancel the active turn and wait for cleanup before switching sessions");
        }
        let should_send_cancel = active_turn
            .as_ref()
            .is_some_and(|turn_id| state.cancel_requested_turn.as_deref() != Some(turn_id.as_str()));
        if should_send_cancel {
            state.cancel_requested_turn = active_turn.clone();
        }
        let runtime = state.runtime.take();
        let session_id = runtime.as_ref().map(|session| session.session_id.clone());
        let input = runtime.as_ref().map(|session| session.input.clone());
        let stop = runtime.as_ref().map(|session| session.stop.clone());
        (session_id, input, stop, active_turn, should_send_cancel, runtime)
    };

    if let Some(input) = input {
        if active_turn.is_some() {
            if should_send_cancel {
                if !input.is_closed()
                    && let Some(stop) = stop.as_ref()
                {
                    signal_runloop_stop(stop).await;
                }
                let _ = input.send(InlineEvent::Cancel);
            }
            let _ = timeout(SESSION_SHUTDOWN_TIMEOUT, wait_for_active_turn_to_finish(context)).await;
        }
        // Dual delivery mirrors the TUI exit callback: the queued event ends
        // an idle loop, while the signal also reaches a runloop paused in
        // initialization (which never polls the queue) so shutdown stays
        // graceful instead of waiting for the abort timeout.
        if let Some(stop) = stop.as_ref() {
            stop_requests::fire_exit_signal(stop);
        }
        let _ = input.send(InlineEvent::Exit);
    }

    if let Some(mut runtime) = runtime.take() {
        match timeout(RUNLOOP_EXIT_TIMEOUT, &mut runtime.exit).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) => {
                tracing::debug!(target: "vtcode.desktop", session_id = %runtime.session_id, error, "desktop runloop exited with an error")
            }
            Ok(Err(_)) => {}
            Err(_) => {
                runtime.abort.abort();
                tracing::warn!(target: "vtcode.desktop", session_id = %runtime.session_id, "desktop runloop did not stop before timeout");
            }
        }
        join_host_command_bridge(runtime.command_forwarder, RUNLOOP_EXIT_TIMEOUT).await;
    }

    if let Some(session_id) = session_id {
        let mut state = context.state.lock().await;
        if state.session_id.as_deref() == Some(session_id.as_str()) {
            state.session_id = None;
            state.restored_output_spools.clear();
            state.active_turn_id = None;
            state.cancel_requested_turn = None;
            // Ephemeral queue state dies with the session; a queued prompt is
            // only meaningful while its runtime can dispatch it.
            state.queue.clear();
            state.queue_dropped_total = 0;
            state.interaction_registry.close_session(&session_id);
            state.pending_interactions.retain(|_, pending| pending.session_id != session_id);
        }
        crate::main_helpers::set_runtime_archive_session_id(None);
    }
    context.turn_finished.notify_waiters();
    Ok(())
}

/// Restart the selected session's runtime under the same session id after an
/// idle configuration change, preserving the thread instead of discarding it.
///
/// Loads resume data before shutdown (the archive is current while idle, and a
/// load failure here leaves the live session untouched), shuts the runtime
/// down, applies `apply` to the shared state, then starts a fresh runtime
/// under the same id so the new configuration takes effect. Queued prompts and
/// restored-output spools are carried across the restart. With no session
/// selected, applies `apply` and starts nothing.
///
/// Requires an idle session: no active turn and no pending interactions. The
/// `config/*` transition guard enforces this before dispatch.
pub(super) async fn restart_session_preserving_thread(
    context: &ServiceContext,
    apply: impl FnOnce(&mut ServerState),
) -> Result<()> {
    let session_id = context.state.lock().await.session_id.clone();
    let Some(session_id) = session_id else {
        let mut state = context.state.lock().await;
        apply(&mut state);
        return Ok(());
    };
    let resume = load_resume_session(&session_id, ArchivedSessionIntent::ResumeInPlace).await?;
    let (queue, queue_dropped_total, restored_output_spools) = {
        let mut state = context.state.lock().await;
        (
            std::mem::take(&mut state.queue),
            std::mem::take(&mut state.queue_dropped_total),
            std::mem::take(&mut state.restored_output_spools),
        )
    };
    shutdown_current_session(context, false).await?;
    {
        let mut state = context.state.lock().await;
        apply(&mut state);
        state.queue = queue;
        state.queue_dropped_total = queue_dropped_total;
        state.restored_output_spools = restored_output_spools;
    }
    start_session(context, session_id, resume).await?;
    Ok(())
}

async fn wait_for_active_turn_to_finish(context: &ServiceContext) {
    loop {
        let notified = context.turn_finished.notified();
        if context.state.lock().await.active_turn_id.is_none() {
            return;
        }
        notified.await;
    }
}
