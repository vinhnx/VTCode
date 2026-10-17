//! Event recording utilities for the agent runner.

mod context;
mod lifecycle;
pub use context::{ExecutionContextTracker, decision_completed_event, validate_decision_input};
pub use lifecycle::{
    SharedLifecycleEmitter, ToolOutputPayload, error_item_completed_event, file_change_completed_event,
    tool_invocation_completed_event, tool_output_completed_event, tool_output_item_id, tool_output_payload_from_value,
    tool_output_started_event, tool_output_updated_event, tool_started_event,
};

use crate::core::threads::{SubmissionId, ThreadRuntimeHandle};
use crate::exec::events::{
    CommandExecutionItem, CommandExecutionStatus, CompactionMode, CompactionTrigger, EVENT_SCHEMA_VERSION, ErrorItem,
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
                            .try_send(SessionStoreRequest::MatrixPersist(queued, reply))
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
    MatrixPersist(QueuedSessionEvent, tokio::sync::oneshot::Sender<Result<()>>),
    MatrixLoad(tokio::sync::oneshot::Sender<Result<Vec<crate::exec::events::matrix::MatrixSnapshot>>>),
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
        if self.state.health.failed.load(Ordering::Acquire) {
            return Err(anyhow!("canonical persistence failed; explanation unavailable"));
        }
        self.state
            .sender
            .lock()
            .as_ref()
            .context("canonical event sink is closed")?
            .try_send(request)
            .map_err(|error| anyhow!("canonical event queue unavailable; retry explanation query: {error}"))
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
            SessionStoreRequest::MatrixPersist(queued, reply) => {
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
                    if let SessionStoreRequest::Event(queued) | SessionStoreRequest::MatrixPersist(queued, _) = request
                    {
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
            if let SessionStoreRequest::Event(queued) | SessionStoreRequest::MatrixPersist(queued, _) = request {
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
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
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

#[derive(Debug, Clone)]
pub struct ActiveCommandHandle {
    id: String,
    command: String,
}

#[derive(Debug, Clone)]
pub struct ActiveToolHandle {
    id: String,
    tool_name: String,
    arguments: Option<Value>,
    tool_call_id: Option<String>,
}

impl ActiveToolHandle {
    #[must_use]
    pub fn item_id(&self) -> &str {
        &self.id
    }
}

/// Helper responsible for recording execution events and relaying them to optional sinks.
#[derive(Default)]
pub struct ExecEventRecorder {
    context: ExecutionContextTracker,
    thread_id: String,
    events: Vec<ThreadEvent>,
    event_sink: Option<EventSink>,
    thread_handle: Option<ThreadRuntimeHandle>,
    active_submission_id: Option<SubmissionId>,
    active_turn_id: Option<String>,
    lifecycle: SharedLifecycleEmitter,
}

impl ExecEventRecorder {
    pub fn new(
        thread_id: impl Into<String>,
        event_sink: Option<EventSink>,
        thread_handle: Option<ThreadRuntimeHandle>,
    ) -> Self {
        let thread_id = thread_id.into();
        let mut recorder = Self {
            context: ExecutionContextTracker::default(),
            thread_id: thread_id.clone(),
            events: Vec::new(),
            event_sink,
            thread_handle,
            active_submission_id: None,
            active_turn_id: None,
            lifecycle: SharedLifecycleEmitter::default(),
        };
        recorder.record_with_context(None, None, ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id }));
        recorder
    }

    fn record(&mut self, event: ThreadEvent) {
        self.record_with_context(self.active_submission_id.clone(), self.active_turn_id.clone(), event);
    }

    fn record_with_context(
        &mut self,
        submission_id: Option<SubmissionId>,
        turn_id: Option<String>,
        mut event: ThreadEvent,
    ) {
        self.context.annotate(&mut event);
        let decision = decision_completed_event(&event);
        let exec_completion = self.context.background_exec_output_event(&mut event);
        if let Some(sink) = &self.event_sink {
            let mut callback = sink.lock();
            callback(&event);
        }
        if let Some(handle) = &self.thread_handle {
            handle.record_event(submission_id, turn_id, event.clone());
        }
        self.events.push(event);
        if let Some(decision) = decision {
            self.record(decision);
        }
        if let Some(completion) = exec_completion {
            self.record(completion);
        }
    }

    /// Attach a session result to its original verifier's canonical output.
    pub fn record_exec_session_output(&mut self, call_item_id: &str, tool_name: &str, args: &Value, output: &Value) {
        if let Some(event) = self.context.exec_session_output_event(call_item_id, tool_name, args, output) {
            self.record(event);
        }
        if let Some(event) = self.context.delegation_status_event(call_item_id, tool_name, output) {
            self.record(event);
        }
    }

    pub fn record_thread_event(&mut self, event: ThreadEvent) {
        self.record(event);
    }

    pub fn record_thread_events<I>(&mut self, events: I)
    where
        I: IntoIterator<Item = ThreadEvent>,
    {
        for event in events {
            self.record(event);
        }
    }

    fn record_pending_lifecycle_events(&mut self) {
        for event in self.lifecycle.drain_events() {
            self.record(event);
        }
    }

    fn next_item_id(&mut self) -> String {
        self.lifecycle.next_item_id()
    }

    pub fn turn_started(&mut self) {
        let turn_id = format!("turn-{}", Uuid::new_v4());
        self.context
            .begin(&self.thread_id, &turn_id, "", crate::exec::events::InputOrigin::Continuation);
        self.active_turn_id = Some(turn_id);
        self.begin_turn_submission();
    }

    fn begin_turn_submission(&mut self) {
        if let Some(handle) = &self.thread_handle {
            match handle.begin_turn() {
                Ok(submission_id) => self.active_submission_id = Some(submission_id),
                Err(err) => {
                    // A failed begin_turn means a previous turn was never
                    // finished (or a concurrent turn is in flight). Surface
                    // it instead of silently dropping the submission id:
                    // without it, every event of this turn loses its
                    // submission context on retried attempts.
                    tracing::warn!(error = %err, "failed to begin turn; submission id unavailable");
                    self.active_submission_id = None;
                }
            }
        }
        self.record(ThreadEvent::TurnStarted(TurnStartedEvent::default()));
    }

    /// Start a task with its original public request.
    pub fn task_started(&mut self, goal: &str) -> String {
        let context = self.context.begin(
            &self.thread_id,
            &format!("turn-{}", Uuid::new_v4()),
            goal,
            crate::exec::events::InputOrigin::User,
        );
        self.active_turn_id = Some(context.turn_id);
        self.begin_turn_submission();
        context.task_id
    }

    pub fn turn_completed(&mut self, usage: Usage) {
        self.record(ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage,
            in_progress_exec_sessions: Vec::new(),
        }));
        self.finish_turn();
    }

    pub fn turn_failed(&mut self, message: &str, usage: Option<Usage>) {
        self.record(ThreadEvent::TurnFailed(TurnFailedEvent {
            completed_at: None,
            message: message.to_string(),
            usage,
        }));
        self.finish_turn();
    }

    pub fn turn_blocked(&mut self, event: TurnBlockedEvent) {
        self.record(ThreadEvent::TurnBlocked(Box::new(event)));
    }

    pub fn thread_completed(
        &mut self,
        session_id: &str,
        subtype: ThreadCompletionSubtype,
        outcome_code: &str,
        result: Option<&str>,
        stop_reason: Option<&str>,
        usage: Usage,
        total_cost_usd: Option<serde_json::Number>,
        num_turns: usize,
    ) {
        self.record(ThreadEvent::ThreadCompleted(Box::new(ThreadCompletedEvent {
            completed_at: None,
            thread_id: self.thread_id.clone(),
            session_id: session_id.to_string(),
            subtype,
            outcome_code: outcome_code.to_string(),
            result: result.map(str::to_string),
            stop_reason: stop_reason.map(str::to_string),
            usage,
            total_cost_usd,
            num_turns,
        })));
    }

    /// Record the terminal lifecycle events for an execution failure.
    pub fn thread_failed(&mut self, session_id: &str, message: &str, num_turns: usize) {
        self.turn_failed(message, None);
        self.thread_completed(
            session_id,
            ThreadCompletionSubtype::ErrorDuringExecution,
            "error",
            None,
            Some(message),
            Usage::default(),
            None,
            num_turns,
        );
    }

    pub fn compact_boundary(
        &mut self,
        trigger: CompactionTrigger,
        mode: CompactionMode,
        original_message_count: usize,
        compacted_message_count: usize,
        history_artifact_path: Option<&str>,
        previous_envelope: Option<&crate::core::agent::request_envelope::SessionRequestEnvelope>,
        new_envelope: Option<&crate::core::agent::request_envelope::SessionRequestEnvelope>,
    ) {
        let previous_segment_id = previous_envelope.map(|envelope| envelope.segment_id().to_owned());
        let new_segment_id = new_envelope.map(|envelope| envelope.segment_id().to_owned());
        let previous_prefix_hash = previous_envelope.map(|envelope| format!("{:016x}", envelope.prefix_hash()));
        let new_prefix_hash = new_envelope.map(|envelope| format!("{:016x}", envelope.prefix_hash()));
        let previous_catalog_hash = previous_envelope
            .and_then(crate::core::agent::request_envelope::SessionRequestEnvelope::catalog_hash)
            .map(|hash| format!("{hash:016x}"));
        let new_catalog_hash = new_envelope
            .and_then(crate::core::agent::request_envelope::SessionRequestEnvelope::catalog_hash)
            .map(|hash| format!("{hash:016x}"));
        self.record(ThreadEvent::ThreadCompactBoundary(Box::new(ThreadCompactBoundaryEvent {
            thread_id: self.thread_id.clone(),
            trigger,
            mode,
            original_message_count,
            compacted_message_count,
            history_artifact_path: history_artifact_path.map(str::to_string),
            previous_segment_id,
            new_segment_id,
            previous_prefix_hash,
            new_prefix_hash,
            previous_catalog_hash,
            new_catalog_hash,
        })));
    }

    fn finish_turn(&mut self) {
        if let Some(handle) = &self.thread_handle {
            handle.finish_turn();
        }
        self.active_submission_id = None;
        self.active_turn_id = None;
    }

    pub fn agent_message(&mut self, text: &str) {
        self.lifecycle.emit_completed_agent_message(text);
        self.record_pending_lifecycle_events();
    }

    pub fn agent_message_stream_update(&mut self, text: &str) -> bool {
        if text.trim().is_empty() || !self.lifecycle.replace_assistant_text(text) {
            return false;
        }
        let emitted = self.lifecycle.emit_assistant_snapshot(None);
        self.record_pending_lifecycle_events();
        emitted
    }

    pub fn agent_message_stream_complete(&mut self) {
        let _ = self.lifecycle.complete_assistant_stream();
        self.record_pending_lifecycle_events();
    }

    pub fn reasoning(&mut self, text: &str) {
        self.lifecycle.emit_completed_reasoning(text);
        self.record_pending_lifecycle_events();
    }

    pub fn set_reasoning_stage(&mut self, stage: &str) {
        if !self.lifecycle.set_reasoning_stage(Some(stage.to_string())) {
            return;
        }
        let _ = self.lifecycle.emit_reasoning_stage_update();
        self.record_pending_lifecycle_events();
    }

    pub fn reasoning_stream_update(&mut self, text: &str) -> bool {
        if text.trim().is_empty() || !self.lifecycle.replace_reasoning_text(text) {
            return false;
        }
        let emitted = self.lifecycle.emit_reasoning_snapshot(None);
        self.record_pending_lifecycle_events();
        emitted
    }

    pub fn reasoning_stream_complete(&mut self) {
        let _ = self.lifecycle.complete_reasoning_stream();
        self.record_pending_lifecycle_events();
    }

    pub fn tool_started(
        &mut self,
        tool_name: &str,
        arguments: Option<&Value>,
        tool_call_id: Option<&str>,
    ) -> ActiveToolHandle {
        let handle = ActiveToolHandle {
            id: self.next_item_id(),
            tool_name: tool_name.to_string(),
            arguments: arguments.cloned(),
            tool_call_id: tool_call_id.map(str::to_string),
        };
        self.record(tool_started_event(
            handle.id.clone(),
            &handle.tool_name,
            handle.arguments.as_ref(),
            handle.tool_call_id.as_deref(),
        ));
        handle
    }

    pub fn tool_finished(
        &mut self,
        handle: &ActiveToolHandle,
        status: crate::exec::events::ToolCallStatus,
        exit_code: Option<i32>,
        aggregated_output: &str,
        spool_path: Option<&str>,
    ) {
        let outcome = tool_outcome_from_status(&status);
        self.record(tool_invocation_completed_event(
            handle.id.clone(),
            &handle.tool_name,
            handle.arguments.as_ref(),
            handle.tool_call_id.as_deref(),
            status.clone(),
            outcome,
        ));
        self.record(tool_output_completed_event(
            handle.id.clone(),
            handle.tool_call_id.as_deref(),
            status,
            exit_code,
            spool_path,
            aggregated_output,
        ));
    }

    pub fn tool_output_started(&mut self, call_item_id: &str, tool_call_id: Option<&str>) {
        self.record(tool_output_started_event(call_item_id.to_string(), tool_call_id));
    }

    pub fn tool_output_updated(&mut self, call_item_id: &str, tool_call_id: Option<&str>, output: &str) {
        self.record(tool_output_updated_event(call_item_id.to_string(), tool_call_id, output));
    }

    pub fn tool_output_finished(
        &mut self,
        call_item_id: &str,
        tool_call_id: Option<&str>,
        status: crate::exec::events::ToolCallStatus,
        exit_code: Option<i32>,
        aggregated_output: &str,
        spool_path: Option<&str>,
    ) {
        self.record(tool_output_completed_event(
            call_item_id.to_string(),
            tool_call_id,
            status,
            exit_code,
            spool_path,
            aggregated_output,
        ));
    }

    pub fn tool_rejected(
        &mut self,
        tool_name: &str,
        arguments: Option<&Value>,
        tool_call_id: Option<&str>,
        detail: &str,
    ) {
        let handle = self.tool_started(tool_name, arguments, tool_call_id);
        let call_item_id = handle.id.clone();
        self.record(tool_invocation_completed_event(
            call_item_id.clone(),
            tool_name,
            arguments,
            tool_call_id,
            crate::exec::events::ToolCallStatus::Failed,
            ToolOutcome::HookDenied,
        ));
        self.record(tool_output_started_event(call_item_id.clone(), tool_call_id));
        self.record(tool_output_completed_event(
            call_item_id,
            tool_call_id,
            crate::exec::events::ToolCallStatus::Failed,
            None,
            None,
            detail,
        ));
        let error_item_id = self.next_item_id();
        self.record(error_item_completed_event(error_item_id, detail.to_string()));
    }

    pub fn permission_requested(&mut self, tool_name: &str) {
        self.record(ThreadEvent::PermissionRequested(crate::exec::events::PermissionRequestedEvent {
            tool_name: tool_name.to_string(),
        }));
    }

    pub fn permission_resolved(
        &mut self,
        tool_name: &str,
        decision: crate::exec::events::PermissionDecision,
        wait_ms: u64,
    ) {
        self.record(ThreadEvent::PermissionResolved(crate::exec::events::PermissionResolvedEvent {
            tool_name: tool_name.to_string(),
            decision,
            wait_ms,
        }));
    }

    pub fn command_started(&mut self, command: &str) -> ActiveCommandHandle {
        let id = self.next_item_id();
        let item = ThreadItem {
            context: None,
            id: id.clone(),
            details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: command.to_string(),
                arguments: None,
                aggregated_output: String::new(),
                exit_code: None,
                status: CommandExecutionStatus::InProgress,
            })),
        };
        self.record(ThreadEvent::ItemStarted(ItemStartedEvent { item }));
        ActiveCommandHandle { id, command: command.to_string() }
    }

    pub fn command_finished(
        &mut self,
        handle: &ActiveCommandHandle,
        status: CommandExecutionStatus,
        exit_code: Option<i32>,
        aggregated_output: &str,
    ) {
        let item = ThreadItem {
            context: None,
            id: handle.id.clone(),
            details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: handle.command.clone(),
                arguments: None,
                aggregated_output: aggregated_output.to_string(),
                exit_code,
                status,
            })),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    pub fn warning(&mut self, message: &str) {
        let item = ThreadItem {
            context: None,
            id: self.next_item_id(),
            details: ThreadItemDetails::Error(ErrorItem { message: message.to_string() }),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    pub fn harness_event(
        &mut self,
        event: HarnessEventKind,
        message: Option<String>,
        command: Option<String>,
        path: Option<String>,
        exit_code: Option<i32>,
        attempt: Option<u32>,
        error_category: Option<String>,
    ) {
        let item = ThreadItem {
            context: None,
            id: self.next_item_id(),
            details: ThreadItemDetails::Harness(Box::new(HarnessEventItem {
                event,
                message,
                command,
                path,
                exit_code,
                attempt,
                error_category,
                duration_ms: None,
                task_id: None,
                session_id: None,
                exec_session_id: None,
                status: None,
                transcript_path: None,
                archive_path: None,
            })),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    /// Emit a tool latency harness event with recorded duration.
    pub fn record_tool_latency(&mut self, tool_name: &str, duration_ms: u64) {
        self.record_tool_outcome(tool_name, 1, duration_ms, None);
    }

    /// Emit the single terminal harness observation for one invocation.
    pub fn record_tool_outcome(
        &mut self,
        tool_name: &str,
        attempts: u32,
        duration_ms: u64,
        error_category: Option<&str>,
    ) {
        let item = ThreadItem {
            context: None,
            id: self.next_item_id(),
            details: ThreadItemDetails::Harness(Box::new(HarnessEventItem {
                event: HarnessEventKind::ToolLatencyRecorded,
                message: Some(format!("{tool_name} completed in {duration_ms}ms")),
                command: None,
                path: None,
                exit_code: None,
                attempt: Some(attempts),
                error_category: error_category.map(str::to_string),
                duration_ms: Some(duration_ms),
                task_id: None,
                session_id: None,
                exec_session_id: None,
                status: None,
                transcript_path: None,
                archive_path: None,
            })),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    /// Emit an `ErrorRecovered` harness event, recording that the agent
    /// successfully recovered from a transient error after retries.
    pub fn error_recovered(&mut self, tool_name: &str, attempt: u32, error_category: &str) {
        self.harness_event(
            HarnessEventKind::ErrorRecovered,
            Some(format!("{tool_name} recovered after {attempt} retries")),
            None,
            None,
            None,
            Some(attempt),
            Some(error_category.to_string()),
        );
    }

    /// Emit a `ToolRetryAttempted` harness event, recording that a transient
    /// tool failure triggered an automatic retry.
    pub fn tool_retry_attempted(&mut self, tool_name: &str, attempt: u32, error_category: &str, delay_ms: u64) {
        self.harness_event(
            HarnessEventKind::ToolRetryAttempted,
            Some(format!("{tool_name}: retry {attempt} after {delay_ms}ms")),
            None,
            None,
            None,
            Some(attempt),
            Some(error_category.to_string()),
        );
    }

    /// Drain the collected events while keeping the recorder available for a
    /// terminal failure path that may run after the normal result assembly.
    pub fn take_events(&mut self) -> Vec<ThreadEvent> {
        self.lifecycle.complete_open_items();
        self.record_pending_lifecycle_events();
        std::mem::take(&mut self.events)
    }

    pub fn into_events(mut self) -> Vec<ThreadEvent> {
        self.take_events()
    }
}

#[cfg(test)]
mod tests {
    fn pending_verifier(recorder: &mut ExecEventRecorder, session: &str) -> String {
        let args = serde_json::json!({"cmd":"cargo check --locked"});
        let launch = recorder.tool_started("exec_command", Some(&args), Some(session));
        recorder.tool_finished(&launch, crate::exec::events::ToolCallStatus::Completed, None, "still running", None);
        recorder.record_exec_session_output(
            &launch.id,
            "exec_command",
            &args,
            &serde_json::json!({"session_id":session,"output":"still running"}),
        );
        launch.id
    }

    #[tokio::test]
    async fn exec_session_verification_tracks_terminal_output_and_preserves_launch_task() {
        use vtcode_memory::explanation::ExplanationScope;
        let workspace = tempfile::tempdir().unwrap();
        let sink = SessionStoreSink::open(workspace.path(), "exec-verification").await.unwrap();
        let mut recorder = ExecEventRecorder::new("exec-verification", Some(sink.event_sink()), None);
        let launch_task = recorder.task_started("Verify changes");
        for session in ["successful", "failed", "pending"] {
            pending_verifier(&mut recorder, session);
        }
        let pending = sink.explanation(ExplanationScope::Task).await.unwrap();
        assert_eq!(pending.verification.len(), 3);
        assert!(
            pending
                .verification
                .iter()
                .all(|entry| entry.fact.status == "pending" && !entry.fresh)
        );
        for (requested, returned) in [("unrelated", "unrelated"), ("successful", "failed")] {
            recorder.record_exec_session_output(
                "poll",
                "write_stdin",
                &serde_json::json!({"session_id":requested,"chars":""}),
                &serde_json::json!({"session_id":returned,"exit_code":0}),
            );
        }
        assert!(
            sink.explanation(ExplanationScope::Task)
                .await
                .unwrap()
                .verification
                .iter()
                .all(|entry| entry.fact.status == "pending")
        );
        for (session, exit) in [("successful", Some(0)), ("failed", Some(7)), ("pending", None)] {
            recorder.record_exec_session_output(
                "poll",
                "write_stdin",
                &serde_json::json!({"session_id":session,"chars":""}),
                &serde_json::json!({"session_id":session,"exit_code":exit,"output":"terminal observation"}),
            );
        }
        let terminal = sink.explanation(ExplanationScope::Task).await.unwrap();
        assert_eq!(
            terminal
                .verification
                .iter()
                .map(|entry| (entry.fact.status.as_str(), entry.exit_code, entry.fresh))
                .collect::<Vec<_>>(),
            vec![
                ("passed", Some(0), true),
                ("failed or denied", Some(7), false),
                ("pending", None, false)
            ]
        );
        let new_task = recorder.task_started("A different task");
        assert_ne!(launch_task, new_task);
        recorder.record_exec_session_output(
            "poll-new-task",
            "write_stdin",
            &serde_json::json!({"session_id":"pending","chars":""}),
            &serde_json::json!({"session_id":"pending","exit_code":0}),
        );
        assert!(sink.explanation(ExplanationScope::Task).await.unwrap().verification.is_empty());
        let all = sink.explanation(ExplanationScope::Session).await.unwrap();
        assert_eq!(all.verification.len(), 3);
        assert!(
            all.verification
                .iter()
                .all(|entry| entry.fact.task_id.as_deref() == Some(launch_task.as_str()))
        );
        assert_eq!(all.verification[2].fact.status, "passed");
        sink.close().await.unwrap();
    }

    #[tokio::test]
    async fn exec_session_verification_launched_before_a_mutation_remains_stale() {
        let workspace = tempfile::tempdir().unwrap();
        let sink = SessionStoreSink::open(workspace.path(), "exec-freshness").await.unwrap();
        let mut recorder = ExecEventRecorder::new("exec-freshness", Some(sink.event_sink()), None);
        recorder.task_started("Verify changes");
        pending_verifier(&mut recorder, "before-edit");
        recorder.record_thread_event(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "edit".into(),
                context: None,
                details: ThreadItemDetails::FileChange(Box::new(crate::exec::events::FileChangeItem {
                    changes: vec![crate::exec::events::FileUpdateChange {
                        path: "parser.rs".into(),
                        kind: crate::exec::events::PatchChangeKind::Update,
                    }],
                    status: crate::exec::events::PatchApplyStatus::Completed,
                    unified_diff: None,
                    diff_incomplete: None,
                    additions: Some(1),
                    deletions: Some(0),
                })),
            },
        }));
        recorder.record_exec_session_output(
            "poll",
            "write_stdin",
            &serde_json::json!({"session_id":"before-edit","chars":""}),
            &serde_json::json!({"session_id":"before-edit","exit_code":0}),
        );
        let model = sink
            .explanation(vtcode_memory::explanation::ExplanationScope::Task)
            .await
            .unwrap();
        assert_eq!(model.verification[0].fact.status, "passed");
        assert!(!model.verification[0].fresh);
        pending_verifier(&mut recorder, "after-edit");
        recorder.record_exec_session_output(
            "poll",
            "write_stdin",
            &serde_json::json!({"session_id":"after-edit","chars":""}),
            &serde_json::json!({"session_id":"after-edit","exit_code":0}),
        );
        assert!(
            sink.explanation(vtcode_memory::explanation::ExplanationScope::Task)
                .await
                .unwrap()
                .verification[1]
                .fresh
        );
        sink.close().await.unwrap();
    }

    #[tokio::test]
    async fn explanation_barrier_and_decision_validation_include_accepted_events() {
        let workspace = tempfile::tempdir().unwrap();
        let sink = SessionStoreSink::open(workspace.path(), "explanation-barrier").await.unwrap();
        let mut recorder = ExecEventRecorder::new("explanation-barrier", Some(sink.event_sink()), None);
        let task_id = recorder.task_started("Fix parser");
        recorder.record_thread_event(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "parser-change".into(),
                context: None,
                details: ThreadItemDetails::Decision(Box::new(crate::exec::events::DecisionItem {
                    summary: "Use existing parser".into(),
                    rationale: "Preserve validated inputs".into(),
                    alternatives: vec![],
                    evidence_ids: vec![],
                })),
            },
        }));
        let validate = sink.decision_validator();
        validate(task_id.clone(), vec!["parser-change".into()]).await.unwrap();
        assert!(validate("other-task".into(), vec!["parser-change".into()]).await.is_err());
        assert!(validate(task_id.clone(), vec!["missing".into()]).await.is_err());
        let model = sink
            .explanation(vtcode_memory::explanation::ExplanationScope::Task)
            .await
            .unwrap();
        assert_eq!(model.task_id.as_deref(), Some(task_id.as_str()));
        assert_eq!(model.decisions.len(), 1);
        let projected_page = sink
            .explanation_page(vtcode_memory::explanation::ExplanationScope::Task, 0)
            .await
            .unwrap();
        assert_eq!(projected_page.model.revision, model.revision);
        assert_eq!(projected_page.model.decisions, model.decisions);
        let reference = model.decisions[0].fact.evidence.clone();
        let page = sink.evidence(reference, 0).await.unwrap();
        assert!(page.text.contains("Preserve validated inputs"));
        recorder.turn_completed(Usage::default());
        recorder.turn_started();
        let updated_page = sink
            .explanation_page(vtcode_memory::explanation::ExplanationScope::Task, 0)
            .await
            .unwrap();
        assert_ne!(updated_page.model.revision, projected_page.model.revision);
        assert_eq!(updated_page.model.decisions, model.decisions);
        let starts: Vec<_> = recorder
            .events
            .iter()
            .filter_map(|e| {
                if let ThreadEvent::TurnStarted(t) = e {
                    t.context.as_deref()
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(starts.len(), 2);
        assert_eq!(starts[0].task_id, starts[1].task_id);
        assert_ne!(starts[0].turn_id, starts[1].turn_id);
        sink.close().await.unwrap();
        assert!(validate(task_id, vec!["parser-change".into()]).await.is_err());
    }
    #[test]
    fn ten_thousand_events_share_cached_projections_for_bounded_pages() {
        use vtcode_memory::explanation::{ExplanationScope, page_explanation};

        let workspace = tempfile::tempdir().unwrap();
        let log = vtcode_memory::open(workspace.path(), "large-explanation", DEFAULT_MAX_EVENTS).unwrap();
        let mut context = ExecutionContextTracker::default();
        context.begin("root", "turn", "Verify retained work", crate::exec::events::InputOrigin::User);
        let mut started = ThreadEvent::TurnStarted(TurnStartedEvent::default());
        context.annotate(&mut started);
        log.append(&started).unwrap();
        for index in 0..DEFAULT_MAX_EVENTS - 1 {
            let mut event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ThreadItem {
                    id: format!("check-{index}"),
                    context: None,
                    details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                        command: format!("cargo check --locked # {}", "x".repeat(256)),
                        arguments: None,
                        aggregated_output: "discarded output".repeat(64),
                        exit_code: Some(0),
                        status: CommandExecutionStatus::Completed,
                    })),
                },
            });
            context.annotate(&mut event);
            log.append(&event).unwrap();
        }
        let mut cache = Vec::new();
        let original = cached_explanation(&log, ExplanationScope::Task, &mut cache).unwrap();
        assert_eq!(original.actions.len(), DEFAULT_MAX_EVENTS - 1);
        let mut counter = LimitedByteCounter { bytes: 0, limit: EXPLANATION_CACHE_MAX_BYTES };
        serde_json::to_writer(&mut counter, original.as_ref()).unwrap();
        assert!(counter.bytes > 8 * 1024 * 1024, "fixture exceeds the old cache ceiling");
        let mut offset = 0;
        let mut seen = 0;
        loop {
            let shared = cached_explanation(&log, ExplanationScope::Task, &mut cache).unwrap();
            assert!(Arc::ptr_eq(&original, &shared), "pages reuse the same reduced snapshot");
            let page = page_explanation(&shared, offset);
            assert!(serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
            assert_eq!(page.model.revision, original.revision);
            seen += page.model.actions.len();
            match page.next_offset {
                Some(next) => offset = next,
                None => break,
            }
        }
        assert_eq!(seen, original.actions.len());
        assert_eq!(cache.len(), 1);
    }

    use super::*;
    use crate::core::threads::{ThreadBootstrap, ThreadManager};
    use std::fs;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::sync::oneshot;
    use tokio::time::timeout;

    fn make_recorder() -> ExecEventRecorder {
        ExecEventRecorder::new("thread", None, None)
    }

    #[test]
    fn compact_boundary_reports_the_installed_segment_identity() {
        let previous = crate::core::agent::request_envelope::SessionRequestEnvelope::with_prefix_hash(
            "segment-1",
            "prompt",
            vec![crate::llm::provider::ToolDefinition::function(
                "read_file".to_string(),
                "Read a file".to_string(),
                serde_json::json!({"type": "object"}),
            )],
            11,
            22,
        );
        let next = previous.begin_segment("segment-2");
        let mut recorder = make_recorder();

        recorder.compact_boundary(
            CompactionTrigger::Auto,
            CompactionMode::Local,
            20,
            8,
            None,
            Some(&previous),
            Some(&next),
        );

        let events = recorder.into_events();
        let Some(ThreadEvent::ThreadCompactBoundary(boundary)) = events.last() else {
            panic!("expected compact boundary");
        };
        assert_eq!(boundary.previous_segment_id.as_deref(), Some(previous.segment_id()));
        assert_eq!(boundary.new_segment_id.as_deref(), Some(next.segment_id()));
        assert_eq!(boundary.previous_prefix_hash, boundary.new_prefix_hash);
        assert_eq!(boundary.previous_catalog_hash, boundary.new_catalog_hash);
        assert!(boundary.previous_catalog_hash.is_some());
    }

    #[tokio::test]
    async fn session_store_sink_preserves_order_when_queue_is_small() {
        let workspace = TempDir::new().expect("workspace");
        let (sink, handle) = session_store_sink_with_capacity_handle(workspace.path(), "session", 1)
            .await
            .expect("session sink");
        let health = Arc::clone(&handle.state.health);
        let events = vec![
            ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread".to_string() }),
            ThreadEvent::TurnStarted(TurnStartedEvent::default()),
            ThreadEvent::TurnCompleted(TurnCompletedEvent {
                completed_at: None,
                usage: Usage::default(),
                in_progress_exec_sessions: Vec::new(),
            }),
        ];

        for (index, event) in events.iter().enumerate() {
            if index > 0 {
                timeout(Duration::from_secs(5), async {
                    while health.snapshot().persisted_events < index as u64 {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("bounded queue should keep draining");
            }
            let mut callback = sink.lock();
            callback(event);
            assert_eq!(health.snapshot().accepted_events, index as u64 + 1);
        }
        drop(sink);

        timeout(Duration::from_secs(5), handle.close())
            .await
            .expect("session sink should drain before timeout")
            .expect("session sink should close successfully");

        assert_eq!(health.snapshot().accepted_events, events.len() as u64);
        let log = vtcode_memory::open(workspace.path(), "session", DEFAULT_MAX_EVENTS).expect("reopen session");
        assert_eq!(log.event_count(), events.len() as u64);
        let event_path = workspace.path().join(".vtcode/sessions/session/events.jsonl");
        let persisted = fs::read_to_string(event_path)
            .expect("read persisted events")
            .lines()
            .map(|line| serde_json::from_str::<VersionedThreadEvent>(line).expect("decode event"))
            .map(VersionedThreadEvent::into_event)
            .collect::<Vec<_>>();
        assert_eq!(persisted, events);
        assert_eq!(health.snapshot().append_failures, 0);
        assert_eq!(health.snapshot().channel_failures, 0);
        assert!(!health.snapshot().failed);
    }

    #[tokio::test]
    async fn session_store_sink_open_failure_is_propagated() {
        let temp_dir = TempDir::new().expect("workspace");
        let workspace_file = temp_dir.path().join("workspace-file");
        fs::write(&workspace_file, "not a directory").expect("workspace file");

        let result = SessionStoreSink::open(&workspace_file, "session").await;
        assert!(result.is_err(), "canonical store setup must fail closed");
    }

    #[tokio::test]
    async fn session_store_sink_drain_failure_is_propagated() {
        let health = Arc::new(SessionStoreSinkHealth::default());
        health.failed.store(true, Ordering::Release);
        let state = Arc::new(SessionStoreSinkState {
            sender: Mutex::new(None),
            reserved_bytes: AtomicU64::new(0),
            max_bytes: SESSION_STORE_DRAIN_MAX_BYTES,
            health,
        });
        let drain = tokio::spawn(async {});
        let handle = SessionStoreSinkHandle { state, drain: Some(drain) };

        let result = handle.close().await;
        assert!(result.is_err(), "canonical drain failures must fail closed");
    }

    #[tokio::test]
    async fn session_store_sink_concurrent_close_waits_for_shared_completion() {
        let health = Arc::new(SessionStoreSinkHealth::default());
        let state = Arc::new(SessionStoreSinkState {
            sender: Mutex::new(None),
            reserved_bytes: AtomicU64::new(0),
            max_bytes: SESSION_STORE_DRAIN_MAX_BYTES,
            health,
        });
        let (started_sender, started_receiver) = oneshot::channel();
        let (release_sender, release_receiver) = oneshot::channel();
        let drain = tokio::spawn(async move {
            started_sender.send(()).expect("notify drain start");
            release_receiver.await.expect("release drain");
        });
        let handle = SessionStoreSinkHandle { state: Arc::clone(&state), drain: Some(drain) };
        let sink = Arc::new(SessionStoreSink {
            state,
            handle: Arc::new(Mutex::new(Some(handle))),
            close_result: Arc::new(OnceCell::new()),
        });

        let first = {
            let sink = Arc::clone(&sink);
            tokio::spawn(async move { sink.close().await })
        };
        started_receiver.await.expect("drain should start");
        let second = {
            let sink = Arc::clone(&sink);
            tokio::spawn(async move { sink.close().await })
        };
        tokio::task::yield_now().await;
        assert!(!second.is_finished(), "concurrent close must wait for the first drain");

        release_sender.send(()).expect("release drain");
        assert!(first.await.expect("first close task").is_ok());
        assert!(second.await.expect("second close task").is_ok());
    }

    #[tokio::test]
    async fn session_store_sink_rejects_events_over_byte_budget() {
        let workspace = TempDir::new().expect("workspace");
        let (state, handle) = open_session_store_sink_with_limits(workspace.path(), "session", 4, 128)
            .await
            .expect("session sink");
        let health = Arc::clone(&state.health);
        let event = ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "x".repeat(256) });

        let result = enqueue_session_event(&state, &event);
        assert!(result.is_err(), "oversized events must fail closed");
        handle.close().await.expect_err("failed sink must not close successfully");

        let snapshot = health.snapshot();
        assert_eq!(snapshot.accepted_events, 0);
        assert_eq!(snapshot.serialization_failures, 1);
        assert!(snapshot.failed);
    }

    #[test]
    fn session_store_sink_saturation_fails_closed() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let state = SessionStoreSinkState {
            sender: Mutex::new(Some(sender)),
            reserved_bytes: AtomicU64::new(0),
            max_bytes: SESSION_STORE_DRAIN_MAX_BYTES,
            health: Arc::new(SessionStoreSinkHealth::default()),
        };
        let first = ThreadEvent::TurnStarted(TurnStartedEvent::default());
        let second = ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        });

        enqueue_session_event(&state, &first).expect("first event should fit");
        assert!(enqueue_session_event(&state, &second).is_err(), "queue saturation must fail closed");

        let snapshot = state.health.snapshot();
        assert_eq!(snapshot.accepted_events, 1);
        assert_eq!(snapshot.channel_failures, 1);
        assert!(snapshot.failed);
        drop(receiver);
    }

    #[test]
    fn closed_session_store_channel_is_observable() {
        let (sender, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        let health = SessionStoreSinkHealth::default();
        let event = ThreadEvent::TurnStarted(TurnStartedEvent::default());

        if sender.send(event).is_err() {
            health.channel_failures.fetch_add(1, Ordering::Relaxed);
        }

        assert_eq!(health.snapshot().channel_failures, 1);
    }

    #[test]
    fn streaming_events_flush_on_completion() {
        let mut recorder = make_recorder();
        recorder.turn_started();
        assert!(recorder.agent_message_stream_update("partial"));
        recorder.agent_message_stream_complete();
        let events = recorder.into_events();
        assert!(events.iter().any(|event| matches!(event, ThreadEvent::ItemCompleted(_))));
    }

    #[test]
    fn thread_failure_emits_terminal_lifecycle_events() {
        let mut recorder = make_recorder();
        recorder.turn_started();
        recorder.thread_failed("session", "setup failed", 1);
        let events = recorder.into_events();

        assert!(events.iter().any(|event| matches!(event, ThreadEvent::TurnFailed(_))));
        assert!(events.iter().any(|event| {
            matches!(
                event,
                ThreadEvent::ThreadCompleted(item)
                    if item.subtype == ThreadCompletionSubtype::ErrorDuringExecution
                        && item.outcome_code == "error"
            )
        }));
    }

    #[test]
    fn command_events_capture_status() {
        let mut recorder = make_recorder();
        let handle = recorder.command_started("git status");
        recorder.command_finished(&handle, CommandExecutionStatus::Completed, Some(0), "");
        let events = recorder.into_events();
        let command = events
            .into_iter()
            .filter_map(|event| match event {
                ThreadEvent::ItemCompleted(event) => Some(event.item),
                _ => None,
            })
            .find(|item| matches!(item.details, ThreadItemDetails::CommandExecution(_)))
            .expect("command event should be emitted");

        match command.details {
            ThreadItemDetails::CommandExecution(details) => {
                assert_eq!(details.command, "git status");
                assert_eq!(details.status, CommandExecutionStatus::Completed);
            }
            _ => panic!("unexpected event variant"),
        }
    }

    #[test]
    fn rejected_tool_call_emits_failed_tool_output_item() {
        let mut recorder = make_recorder();
        recorder.tool_rejected("read_file", None, Some("call_1"), "Tool permission denied");

        let events = recorder.into_events();
        let tool_outputs = events
            .iter()
            .filter_map(|event| match event {
                ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) => match &item.details {
                    ThreadItemDetails::ToolOutput(details) => Some(details),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_outputs.len(), 1);
        assert_eq!(tool_outputs[0].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(tool_outputs[0].status, crate::exec::events::ToolCallStatus::Failed);
        assert_eq!(tool_outputs[0].output, "Tool permission denied");
    }

    #[test]
    fn thread_backed_recorder_reuses_submission_id_within_turn() {
        let handle = ThreadManager::new().start_thread_with_identifier("thread", ThreadBootstrap::new(None));
        let mut recorder = ExecEventRecorder::new("thread", None, Some(handle.clone()));

        recorder.turn_started();
        recorder.agent_message("hello");
        recorder.turn_completed(Usage::default());

        let records = handle.replay_recent();
        let submission_ids: std::collections::BTreeSet<String> = records
            .iter()
            .filter_map(|record| record.submission_id.as_ref().map(|id| id.as_str().to_string()))
            .collect();

        assert_eq!(submission_ids.len(), 1);
        assert!(
            records
                .iter()
                .any(|record| matches!(record.event, ThreadEvent::TurnStarted(_)) && record.submission_id.is_some())
        );
        assert!(
            records
                .iter()
                .any(|record| matches!(record.event, ThreadEvent::TurnCompleted(_)) && record.submission_id.is_some())
        );
    }

    #[test]
    fn thread_backed_recorder_keeps_full_event_history_beyond_thread_buffer() {
        let handle = ThreadManager::with_event_buffer_capacity(2)
            .start_thread_with_identifier("thread", ThreadBootstrap::new(None));
        let mut recorder = ExecEventRecorder::new("thread", None, Some(handle.clone()));

        recorder.turn_started();
        recorder.agent_message("first");
        recorder.agent_message("second");
        recorder.turn_completed(Usage::default());

        let full_events = recorder.into_events();
        let buffered_events = handle.recent_events();

        assert_eq!(buffered_events.len(), 2);
        assert!(full_events.len() > buffered_events.len());
        assert_eq!(
            full_events
                .iter()
                .filter(|event| matches!(event, ThreadEvent::ItemCompleted(_)))
                .count(),
            2
        );
    }
}
