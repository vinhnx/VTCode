use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use vtcode_core::exec::events::{
    CommandExecutionItem, CommandExecutionStatus, ItemCompletedEvent, ItemStartedEvent,
    ThreadEvent, ThreadItem, ThreadItemDetails, TurnCompletedEvent, TurnFailedEvent,
    TurnStartedEvent, Usage, VersionedThreadEvent,
};

use crate::agent::runloop::unified::run_loop_context::TurnRunId;

pub struct HarnessEventEmitter {
    #[allow(dead_code)]
    path: PathBuf,
    writer: Mutex<BufWriter<File>>,
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
        })
    }

    pub fn emit(&self, event: ThreadEvent) -> Result<()> {
        let payload = VersionedThreadEvent::new(event);
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
        Ok(())
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
}
