use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Result, anyhow};

use crate::config::PtyConfig;

use super::PtyManager;

/// RAII guard to automatically decrement session count when dropped
#[derive(Debug)]
pub struct PtySessionGuard {
    active_sessions: Arc<AtomicUsize>,
}

impl Drop for PtySessionGuard {
    fn drop(&mut self) {
        let current = self.active_sessions.load(Ordering::SeqCst);
        if current > 0 {
            self.active_sessions.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

#[derive(Clone)]
pub struct PtySessionManager {
    config: PtyConfig,
    manager: PtyManager,
    active_sessions: Arc<AtomicUsize>,
}

impl PtySessionManager {
    pub fn new(workspace_root: PathBuf, config: PtyConfig) -> Self {
        let manager = PtyManager::new(workspace_root, config.clone());

        Self {
            config,
            manager,
            active_sessions: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn config(&self) -> &PtyConfig {
        &self.config
    }

    pub fn manager(&self) -> &PtyManager {
        &self.manager
    }

    pub fn can_start_session(&self) -> bool {
        if !self.config.enabled {
            return false;
        }

        self.active_sessions.load(Ordering::SeqCst) < self.config.max_sessions
    }

    /// Start a PTY session and return an RAII guard that will automatically decrement
    /// the session count when dropped, even if an error occurs during execution.
    pub fn start_session(&self) -> Result<PtySessionGuard> {
        if !self.can_start_session() {
            return Err(anyhow!(
                "Maximum PTY sessions ({}) exceeded. Current active sessions: {}",
                self.config.max_sessions,
                self.active_sessions.load(Ordering::SeqCst)
            ));
        }

        self.active_sessions.fetch_add(1, Ordering::SeqCst);
        Ok(PtySessionGuard {
            active_sessions: Arc::clone(&self.active_sessions),
        })
    }

    pub fn end_session(&self) {
        let current = self.active_sessions.load(Ordering::SeqCst);
        if current > 0 {
            self.active_sessions.fetch_sub(1, Ordering::SeqCst);
        }
    }

    pub fn active_sessions(&self) -> usize {
        self.active_sessions.load(Ordering::SeqCst)
    }

    pub fn terminate_all(&self) {
        self.manager.terminate_all_sessions();
        self.active_sessions.store(0, Ordering::SeqCst);
    }
}
