use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use vtcode_config::OpenResponsesConfig;
use vtcode_core::exec::events::{
    CommandExecutionItem, CommandExecutionStatus, ItemCompletedEvent, ItemStartedEvent,
    ThreadEvent, ThreadItem, ThreadItemDetails, TurnCompletedEvent, TurnFailedEvent,
    TurnStartedEvent, Usage, VersionedThreadEvent,
};
use vtcode_core::open_responses::{OpenResponsesIntegration, SequencedEvent};

use crate::agent::runloop::unified::run_loop_context::TurnRunId;

pub struct HarnessEventEmitter {
    #[allow(dead_code)]
    path: PathBuf,
    writer: Mutex<BufWriter<File>>,
    open_responses: Mutex<Option<OpenResponsesState>>,
}

/// State for Open Responses event emission.
struct OpenResponsesState {
    integration: OpenResponsesIntegration,
    writer: Option<BufWriter<File>>,
    sequence_counter: u64,
}

impl HarnessEventEmitter {
    pub fn new(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create harness log dir {}", parent.display())
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to open harness log {}", path.display()))?;
        Ok(Self {
            path,
            writer: Mutex::new(BufWriter::new(file)),
            open_responses: Mutex::new(None),
        })
    }

    /// Enables Open Responses event emission with the given configuration.
    ///
    /// When enabled, events are also written in Open Responses format to a separate file.
    pub fn enable_open_responses(
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
                std::fs::create_dir_all(parent)?;
            }
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;
            Some(BufWriter::new(file))
        } else {
            None
        };

        let mut integration = OpenResponsesIntegration::new(config);
        integration.start_response(model);

        let mut guard = self
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

    pub fn emit(&self, event: ThreadEvent) -> Result<()> {
        // Write to harness log (internal format)
        let payload = VersionedThreadEvent::new(event.clone());
        {
            let mut writer = self
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
        if let Ok(mut guard) = self.open_responses.lock() {
            if let Some(ref mut state) = *guard {
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
        }

        Ok(())
    }

    /// Finishes the Open Responses session and writes the terminal marker.
    pub fn finish_open_responses(&self) {
        if let Ok(mut guard) = self.open_responses.lock() {
            if let Some(ref mut state) = *guard {
                let _ = state.integration.finish_response();
                if let Some(ref mut writer) = state.writer {
                    let _ = writeln!(writer, "data: [DONE]");
                    let _ = writer.flush();
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn resolve_event_log_path(path: &str, run_id: &TurnRunId) -> PathBuf {
    let mut base = PathBuf::from(path);
    if base.extension().is_none() {
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
        base = base.join(format!("harness-{}-{}.jsonl", run_id.0, timestamp));
    }
    base
}

pub fn tool_started_event(item_id: String, tool_name: &str) -> ThreadEvent {
    ThreadEvent::ItemStarted(ItemStartedEvent {
        item: ThreadItem {
            id: item_id,
            details: ThreadItemDetails::CommandExecution(CommandExecutionItem {
                command: tool_name.to_string(),
                aggregated_output: String::new(),
                exit_code: None,
                status: CommandExecutionStatus::InProgress,
            }),
        },
    })
}

pub fn tool_completed_event(
    item_id: String,
    tool_name: &str,
    status: CommandExecutionStatus,
    exit_code: Option<i32>,
) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            id: item_id,
            details: ThreadItemDetails::CommandExecution(CommandExecutionItem {
                command: tool_name.to_string(),
                aggregated_output: String::new(),
                exit_code,
                status,
            }),
        },
    })
}

pub fn turn_started_event() -> ThreadEvent {
    ThreadEvent::TurnStarted(TurnStartedEvent::default())
}

pub fn turn_completed_event() -> ThreadEvent {
    ThreadEvent::TurnCompleted(TurnCompletedEvent {
        usage: Usage::default(),
    })
}

pub fn turn_failed_event(message: impl Into<String>) -> ThreadEvent {
    ThreadEvent::TurnFailed(TurnFailedEvent {
        message: message.into(),
        usage: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
            .enable_open_responses(config, "claude-3-sonnet", Some(or_path.clone()))
            .expect("enable");

        // Emit events
        emitter
            .emit(ThreadEvent::ThreadStarted(ThreadStartedEvent {
                thread_id: "test-thread".to_string(),
            }))
            .expect("emit");
        emitter.emit(turn_started_event()).expect("emit turn");
        emitter.emit(turn_completed_event()).expect("emit completed");
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
}
