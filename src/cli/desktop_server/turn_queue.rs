//! Turn lifecycle: submit, queue, drain, and take-latest.
//!
//! Busy-time submissions queue behind the running turn and drain FIFO on
//! terminal events through the same batching core as the TUI
//! (`crate::agent::runloop::unified::{push_capped, pop_batched_submission}`).

use super::*;
use crate::agent::runloop::unified::{pop_batched_submission, push_capped, stop_requests};
use vtcode_ui::tui::app::SubmittedInput;

/// How a turn submission behaves when another turn is already running.
#[derive(Debug, Clone, Copy)]
pub(super) enum EnqueueMode {
    /// Reject a busy session. WebMCP turns carry a runtime turn id the
    /// browser tracks, so they cannot wait in the queue.
    Immediate,
    /// Queue behind the running turn. `batchable` marks Ctrl+Enter-style
    /// submissions eligible for single-turn coalescing on drain.
    Queueable {
        /// Whether the queued submission may merge with adjacent text-only
        /// submissions into one turn.
        batchable: bool,
    },
}

impl EnqueueMode {
    fn queueable(self) -> bool {
        matches!(self, Self::Queueable { .. })
    }
}

/// Busy/idle decision made under the state lock; notifications happen after
/// the lock is released.
enum EnqueuePlan {
    Immediate {
        turn_id: String,
    },
    Queued {
        status: protocol::TurnQueueStatus,
        dropped: usize,
    },
}

/// A queued batch popped for dispatch, with its recorded turn id and the live
/// runtime input it must cross.
pub(super) struct DrainedTurn {
    pub(super) turn_id: String,
    pub(super) batch: QueuedInput,
    pub(super) input: mpsc::UnboundedSender<InlineEvent>,
}

impl ServerState {
    /// Snapshot the authoritative queue state for `turn/queue-status`.
    pub(super) fn queue_status(&self, session_id: &str) -> protocol::TurnQueueStatus {
        protocol::TurnQueueStatus {
            session_id: session_id.to_string(),
            queue_depth: self.queue.len(),
            previews: self
                .queue
                .iter()
                .take(protocol::MAX_QUEUE_STATUS_PREVIEWS)
                .map(|queued| protocol::queue_preview_label(&queued.input.text))
                .collect(),
            dropped_total: self.queue_dropped_total,
        }
    }

    /// Pop the next batch for dispatch after a terminal event, recording its
    /// fresh turn id. Returns the dispatch or `None` when there is nothing to
    /// dispatch or the session is busy again. Callers send `Submit` after
    /// releasing the lock.
    pub(super) fn pop_next_queued_turn(&mut self) -> Option<DrainedTurn> {
        let input = self
            .runtime
            .as_ref()
            .filter(|runtime| !runtime.input.is_closed())
            .map(|runtime| runtime.input.clone())?;
        if self.session_transitioning || self.active_turn_id.is_some() {
            return None;
        }
        let batch = pop_batched_submission(&mut self.queue, &mut false)?;
        let turn_id = uuid::Uuid::new_v4().to_string();
        self.cancel_requested_turn = None;
        self.active_turn_id = Some(turn_id.clone());
        Some(DrainedTurn { turn_id, batch, input })
    }
}

pub(super) async fn enqueue_turn(
    state: &Mutex<ServerState>,
    notifications: &mpsc::Sender<protocol::Notification>,
    session_id: &str,
    prompt: String,
    attachment_parts: Vec<ContentPart>,
    mode: EnqueueMode,
) -> Result<protocol::TurnAccepted> {
    if prompt.trim().is_empty() {
        bail!("prompt must not be empty");
    }
    let turn_id = uuid::Uuid::new_v4().to_string();
    let mut submission = SubmittedInput::new(prompt, attachment_parts);
    if matches!(mode, EnqueueMode::Queueable { batchable: true }) {
        submission = submission.batchable();
    }
    // The busy/idle decision and its mutation share one lock acquisition so a
    // concurrent submit cannot slip an immediate turn ahead of a queued one.
    let plan = {
        let mut state = state.lock().await;
        if state.session_transitioning {
            bail!("a session change is in progress");
        }
        if state.session_id.as_deref() != Some(session_id) {
            bail!("session is not selected");
        }
        if state.active_turn_id.is_none() {
            if !state.pending_interactions.is_empty() {
                if state.pending_interactions.values().all(|interaction| {
                    interaction.session_id == session_id && interaction.turn_id == protocol::SESSION_START_TURN_ID
                }) {
                    return Err(StartupInteractionPending.into());
                }
                bail!("resolve the pending CLI interaction before submitting a turn");
            }
            let runtime = state
                .runtime
                .as_ref()
                .ok_or_else(|| anyhow!("selected session is not running"))?;
            if runtime.input.is_closed() {
                bail!("selected session runtime has stopped; reload the session to continue");
            }
            let input = runtime.input.clone();
            // Clear the previous turn's handled stop before exposing the new
            // turn to cancellation, and preserve Submit/Cancel ordering.
            stop_requests::reset_handled_stop(&runtime.stop);
            input
                .send(InlineEvent::Submit(submission))
                .map_err(|_error| anyhow!("selected session runtime has stopped; reload the session to continue"))?;
            state.cancel_requested_turn = None;
            state.active_turn_id = Some(turn_id.clone());
            EnqueuePlan::Immediate { turn_id }
        } else if mode.queueable() {
            // A turn is running: queue behind it with CLI batching semantics
            // instead of rejecting. Pending interactions do not block
            // queueing; the prompt dispatches once the turn ends (CLI parity:
            // the TUI queues regardless of overlays).
            let runtime = state.runtime.as_ref().filter(|runtime| !runtime.input.is_closed());
            if runtime.is_none() {
                bail!("selected session runtime has stopped; reload the session to continue");
            }
            let dropped = push_capped(&mut state.queue, QueuedInput::new(submission, None));
            state.queue_dropped_total += dropped;
            EnqueuePlan::Queued { status: state.queue_status(session_id), dropped }
        } else {
            bail!("another turn is already running");
        }
    };
    match plan {
        EnqueuePlan::Queued { status, dropped } => {
            publish_queue_status_sync(notifications, &status);
            Ok(protocol::TurnAccepted {
                session_id: session_id.to_string(),
                // A queued prompt owns no turn yet; the dispatch turn id is
                // minted when its batch leaves the queue. Clients branch on
                // `queued` before reading `turn_id`.
                turn_id: String::new(),
                queued: true,
                queue_depth: status.queue_depth,
                dropped,
            })
        }
        EnqueuePlan::Immediate { turn_id } => {
            // The prompt has crossed the runtime boundary. Status is advisory: waiting
            // for this queue here can deadlock the request loop, and reporting failure
            // after delivery could cause a client to submit the same prompt twice.
            if let Err(error) = notifications.try_send(protocol::Notification {
                jsonrpc: "2.0".to_string(),
                method: protocol::method::TURN_STATUS.to_string(),
                params: json!({ "sessionId": session_id, "turnId": turn_id, "status": "running" }),
            }) {
                tracing::warn!(target: "vtcode.desktop", %error, "could not publish turn status");
            }
            Ok(protocol::TurnAccepted {
                session_id: session_id.to_string(),
                turn_id,
                queued: false,
                queue_depth: 0,
                dropped: 0,
            })
        }
    }
}

/// Publish a queue snapshot without awaiting: the request loop must never
/// block on the bounded notification queue (see `enqueue_turn`).
pub(super) fn publish_queue_status_sync(
    notifications: &mpsc::Sender<protocol::Notification>,
    status: &protocol::TurnQueueStatus,
) {
    let params = serde_json::to_value(status).unwrap_or(Value::Null);
    debug_assert!(params.is_object(), "queue status must serialize to an object");
    if let Err(error) = notifications.try_send(protocol::Notification {
        jsonrpc: "2.0".to_string(),
        method: protocol::method::TURN_QUEUE_STATUS.to_string(),
        params,
    }) {
        tracing::warn!(target: "vtcode.desktop", %error, "could not publish turn queue status");
    }
}

/// Resume a queue preserved across a runtime restart only after startup and
/// the session transition have both completed.
pub(super) fn spawn_resume_queue(
    session_id: String,
    generation: uuid::Uuid,
    stop: StopHandleSlot,
    input: mpsc::UnboundedSender<InlineEvent>,
    state: std::sync::Arc<Mutex<ServerState>>,
    notifications: mpsc::Sender<protocol::Notification>,
    turn_finished: std::sync::Arc<Notify>,
) {
    tokio::spawn(async move {
        tokio::select! {
            _ = stop_requests::wait_for_host_turns_ready(&stop) => {}
            _ = input.closed() => return,
        }
        loop {
            let notified = turn_finished.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let drained = {
                let mut state = state.lock().await;
                if state.session_id.as_deref() != Some(session_id.as_str())
                    || state.runtime_generation != Some(generation)
                {
                    return;
                }
                if state.session_transitioning
                    || state.active_turn_id.is_some()
                    || !state.pending_interactions.is_empty()
                {
                    None
                } else if state.queue.is_empty() {
                    return;
                } else {
                    state.pop_next_queued_turn()
                }
            };
            if let Some(drained) = drained {
                if let Err(error) = dispatch_drained_turn(
                    &state,
                    &notifications,
                    &turn_finished,
                    &session_id,
                    drained.turn_id,
                    drained.batch,
                    drained.input,
                )
                .await
                {
                    tracing::warn!(target: "vtcode.desktop", %error, "failed to resume queued turn");
                    continue;
                }
                return;
            }
            tokio::select! {
                _ = &mut notified => {}
                _ = input.closed() => return,
            }
        }
    });
}

/// Submit the batch drained after a terminal event. The turn id is already
/// recorded under the state lock; this only crosses the runtime boundary and
/// publishes the new turn plus the updated queue state. On delivery failure
/// the batch is restored at the front so no queued prompt is lost.
pub(super) async fn dispatch_drained_turn(
    state: &Mutex<ServerState>,
    notifications: &mpsc::Sender<protocol::Notification>,
    turn_finished: &Notify,
    session_id: &str,
    turn_id: String,
    batch: QueuedInput,
    input: mpsc::UnboundedSender<InlineEvent>,
) -> Result<()> {
    // Recheck ownership and deliver while holding the turn lock. A session
    // transition may have started while the terminal notification was queued.
    let mut guard = state.lock().await;
    let current = guard.session_id.as_deref() == Some(session_id)
        && !guard.session_transitioning
        && guard.active_turn_id.as_deref() == Some(turn_id.as_str())
        && guard.runtime.as_ref().is_some_and(|runtime| runtime.input.same_channel(&input));
    if !current || input.send(InlineEvent::Submit(batch.input.clone())).is_err() {
        if guard.active_turn_id.as_deref() == Some(turn_id.as_str()) {
            guard.active_turn_id = None;
            guard.cancel_requested_turn = None;
        }
        // Restore unless the session moved on: teardown intentionally drops
        // ephemeral queue state with the session, but a changed turn slot
        // (e.g. a duplicate terminal event cleared it) must not lose the
        // batch. The batch left from the front; restoring it there preserves
        // order. A concurrent refill can transiently overshoot the soft cap
        // by one; the next accept trims from the front with exact accounting.
        if guard.session_id.as_deref() == Some(session_id) {
            guard.queue.push_front(batch);
            let status = guard.queue_status(session_id);
            drop(guard);
            publish_queue_status_sync(notifications, &status);
        }
        turn_finished.notify_waiters();
        bail!("selected session runtime has stopped; reload the session to continue");
    }
    let generation = guard.runtime_generation;
    drop(guard);
    send_runtime_notification(
        notifications,
        protocol::method::TURN_STATUS,
        json!({ "sessionId": session_id, "turnId": turn_id, "status": "running" }),
        generation,
    )
    .await?;
    let status = state.lock().await.queue_status(session_id);
    send_runtime_notification(
        notifications,
        protocol::method::TURN_QUEUE_STATUS,
        serde_json::to_value(status)?,
        generation,
    )
    .await?;
    Ok(())
}
