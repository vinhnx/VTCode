//! PTY-related ToolRegistry accessors.

use std::sync::Arc;

use anyhow::Result;

use crate::config::PtyConfig;
use crate::tools::pty::PtyManager;

use super::ToolRegistry;
use super::pty;

impl ToolRegistry {
    pub fn pty_manager(&self) -> &PtyManager {
        self.pty_sessions.manager()
    }

    pub fn pty_config(&self) -> &PtyConfig {
        self.pty_sessions.config()
    }

    pub fn can_start_pty_session(&self) -> bool {
        self.pty_sessions.can_start_session()
    }

    pub fn start_pty_session(&self) -> Result<pty::PtySessionGuard> {
        self.pty_sessions.start_session()
    }

    pub fn end_pty_session(&self) {
        self.pty_sessions.end_session();
    }

    pub fn active_pty_sessions(&self) -> usize {
        self.pty_sessions.active_sessions()
    }

    pub fn terminate_all_pty_sessions(&self) {
        self.pty_sessions.terminate_all();
    }

    /// Set the active PTY sessions counter for tracking
    pub fn set_active_pty_sessions(&self, counter: Arc<std::sync::atomic::AtomicUsize>) {
        if let Ok(mut guard) = self.active_pty_sessions.write() {
            *guard = Some(counter);
        }
    }

    /// Increment active PTY sessions count
    pub fn increment_active_pty_sessions(&self) {
        if let Some(counter) = self
            .active_pty_sessions
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(Arc::clone))
        {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Decrement active PTY sessions count
    pub fn decrement_active_pty_sessions(&self) {
        if let Some(counter) = self
            .active_pty_sessions
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(Arc::clone))
        {
            counter.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Get the current active PTY sessions count
    pub fn active_pty_sessions_count(&self) -> usize {
        self.active_pty_sessions
            .read()
            .ok()
            .and_then(|g| {
                g.as_ref()
                    .map(|c| c.load(std::sync::atomic::Ordering::Relaxed))
            })
            .unwrap_or(0)
    }
}
