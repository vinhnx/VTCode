use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::llm::provider::Message;

/// Session identifier for conversation persistence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

/// Session state that can be persisted and resumed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub id: SessionId,
    pub created_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub history: Vec<Message>,
    pub active_skills: Vec<String>,
    pub working_dir: PathBuf,
}

impl SessionState {
    pub fn new(
        id: SessionId,
        created_at: DateTime<Utc>,
        history: Vec<Message>,
        active_skills: Vec<String>,
        working_dir: PathBuf,
    ) -> Self {
        Self {
            id,
            created_at,
            last_updated: created_at,
            history,
            active_skills,
            working_dir,
        }
    }

    /// Save session to disk.
    pub fn save(&self, path: &Path) -> Result<()> {
        let json =
            serde_json::to_string_pretty(self).context("Failed to serialize session state")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create session directory {}", parent.display())
            })?;
        }
        std::fs::write(path, json)
            .with_context(|| format!("Failed to write session state to {}", path.display()))?;
        Ok(())
    }

    /// Load session from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read session state from {}", path.display()))?;
        serde_json::from_str(&json).context("Failed to deserialize session state")
    }
}

/// Resolve the default session persistence path for a workspace.
pub fn session_path(workspace_root: &Path, id: &SessionId) -> PathBuf {
    workspace_root
        .join(".vtcode")
        .join("sessions")
        .join(format!("{}.json", id.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn session_state_round_trip() {
        let tmp = TempDir::new().expect("temp dir");
        let id = SessionId::from_string("session-1");
        let created_at = Utc::now();
        let history = vec![Message::user("hello".to_string())];
        let state = SessionState::new(
            id.clone(),
            created_at,
            history.clone(),
            vec!["skill-a".to_string()],
            tmp.path().to_path_buf(),
        );

        let path = session_path(tmp.path(), &id);
        state.save(&path).expect("save");
        let loaded = SessionState::load(&path).expect("load");

        assert_eq!(loaded.id, id);
        assert_eq!(loaded.history, history);
    }
}
