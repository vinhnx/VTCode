//! Turn lifecycle: submit, queue, drain, and take-latest.
//!
//! Busy-time submissions queue behind the running turn and drain FIFO on
//! terminal events through the same batching core as the TUI
//! (`crate::agent::runloop::unified::{push_capped, pop_batched_submission_recording}`).

use super::*;
use crate::agent::runloop::unified::{MAX_QUEUED_INPUTS, pop_batched_submission_recording, push_capped, stop_requests};
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
        entry_id: String,
    },
}

/// A queued batch popped for dispatch, with its recorded turn id and the live
/// runtime input it must cross.
pub(super) struct DrainedTurn {
    pub(super) turn_id: String,
    pub(super) batch: QueuedInput,
    pub(super) original_entries: Vec<QueuedInput>,
    pub(super) input: mpsc::UnboundedSender<InlineEvent>,
}

impl ServerState {
    /// Snapshot the authoritative queue state for `turn/queue-status`.
    pub(super) fn queue_status(&self, session_id: &str) -> protocol::TurnQueueStatus {
        let bounded = self.queue.iter().take(protocol::MAX_QUEUE_STATUS_PREVIEWS);
        protocol::TurnQueueStatus {
            session_id: session_id.to_string(),
            queue_depth: self.queue.len(),
            previews: bounded
                .clone()
                .map(|queued| protocol::queue_preview_label(&queued.input.text))
                .collect(),
            entry_ids: bounded.clone().map(|queued| queued.entry_id()).collect(),
            queued_for_secs: bounded.map(|queued| queued.wait_secs()).collect(),
            dropped_total: self.queue_dropped_total,
            dispatched_entry_ids: Some(Vec::new()),
            discarded_entry_ids: Vec::new(),
        }
    }

    /// Mint a fresh bounded entry id for a queue entry stamp.
    pub(super) fn mint_entry_id() -> String {
        format!("qe-{}", uuid::Uuid::new_v4())
    }

    /// Take the active turn exactly once: clears the cancel request and
    /// resolves the turn's interactions. Whoever takes `Some` owns the single
    /// terminal emission for the turn — the pump terminal path, pump exit, and
    /// cancel escalation race through here.
    pub(super) fn take_active_turn(&mut self, session_id: &str) -> Option<String> {
        let turn_id = self.active_turn_id.take()?;
        self.cancel_requested_turn = None;
        self.interaction_registry.finish_turn(session_id, &turn_id);
        self.pending_interactions
            .retain(|_, interaction| interaction.session_id != session_id || interaction.turn_id != turn_id);
        Some(turn_id)
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
        if self.session_transitioning || self.active_turn_id.is_some() || self.queued_steer_entry_id.is_some() {
            return None;
        }
        let mut original_entries = Vec::new();
        let batch = pop_batched_submission_recording(&mut self.queue, &mut false, |entry| {
            // A text batch can consume several identities. Retain its
            // original rows so a failed handoff can restore each one.
            if entry.batchable && !entry.input.has_attachments() {
                original_entries.push(entry.clone());
            }
        })?;
        let turn_id = uuid::Uuid::new_v4().to_string();
        self.cancel_requested_turn = None;
        self.active_turn_id = Some(turn_id.clone());
        Some(DrainedTurn { turn_id, batch, original_entries, input })
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
        if state.active_turn_id.is_none() && state.queued_steer_entry_id.is_none() {
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
            if state.queue.len() >= MAX_QUEUED_INPUTS && state.queued_steer_entry_id.is_some() {
                bail!("the queue is full while a steering request is pending");
            }
            let entry_id = ServerState::mint_entry_id();
            let discarded_entry_ids = state
                .queue
                .iter()
                .take(state.queue.len().saturating_add(1).saturating_sub(MAX_QUEUED_INPUTS))
                .map(QueuedInput::entry_id)
                .collect();
            let dropped = push_capped(
                &mut state.queue,
                QueuedInput::new(submission, None).stamped(entry_id.clone(), Instant::now()),
            );
            state.queue_dropped_total += dropped;
            let mut status = state.queue_status(session_id);
            status.discarded_entry_ids = discarded_entry_ids;
            EnqueuePlan::Queued { status, dropped, entry_id }
        } else {
            bail!("another turn is already running");
        }
    };
    match plan {
        EnqueuePlan::Queued { status, dropped, entry_id } => {
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
                entry_id: Some(entry_id),
                discarded_entry_ids: status.discarded_entry_ids,
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
                entry_id: None,
                discarded_entry_ids: Vec::new(),
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
                if let Err(error) =
                    dispatch_drained_turn(&state, &notifications, &turn_finished, &session_id, drained).await
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
    DrainedTurn { turn_id, batch, original_entries, input }: DrainedTurn,
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
        // order. A concurrent refill can temporarily exceed the soft cap;
        // the next accept trims from the front with exact accounting.
        if guard.session_id.as_deref() == Some(session_id) {
            if original_entries.is_empty() {
                guard.queue.push_front(batch);
            } else {
                for original in original_entries.into_iter().rev() {
                    guard.queue.push_front(original);
                }
            }
            let status = guard.queue_status(session_id);
            drop(guard);
            publish_queue_status_sync(notifications, &status);
        }
        turn_finished.notify_waiters();
        bail!("selected session runtime has stopped; reload the session to continue");
    }
    let generation = guard.runtime_generation;
    let mut status = guard.queue_status(session_id);
    status.dispatched_entry_ids = Some(if original_entries.is_empty() {
        vec![batch.entry_id()]
    } else {
        original_entries.iter().map(QueuedInput::entry_id).collect()
    });
    drop(guard);
    send_runtime_notification(
        notifications,
        protocol::method::TURN_STATUS,
        json!({ "sessionId": session_id, "turnId": turn_id, "status": "running" }),
        generation,
    )
    .await?;
    send_runtime_notification(
        notifications,
        protocol::method::TURN_QUEUE_STATUS,
        serde_json::to_value(status)?,
        generation,
    )
    .await?;
    Ok(())
}

pub(super) async fn edit_queued_turn(context: &ServiceContext, params: protocol::TurnQueueEditParams) -> Result<Value> {
    // Keep the wire bounded: a queued prompt uses the same per-message
    // budget as the batch drain so one edit cannot grow the queue
    // without limit.
    const MAX_EDIT_BYTES: usize = 64 * 1024;
    if params.entry_id.is_empty() || params.entry_id.len() > protocol::MAX_QUEUE_ENTRY_ID_LEN {
        bail!("invalid queue entry identity");
    }
    if params.text.is_empty() || params.text.len() > MAX_EDIT_BYTES {
        bail!("edited prompt is empty or too long");
    }
    let (result, status) = {
        let mut state = context.state.lock().await;
        if state.session_id.as_deref() != Some(params.session_id.as_str()) {
            bail!("session is not selected");
        }
        if state.queued_steer_entry_id.as_deref() == Some(params.entry_id.as_str()) {
            bail!("the queued prompt is awaiting a steering result");
        }
        let edited = state
            .queue
            .iter_mut()
            .find(|queued| queued.entry_id() == params.entry_id)
            .is_some_and(|queued| {
                queued.input.text = params.text.clone();
                true
            });
        let status = edited.then(|| state.queue_status(&params.session_id));
        (
            protocol::TurnQueueEditResult {
                session_id: params.session_id,
                edited,
                queue_depth: state.queue.len(),
            },
            status,
        )
    };
    if let Some(status) = status {
        publish_queue_status_sync(&context.notifications, &status);
    }
    Ok(serde_json::to_value(result)?)
}

pub(super) async fn take_latest_queued_turn(
    context: &ServiceContext,
    params: protocol::TurnQueueTakeLatestParams,
) -> Result<Value> {
    let (result, status) = {
        let mut state = context.state.lock().await;
        if state.session_id.as_deref() != Some(params.session_id.as_str()) {
            bail!("session is not selected");
        }
        if state.queued_steer_entry_id.is_some() {
            bail!("the queued prompt is awaiting a steering result");
        }
        // Text-only restore mirrors the CLI queue edit: attachments
        // stay with their dispatched turn and the composer re-attaches
        // on resubmit.
        let popped = state.queue.pop_back();
        let result = protocol::TurnQueueTakeLatestResult {
            session_id: params.session_id,
            queue_depth: state.queue.len(),
            entry_id: popped.as_ref().map(QueuedInput::entry_id),
            text: popped.map(|entry| entry.input.text),
        };
        let status = result.text.is_some().then(|| state.queue_status(&result.session_id));
        (result, status)
    };
    if let Some(status) = status {
        publish_queue_status_sync(&context.notifications, &status);
    }
    Ok(serde_json::to_value(result)?)
}

pub(super) async fn remove_queued_turn(
    context: &ServiceContext,
    params: protocol::TurnQueueRemoveParams,
) -> Result<Value> {
    let (result, status) = {
        let mut state = context.state.lock().await;
        if state.session_id.as_deref() != Some(params.session_id.as_str()) {
            bail!("session is not selected");
        }
        if state.queued_steer_entry_id.as_deref() == Some(params.entry_id.as_str()) {
            bail!("the queued prompt is awaiting a steering result");
        }
        // Accepting removes never dispatch, so an empty queue stays a
        // clean `removed: false` instead of an error: a stale id (a
        // concurrent drain won the race) is a normal outcome the
        // client reconciles from the published snapshot.
        let position = state.queue.iter().position(|queued| queued.entry_id() == params.entry_id);
        let removed = position.is_some();
        if let Some(position) = position {
            state.queue.remove(position);
        }
        let status = removed.then(|| state.queue_status(&params.session_id));
        let result = protocol::TurnQueueRemoveResult {
            session_id: params.session_id,
            entry_id: Some(params.entry_id),
            removed,
            queue_depth: state.queue.len(),
        };
        (result, status)
    };
    if let Some(status) = status {
        publish_queue_status_sync(&context.notifications, &status);
    }
    Ok(serde_json::to_value(result)?)
}
