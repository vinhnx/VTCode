use crate::utils::dot_config::DotManager;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const SESSION_FILE_PREFIX: &str = "session";
const SESSION_FILE_EXTENSION: &str = "json";
pub const SESSION_DIR_ENV: &str = "VT_SESSION_DIR";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionArchiveMetadata {
    pub workspace_label: String,
    pub workspace_path: String,
    pub model: String,
    pub provider: String,
    pub theme: String,
    pub reasoning_effort: String,
}

impl SessionArchiveMetadata {
    pub fn new(
        workspace_label: impl Into<String>,
        workspace_path: impl Into<String>,
        model: impl Into<String>,
        provider: impl Into<String>,
        theme: impl Into<String>,
        reasoning_effort: impl Into<String>,
    ) -> Self {
        Self {
            workspace_label: workspace_label.into(),
            workspace_path: workspace_path.into(),
            model: model.into(),
            provider: provider.into(),
            theme: theme.into(),
            reasoning_effort: reasoning_effort.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub metadata: SessionArchiveMetadata,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub total_messages: usize,
    pub distinct_tools: Vec<String>,
    pub transcript: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SessionListing {
    pub path: PathBuf,
    pub snapshot: SessionSnapshot,
}

#[derive(Debug, Clone)]
pub struct SessionArchive {
    path: PathBuf,
    metadata: SessionArchiveMetadata,
    started_at: DateTime<Utc>,
}

impl SessionArchive {
    pub fn new(metadata: SessionArchiveMetadata) -> Result<Self> {
        let sessions_dir = resolve_sessions_dir()?;
        let started_at = Utc::now();
        let file_name = format!(
            "{}-{}-{}.{}",
            SESSION_FILE_PREFIX,
            sanitize_component(&metadata.workspace_label),
            started_at.format("%Y%m%dT%H%M%SZ"),
            SESSION_FILE_EXTENSION
        );
        let path = sessions_dir.join(file_name);

        Ok(Self {
            path,
            metadata,
            started_at,
        })
    }

    pub fn finalize(
        &self,
        transcript: Vec<String>,
        total_messages: usize,
        distinct_tools: Vec<String>,
    ) -> Result<PathBuf> {
        let snapshot = SessionSnapshot {
            metadata: self.metadata.clone(),
            started_at: self.started_at,
            ended_at: Utc::now(),
            total_messages,
            distinct_tools,
            transcript,
        };

        let payload = serde_json::to_string_pretty(&snapshot)
            .context("failed to serialize session snapshot")?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create session directory: {}", parent.display())
            })?;
        }
        fs::write(&self.path, payload)
            .with_context(|| format!("failed to write session archive: {}", self.path.display()))?;

        Ok(self.path.clone())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn list_recent_sessions(limit: usize) -> Result<Vec<SessionListing>> {
    let sessions_dir = match resolve_sessions_dir() {
        Ok(dir) => dir,
        Err(_) => return Ok(Vec::new()),
    };

    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut listings = Vec::new();
    for entry in fs::read_dir(&sessions_dir).with_context(|| {
        format!(
            "failed to read session directory: {}",
            sessions_dir.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!("failed to read session entry in {}", sessions_dir.display())
        })?;
        let path = entry.path();
        if !is_session_file(&path) {
            continue;
        }

        let data = fs::read_to_string(&path)
            .with_context(|| format!("failed to read session file: {}", path.display()))?;
        let snapshot: SessionSnapshot = match serde_json::from_str(&data) {
            Ok(snapshot) => snapshot,
            Err(_) => continue,
        };
        listings.push(SessionListing { path, snapshot });
    }

    listings.sort_by(|a, b| b.snapshot.ended_at.cmp(&a.snapshot.ended_at));
    if limit > 0 && listings.len() > limit {
        listings.truncate(limit);
    }

    Ok(listings)
}

fn resolve_sessions_dir() -> Result<PathBuf> {
    if let Some(custom) = env::var_os(SESSION_DIR_ENV) {
        let path = PathBuf::from(custom);
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create custom session dir: {}", path.display()))?;
        return Ok(path);
    }

    let manager = DotManager::new().context("failed to load VTCode dot manager")?;
    manager
        .initialize()
        .context("failed to initialize VTCode dot directory structure")?;
    let dir = manager.sessions_dir();
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create session directory: {}", dir.display()))?;
    Ok(dir)
}

fn sanitize_component(value: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if matches!(ch, '-' | '_') {
            if !last_was_separator {
                normalized.push(ch);
                last_was_separator = true;
            }
        } else if !last_was_separator {
            normalized.push('-');
            last_was_separator = true;
        }
    }

    let trimmed = normalized.trim_matches(|c| c == '-' || c == '_');
    if trimmed.is_empty() {
        "workspace".to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_session_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case(SESSION_FILE_EXTENSION))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct EnvGuard {
        key: &'static str,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            unsafe {
                env::set_var(key, value);
            }
            Self { key }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn session_archive_persists_snapshot() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

        let metadata = SessionArchiveMetadata::new(
            "ExampleWorkspace",
            "/tmp/example",
            "model-x",
            "provider-y",
            "dark",
            "medium",
        );
        let archive = SessionArchive::new(metadata.clone())?;
        let transcript = vec!["line one".to_string(), "line two".to_string()];
        let path = archive.finalize(transcript.clone(), 4, vec!["tool_a".to_string()])?;

        let stored = fs::read_to_string(&path)
            .with_context(|| format!("failed to read stored session: {}", path.display()))?;
        let snapshot: SessionSnapshot =
            serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

        assert_eq!(snapshot.metadata, metadata);
        assert_eq!(snapshot.transcript, transcript);
        assert_eq!(snapshot.total_messages, 4);
        assert_eq!(snapshot.distinct_tools, vec!["tool_a".to_string()]);
        Ok(())
    }

    #[test]
    fn list_recent_sessions_orders_entries() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

        let first_metadata = SessionArchiveMetadata::new(
            "First",
            "/tmp/first",
            "model-a",
            "provider-a",
            "light",
            "medium",
        );
        let first_archive = SessionArchive::new(first_metadata.clone())?;
        first_archive.finalize(vec!["first".to_string()], 1, Vec::new())?;

        std::thread::sleep(Duration::from_millis(10));

        let second_metadata = SessionArchiveMetadata::new(
            "Second",
            "/tmp/second",
            "model-b",
            "provider-b",
            "dark",
            "high",
        );
        let second_archive = SessionArchive::new(second_metadata.clone())?;
        second_archive.finalize(vec!["second".to_string()], 2, vec!["tool_b".to_string()])?;

        let listings = list_recent_sessions(10)?;
        assert_eq!(listings.len(), 2);
        assert_eq!(listings[0].snapshot.metadata, second_metadata);
        assert_eq!(listings[1].snapshot.metadata, first_metadata);
        Ok(())
    }
}
