use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;

use uuid::Uuid;
use vtcode_config::OpenResponsesConfig;
use vtcode_core::core::agent::events::{
    tool_invocation_completed_event as shared_tool_invocation_completed_event,
    tool_output_completed_event as shared_tool_output_completed_event,
    tool_output_started_event as shared_tool_output_started_event,
    tool_output_updated_event as shared_tool_output_updated_event, tool_started_event as shared_tool_started_event,
};
#[cfg(test)]
use vtcode_core::exec::events::ThreadStartedEvent;
use vtcode_core::exec::events::{
    AgentMessageItem, CompactionMode, CompactionTrigger, HarnessEventItem, HarnessEventKind, ItemCompletedEvent,
    ReasoningItem, ThreadCompactBoundaryEvent, ThreadCompletedEvent, ThreadCompletionSubtype, ThreadEvent, ThreadItem,
    ThreadItemDetails, ToolCallStatus, ToolOutcome, TurnBlockedEvent, TurnCompletedEvent, TurnFailedEvent,
    TurnStartedEvent, Usage, VersionedThreadEvent,
};
#[cfg(test)]
use vtcode_webmcp::EventHubConfig;
use vtcode_webmcp::WebmcpEventHub;

mod atif;
mod canonical;
mod legacy;
mod open_responses;
mod paths;

use atif::AtifExporter;
use canonical::CanonicalEventSink;
use legacy::LegacyWriter;
use open_responses::OpenResponsesExporter;
pub(crate) use paths::{HARNESS_LOG_MAX_AGE_DAYS, prune_old_harness_logs, resolve_event_log_path};

#[derive(Clone)]
pub(crate) struct HarnessEventEmitter {
    inner: Arc<HarnessEventEmitterInner>,
}

struct HarnessEventEmitterInner {
    context: Mutex<vtcode_core::core::agent::events::ExecutionContextTracker>,
    path: PathBuf,
    session_id: Option<String>,
    canonical: Option<CanonicalEventSink>,
    legacy: Mutex<Option<LegacyWriter>>,
    open_responses: Mutex<Option<OpenResponsesExporter>>,
    atif: Mutex<Option<AtifExporter>>,
    webmcp_event_hub: Mutex<Option<WebmcpEventHub>>,
    dispatch_gate: Mutex<()>,
    finalized: AtomicBool,
    finish_result: OnceCell<std::result::Result<(), String>>,
}

impl HarnessEventEmitter {
    pub(crate) fn matrix_persistence(&self) -> Option<vtcode_core::subagents::matrix::MatrixPersistence> {
        self.inner.canonical.as_ref().map(CanonicalEventSink::matrix_persistence)
    }
    pub(crate) fn decision_validator(&self) -> Option<vtcode_core::core::agent::events::DecisionEvidenceValidator> {
        self.inner.canonical.as_ref().map(CanonicalEventSink::decision_validator)
    }
    pub(crate) async fn explanation(
        &self,
        scope: vtcode_memory::explanation::ExplanationScope,
    ) -> Result<vtcode_memory::explanation::ExplanationModel> {
        self.inner
            .canonical
            .as_ref()
            .context("canonical evidence is unavailable")?
            .explanation(scope)
            .await
    }
    pub(crate) async fn explanation_page(
        &self,
        scope: vtcode_memory::explanation::ExplanationScope,
        offset: usize,
    ) -> Result<vtcode_memory::explanation::ExplanationPage> {
        self.inner
            .canonical
            .as_ref()
            .context("canonical evidence is unavailable")?
            .explanation_page(scope, offset)
            .await
    }
    pub(crate) async fn evidence(
        &self,
        reference: vtcode_memory::explanation::EvidenceRef,
        offset: usize,
    ) -> Result<vtcode_memory::explanation::EvidencePage> {
        self.inner
            .canonical
            .as_ref()
            .context("canonical evidence is unavailable")?
            .evidence(reference, offset)
            .await
    }
    /// Construct a compatibility-only emitter for tests and explicit legacy
    /// integrations. Interactive production sessions use [`Self::new_async`]
    /// so canonical persistence is opened before the session starts.
    #[cfg(test)]
    pub(crate) fn new(path: PathBuf) -> Result<Self> {
        let legacy = LegacyWriter::new_sync(&path)?;
        Ok(Self {
            inner: Arc::new(HarnessEventEmitterInner {
                context: Mutex::new(Default::default()),
                path,
                session_id: None,
                canonical: None,
                legacy: Mutex::new(Some(legacy)),
                open_responses: Mutex::new(None),
                atif: Mutex::new(None),
                webmcp_event_hub: Mutex::new(None),
                dispatch_gate: Mutex::new(()),
                finalized: AtomicBool::new(false),
                finish_result: OnceCell::new(),
            }),
        })
    }

    /// Construct an emitter with the workspace session store as its
    /// authoritative sink. `legacy_path` is optional and is only used for the
    /// explicit compatibility/export path configured by the user.
    pub(crate) async fn new_async(workspace: &Path, session_id: &str, legacy_path: Option<PathBuf>) -> Result<Self> {
        let canonical = CanonicalEventSink::open(workspace, session_id).await?;
        let session_path = vtcode_memory::session_directory(workspace, session_id);
        let legacy = match legacy_path {
            Some(path) => match LegacyWriter::new_async(path).await {
                Ok(writer) => Some(writer),
                Err(error) => {
                    tracing::warn!(
                        target: "vtcode.harness",
                        phase = "legacy_export_setup",
                        error = %error,
                        "optional legacy harness export setup failed; continuing with canonical persistence"
                    );
                    None
                }
            },
            None => None,
        };

        Ok(Self {
            inner: Arc::new(HarnessEventEmitterInner {
                context: Mutex::new(Default::default()),
                path: session_path.join("events.jsonl"),
                session_id: Some(session_id.to_string()),
                canonical: Some(canonical),
                legacy: Mutex::new(legacy),
                open_responses: Mutex::new(None),
                atif: Mutex::new(None),
                webmcp_event_hub: Mutex::new(None),
                dispatch_gate: Mutex::new(()),
                finalized: AtomicBool::new(false),
                finish_result: OnceCell::new(),
            }),
        })
    }

    /// Enables Open Responses event emission with the given configuration.
    ///
    /// When enabled, events are also written in Open Responses format to a separate file.
    #[cfg(test)]
    fn enable_open_responses(
        &self,
        config: OpenResponsesConfig,
        model: &str,
        output_path: Option<PathBuf>,
    ) -> Result<()> {
        let Some(exporter) = OpenResponsesExporter::new_sync(config, model, output_path)? else {
            return Ok(());
        };
        let mut guard = self
            .inner
            .open_responses
            .lock()
            .map_err(|e| anyhow::anyhow!("Open Responses lock poisoned: {e}"))?;
        *guard = Some(exporter);

        Ok(())
    }

    /// Enables Open Responses output using the non-blocking exporter path.
    pub(crate) async fn enable_open_responses_async(
        &self,
        config: OpenResponsesConfig,
        model: &str,
        output_path: Option<PathBuf>,
    ) -> Result<()> {
        let Some(exporter) = OpenResponsesExporter::new_async(config, model, output_path).await? else {
            return Ok(());
        };
        let mut guard = self
            .inner
            .open_responses
            .lock()
            .map_err(|error| anyhow::anyhow!("Open Responses lock poisoned: {error}"))?;
        *guard = Some(exporter);
        Ok(())
    }

    /// Enables ATIF trajectory export.
    ///
    /// When enabled, events are collected and written as one JSON file during
    /// asynchronous finalization.
    pub(crate) fn enable_atif(&self, model: &str, output_path: PathBuf) -> Result<()> {
        let mut guard = self.inner.atif.lock().map_err(|e| anyhow::anyhow!("ATIF lock poisoned: {e}"))?;
        *guard = Some(AtifExporter::new(model, output_path));
        Ok(())
    }

    /// Finishes the ATIF trajectory and writes the JSON file off the async
    /// runtime. Returns (prompt_tokens, completion_tokens, cached_tokens).
    async fn finish_atif(&self) -> Result<(u64, u64, u64)> {
        let state = match self.inner.atif.lock() {
            Ok(mut guard) => guard.take(),
            Err(error) => return Err(anyhow::anyhow!("ATIF state lock poisoned: {error}")),
        };
        let Some(state) = state else {
            return Ok((0, 0, 0));
        };
        state.finish().await
    }

    pub(crate) fn begin_task_turn(
        &self,
        turn: &str,
        input: &str,
        origin: vtcode_core::exec::events::InputOrigin,
    ) -> Result<String> {
        let mut tracker = self
            .inner
            .context
            .lock()
            .map_err(|error| anyhow::anyhow!("execution context lock poisoned: {error}"))?;
        let context = tracker.begin(self.inner.session_id.as_deref().unwrap_or("root"), turn, input, origin);
        Ok(context.task_id)
    }

    pub(crate) fn emit(&self, mut event: ThreadEvent) -> Result<()> {
        let _dispatch = self
            .inner
            .dispatch_gate
            .lock()
            .map_err(|error| anyhow::anyhow!("harness dispatch gate poisoned: {error}"))?;
        if self.inner.finalized.load(Ordering::Acquire) {
            return Err(anyhow::anyhow!("harness emitter has already been finalized"));
        }
        let decision = vtcode_core::core::agent::events::decision_completed_event(&event);
        let exec_completion = self
            .inner
            .context
            .lock()
            .map_err(|error| anyhow::anyhow!("execution context lock poisoned: {error}"))?
            .background_exec_output_event(&mut event);
        self.emit_single(event)?;
        if let Some(decision) = decision {
            self.emit_single(decision)?;
        }
        if let Some(completion) = exec_completion {
            self.emit_single(completion)?;
        }
        Ok(())
    }

    pub(crate) fn emit_exec_session_output(
        &self,
        call_item_id: &str,
        tool_name: &str,
        args: &Value,
        output: &Value,
    ) -> Result<()> {
        let _dispatch = self
            .inner
            .dispatch_gate
            .lock()
            .map_err(|error| anyhow::anyhow!("harness dispatch gate poisoned: {error}"))?;
        if self.inner.finalized.load(Ordering::Acquire) {
            return Err(anyhow::anyhow!("harness emitter has already been finalized"));
        }
        let event = self
            .inner
            .context
            .lock()
            .map_err(|error| anyhow::anyhow!("execution context lock poisoned: {error}"))?
            .exec_session_output_event(call_item_id, tool_name, args, output);
        if let Some(event) = event {
            self.emit_single(event)?;
        }
        let delegation = self
            .inner
            .context
            .lock()
            .map_err(|error| anyhow::anyhow!("execution context lock poisoned: {error}"))?
            .delegation_status_event(call_item_id, tool_name, output);
        if let Some(event) = delegation {
            self.emit_single(event)?;
        }
        Ok(())
    }

    fn emit_single(&self, mut event: ThreadEvent) -> Result<()> {
        self.inner
            .context
            .lock()
            .map_err(|error| anyhow::anyhow!("execution context lock poisoned: {error}"))?
            .annotate(&mut event);

        // Canonical persistence is the authoritative path. Its bounded handoff
        // may apply backpressure, but it never performs filesystem I/O here.
        if let Some(canonical) = &self.inner.canonical {
            canonical.emit(&event)?;
        }

        {
            match self.inner.webmcp_event_hub.lock() {
                Ok(webmcp_hub) => {
                    if let Some(hub) = webmcp_hub.as_ref()
                        && let Err(error) = hub.publish(VersionedThreadEvent::new(event.clone()))
                    {
                        // WebMCP is an optional replay consumer. A browser
                        // disconnect or a full bridge queue must not prevent
                        // the canonical event (already persisted above) or
                        // the remaining exporters from observing this event.
                        tracing::warn!(
                            target: "vtcode.harness",
                            phase = "webmcp_export",
                            path = %self.inner.path.display(),
                            error = %error,
                            "optional WebMCP event publish failed"
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        target: "vtcode.harness",
                        phase = "webmcp_export",
                        path = %self.inner.path.display(),
                        error = %error,
                        "optional WebMCP event hub lock poisoned"
                    );
                }
            }
        }

        // The legacy JSONL file is an optional compatibility export.
        if let Ok(guard) = self.inner.legacy.lock() {
            if let Some(writer) = guard.as_ref() {
                match serde_json::to_string(&VersionedThreadEvent::new(event.clone())) {
                    Ok(serialized) => {
                        if let Err(error) = writer.write_line(serialized) {
                            tracing::warn!(target: "vtcode.harness", phase = "legacy_export", path = %self.inner.path.display(), error = %error, "legacy harness export write failed");
                        }
                    }
                    Err(error) => {
                        tracing::warn!(target: "vtcode.harness", phase = "legacy_export", path = %self.inner.path.display(), error = %error, "legacy harness event serialization failed")
                    }
                }
            }
        } else {
            tracing::warn!(target: "vtcode.harness", phase = "legacy_export", path = %self.inner.path.display(), "legacy harness exporter lock poisoned");
        }

        // All consumers are fed while holding one gate, so their observed order
        // is identical even when an optional exporter is slow or saturated.
        match self.inner.open_responses.lock() {
            Ok(mut guard) => {
                if let Some(state) = guard.as_mut() {
                    state.process_event(&event, &self.inner.path);
                }
            }
            Err(error) => {
                tracing::warn!(target: "vtcode.harness", phase = "open_responses_export", path = %self.inner.path.display(), error = %error, "Open Responses exporter lock poisoned")
            }
        }

        match self.inner.atif.lock() {
            Ok(mut guard) => {
                if let Some(state) = guard.as_mut() {
                    state.process_event(&event);
                }
            }
            Err(error) => {
                tracing::warn!(target: "vtcode.harness", phase = "atif_export", path = %self.inner.path.display(), error = %error, "ATIF exporter lock poisoned")
            }
        }

        Ok(())
    }

    /// Attach the active WebMCP event hub to this session's canonical event
    /// fan-out. Runtime events remain `VersionedThreadEvent` values; the hub
    /// only adds transport sequencing for browser replay.
    pub(crate) fn attach_webmcp_event_hub(&self, hub: WebmcpEventHub) -> Result<()> {
        let mut guard = self
            .inner
            .webmcp_event_hub
            .lock()
            .map_err(|error| anyhow::anyhow!("WebMCP event hub lock poisoned: {error}"))?;
        *guard = Some(hub);
        Ok(())
    }

    /// Emit a completed assistant message for a response that did not come
    /// from the streaming lifecycle bridge, such as deterministic recovery.
    /// Keeping this on the normal emitter preserves the same ThreadEvent
    /// contract for interactive, headless, Open Responses, and ATIF output.
    pub(crate) fn emit_assistant_message(&self, turn_id: &str, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }

        self.emit(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: format!("{turn_id}-assistant-final-{}", Uuid::new_v4()),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: text.to_string() }),
            },
        }))
    }

    /// Emit bounded, non-secret failure diagnosis as a normal reasoning item.
    /// This uses the existing item contract so it remains available to session
    /// replay and exporters without exposing provider-native chain-of-thought.
    pub(crate) fn emit_diagnosis(&self, turn_id: &str, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }

        self.emit(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: format!("{turn_id}-diagnosis-{}", Uuid::new_v4()),
                details: ThreadItemDetails::Reasoning(Box::new(ReasoningItem {
                    text: text.to_string(),
                    stage: Some("diagnosis".to_string()),
                })),
            },
        }))
    }

    /// Finish every exporter and drain canonical persistence exactly once.
    pub(crate) async fn finish(&self) -> Result<()> {
        let result = self
            .inner
            .finish_result
            .get_or_init(|| async { self.finish_once().await.map_err(|error| error.to_string()) })
            .await;
        match result {
            Ok(()) => Ok(()),
            Err(error) => Err(anyhow::anyhow!(error.clone())),
        }
    }

    async fn finish_once(&self) -> Result<()> {
        let already_finalized = {
            let _dispatch = self
                .inner
                .dispatch_gate
                .lock()
                .map_err(|error| anyhow::anyhow!("harness dispatch gate poisoned: {error}"))?;
            self.inner.finalized.swap(true, Ordering::AcqRel)
        };
        if already_finalized {
            return Ok(());
        }

        self.finish_open_responses_async().await;
        if let Err(error) = self.finish_atif().await {
            tracing::warn!(target: "vtcode.harness", phase = "atif_finish", path = %self.inner.path.display(), error = %error, "ATIF export finalization failed");
        }

        let legacy = self.inner.legacy.lock().ok().and_then(|mut guard| guard.take());
        if let Some(writer) = legacy {
            if let Err(error) = writer.flush().await {
                tracing::warn!(target: "vtcode.harness", phase = "legacy_export_flush", path = %self.inner.path.display(), error = %error, "legacy harness export flush failed");
            }
            let diagnostics = writer.diagnostics();
            if diagnostics.dropped_lines > 0 || diagnostics.write_failures > 0 {
                tracing::warn!(
                    target: "vtcode.harness",
                    phase = "legacy_export_drops",
                    path = %self.inner.path.display(),
                    dropped_lines = diagnostics.dropped_lines,
                    dropped_bytes = diagnostics.dropped_bytes,
                    write_failures = diagnostics.write_failures,
                    "optional legacy harness export dropped data"
                );
            }
        }

        if let Some(canonical) = &self.inner.canonical {
            canonical.close().await.context("canonical session event drain failed")?;
        }
        Ok(())
    }

    async fn finish_open_responses_async(&self) {
        let state = self.inner.open_responses.lock().ok().and_then(|mut guard| guard.take());
        let Some(state) = state else {
            return;
        };
        state.finish_async(&self.inner.path).await;
    }

    pub(crate) async fn finish_after_unexpected_exit(&self) {
        let Some(session_id) = self.inner.session_id.clone() else {
            return;
        };
        if let Err(error) = self.emit(turn_failed_event("interactive session exited before harness finalization", None))
        {
            tracing::error!(target: "vtcode.harness", phase = "drop_finalize", error = %error, "failed to enqueue unexpected-exit turn.failed event");
        }
        if let Err(error) = self.emit(thread_completed_event(
            session_id.clone(),
            session_id,
            ThreadCompletionSubtype::ErrorDuringExecution,
            "error",
            None,
            Some("interactive session exited before harness finalization".to_string()),
            Usage::default(),
            None,
            0,
        )) {
            tracing::error!(target: "vtcode.harness", phase = "drop_finalize", error = %error, "failed to enqueue unexpected-exit thread.completed event");
        }
        if let Err(error) = self.finish().await {
            tracing::error!(target: "vtcode.harness", phase = "drop_finalize", error = %error, "failed to finalize harness after an unexpected session exit");
        }
    }

    /// Synchronous compatibility finalizer used by focused unit tests.
    #[cfg(test)]
    fn finish_open_responses(&self) {
        let state = self.inner.open_responses.lock().ok().and_then(|mut guard| guard.take());
        let Some(state) = state else {
            return;
        };
        state.finish_sync(&self.inner.path);
    }

    #[cfg(test)]
    fn path(&self) -> &Path {
        &self.inner.path
    }
}

impl Drop for HarnessEventEmitter {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) != 1
            || self.inner.canonical.is_none()
            || self.inner.finalized.load(Ordering::Acquire)
        {
            return;
        }

        let emitter = self.clone();
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::error!(target: "vtcode.harness", phase = "drop_finalize", "cannot finalize harness after an unexpected session exit without a Tokio runtime");
            return;
        };
        runtime.spawn(async move { emitter.finish_after_unexpected_exit().await });
    }
}

pub(crate) fn tool_started_event(
    item_id: String,
    tool_name: &str,
    args: Option<&Value>,
    tool_call_id: Option<&str>,
) -> ThreadEvent {
    shared_tool_started_event(item_id, tool_name, args, tool_call_id)
}

pub(crate) fn tool_invocation_completed_event(
    item_id: String,
    tool_name: &str,
    args: Option<&Value>,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
) -> ThreadEvent {
    shared_tool_invocation_completed_event(item_id, tool_name, args, tool_call_id, status, ToolOutcome::Success)
}

pub(crate) fn tool_output_started_event(call_item_id: String, tool_call_id: Option<&str>) -> ThreadEvent {
    shared_tool_output_started_event(call_item_id, tool_call_id)
}

pub(crate) fn tool_output_completed_event(
    call_item_id: String,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
    exit_code: Option<i32>,
    spool_path: Option<&str>,
    output: impl Into<String>,
) -> ThreadEvent {
    shared_tool_output_completed_event(call_item_id, tool_call_id, status, exit_code, spool_path, output)
}

pub(crate) fn tool_updated_event(
    call_item_id: String,
    tool_call_id: Option<&str>,
    output: impl Into<String>,
) -> ThreadEvent {
    shared_tool_output_updated_event(call_item_id, tool_call_id, output)
}

pub(crate) fn turn_started_event() -> ThreadEvent {
    ThreadEvent::TurnStarted(TurnStartedEvent::default())
}

pub(crate) fn turn_completed_event(usage: Usage) -> ThreadEvent {
    turn_completed_event_with_sessions(usage, Vec::new())
}

pub(crate) fn turn_completed_event_with_sessions(usage: Usage, sessions: Vec<String>) -> ThreadEvent {
    ThreadEvent::TurnCompleted(TurnCompletedEvent {
        completed_at: None,
        usage,
        in_progress_exec_sessions: sessions
            .into_iter()
            .take(vtcode_core::exec::events::MAX_IN_PROGRESS_EXEC_SESSIONS)
            .collect(),
    })
}

pub(crate) fn turn_failed_event(message: impl Into<String>, usage: Option<Usage>) -> ThreadEvent {
    ThreadEvent::TurnFailed(TurnFailedEvent { completed_at: None, message: message.into(), usage })
}

pub(crate) fn turn_blocked_event(event: TurnBlockedEvent) -> ThreadEvent {
    ThreadEvent::TurnBlocked(Box::new(event))
}

pub(crate) fn thread_completed_event(
    thread_id: impl Into<String>,
    session_id: impl Into<String>,
    subtype: ThreadCompletionSubtype,
    outcome_code: impl Into<String>,
    result: Option<String>,
    stop_reason: Option<String>,
    usage: Usage,
    total_cost_usd: Option<serde_json::Number>,
    num_turns: usize,
) -> ThreadEvent {
    ThreadEvent::ThreadCompleted(Box::new(ThreadCompletedEvent {
        completed_at: None,
        thread_id: thread_id.into(),
        session_id: session_id.into(),
        subtype,
        outcome_code: outcome_code.into(),
        result,
        stop_reason,
        usage,
        total_cost_usd,
        num_turns,
    }))
}

pub(crate) fn compact_boundary_event(
    thread_id: impl Into<String>,
    trigger: CompactionTrigger,
    mode: CompactionMode,
    original_message_count: usize,
    compacted_message_count: usize,
    history_artifact_path: Option<String>,
    segment_transition: Option<&crate::agent::runloop::unified::state::RequestSegmentTransition>,
) -> ThreadEvent {
    ThreadEvent::ThreadCompactBoundary(Box::new(ThreadCompactBoundaryEvent {
        thread_id: thread_id.into(),
        trigger,
        mode,
        original_message_count,
        compacted_message_count,
        history_artifact_path,
        previous_segment_id: segment_transition.and_then(|transition| transition.previous_segment_id.clone()),
        new_segment_id: segment_transition.map(|transition| transition.new_segment_id.clone()),
        previous_prefix_hash: segment_transition.and_then(|transition| transition.previous_prefix_hash.clone()),
        new_prefix_hash: segment_transition.and_then(|transition| transition.previous_prefix_hash.clone()),
        previous_catalog_hash: segment_transition.and_then(|transition| transition.previous_catalog_hash.clone()),
        new_catalog_hash: segment_transition.and_then(|transition| transition.previous_catalog_hash.clone()),
    }))
}

pub(crate) fn harness_event(
    event: HarnessEventKind,
    message: Option<String>,
    path: Option<String>,
    attempt: Option<u32>,
    error_category: Option<String>,
) -> ThreadEvent {
    harness_event_with_duration(event, message, path, attempt, error_category, None)
}

pub(crate) fn tool_outcome_observation_event(
    tool_name: &str,
    attempts: u32,
    duration_ms: u64,
    error_category: Option<String>,
) -> ThreadEvent {
    harness_event_with_duration(
        HarnessEventKind::ToolLatencyRecorded,
        Some(format!("{tool_name} completed in {duration_ms}ms")),
        None,
        Some(attempts.max(1)),
        error_category,
        Some(duration_ms),
    )
}

fn harness_event_with_duration(
    event: HarnessEventKind,
    message: Option<String>,
    path: Option<String>,
    attempt: Option<u32>,
    error_category: Option<String>,
    duration_ms: Option<u64>,
) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: format!("harness-{}", Uuid::new_v4()),
            details: ThreadItemDetails::Harness(Box::new(HarnessEventItem {
                event,
                message,
                command: None,
                path,
                exit_code: None,
                attempt,
                error_category,
                duration_ms,
                task_id: None,
                session_id: None,
                exec_session_id: None,
                status: None,
                transcript_path: None,
                archive_path: None,
            })),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::run_loop_context::TurnRunId;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;
    use vtcode_core::exec::events::ItemStartedEvent;
    use vtcode_memory::DEFAULT_MAX_EVENTS;

    #[test]
    fn resolve_event_log_path_appends_jsonl_when_directory() {
        let tmp = TempDir::new().expect("temp dir");
        let run_id = TurnRunId("run-123".to_string());
        let resolved = resolve_event_log_path(tmp.path().to_str().expect("path"), &run_id);

        let file_name = resolved.file_name().and_then(|name| name.to_str()).expect("file name");
        assert!(file_name.starts_with("harness-run-123-"));
        assert!(file_name.ends_with(".jsonl"));
    }

    #[test]
    fn tool_outcome_observation_carries_attempt_duration_and_category() {
        let event = tool_outcome_observation_event("read_file", 2, 37, Some("Network".to_string()));
        let ThreadEvent::ItemCompleted(completed) = event else {
            panic!("expected completed harness item");
        };
        let ThreadItemDetails::Harness(item) = completed.item.details else {
            panic!("expected harness details");
        };
        assert_eq!(item.event, HarnessEventKind::ToolLatencyRecorded);
        assert_eq!(item.attempt, Some(2));
        assert_eq!(item.duration_ms, Some(37));
        assert_eq!(item.error_category.as_deref(), Some("Network"));
    }

    #[test]
    fn emit_writes_versioned_event() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("events.jsonl");
        let emitter = HarnessEventEmitter::new(path.clone()).expect("emitter");

        // Use the path method to verify it works
        assert_eq!(emitter.path(), path.as_path());

        emitter.emit(turn_started_event()).expect("emit");

        let payload = fs::read_to_string(&path).expect("read log");
        let line = payload.lines().next().expect("line");
        let value: Value = serde_json::from_str(line).expect("json");

        assert_eq!(
            value.get("schema_version").and_then(|v| v.as_str()),
            Some(vtcode_core::exec::events::EVENT_SCHEMA_VERSION)
        );
        assert_eq!(value.get("event").and_then(|v| v.get("type")).and_then(|v| v.as_str()), Some("turn.started"));
    }

    #[test]
    fn assistant_message_fallback_is_written_as_completed_item() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("events.jsonl");
        let emitter = HarnessEventEmitter::new(path.clone()).expect("emitter");

        emitter
            .emit_assistant_message("turn-1", "The turn was blocked; retry the request.")
            .expect("emit assistant message");

        let content = fs::read_to_string(path).expect("read harness log");
        assert!(content.contains("item.completed"));
        assert!(content.contains("agent_message"));
        assert!(content.contains("The turn was blocked; retry the request."));
    }

    #[test]
    fn diagnosis_is_written_as_a_reasoning_item_with_a_diagnosis_stage() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("events.jsonl");
        let emitter = HarnessEventEmitter::new(path.clone()).expect("emitter");

        emitter
            .emit_diagnosis("turn-1", "Diagnosis: exec\nObserved: exit 1")
            .expect("emit diagnosis");

        let content = fs::read_to_string(path).expect("read harness log");
        assert!(content.contains("\"type\":\"reasoning\""));
        assert!(content.contains("\"stage\":\"diagnosis\""));
        assert!(content.contains("Observed: exit 1"));
    }

    #[test]
    fn open_responses_integration_writes_sse_events() {
        let tmp = TempDir::new().expect("temp dir");
        let harness_path = tmp.path().join("harness.jsonl");
        let or_path = tmp.path().join("open-responses.jsonl");

        let emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");

        // Enable Open Responses
        let config = OpenResponsesConfig {
            enabled: true,
            emit_events: true,
            include_extensions: true,
            map_tool_calls: true,
            include_reasoning: true,
        };
        emitter
            .enable_open_responses(config, "claude-sonnet-5", Some(or_path.clone()))
            .expect("enable");

        // Emit events
        emitter
            .emit(ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "test-thread".to_string() }))
            .expect("emit");
        emitter.emit(turn_started_event()).expect("emit turn");
        emitter
            .emit(turn_completed_event(Usage {
                input_tokens: 12,
                cached_input_tokens: 3,
                cache_creation_tokens: 0,
                output_tokens: 5,
            }))
            .expect("emit completed");
        emitter.finish_open_responses();

        // Verify harness log
        let harness_content = fs::read_to_string(&harness_path).expect("read harness");
        assert!(harness_content.contains("thread.started"));
        assert!(harness_content.contains("turn.started"));

        // Verify Open Responses log
        let or_content = fs::read_to_string(&or_path).expect("read OR");
        assert!(or_content.contains("response.created"));
        assert!(or_content.contains("response.completed"));
        assert!(or_content.contains("\n\n"), "SSE events must be separated by a blank line");
        assert!(or_content.contains("[DONE]"));
    }

    #[tokio::test]
    async fn async_emitter_drains_large_canonical_batch_and_uses_unique_ids() {
        let tmp = TempDir::new().expect("temp dir");
        let emitter = HarnessEventEmitter::new_async(tmp.path(), "large-session", None)
            .await
            .expect("canonical emitter");

        for index in 0..3_253 {
            emitter
                .emit(harness_event(
                    HarnessEventKind::ToolLatencyRecorded,
                    Some(format!("event-{index}")),
                    None,
                    None,
                    None,
                ))
                .expect("emit canonical event");
        }
        emitter.finish().await.expect("canonical drain");

        let event_path = tmp.path().join(".vtcode/sessions/large-session/events.jsonl");
        let lines = fs::read_to_string(event_path)
            .expect("read canonical events")
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 3_253);

        let mut ids = std::collections::HashSet::new();
        for line in lines {
            let event: VersionedThreadEvent = serde_json::from_str(&line).expect("decode canonical event");
            let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = event.into_event() else {
                panic!("expected harness item event");
            };
            assert!(ids.insert(item.id), "harness item id was duplicated");
        }
    }

    #[tokio::test]
    async fn optional_legacy_export_setup_failure_does_not_disable_canonical_persistence() {
        let tmp = TempDir::new().expect("temp dir");
        let legacy_path = tmp.path().join("legacy-directory");
        fs::create_dir(&legacy_path).expect("create legacy directory");

        let emitter = HarnessEventEmitter::new_async(tmp.path(), "canonical-session", Some(legacy_path))
            .await
            .expect("canonical emitter should remain available");
        emitter.emit(turn_started_event()).expect("emit canonical event");
        emitter.finish().await.expect("canonical drain");

        let event_path = tmp.path().join(".vtcode/sessions/canonical-session/events.jsonl");
        let event_lines = fs::read_to_string(event_path).expect("read canonical events").lines().count();
        assert_eq!(event_lines, 1);
    }

    #[tokio::test]
    async fn webmcp_export_failure_does_not_disable_canonical_persistence() {
        let tmp = TempDir::new().expect("temp dir");
        let emitter = HarnessEventEmitter::new_async(tmp.path(), "webmcp-failure-session", None)
            .await
            .expect("canonical emitter");
        let hub = WebmcpEventHub::new_with_max_event_bytes(EventHubConfig::default(), 1).expect("bounded hub");
        emitter.attach_webmcp_event_hub(hub).expect("attach WebMCP hub");

        emitter
            .emit(turn_started_event())
            .expect("optional WebMCP failure must be isolated");
        emitter.finish().await.expect("canonical emitter should still finish");

        let event_path = tmp.path().join(".vtcode/sessions/webmcp-failure-session/events.jsonl");
        let event_lines = fs::read_to_string(event_path).expect("read canonical events").lines().count();
        assert_eq!(event_lines, 1);
    }

    #[tokio::test]
    async fn async_exporters_share_order_and_finalize_once() {
        let tmp = TempDir::new().expect("temp dir");
        let legacy_path = tmp.path().join("legacy.jsonl");
        let emitter = HarnessEventEmitter::new_async(tmp.path(), "ordered-session", Some(legacy_path.clone()))
            .await
            .expect("canonical emitter");
        let config = OpenResponsesConfig {
            enabled: true,
            emit_events: true,
            include_extensions: true,
            map_tool_calls: true,
            include_reasoning: true,
        };
        let open_responses_path = tmp.path().join("open-responses.jsonl");
        emitter
            .enable_open_responses_async(config, "test-model", Some(open_responses_path.clone()))
            .await
            .expect("Open Responses exporter");

        let events = vec![
            ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "ordered-thread".to_string() }),
            turn_started_event(),
            turn_completed_event(Usage::default()),
        ];
        for event in events {
            emitter.emit(event).expect("emit ordered event");
        }
        emitter.finish().await.expect("finish exporters");
        emitter.finish().await.expect("second finish is idempotent");

        let canonical = fs::read_to_string(tmp.path().join(".vtcode/sessions/ordered-session/events.jsonl"))
            .expect("read canonical events");
        let legacy = fs::read_to_string(legacy_path).expect("read legacy events");
        assert_eq!(canonical.lines().collect::<Vec<_>>(), legacy.lines().collect::<Vec<_>>());

        let open_responses = fs::read_to_string(open_responses_path).expect("read Open Responses events");
        assert!(open_responses.contains("response.created"));
        assert_eq!(open_responses.matches("data: [DONE]").count(), 1);
    }

    #[tokio::test]
    async fn canonical_lifecycle_status_changes_only_at_thread_completion() {
        let tmp = TempDir::new().expect("temp dir");
        let emitter = HarnessEventEmitter::new_async(tmp.path(), "lifecycle-session", None)
            .await
            .expect("canonical emitter");
        emitter
            .emit(ThreadEvent::TurnStarted(TurnStartedEvent::default()))
            .expect("turn start");
        emitter.emit(turn_completed_event(Usage::default())).expect("turn completion");
        emitter.finish().await.expect("finish active session");

        let active =
            vtcode_memory::open(tmp.path(), "lifecycle-session", DEFAULT_MAX_EVENTS).expect("open active session");
        assert_eq!(active.manifest().status, "active");
    }

    #[test]
    fn turn_completed_event_preserves_usage_payload() {
        let event = turn_completed_event(Usage {
            input_tokens: 42,
            cached_input_tokens: 7,
            cache_creation_tokens: 0,
            output_tokens: 9,
        });

        let ThreadEvent::TurnCompleted(TurnCompletedEvent { completed_at: None, usage, .. }) = event else {
            panic!("expected turn.completed");
        };

        assert_eq!(usage.input_tokens, 42);
        assert_eq!(usage.cached_input_tokens, 7);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.output_tokens, 9);
    }

    #[test]
    fn turn_completed_event_with_sessions_caps_ids() {
        let sessions = vec![
            "run-1".to_string(),
            "run-2".to_string(),
            "run-3".to_string(),
            "run-4".to_string(),
            "run-5".to_string(),
            "run-6".to_string(),
        ];
        let event = turn_completed_event_with_sessions(Usage::default(), sessions);
        let ThreadEvent::TurnCompleted(completed) = event else {
            panic!("expected turn.completed");
        };
        assert_eq!(
            completed.in_progress_exec_sessions,
            vec![
                "run-1".to_string(),
                "run-2".to_string(),
                "run-3".to_string(),
                "run-4".to_string(),
            ]
        );

        let empty = turn_completed_event(Usage::default());
        let ThreadEvent::TurnCompleted(empty_completed) = empty else {
            panic!("expected turn.completed");
        };
        assert!(empty_completed.in_progress_exec_sessions.is_empty());
    }

    #[test]
    fn tool_started_event_captures_arguments() {
        let args = json!({ "path": "README.md" });
        let event = tool_started_event("tool-1".to_string(), "read_file", Some(&args), Some("tool_call_0"));

        let ThreadEvent::ItemStarted(ItemStartedEvent { item }) = event else {
            panic!("expected item.started");
        };
        let ThreadItemDetails::ToolInvocation(details) = item.details else {
            panic!("expected tool invocation item");
        };

        assert_eq!(details.tool_name, "read_file");
        assert_eq!(details.arguments, Some(json!({ "path": "README.md" })));
        assert_eq!(details.tool_call_id.as_deref(), Some("tool_call_0"));
        assert_eq!(details.status, ToolCallStatus::InProgress);
    }

    #[test]
    fn tool_invocation_completed_event_captures_raw_tool_call_id() {
        let args = json!({ "path": "README.md" });
        let event = tool_invocation_completed_event(
            "tool-1".to_string(),
            "read_file",
            Some(&args),
            Some("tool_call_0"),
            ToolCallStatus::Completed,
        );

        let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = event else {
            panic!("expected item.completed");
        };
        let ThreadItemDetails::ToolInvocation(details) = item.details else {
            panic!("expected tool invocation item");
        };

        assert_eq!(details.tool_name, "read_file");
        assert_eq!(details.arguments, Some(json!({ "path": "README.md" })));
        assert_eq!(details.tool_call_id.as_deref(), Some("tool_call_0"));
        assert_eq!(details.status, ToolCallStatus::Completed);
    }

    #[test]
    fn tool_output_completed_event_captures_output() {
        let event = tool_output_completed_event(
            "tool-1".to_string(),
            Some("tool_call_0"),
            ToolCallStatus::Completed,
            Some(0),
            None,
            "On branch main",
        );

        let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = event else {
            panic!("expected item.completed");
        };
        assert_eq!(item.id, "tool-1:output");
        let ThreadItemDetails::ToolOutput(details) = item.details else {
            panic!("expected tool output item");
        };

        assert_eq!(details.call_id, "tool-1");
        assert_eq!(details.tool_call_id.as_deref(), Some("tool_call_0"));
        assert_eq!(details.spool_path, None);
        assert_eq!(details.output, "On branch main");
        assert_eq!(details.exit_code, Some(0));
        assert_eq!(details.status, ToolCallStatus::Completed);
    }

    #[test]
    fn tool_output_started_event_starts_empty_output_item() {
        let event = tool_output_started_event("tool-1".to_string(), Some("tool_call_0"));

        let ThreadEvent::ItemStarted(ItemStartedEvent { item }) = event else {
            panic!("expected item.started");
        };
        assert_eq!(item.id, "tool-1:output");
        let ThreadItemDetails::ToolOutput(details) = item.details else {
            panic!("expected tool output item");
        };

        assert_eq!(details.call_id, "tool-1");
        assert_eq!(details.tool_call_id.as_deref(), Some("tool_call_0"));
        assert_eq!(details.spool_path, None);
        assert!(details.output.is_empty());
        assert_eq!(details.status, ToolCallStatus::InProgress);
    }

    #[test]
    fn tool_updated_event_captures_streamed_output() {
        let event = tool_updated_event("tool-1".to_string(), Some("tool_call_0"), "On branch main");

        let ThreadEvent::ItemUpdated(vtcode_core::exec::events::ItemUpdatedEvent { item }) = event else {
            panic!("expected item.updated");
        };
        assert_eq!(item.id, "tool-1:output");
        let ThreadItemDetails::ToolOutput(details) = item.details else {
            panic!("expected tool output item");
        };

        assert_eq!(details.call_id, "tool-1");
        assert_eq!(details.tool_call_id.as_deref(), Some("tool_call_0"));
        assert_eq!(details.spool_path, None);
        assert_eq!(details.output, "On branch main");
        assert_eq!(details.status, ToolCallStatus::InProgress);
    }
}
