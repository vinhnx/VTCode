//! PTY-related ToolRegistry accessors.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;

use anyhow::Result;

use crate::config::PtyConfig;
use crate::tools::exec_session::ExecSessionManager;
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

    /// Inline-delegating wrapper over [`pty::PtySessionManager::terminate_all_async`].
    /// Returns the inner future directly to avoid an extra coroutine state machine
    /// (per the async-state-machine bloat audit, section 16).
    pub fn terminate_all_pty_sessions_async(&self) -> impl Future<Output = Result<()>> + '_ {
        self.pty_sessions.terminate_all_async()
    }

    /// Terminates every exec session, then retires verification bookkeeping
    /// for the sessions that are actually gone (see
    /// `Self::terminate_exec_sessions_and_observe_cleanup`).
    pub async fn terminate_all_exec_sessions_async(&self) -> Result<()> {
        self.terminate_exec_sessions_and_observe_cleanup(false, false).await
    }

    /// Exit-path variant: terminates every exec session with `Immediate` PTY
    /// semantics (group SIGKILL, no SIGTERM grace window) so user-initiated
    /// exit is not parked ~600 ms per live PTY session.
    pub async fn terminate_all_exec_sessions_for_exit_async(&self) -> Result<()> {
        self.terminate_exec_sessions_and_observe_cleanup(false, true).await
    }

    /// Terminates only foreground exec sessions after a turn interruption,
    /// preserving user-owned background sessions, then retires verification
    /// bookkeeping for the sessions that are actually gone.
    pub async fn terminate_active_exec_sessions_async(&self) -> Result<()> {
        self.terminate_exec_sessions_and_observe_cleanup(true, false).await
    }

    /// Exit-path variant of [`Self::terminate_active_exec_sessions_async`]
    /// using `Immediate` PTY termination.
    pub async fn terminate_active_exec_sessions_for_exit_async(&self) -> Result<()> {
        self.terminate_exec_sessions_and_observe_cleanup(true, true).await
    }

    /// Shared bulk-termination path: snapshots the in-scope session ids,
    /// terminates, then retires verification bookkeeping for the sessions
    /// that are actually gone. Only gone ids are observed so a live mutator
    /// (e.g. when termination partially fails) keeps its pending boundary.
    /// Verification debt (`generation`) is preserved; only the
    /// terminated-session block is released so fresh checks can satisfy it.
    async fn terminate_exec_sessions_and_observe_cleanup(&self, active_only: bool, immediate: bool) -> Result<()> {
        let before = self.scoped_exec_session_ids(active_only).await;
        let result = if immediate {
            if active_only {
                self.exec_sessions.terminate_active_sessions_for_exit_async().await
            } else {
                self.exec_sessions.terminate_all_sessions_for_exit_async().await
            }
        } else if active_only {
            self.exec_sessions.terminate_active_sessions_async().await
        } else {
            self.exec_sessions.terminate_all_sessions_async().await
        };
        self.observe_terminated_exec_sessions(before).await;
        result
    }

    pub fn exec_session_manager(&self) -> ExecSessionManager {
        self.exec_sessions.clone()
    }

    /// Force-stop every foreground exec session and close it (TUI
    /// ForceCancel escape hatch), then retire verification bookkeeping for
    /// the sessions that are actually gone so a force-cancelled mutator
    /// cannot strand `pending_mutation_sessions` forever. Returns
    /// `(stopped, closed, failed)` counts from the session manager.
    /// Background sessions are user-owned and are left alone.
    pub async fn force_cancel_foreground_sessions_and_observe_cleanup(&self) -> (usize, usize, usize) {
        let before = self.scoped_exec_session_ids(true).await;
        let counts = self.exec_sessions.force_cancel_foreground_sessions().await;
        self.observe_terminated_exec_sessions(before).await;
        counts
    }

    /// Snapshot the ids of in-scope exec sessions (foreground only when
    /// `active_only`) for post-cleanup reconciliation.
    async fn scoped_exec_session_ids(&self, active_only: bool) -> Vec<String> {
        self.exec_sessions
            .list_sessions()
            .await
            .into_iter()
            .filter(|session| !active_only || !session.background)
            .map(|session| session.id.as_str().to_owned())
            .collect()
    }

    /// Retire verification bookkeeping for snapshotted ids that are gone
    /// now. Only gone ids are observed so a live mutator (e.g. when
    /// termination partially fails) keeps its pending boundary. Verification
    /// debt is preserved; only the terminated-session block is released.
    async fn observe_terminated_exec_sessions(&self, before: Vec<String>) {
        if before.is_empty() {
            return;
        }
        let remaining: HashSet<String> = self
            .exec_sessions
            .list_sessions()
            .await
            .into_iter()
            .map(|session| session.id.as_str().to_owned())
            .collect();
        let terminated: Vec<String> = before.into_iter().filter(|id| !remaining.contains(id)).collect();
        if !terminated.is_empty() {
            self.observe_completion_exec_sessions_terminated(&terminated);
        }
    }

    /// Set the active PTY sessions counter for tracking
    pub fn set_active_pty_sessions(&self, counter: Arc<std::sync::atomic::AtomicUsize>) {
        self.exec_sessions.set_foreground_pty_counter(Arc::clone(&counter));
        if let Ok(mut guard) = self.active_pty_sessions.write() {
            *guard = Some(counter);
        }
    }

    /// Increment active PTY sessions count
    pub fn increment_active_pty_sessions(&self) {
        if let Some(counter) = self
            .active_pty_sessions
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(Arc::clone)
        {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Decrement active PTY sessions count
    pub fn decrement_active_pty_sessions(&self) {
        if let Some(counter) = self
            .active_pty_sessions
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(Arc::clone)
        {
            counter.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Get the current active PTY sessions count
    pub fn active_pty_sessions_count(&self) -> usize {
        self.active_pty_sessions
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|c| c.load(std::sync::atomic::Ordering::Relaxed)))
            .unwrap_or(0)
    }
}
