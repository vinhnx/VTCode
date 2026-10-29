//! Event recording utilities for the agent runner.

mod context;
mod lifecycle;
mod recorder;
pub use context::{ExecutionContextTracker, decision_completed_event, validate_decision_input};
pub use lifecycle::{
    SharedLifecycleEmitter, ToolOutputPayload, error_item_completed_event, file_change_completed_event,
    tool_invocation_completed_event, tool_output_completed_event, tool_output_item_id, tool_output_payload_from_value,
    tool_output_started_event, tool_output_updated_event, tool_started_event,
};
pub use recorder::{ActiveCommandHandle, ActiveToolHandle, ExecEventRecorder};

use crate::core::threads::{SubmissionId, ThreadRuntimeHandle};
use crate::exec::events::{
    CommandExecutionItem, CommandExecutionStatus, CompactionMode, CompactionTrigger, EVENT_SCHEMA_VERSION,
    HarnessEventItem, HarnessEventKind, ItemCompletedEvent, ItemStartedEvent, ThreadCompactBoundaryEvent,
    ThreadCompletedEvent, ThreadCompletionSubtype, ThreadEvent, ThreadItem, ThreadItemDetails, ThreadStartedEvent,
    ToolOutcome, TurnBlockedEvent, TurnCompletedEvent, TurnFailedEvent, TurnStartedEvent, Usage, VersionedThreadEvent,
    tool_outcome_from_status,
};
use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;
use std::io::{self, Write};
use std::mem::size_of;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TrySendError};
use tokio::sync::OnceCell;
use tokio::task::JoinHandle;
use tokio::task::spawn_blocking;
use uuid::Uuid;

use vtcode_memory::event_log::DEFAULT_MAX_EVENTS;

const SESSION_STORE_DRAIN_CAPACITY: usize = 8192;
const SESSION_STORE_DRAIN_MAX_BYTES: usize = 16 * 1024 * 1024;
const EXPLANATION_CACHE_MAX_BYTES: usize = 32 * 1024 * 1024;

/// Callback type alias for streaming structured events.
pub type EventSink = Arc<Mutex<Box<dyn FnMut(&ThreadEvent) + Send>>>;

/// Ordered validation of canonical evidence identities within the current task.
pub type DecisionEvidenceValidator =
    Arc<dyn Fn(String, Vec<String>) -> futures::future::BoxFuture<'static, Result<()>> + Send + Sync>;

fn decision_validator(state: Arc<SessionStoreSinkState>) -> DecisionEvidenceValidator {
    Arc::new(move |task, ids| {
        let state = Arc::clone(&state);
        Box::pin(async move {
            anyhow::ensure!(
                !state.health.failed.load(Ordering::Acquire),
                "canonical persistence failed; decision evidence unavailable"
            );
            let (reply, receiver) = tokio::sync::oneshot::channel();
            state
                .sender
                .lock()
                .as_ref()
                .context("canonical event sink is closed")?
                .try_send(SessionStoreRequest::ValidateDecision(task, ids, reply))
                .map_err(|error| anyhow!("canonical event queue unavailable; retry decision recording: {error}"))?;
            receiver.await.context("decision evidence drain unavailable")?
        })
    })
}

fn matrix_persistence(state: Arc<SessionStoreSinkState>) -> crate::subagents::matrix::MatrixPersistence {
    let write_state = Arc::clone(&state);
    crate::subagents::matrix::MatrixPersistence {
        persist: Arc::new(move |snapshot| {
            let state = Arc::clone(&write_state);
            Box::pin(async move {
                anyhow::ensure!(!state.health.failed.load(Ordering::Acquire), "canonical persistence failed");
                let event = ThreadEvent::MatrixUpdated(Box::new(snapshot));
                let queued = prepare_queued_session_event(&state, &event)?;
                let reserved_bytes = queued.reserved_bytes;
                let (reply, receiver) = tokio::sync::oneshot::channel();
                let sent = state
                    .sender
                    .lock()
                    .as_ref()
                    .context("canonical event sink is closed")
                    .and_then(|sender| {
                        sender
                            .try_send(SessionStoreRequest::Checkpoint(queued, reply))
                            .map_err(|error| anyhow!("canonical matrix queue unavailable: {error}"))
                    });
                if let Err(error) = sent {
                    release_reserved_bytes(&state, reserved_bytes);
                    state.health.failed.store(true, Ordering::Release);
                    return Err(error);
                }
                state.health.accepted_events.fetch_add(1, Ordering::Relaxed);
                receiver.await.context("matrix persistence barrier unavailable")?
            })
        }),
        load: Arc::new(move || {
            let state = Arc::clone(&state);
            Box::pin(async move {
                anyhow::ensure!(!state.health.failed.load(Ordering::Acquire), "canonical persistence failed");
                let (reply, receiver) = tokio::sync::oneshot::channel();
                state
                    .sender
                    .lock()
                    .as_ref()
                    .context("canonical event sink is closed")?
                    .try_send(SessionStoreRequest::MatrixLoad(reply))
                    .map_err(|error| anyhow!("canonical matrix queue unavailable: {error}"))?;
                receiver.await.context("matrix replay barrier unavailable")?
            })
        }),
    }
}

#[derive(Debug, Default)]
struct SessionStoreSinkHealth {
    accepted_events: AtomicU64,
    persisted_events: AtomicU64,
    append_failures: AtomicU64,
    serialization_failures: AtomicU64,
    channel_failures: AtomicU64,
    failed: AtomicBool,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SessionStoreSinkHealthSnapshot {
    accepted_events: u64,
    persisted_events: u64,
    append_failures: u64,
    serialization_failures: u64,
    channel_failures: u64,
    failed: bool,
}

impl SessionStoreSinkHealth {
    #[cfg(test)]
    fn snapshot(&self) -> SessionStoreSinkHealthSnapshot {
        SessionStoreSinkHealthSnapshot {
            accepted_events: self.accepted_events.load(Ordering::Relaxed),
            persisted_events: self.persisted_events.load(Ordering::Relaxed),
            append_failures: self.append_failures.load(Ordering::Relaxed),
            serialization_failures: self.serialization_failures.load(Ordering::Relaxed),
            channel_failures: self.channel_failures.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
        }
    }
}

enum SessionStoreRequest {
    Checkpoint(QueuedSessionEvent, tokio::sync::oneshot::Sender<Result<()>>),
    MatrixLoad(tokio::sync::oneshot::Sender<Result<Vec<crate::exec::events::matrix::MatrixSnapshot>>>),
    DiagnosticsLoad(tokio::sync::oneshot::Sender<Result<Vec<crate::exec::events::Diagnostic>>>),
    PlanningLoad(tokio::sync::oneshot::Sender<Result<Option<crate::exec::events::planning::PlanningRecoverySnapshot>>>),
    Event(QueuedSessionEvent),
    ValidateDecision(String, Vec<String>, tokio::sync::oneshot::Sender<Result<()>>),
    Explanation(
        vtcode_memory::explanation::ExplanationScope,
        tokio::sync::oneshot::Sender<Result<vtcode_memory::explanation::ExplanationModel>>,
    ),
    ExplanationPage(
        vtcode_memory::explanation::ExplanationScope,
        usize,
        tokio::sync::oneshot::Sender<Result<vtcode_memory::explanation::ExplanationPage>>,
    ),
    Evidence(
        vtcode_memory::explanation::EvidenceRef,
        usize,
        tokio::sync::oneshot::Sender<Result<vtcode_memory::explanation::EvidencePage>>,
    ),
}

struct QueuedSessionEvent {
    event: ThreadEvent,
    reserved_bytes: usize,
}

struct SessionStoreSinkState {
    sender: Mutex<Option<mpsc::SyncSender<SessionStoreRequest>>>,
    reserved_bytes: AtomicU64,
    max_bytes: usize,
    health: Arc<SessionStoreSinkHealth>,
}

/// Owns the session persistence drain and exposes its final health result.
///
/// The event callback remains synchronous for compatibility, while the runner
/// retains this handle so a completed task cannot report success before its
/// authoritative event queue has drained.
pub(crate) struct SessionStoreSinkHandle {
    state: Arc<SessionStoreSinkState>,
    drain: Option<JoinHandle<()>>,
}

impl SessionStoreSinkHandle {
    pub(crate) async fn persist_checkpoint(&self, event: &ThreadEvent) -> Result<()> {
        persist_session_checkpoint(&self.state, event).await
    }
    pub(crate) async fn planning_recovery(
        &self,
    ) -> Result<Option<crate::exec::events::planning::PlanningRecoverySnapshot>> {
        load_planning_recovery(&self.state).await
    }
    pub(crate) fn matrix_persistence(&self) -> crate::subagents::matrix::MatrixPersistence {
        matrix_persistence(Arc::clone(&self.state))
    }
    pub(crate) fn decision_validator(&self) -> DecisionEvidenceValidator {
        decision_validator(Arc::clone(&self.state))
    }
    pub(crate) async fn close(mut self) -> Result<()> {
        self.state.sender.lock().take();
        if let Some(drain) = self.drain.take() {
            drain.await.context("session event drain task failed")?;
        }
        if self.state.health.failed.load(Ordering::Acquire) {
            return Err(anyhow!("session event persistence failed"));
        }
        Ok(())
    }
}

impl Drop for SessionStoreSinkHandle {
    fn drop(&mut self) {
        self.state.sender.lock().take();
    }
}

/// Authoritative event sink for one canonical session store.
///
/// Events are accepted in order through one bounded, non-blocking queue owned
/// by a blocking drain actor. The queue is bounded both by event count and by
/// an estimated serialized payload budget, so a burst of large tool outputs
/// cannot retain unbounded memory. If the queue or store fails, the sink fails
/// closed and [`Self::close`] reports the loss. Call [`Self::close`] exactly
/// once after the terminal event and propagate its error before reporting a
/// successful run.
pub struct SessionStoreSink {
    state: Arc<SessionStoreSinkState>,
    handle: Arc<Mutex<Option<SessionStoreSinkHandle>>>,
    close_result: Arc<OnceCell<std::result::Result<(), String>>>,
}

impl SessionStoreSink {
    /// Ordered, acknowledged matrix writes and replay through this session drain.
    pub fn matrix_persistence(&self) -> crate::subagents::matrix::MatrixPersistence {
        matrix_persistence(Arc::clone(&self.state))
    }
    /// Validate task-owned evidence after all previously accepted events.
    pub fn decision_validator(&self) -> DecisionEvidenceValidator {
        decision_validator(Arc::clone(&self.state))
    }
    /// Open the canonical session store and start its bounded drain.
    pub async fn open(workspace: &Path, session_id: &str) -> Result<Self> {
        let (state, handle) = open_session_store_sink(workspace, session_id, SESSION_STORE_DRAIN_CAPACITY).await?;
        Ok(Self {
            state,
            handle: Arc::new(Mutex::new(Some(handle))),
            close_result: Arc::new(OnceCell::new()),
        })
    }

    /// Enqueue one event for canonical persistence.
    pub fn emit(&self, event: &ThreadEvent) -> Result<()> {
        enqueue_session_event(&self.state, event)
    }

    /// Persist a checkpoint and acknowledge its durable write before an external effect.
    pub async fn persist_checkpoint(&self, event: &ThreadEvent) -> Result<()> {
        persist_session_checkpoint(&self.state, event).await
    }

    /// Restore the latest complete planning checkpoint after all accepted events.
    pub async fn planning_recovery(&self) -> Result<Option<crate::exec::events::planning::PlanningRecoverySnapshot>> {
        load_planning_recovery(&self.state).await
    }

    /// Restore complete latest diagnostic states without replaying them into model history.
    pub async fn diagnostics(&self) -> Result<Vec<crate::exec::events::Diagnostic>> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.enqueue_query(SessionStoreRequest::DiagnosticsLoad(sender))?;
        receiver.await.context("diagnostic replay barrier unavailable")?
    }

    /// Return the callback form used by the core event recorder.
    pub fn event_sink(&self) -> EventSink {
        event_sink_for_state(Arc::clone(&self.state))
    }

    /// Ordered query barrier; accepted events precede this retained snapshot.
    pub async fn explanation(
        &self,
        scope: vtcode_memory::explanation::ExplanationScope,
    ) -> Result<vtcode_memory::explanation::ExplanationModel> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.enqueue_query(SessionStoreRequest::Explanation(scope, sender))?;
        receiver.await.context("explanation snapshot drain unavailable")?
    }

    /// Return only a bounded page from the ordered cached projection.
    pub async fn explanation_page(
        &self,
        scope: vtcode_memory::explanation::ExplanationScope,
        offset: usize,
    ) -> Result<vtcode_memory::explanation::ExplanationPage> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.enqueue_query(SessionStoreRequest::ExplanationPage(scope, offset, sender))?;
        receiver.await.context("explanation page drain unavailable")?
    }

    /// Resolve a bounded evidence page through the same ordered barrier.
    pub async fn evidence(
        &self,
        reference: vtcode_memory::explanation::EvidenceRef,
        offset: usize,
    ) -> Result<vtcode_memory::explanation::EvidencePage> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.enqueue_query(SessionStoreRequest::Evidence(reference, offset, sender))?;
        receiver.await.context("explanation evidence drain unavailable")?
    }

    fn enqueue_query(&self, request: SessionStoreRequest) -> Result<()> {
        enqueue_session_query(&self.state, request)
    }

    /// Drain and close the canonical persistence task.
    pub async fn close(&self) -> Result<()> {
        let result = self
            .close_result
            .get_or_init(|| async {
                let handle = self.handle.lock().take();
                match handle {
                    Some(handle) => handle.close().await.map_err(|error| error.to_string()),
                    None => Ok(()),
                }
            })
            .await;
        match result {
            Ok(()) => Ok(()),
            Err(error) => Err(anyhow!(error.clone())),
        }
    }
}

async fn persist_session_checkpoint(state: &SessionStoreSinkState, event: &ThreadEvent) -> Result<()> {
    anyhow::ensure!(!state.health.failed.load(Ordering::Acquire), "canonical persistence failed");
    let queued = prepare_queued_session_event(state, event)?;
    let reserved_bytes = queued.reserved_bytes;
    let (reply, receiver) = tokio::sync::oneshot::channel();
    let sent = state
        .sender
        .lock()
        .as_ref()
        .context("canonical event sink is closed")
        .and_then(|sender| {
            sender
                .try_send(SessionStoreRequest::Checkpoint(queued, reply))
                .map_err(|error| anyhow!("canonical checkpoint queue unavailable: {error}"))
        });
    if let Err(error) = sent {
        release_reserved_bytes(state, reserved_bytes);
        return Err(error);
    }
    state.health.accepted_events.fetch_add(1, Ordering::Relaxed);
    receiver.await.context("checkpoint persistence barrier unavailable")?
}

async fn load_planning_recovery(
    state: &SessionStoreSinkState,
) -> Result<Option<crate::exec::events::planning::PlanningRecoverySnapshot>> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    enqueue_session_query(state, SessionStoreRequest::PlanningLoad(sender))?;
    receiver.await.context("planning replay barrier unavailable")?
}

fn enqueue_session_query(state: &SessionStoreSinkState, request: SessionStoreRequest) -> Result<()> {
    if state.health.failed.load(Ordering::Acquire) {
        return Err(anyhow!("canonical persistence failed; explanation unavailable"));
    }
    state
        .sender
        .lock()
        .as_ref()
        .context("canonical event sink is closed")?
        .try_send(request)
        .map_err(|error| anyhow!("canonical event queue unavailable; retry explanation query: {error}"))
}

impl Drop for SessionStoreSink {
    fn drop(&mut self) {
        self.state.sender.lock().take();
    }
}

#[doc(hidden)]
pub fn event_sink<F>(callback: F) -> EventSink
where
    F: FnMut(&ThreadEvent) + Send + 'static,
{
    Arc::new(Mutex::new(Box::new(callback)))
}

/// Build an event sink that persists every recorded event to the unified
/// per-session store ([`vtcode_memory`]), making it the canonical source of
/// truth for session state/history.
///
/// The sink hands events to one bounded, non-blocking queue and a blocking
/// background drain. If the queue cannot accept an event, the sink records a
/// fatal persistence failure and `close()` fails the run. Disk I/O and
/// manifest writes remain off the Tokio runtime worker through `spawn_blocking`.
pub async fn session_store_sink(workspace: &Path, session_id: &str) -> Result<SessionStoreSink> {
    SessionStoreSink::open(workspace, session_id).await
}

pub(crate) async fn session_store_sink_with_handle(
    workspace: &Path,
    session_id: &str,
) -> Result<(EventSink, SessionStoreSinkHandle)> {
    let (state, handle) = open_session_store_sink(workspace, session_id, SESSION_STORE_DRAIN_CAPACITY).await?;
    Ok((event_sink_for_state(state), handle))
}

async fn open_session_store_sink(
    workspace: &Path,
    session_id: &str,
    capacity: usize,
) -> Result<(Arc<SessionStoreSinkState>, SessionStoreSinkHandle)> {
    open_session_store_sink_with_limits(workspace, session_id, capacity, SESSION_STORE_DRAIN_MAX_BYTES).await
}

async fn open_session_store_sink_with_limits(
    workspace: &Path,
    session_id: &str,
    capacity: usize,
    max_bytes: usize,
) -> Result<(Arc<SessionStoreSinkState>, SessionStoreSinkHandle)> {
    let workspace = workspace.to_path_buf();
    let session_id_owned = session_id.to_string();
    let log = spawn_blocking(move || vtcode_memory::open(&workspace, &session_id_owned, DEFAULT_MAX_EVENTS))
        .await
        .context("canonical session store open task failed")??;

    let queue_capacity = capacity.max(1);
    let (sender, receiver) = mpsc::sync_channel::<SessionStoreRequest>(queue_capacity);
    let health = Arc::new(SessionStoreSinkHealth::default());
    let state = Arc::new(SessionStoreSinkState {
        sender: Mutex::new(Some(sender)),
        reserved_bytes: AtomicU64::new(0),
        max_bytes: max_bytes.max(1),
        health: Arc::clone(&health),
    });
    let drain_state = Arc::clone(&state);
    let drain_session_id = session_id.to_string();
    let drain = spawn_blocking(move || drain_session_events(receiver, log, drain_session_id, drain_state));

    Ok((state.clone(), SessionStoreSinkHandle { state, drain: Some(drain) }))
}

#[cfg(test)]
async fn session_store_sink_with_capacity_handle(
    workspace: &Path,
    session_id: &str,
    capacity: usize,
) -> Result<(EventSink, SessionStoreSinkHandle)> {
    let (state, handle) = open_session_store_sink(workspace, session_id, capacity).await?;
    Ok((event_sink_for_state(state), handle))
}

fn event_sink_for_state(state: Arc<SessionStoreSinkState>) -> EventSink {
    event_sink(move |event: &ThreadEvent| {
        if let Err(error) = enqueue_session_event(&state, event) {
            tracing::error!(error = %error, "canonical session event was not accepted");
        }
    })
}

struct CachedExplanation {
    scope: vtcode_memory::explanation::ExplanationScope,
    model: Arc<vtcode_memory::explanation::ExplanationModel>,
}

fn cached_explanation(
    log: &vtcode_memory::SessionEventLog,
    scope: vtcode_memory::explanation::ExplanationScope,
    explanations: &mut Vec<CachedExplanation>,
) -> Result<Arc<vtcode_memory::explanation::ExplanationModel>> {
    if let Some(cached) = explanations.iter().find(|cached| cached.scope == scope) {
        return Ok(Arc::clone(&cached.model));
    }
    let model = Arc::new(vtcode_memory::explanation::query_explanation(log, scope)?);
    // Two scopes, invalidated on append; oversized projections remain uncached.
    let mut counter = LimitedByteCounter { bytes: 0, limit: EXPLANATION_CACHE_MAX_BYTES };
    if serde_json::to_writer(&mut counter, model.as_ref()).is_ok() {
        explanations.push(CachedExplanation { scope, model: Arc::clone(&model) });
    }
    Ok(model)
}

fn matrix_command_completed_event(
    snapshot: &crate::exec::events::matrix::MatrixSnapshot,
    task: &crate::exec::events::matrix::MatrixTaskState,
    evidence: &crate::exec::events::matrix::MatrixCommandEvidence,
    session_id: &str,
) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            id: evidence.event_id.clone(),
            context: Some(Box::new(crate::exec::events::ItemContext {
                task_id: format!("{}/{}", snapshot.spec.id, task.id),
                turn_id: evidence.attempt_id.clone(),
                actor_id: evidence.worker_id.clone(),
                parent_actor_id: Some(session_id.to_owned()),
                timestamp: chrono::Utc::now().to_rfc3339(),
                activity: Some(crate::exec::events::CommandActivity::Verification),
            })),
            details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: evidence.command.clone(),
                arguments: Some(serde_json::json!({
                    "matrix_id":snapshot.spec.id, "task_id":task.id,
                    "attempt_id":evidence.attempt_id, "worker_id":evidence.worker_id,
                    "generation":evidence.generation, "cancelled":evidence.cancelled,
                })),
                aggregated_output: String::new(),
                exit_code: evidence.exit_code,
                status: if evidence.exit_code == Some(0) && !evidence.cancelled {
                    CommandExecutionStatus::Completed
                } else {
                    CommandExecutionStatus::Failed
                },
            })),
        },
    })
}

fn restore_matrix_evidence_ids(
    log: &vtcode_memory::SessionEventLog,
    ids: &mut std::collections::HashSet<String>,
) -> Result<()> {
    log.visit_snapshot(|_, bytes| {
        if let Ok(versioned) = serde_json::from_slice::<VersionedThreadEvent>(bytes) {
            match versioned.into_event() {
                ThreadEvent::ItemCompleted(event)
                    if matches!(event.item.details, ThreadItemDetails::CommandExecution(_)) =>
                {
                    ids.insert(event.item.id);
                }
                ThreadEvent::MatrixUpdated(snapshot) => {
                    // Cap rewrites can retain checkpoints after evicting the
                    // command records. Their evidence remains historical.
                    ids.extend(
                        snapshot
                            .tasks
                            .into_iter()
                            .flat_map(|task| task.attempts)
                            .flat_map(|attempt| attempt.evidence)
                            .map(|evidence| evidence.event_id),
                    );
                }
                _ => {}
            }
        }
    })
    .context("restore canonical matrix command identities")?;
    Ok(())
}

fn drain_session_events(
    rx: Receiver<SessionStoreRequest>,
    log: vtcode_memory::SessionEventLog,
    session_id: String,
    state: Arc<SessionStoreSinkState>,
) {
    let mut explanations = Vec::new();
    let mut matrix_evidence_ids = std::collections::HashSet::new();
    let mut matrix_evidence_restored = false;
    while let Ok(request) = rx.recv() {
        let queued = match request {
            SessionStoreRequest::Checkpoint(queued, reply) => {
                let result = (|| -> Result<()> {
                    if let ThreadEvent::MatrixUpdated(snapshot) = &queued.event {
                        if !matrix_evidence_restored {
                            restore_matrix_evidence_ids(&log, &mut matrix_evidence_ids)?;
                            matrix_evidence_restored = true;
                        }
                        for task in &snapshot.tasks {
                            for attempt in &task.attempts {
                                for evidence in &attempt.evidence {
                                    if matrix_evidence_ids.insert(evidence.event_id.clone()) {
                                        log.append(&matrix_command_completed_event(
                                            snapshot,
                                            task,
                                            evidence,
                                            &session_id,
                                        ))?;
                                    }
                                }
                            }
                        }
                    }
                    log.append(&queued.event)?;
                    log.flush()?;
                    Ok(())
                })();
                release_reserved_bytes(&state, queued.reserved_bytes);
                let failed = result.is_err();
                if failed {
                    state.health.append_failures.fetch_add(1, Ordering::Relaxed);
                    state.health.failed.store(true, Ordering::Release);
                } else {
                    state.health.persisted_events.fetch_add(1, Ordering::Relaxed);
                }
                explanations.clear();
                let _ = reply.send(result);
                if failed {
                    break;
                }
                continue;
            }
            SessionStoreRequest::MatrixLoad(reply) => {
                let result = (|| -> Result<_> {
                    let mut latest = std::collections::BTreeMap::<String, vtcode_memory::matrix::MatrixState>::new();
                    let mut replay_error = None;
                    log.visit_snapshot(|_, bytes| {
                        if replay_error.is_some() {
                            return;
                        }
                        if let Ok(versioned) = serde_json::from_slice::<VersionedThreadEvent>(bytes)
                            && let ThreadEvent::MatrixUpdated(snapshot) = versioned.into_event()
                        {
                            let previous = latest.remove(&snapshot.spec.id);
                            let events = previous
                                .into_iter()
                                .map(|state| state.event())
                                .chain([ThreadEvent::MatrixUpdated(snapshot)]);
                            match vtcode_memory::matrix::replay(events) {
                                Ok(replayed) => latest.extend(replayed),
                                Err(error) => replay_error = Some(error),
                            }
                        }
                    })?;
                    if let Some(error) = replay_error {
                        return Err(error.into());
                    }
                    Ok(latest.into_values().map(|state| state.snapshot().clone()).collect())
                })();
                let _ = reply.send(result);
                continue;
            }
            SessionStoreRequest::DiagnosticsLoad(reply) => {
                let result = (|| -> Result<_> {
                    let mut projection = crate::exec::events::diagnostic::DiagnosticProjection::default();
                    log.visit_snapshot(|_, bytes| {
                        if let Ok(event) = serde_json::from_slice::<VersionedThreadEvent>(bytes) {
                            projection.observe(&event.into_event());
                        }
                    })?;
                    Ok(projection.into_diagnostics())
                })();
                let _ = reply.send(result);
                continue;
            }
            SessionStoreRequest::PlanningLoad(reply) => {
                let result = (|| -> Result<_> {
                    let mut latest = None;
                    log.visit_snapshot(|_, bytes| {
                        if let Ok(versioned) = serde_json::from_slice::<VersionedThreadEvent>(bytes)
                            && let ThreadEvent::PlanDelta(event) = versioned.into_event()
                            && let Some(snapshot) = event.recovery
                        {
                            latest = Some(*snapshot);
                        }
                    })?;
                    Ok(latest)
                })();
                let _ = reply.send(result);
                continue;
            }
            SessionStoreRequest::Event(event) => event,
            SessionStoreRequest::ValidateDecision(task_id, ids, reply) => {
                let result = (|| -> Result<()> {
                    let mut found = std::collections::HashSet::new();
                    log.visit_snapshot(|_, bytes| {
                        if let Ok(versioned) = serde_json::from_slice::<VersionedThreadEvent>(bytes) {
                            let item = match versioned.into_event() {
                                ThreadEvent::ItemStarted(e) => Some(e.item),
                                ThreadEvent::ItemUpdated(e) => Some(e.item),
                                ThreadEvent::ItemCompleted(e) => Some(e.item),
                                _ => None,
                            };
                            if let Some(item) = item
                                && item.context.as_ref().is_some_and(|c| c.task_id == task_id)
                                && ids.contains(&item.id)
                            {
                                found.insert(item.id);
                            }
                        }
                    })?;
                    anyhow::ensure!(
                        ids.iter().all(|id| found.contains(id)),
                        "decision evidence is unavailable or does not belong to the current task"
                    );
                    Ok(())
                })();
                let _ = reply.send(result);
                continue;
            }
            SessionStoreRequest::Explanation(scope, reply) => {
                let result = cached_explanation(&log, scope, &mut explanations).map(|model| model.as_ref().clone());
                let _ = reply.send(result);
                continue;
            }
            SessionStoreRequest::ExplanationPage(scope, offset, reply) => {
                let result = cached_explanation(&log, scope, &mut explanations)
                    .map(|model| vtcode_memory::explanation::page_explanation(&model, offset));
                let _ = reply.send(result);
                continue;
            }
            SessionStoreRequest::Evidence(reference, offset, reply) => {
                let result = vtcode_memory::explanation::query_evidence(&log, &reference, offset, 32 * 1024)
                    .map_err(anyhow::Error::from);
                let _ = reply.send(result);
                continue;
            }
        };
        explanations.clear();
        let reserved_bytes = queued.reserved_bytes;
        match log.append(&queued.event) {
            Ok(()) => {
                state.health.persisted_events.fetch_add(1, Ordering::Relaxed);
            }
            Err(err) => {
                state.health.append_failures.fetch_add(1, Ordering::Relaxed);
                state.health.failed.store(true, Ordering::Release);
                tracing::error!(
                    session_id = %session_id,
                    error = %err,
                    "failed to persist session event; stopping authoritative drain"
                );
                release_reserved_bytes(&state, reserved_bytes);
                while let Ok(request) = rx.try_recv() {
                    if let SessionStoreRequest::Event(queued) | SessionStoreRequest::Checkpoint(queued, _) = request {
                        release_reserved_bytes(&state, queued.reserved_bytes);
                    }
                }
                break;
            }
        }
        release_reserved_bytes(&state, reserved_bytes);
    }

    if let Err(err) = log.flush() {
        state.health.append_failures.fetch_add(1, Ordering::Relaxed);
        state.health.failed.store(true, Ordering::Release);
        tracing::error!(
            session_id = %session_id,
            error = %err,
            "failed to flush session event log during drain shutdown"
        );
    }
}

fn prepare_queued_session_event(state: &SessionStoreSinkState, event: &ThreadEvent) -> Result<QueuedSessionEvent> {
    if state.health.failed.load(Ordering::Acquire) {
        state.health.channel_failures.fetch_add(1, Ordering::Relaxed);
        return Err(anyhow!("canonical session event sink has failed"));
    }

    let serialized_bytes = match serialized_event_size(event, state.max_bytes) {
        Ok(bytes) => bytes,
        Err(error) => {
            state.health.serialization_failures.fetch_add(1, Ordering::Relaxed);
            state.health.failed.store(true, Ordering::Release);
            return Err(error.context("failed to serialize canonical session event for bounded handoff"));
        }
    };
    let reserved_bytes = serialized_bytes.saturating_mul(2).saturating_add(size_of::<ThreadEvent>());
    if reserved_bytes > state.max_bytes {
        state.health.serialization_failures.fetch_add(1, Ordering::Relaxed);
        state.health.failed.store(true, Ordering::Release);
        return Err(anyhow!(
            "canonical session event exceeds the bounded persistence budget ({reserved_bytes} > {})",
            state.max_bytes
        ));
    }
    if !reserve_bytes(&state.reserved_bytes, reserved_bytes, state.max_bytes) {
        state.health.failed.store(true, Ordering::Release);
        state.health.channel_failures.fetch_add(1, Ordering::Relaxed);
        return Err(anyhow!("canonical session event queue reached its bounded byte capacity"));
    }

    Ok(QueuedSessionEvent { event: event.clone(), reserved_bytes })
}

fn enqueue_session_event(state: &SessionStoreSinkState, event: &ThreadEvent) -> Result<()> {
    let queued = prepare_queued_session_event(state, event)?;
    let reserved_bytes = queued.reserved_bytes;
    let sender = state.sender.lock();
    let Some(sender) = sender.as_ref() else {
        release_reserved_bytes(state, reserved_bytes);
        state.health.failed.store(true, Ordering::Release);
        state.health.channel_failures.fetch_add(1, Ordering::Relaxed);
        return Err(anyhow!("canonical session event sink is closed"));
    };
    match sender.try_send(SessionStoreRequest::Event(queued)) {
        Ok(()) => {
            state.health.accepted_events.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        Err(TrySendError::Full(request) | TrySendError::Disconnected(request)) => {
            if let SessionStoreRequest::Event(queued) | SessionStoreRequest::Checkpoint(queued, _) = request {
                release_reserved_bytes(state, queued.reserved_bytes);
            }
            state.health.failed.store(true, Ordering::Release);
            state.health.channel_failures.fetch_add(1, Ordering::Relaxed);
            Err(anyhow!("canonical session event queue could not accept the event"))
        }
    }
}

fn reserve_bytes(reserved_bytes: &AtomicU64, bytes: usize, max_bytes: usize) -> bool {
    let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
    let max_bytes = u64::try_from(max_bytes).unwrap_or(u64::MAX);
    reserved_bytes
        .try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current.checked_add(bytes).filter(|next| *next <= max_bytes)
        })
        .is_ok()
}

fn release_reserved_bytes(state: &SessionStoreSinkState, bytes: usize) {
    let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
    state.reserved_bytes.fetch_sub(bytes, Ordering::AcqRel);
}

#[derive(Serialize)]
struct BorrowedVersionedThreadEvent<'a> {
    schema_version: &'static str,
    event: &'a ThreadEvent,
}

struct LimitedByteCounter {
    bytes: usize,
    limit: usize,
}

impl Write for LimitedByteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .bytes
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("canonical event size overflow"))?;
        if next > self.limit {
            return Err(io::Error::other("canonical event exceeds bounded size"));
        }
        self.bytes = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn serialized_event_size(event: &ThreadEvent, limit: usize) -> Result<usize> {
    let mut counter = LimitedByteCounter { bytes: 0, limit };
    serde_json::to_writer(&mut counter, &BorrowedVersionedThreadEvent { schema_version: EVENT_SCHEMA_VERSION, event })
        .context("canonical event serialization failed")?;
    counter.bytes.checked_add(1).context("canonical event size overflow")
}

/// Combine two optional event sinks into one that fans out to both.
pub fn combine_event_sinks(a: Option<EventSink>, b: Option<EventSink>) -> Option<EventSink> {
    match (a, b) {
        (None, None) => None,
        (Some(s), None) | (None, Some(s)) => Some(s),
        (Some(a), Some(b)) => Some(event_sink(move |e: &ThreadEvent| {
            a.lock()(e);
            b.lock()(e);
        })),
    }
}

#[cfg(test)]
mod tests;
