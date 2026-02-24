//! Harness context for tool execution tracking.
//!
//! This module provides the context wrapper for tracking sessions and tasks
//! during tool execution.

use std::sync::Arc;
use std::time::SystemTime;

use super::execution_history::HarnessContextSnapshot;

/// Thread-safe context for harness execution.
///
/// Tracks session and task IDs across tool invocations.
#[derive(Debug, Clone)]
pub struct HarnessContext {
    session_id: Arc<std::sync::RwLock<String>>,
    task_id: Arc<std::sync::RwLock<Option<String>>>,
}

impl Default for HarnessContext {
    fn default() -> Self {
        let session_id = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| format!("session-{}", d.as_millis()))
            .unwrap_or_else(|_| "session-unknown".to_string());

        Self {
            session_id: Arc::new(std::sync::RwLock::new(session_id)),
            task_id: Arc::new(std::sync::RwLock::new(None)),
        }
    }
}

impl HarnessContext {
    /// Create a new harness context with a specific session ID.
    pub fn with_session(session_id: impl Into<String>) -> Self {
        Self {
            session_id: Arc::new(std::sync::RwLock::new(session_id.into())),
            task_id: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Set the session ID.
    pub fn set_session_id(&self, session_id: impl Into<String>) {
        if let Ok(mut guard) = self.session_id.write() {
            *guard = session_id.into();
        }
    }

    /// Set the task ID.
    pub fn set_task_id(&self, task_id: Option<String>) {
        if let Ok(mut guard) = self.task_id.write() {
            *guard = task_id;
        }
    }

    /// Get the current session ID.
    pub fn session_id(&self) -> String {
        self.session_id
            .read()
            .ok()
            .map(|g| g.clone())
            .unwrap_or_else(|| "session-unknown".to_string())
    }

    /// Get the current task ID.
    pub fn task_id(&self) -> Option<String> {
        self.task_id.read().ok().and_then(|g| g.clone())
    }

    /// Create a snapshot of the current context.
    pub fn snapshot(&self) -> HarnessContextSnapshot {
        HarnessContextSnapshot::new(self.session_id(), self.task_id())
    }
}
