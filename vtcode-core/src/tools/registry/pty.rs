use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Result, anyhow};

use crate::config::PtyConfig;

use super::PtyManager;

#[derive(Clone)]
pub(super) struct PtySessionManager {
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

    pub fn start_session(&self) -> Result<()> {
        if !self.can_start_session() {
            return Err(anyhow!(
                "Maximum PTY sessions ({}) exceeded. Current active sessions: {}",
                self.config.max_sessions,
                self.active_sessions.load(Ordering::SeqCst)
            ));
        }

        self.active_sessions.fetch_add(1, Ordering::SeqCst);
        Ok(())
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
}
