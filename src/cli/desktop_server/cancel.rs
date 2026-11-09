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

/// Bounded waits for one cancel escalation. Production reuses the shutdown
/// path's bounds; tests inject short deterministic ones.
#[derive(Debug, Clone, Copy)]
pub(super) struct EscalationWaits {
    pub(super) grace: Duration,
    pub(super) turn_wait: Duration,
    pub(super) exit_wait: Duration,
}

/// Five seconds of cooperative cancellation before the watchdog tears the
/// runtime down and rebuilds it from durable history.
pub(super) const CANCEL_ESCALATION: EscalationWaits = EscalationWaits {
    grace: Duration::from_secs(5),
    turn_wait: SESSION_SHUTDOWN_TIMEOUT,
    exit_wait: RUNLOOP_EXIT_TIMEOUT,
};

#[cfg(test)]
pub(super) fn escalation_waits_for(state: &ServerState) -> EscalationWaits {
    state.escalation_waits.unwrap_or(CANCEL_ESCALATION)
}

#[cfg(not(test))]
pub(super) fn escalation_waits_for(_state: &ServerState) -> EscalationWaits {
    CANCEL_ESCALATION
}

/// Owned escalation inputs: everything `start_session` needs, cloned out of
/// the request context so the watchdog task is `'static`.
struct EscalationContext {
    startup: StartupContext,
    workspace: PathBuf,
    state: std::sync::Arc<Mutex<ServerState>>,
    notifications: mpsc::Sender<protocol::Notification>,
    turn_finished: std::sync::Arc<Notify>,
}

impl EscalationContext {
    fn service(&self) -> ServiceContext {
        ServiceContext {
            startup: self.startup.clone(),
            workspace: self.workspace.clone(),
            state: self.state.clone(),
            notifications: self.notifications.clone(),
            turn_finished: self.turn_finished.clone(),
            // The restart only replaces the session runtime; a running WebMCP
            // bridge keeps its own slot on the real service context.
            webmcp: Mutex::new(None),
        }
    }
}

/// Holds `session_transitioning` across an escalation so submits, drains, and
/// session changes serialize against the teardown and restart. The gate
/// releases when the guard drops unless the escalation defuses it to keep
/// submissions blocked after an unconfirmed termination.
struct EscalationGuard {
    state: std::sync::Arc<Mutex<ServerState>>,
    turn_finished: std::sync::Arc<Notify>,
    defused: bool,
}

impl EscalationGuard {
    fn claim(state: std::sync::Arc<Mutex<ServerState>>, turn_finished: std::sync::Arc<Notify>) -> Self {
        Self { state, turn_finished, defused: false }
    }

    fn defuse(mut self) {
        self.defused = true;
    }
}

impl Drop for EscalationGuard {
    fn drop(&mut self) {
        if self.defused {
            return;
        }
        let state = self.state.clone();
        let turn_finished = self.turn_finished.clone();
        // Detached: `drop` cannot await, and every escalation return path —
        // including early-outs — must release the gate it claimed.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                state.lock().await.session_transitioning = false;
                turn_finished.notify_waiters();
            });
        }
    }
}

/// Watch one cancelled turn: if the same turn is still active after the grace
/// period, force-stop its foreground execution, await bounded runtime
/// shutdown, and rebuild the session from durable history. Queued prompts are
/// preserved in place and dispatch once the replacement runtime is ready.
pub(super) fn spawn_cancel_watchdog(
    context: &ServiceContext,
    session_id: String,
    turn_id: String,
    waits: EscalationWaits,
) {
    let context = EscalationContext {
        startup: context.startup.clone(),
        workspace: context.workspace.clone(),
        state: context.state.clone(),
        notifications: context.notifications.clone(),
        turn_finished: context.turn_finished.clone(),
    };
    tokio::spawn(async move {
        tokio::time::sleep(waits.grace).await;
        if let Err(error) = escalate_stalled_turn(&context, &session_id, &turn_id, waits).await {
            tracing::warn!(target: "vtcode.desktop", %error, "cancel escalation failed");
        }
    });
}

async fn escalate_stalled_turn(
    context: &EscalationContext,
    session_id: &str,
    turn_id: &str,
    waits: EscalationWaits,
) -> Result<()> {
    // Claim the escalation under the same lock acquisition as the
    // still-stalled recheck so normal completion always wins the race.
    let (input, stop) = {
        let mut state = context.state.lock().await;
        if !cancel_is_current(&state, session_id, turn_id) || state.session_transitioning {
            return Ok(());
        }
        let Some(runtime) = state.runtime.as_ref() else {
            return Ok(());
        };
        let snapshot = (runtime.input.clone(), runtime.stop.clone());
        state.session_transitioning = true;
        snapshot
    };
    let guard = EscalationGuard::claim(context.state.clone(), context.turn_finished.clone());

    // Load durable history before teardown: the wedged runtime cannot persist,
    // so the archive is current, and a load failure must skip the restart
    // without skipping the teardown the stuck turn still needs.
    let mut restart = true;
    let resume = match load_resume_session(session_id, ArchivedSessionIntent::ResumeInPlace).await {
        Ok(resume) => resume,
        Err(error) => {
            tracing::warn!(target: "vtcode.desktop", %error, "cancel escalation proceeds without restart; history reload failed");
            restart = false;
            None
        }
    };

    // Force-stop foreground execution and owned processes, then re-assert the
    // cooperative signals in case the first delivery was consumed or missed.
    let _ = input.send(InlineEvent::ForceCancelPtySession);
    stop_requests::fire_stop_signal(&stop);
    let _ = input.send(InlineEvent::Cancel);

    // A turn that clears inside this window resolved cooperatively after all;
    // its runtime is healthy, so keep it instead of restarting.
    let _ =
        timeout(waits.turn_wait, wait_for_turn_to_clear(&context.state, &context.turn_finished, session_id, turn_id))
            .await;
    let runtime = {
        let mut state = context.state.lock().await;
        if state.active_turn_id.as_deref() != Some(turn_id) {
            return Ok(());
        }
        // Take the runtime before the exit handshake so late cooperative
        // signals fence themselves off and no new turn can dispatch onto the
        // dying channels.
        state.runtime.take()
    };
    let Some(mut runtime) = runtime else {
        // A direct teardown (process exit) raced the claim; it owns cleanup.
        return Ok(());
    };
    if runtime.session_id.as_str() != session_id {
        // Defensive: never tear down a runtime this escalation did not claim.
        context.state.lock().await.runtime = Some(runtime);
        return Ok(());
    }

    stop_requests::fire_exit_signal(&runtime.stop);
    let _ = runtime.input.send(InlineEvent::Exit);
    let mut exited = timeout(waits.exit_wait, &mut runtime.exit).await.is_ok();
    if !exited {
        runtime.abort.abort();
        tracing::warn!(target: "vtcode.desktop", session_id, turn_id, "cancel escalation aborted a runtime that ignored exit");
        exited = timeout(waits.exit_wait, &mut runtime.exit).await.is_ok();
    }
    let input = runtime.input.clone();
    let forwarder_done = join_host_command_bridge(runtime.command_forwarder, waits.exit_wait).await;
    if !(exited && input.is_closed() && forwarder_done) {
        tracing::error!(
            target: "vtcode.desktop",
            session_id,
            turn_id,
            "cancel escalation could not confirm termination; submissions stay blocked until the service restarts"
        );
        guard.defuse();
        return Ok(());
    }

    // Termination is confirmed. Take the turn exactly once — a racing pump
    // exit may have taken it first — then rebuild the session from history.
    let stopped = {
        let mut state = context.state.lock().await;
        if state.active_turn_id.as_deref() == Some(turn_id) {
            state.take_active_turn(session_id)
        } else {
            None
        }
    };
    if restart {
        let service = context.service();
        if let Err(error) = start_session(&service, session_id.to_string(), resume).await {
            tracing::error!(target: "vtcode.desktop", %error, "cancel escalation tore down the runtime but the restart failed; reload the session to continue");
            restart = false;
        }
    }
    if stopped.is_some() {
        // Untagged: this outcome must survive the restart's generation bump.
        // The client keys it by turn id, so it can never match a newer turn.
        let _ = send_notification(
            &context.notifications,
            protocol::method::TURN_STATUS,
            json!({ "sessionId": session_id, "turnId": turn_id, "status": "failed", "restarted": restart }),
        )
        .await;
        let _ = send_notification(
            &context.notifications,
            protocol::method::UI_PROGRESS,
            json!({ "sessionId": session_id, "active": false }),
        )
        .await;
    }
    Ok(())
}

async fn wait_for_turn_to_clear(
    state: &std::sync::Arc<Mutex<ServerState>>,
    turn_finished: &std::sync::Arc<Notify>,
    session_id: &str,
    turn_id: &str,
) {
    loop {
        let notified = turn_finished.notified();
        let cleared = {
            let state = state.lock().await;
            state.session_id.as_deref() != Some(session_id) || state.active_turn_id.as_deref() != Some(turn_id)
        };
        if cleared {
            return;
        }
        notified.await;
    }
}
