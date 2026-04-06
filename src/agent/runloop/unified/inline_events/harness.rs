use anyhow::{Context, Result};
use serde_json::Value;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::Utc;
#[cfg(test)]
use std::path::Path;
use vtcode_config::OpenResponsesConfig;
use vtcode_core::core::agent::events::{
    tool_invocation_completed_event as shared_tool_invocation_completed_event,
    tool_output_completed_event as shared_tool_output_completed_event,
    tool_output_started_event as shared_tool_output_started_event,
    tool_output_updated_event as shared_tool_output_updated_event,
    tool_started_event as shared_tool_started_event,
};
#[cfg(test)]
use vtcode_core::exec::events::ThreadStartedEvent;
use vtcode_core::exec::events::atif::{AtifAgent, AtifTrajectoryBuilder};
use vtcode_core::exec::events::{
    CompactionMode, CompactionTrigger, HarnessEventItem, HarnessEventKind, ItemCompletedEvent,
    ThreadCompactBoundaryEvent, ThreadCompletedEvent, ThreadCompletionSubtype, ThreadEvent,
    ThreadItem, ThreadItemDetails, ToolCallStatus, TurnCompletedEvent, TurnFailedEvent,
    TurnStartedEvent, Usage, VersionedThreadEvent,
};
use vtcode_core::open_responses::{OpenResponsesIntegration, SequencedEvent};
use vtcode_core::utils::file_utils::ensure_dir_exists_sync;

use crate::agent::runloop::unified::run_loop_context::TurnRunId;

#[derive(Clone)]
pub(crate) struct HarnessEventEmitter {
    inner: Arc<HarnessEventEmitterInner>,
}

struct HarnessEventEmitterInner {
    #[cfg(test)]
    path: PathBuf,
    writer: Mutex<BufWriter<File>>,
    open_responses: Mutex<Option<OpenResponsesState>>,
    atif: Mutex<Option<AtifState>>,
}

/// State for ATIF trajectory export.
struct AtifState {
    builder: AtifTrajectoryBuilder,
    output_path: PathBuf,
}

/// State for Open Responses event emission.
struct OpenResponsesState {
    integration: OpenResponsesIntegration,
    writer: Option<BufWriter<File>>,
    sequence_counter: u64,
}

impl HarnessEventEmitter {
    pub(crate) fn new(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            ensure_dir_exists_sync(parent).with_context(|| {
                format!("Failed to create harness log dir {}", parent.display())
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to open harness log {}", path.display()))?;
        Ok(Self {
            inner: Arc::new(HarnessEventEmitterInner {
                #[cfg(test)]
                path,
                writer: Mutex::new(BufWriter::new(file)),
                open_responses: Mutex::new(None),
                atif: Mutex::new(None),
            }),
        })
    }

    /// Enables Open Responses event emission with the given configuration.
    ///
    /// When enabled, events are also written in Open Responses format to a separate file.
    pub(crate) fn enable_open_responses(
        &self,
        config: OpenResponsesConfig,
        model: &str,
        output_path: Option<PathBuf>,
    ) -> Result<()> {
        if !config.enabled {
            return Ok(());
        }

        let writer = if let Some(path) = output_path {
            if let Some(parent) = path.parent() {
                ensure_dir_exists_sync(parent)?;
            }
            let file = OpenOptions::new().create(true).append(true).open(&path)?;
            Some(BufWriter::new(file))
        } else {
            None
        };

        let mut integration = OpenResponsesIntegration::new(config);
        integration.start_response(model);

        let mut guard = self
            .inner
            .open_responses
            .lock()
            .map_err(|_| anyhow::anyhow!("Open Responses lock poisoned"))?;
        *guard = Some(OpenResponsesState {
            integration,
            writer,
            sequence_counter: 0,
        });

        Ok(())
    }

    /// Enables ATIF trajectory export.
    ///
    /// When enabled, events are collected by an `AtifTrajectoryBuilder` and
    /// written as a single JSON file on [`finish_atif`](Self::finish_atif).
    pub(crate) fn enable_atif(&self, model: &str, output_path: PathBuf) -> Result<()> {
        let agent = AtifAgent::vtcode().with_model(model);
        let builder = AtifTrajectoryBuilder::new(agent);

        let mut guard = self
            .inner
            .atif
            .lock()
            .map_err(|_| anyhow::anyhow!("ATIF lock poisoned"))?;
        *guard = Some(AtifState {
            builder,
            output_path,
        });
        Ok(())
    }

    /// Finishes the ATIF trajectory and writes the JSON file to disk.
    pub(crate) fn finish_atif(&self) {
        let state = self.inner.atif.lock().ok().and_then(|mut g| g.take());
        let Some(state) = state else { return };

        let trajectory = state.builder.finish(None);
        let json = match serde_json::to_string_pretty(&trajectory) {
            Ok(j) => j,
            Err(err) => {
                tracing::debug!(error = %err, "failed to serialize ATIF trajectory");
                return;
            }
        };
        if let Some(parent) = state.output_path.parent() {
            let _ = ensure_dir_exists_sync(parent);
        }
        if let Err(err) = std::fs::write(&state.output_path, json) {
            tracing::debug!(
                error = %err,
                path = %state.output_path.display(),
                "failed to write ATIF trajectory"
            );
        }
    }

    pub(crate) fn emit(&self, event: ThreadEvent) -> Result<()> {
        // Write to harness log (internal format)
        let payload = VersionedThreadEvent::new(event.clone());
        {
            let mut writer = self
                .inner
                .writer
                .lock()
                .map_err(|_| anyhow::anyhow!("Harness log lock poisoned"))?;
            let serialized =
                serde_json::to_string(&payload).context("Failed to serialize harness event")?;
            writer
                .write_all(serialized.as_bytes())
                .context("Failed to write harness event")?;
            writer
                .write_all(b"\n")
                .context("Failed to write harness event newline")?;
            writer.flush().context("Failed to flush harness log")?;
        }

        // Also emit to Open Responses format if enabled
        if let Ok(mut guard) = self.inner.open_responses.lock()
            && let Some(ref mut state) = *guard
        {
            state.integration.process_event(&event);

            // Write any emitted Open Responses events
            for stream_event in state.integration.take_events() {
                if let Some(ref mut writer) = state.writer {
                    let seq = state.sequence_counter;
                    state.sequence_counter += 1;
                    let sequenced = SequencedEvent::new(seq, &stream_event);

                    // SSE format
                    let _ = writeln!(writer, "event: {}", stream_event.event_type());
                    if let Ok(json) = serde_json::to_string(&sequenced) {
                        let _ = writeln!(writer, "data: {}", json);
                    }
                    let _ = writeln!(writer);
                    let _ = writer.flush();
                }
            }
        }

        // Also feed ATIF trajectory builder if enabled
        if let Ok(mut guard) = self.inner.atif.lock()
            && let Some(ref mut state) = *guard
        {
            state.builder.process_event(&event);
        }

        Ok(())
    }

    /// Finishes the Open Responses session and writes the terminal marker.
    pub(crate) fn finish_open_responses(&self) {
        if let Ok(mut guard) = self.inner.open_responses.lock()
            && let Some(ref mut state) = *guard
        {
            let _ = state.integration.finish_response();
            if let Some(ref mut writer) = state.writer {
                let _ = writeln!(writer, "data: [DONE]");
                let _ = writer.flush();
            }
        }
    }

    #[cfg(test)]
    fn path(&self) -> &Path {
        &self.inner.path
    }
}

pub(crate) fn resolve_event_log_path(path: &str, run_id: &TurnRunId) -> PathBuf {
    let mut base = PathBuf::from(path);
    if base.extension().is_none() {
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
        base = base.join(format!("harness-{}-{}.jsonl", run_id.0, timestamp));
    }
    base
}

/// Returns the default harness log directory for the current VT Code session dir.
pub(crate) fn default_harness_log_dir() -> Option<PathBuf> {
    Some(vtcode_core::utils::session_debug::default_sessions_dir())
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
    shared_tool_invocation_completed_event(item_id, tool_name, args, tool_call_id, status)
}

pub(crate) fn tool_output_started_event(
    call_item_id: String,
    tool_call_id: Option<&str>,
) -> ThreadEvent {
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
    shared_tool_output_completed_event(
        call_item_id,
        tool_call_id,
        status,
        exit_code,
        spool_path,
        output,
    )
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
    ThreadEvent::TurnCompleted(TurnCompletedEvent { usage })
}

pub(crate) fn turn_failed_event(message: impl Into<String>, usage: Option<Usage>) -> ThreadEvent {
    ThreadEvent::TurnFailed(TurnFailedEvent {
        message: message.into(),
        usage,
    })
}

#[allow(clippy::too_many_arguments)]
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
    ThreadEvent::ThreadCompleted(ThreadCompletedEvent {
        thread_id: thread_id.into(),
        session_id: session_id.into(),
        subtype,
        outcome_code: outcome_code.into(),
        result,
        stop_reason,
        usage,
        total_cost_usd,
        num_turns,
    })
}

pub(crate) fn compact_boundary_event(
    thread_id: impl Into<String>,
    trigger: CompactionTrigger,
    mode: CompactionMode,
    original_message_count: usize,
    compacted_message_count: usize,
    history_artifact_path: Option<String>,
) -> ThreadEvent {
    ThreadEvent::ThreadCompactBoundary(ThreadCompactBoundaryEvent {
        thread_id: thread_id.into(),
        trigger,
        mode,
        original_message_count,
        compacted_message_count,
        history_artifact_path,
    })
}

pub(crate) fn harness_event(
    event: HarnessEventKind,
    message: Option<String>,
    path: Option<String>,
) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            id: format!("harness-{}", Utc::now().timestamp_millis()),
            details: ThreadItemDetails::Harness(HarnessEventItem {
                event,
                message,
                command: None,
                path,
                exit_code: None,
            }),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;
    use vtcode_core::exec::events::ItemStartedEvent;

    #[test]
    fn resolve_event_log_path_appends_jsonl_when_directory() {
        let tmp = TempDir::new().expect("temp dir");
        let run_id = TurnRunId("run-123".to_string());
        let resolved = resolve_event_log_path(tmp.path().to_str().expect("path"), &run_id);

        let file_name = resolved
            .file_name()
            .and_then(|name| name.to_str())
            .expect("file name");
        assert!(file_name.starts_with("harness-run-123-"));
        assert!(file_name.ends_with(".jsonl"));
    }

    #[test]
    fn emit_writes_versioned_event() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("events.jsonl");
        let emitter = HarnessEventEmitter::new(path.clone()).expect("emitter");

        // Use the path method to verify it works
        assert_eq!(emitter.path(), path.as_path());

        emitter.emit(turn_started_event()).expect("emit");

        let payload = std::fs::read_to_string(&path).expect("read log");
        let line = payload.lines().next().expect("line");
        let value: serde_json::Value = serde_json::from_str(line).expect("json");

        assert_eq!(
            value.get("schema_version").and_then(|v| v.as_str()),
            Some(vtcode_core::exec::events::EVENT_SCHEMA_VERSION)
        );
        assert_eq!(
            value
                .get("event")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("turn.started")
        );
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
            .enable_open_responses(config, "claude-haiku-4-5", Some(or_path.clone()))
            .expect("enable");

        // Emit events
        emitter
            .emit(ThreadEvent::ThreadStarted(ThreadStartedEvent {
                thread_id: "test-thread".to_string(),
            }))
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
        let harness_content = std::fs::read_to_string(&harness_path).expect("read harness");
        assert!(harness_content.contains("thread.started"));
        assert!(harness_content.contains("turn.started"));

        // Verify Open Responses log
        let or_content = std::fs::read_to_string(&or_path).expect("read OR");
        assert!(or_content.contains("response.created"));
        assert!(or_content.contains("response.completed"));
        assert!(or_content.contains("[DONE]"));
    }

    #[test]
    fn turn_completed_event_preserves_usage_payload() {
        let event = turn_completed_event(Usage {
            input_tokens: 42,
            cached_input_tokens: 7,
            cache_creation_tokens: 0,
            output_tokens: 9,
        });

        let ThreadEvent::TurnCompleted(TurnCompletedEvent { usage }) = event else {
            panic!("expected turn.completed");
        };

        assert_eq!(usage.input_tokens, 42);
        assert_eq!(usage.cached_input_tokens, 7);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.output_tokens, 9);
    }

    #[test]
    fn tool_started_event_captures_arguments() {
        let args = json!({ "path": "README.md" });
        let event = tool_started_event(
            "tool-1".to_string(),
            "read_file",
            Some(&args),
            Some("tool_call_0"),
        );

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

        let ThreadEvent::ItemUpdated(vtcode_core::exec::events::ItemUpdatedEvent { item }) = event
        else {
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
