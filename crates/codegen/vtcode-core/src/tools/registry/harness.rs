//! Harness context for tool execution tracking.
//!
//! This module provides the context wrapper for tracking sessions and tasks
//! during tool execution.

use arc_swap::{ArcSwap, ArcSwapOption};
use std::sync::Arc;
use std::time::SystemTime;

use super::execution_history::HarnessContextSnapshot;

/// Thread-safe context for harness execution.
///
/// Tracks session and task IDs across tool invocations.
#[derive(Debug, Clone)]
pub struct HarnessContext {
    session_id: Arc<ArcSwap<String>>,
    task_id: Arc<ArcSwapOption<String>>,
}

impl Default for HarnessContext {
    fn default() -> Self {
        let session_id = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| format!("session-{}", d.as_millis()))
            .unwrap_or_else(|_| "session-unknown".to_string());

        Self {
            session_id: Arc::new(ArcSwap::from_pointee(session_id)),
            task_id: Arc::new(ArcSwapOption::empty()),
        }
    }
}

impl HarnessContext {
    /// Create a new harness context with a specific session ID.
    pub fn with_session(session_id: impl Into<String>) -> Self {
        Self {
            session_id: Arc::new(ArcSwap::from_pointee(session_id.into())),
            task_id: Arc::new(ArcSwapOption::empty()),
        }
    }

    /// Set the session ID.
    pub fn set_session_id(&self, session_id: impl Into<String>) {
        self.session_id.store(Arc::new(session_id.into()));
    }

    /// Set the task ID.
    pub fn set_task_id(&self, task_id: Option<String>) {
        self.task_id.store(task_id.map(Arc::new));
    }

    /// Get the current session ID.
    pub fn session_id(&self) -> String {
        self.session_id.load().as_ref().clone()
    }

    /// Get the current task ID.
    pub fn task_id(&self) -> Option<String> {
        self.task_id.load_full().map(|task_id| (*task_id).clone())
    }

    /// Create a snapshot of the current context.
    pub fn snapshot(&self) -> HarnessContextSnapshot {
        HarnessContextSnapshot::new(self.session_id(), self.task_id())
    }
}
