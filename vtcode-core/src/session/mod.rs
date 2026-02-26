use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::llm::provider::Message;
use crate::utils::file_utils::{read_json_file_sync, write_json_file_sync};

/// Session identifier for conversation persistence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionId(pub String);

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

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
        write_json_file_sync(path, self)
    }

    /// Load session from disk.
    pub fn load(path: &Path) -> Result<Self> {
        read_json_file_sync(path)
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
