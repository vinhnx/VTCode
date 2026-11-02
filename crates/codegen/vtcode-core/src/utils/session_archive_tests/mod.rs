pub(super) use super::*;
pub(super) use crate::config::constants::tools as tool_names;
pub(super) use crate::config::{HistoryPersistence, VTCodeConfig};
pub(super) use crate::core::agent::snapshots::SnapshotTurnDiagnostics;
pub(super) use crate::llm::provider::{
    AssistantPhase, ContentPart, Message, MessageClearAt, MessageContent, MessageRole, ToolCall,
};
pub(super) use crate::utils::error_log_collector::ErrorLogEntry;
pub(super) use anyhow::anyhow;
pub(super) use anyhow::{Context, Result};
pub(super) use chrono::{TimeZone, Timelike, Utc};
pub(super) use std::fs;
pub(super) use std::mem::size_of;
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::process;
pub(super) use std::sync::LazyLock;

use std::time::Duration;

static SESSION_HISTORY_TEST_LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

struct EnvGuard {
    key: &'static str,
}

impl EnvGuard {
    fn set(key: &'static str, value: &Path) -> Self {
        set_test_env_override_path(key, value);
        Self { key }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        clear_test_env_override(self.key);
    }
}

struct HistorySettingsGuard {
    previous: SessionHistorySettings,
}

impl HistorySettingsGuard {
    fn set(persistence: HistoryPersistence, max_bytes: Option<usize>) -> Self {
        let previous = session_history_settings();
        let mut config = VTCodeConfig::default();
        config.history.persistence = persistence;
        config.history.max_bytes = max_bytes;
        apply_session_history_config_from_vtcode(&config);
        Self { previous }
    }
}

impl Drop for HistorySettingsGuard {
    fn drop(&mut self) {
        let mut config = VTCodeConfig::default();
        config.history.persistence = self.previous.persistence;
        config.history.max_bytes = self.previous.max_bytes;
        apply_session_history_config_from_vtcode(&config);
    }
}

async fn lock_history_test_guard() -> tokio::sync::MutexGuard<'static, ()> {
    SESSION_HISTORY_TEST_LOCK.lock().await
}

mod async_progress;
mod identifiers_paths;
mod listing_search;
mod message_roundtrip;
mod progress_transcript;
mod snapshot_persist;
