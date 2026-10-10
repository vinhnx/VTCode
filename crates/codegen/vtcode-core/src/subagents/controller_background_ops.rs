#![allow(
    unused_imports,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use futures::future::select_all;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Notify, RwLock};

use crate::config::VTCodeConfig;
use crate::config::types::ReasoningEffortLevel;
use crate::core::agent::runner::{AgentRunner, RunnerSettings};
use crate::core::agent::task::Task;
use crate::core::threads::{ThreadBootstrap, ThreadId, ThreadRuntimeHandle, ThreadSnapshot};
use crate::hooks::{LifecycleHookEngine, SessionStartTrigger};
use crate::llm::provider::Message;
use crate::tools::exec_session::{ExecSessionCompletionEvent, ExecSessionManager};
use crate::tools::pty::{PtyManager, PtySize};
use crate::utils::session_archive::{SessionArchive, find_session_by_identifier};
use vtcode_config::SubagentSpec;
use vtcode_config::auth::OpenAIChatGptAuthHandle;

use self::background::*;
use self::config::*;
use self::constants::*;
use self::discovery::discover_controller_subagents;
use self::model::*;
use vtcode_config::subagents::SUBAGENT_HARD_CONCURRENCY_LIMIT;

#[allow(
    unused_imports,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::*;

const BACKGROUND_COMPLETION_IDENTITY_CAPACITY: usize = 256;

impl SubagentController {
    fn clone_for_background_completion_monitor(&self) -> Self {
        Self {
            admission: Arc::clone(&self.admission),
            matrix: Arc::clone(&self.matrix),
            config: Arc::clone(&self.config),
            parent_session_id: Arc::clone(&self.parent_session_id),
            lifecycle_hooks: self.lifecycle_hooks.clone(),
            state: Arc::clone(&self.state),
            shutdown_requested: Arc::clone(&self.shutdown_requested),
            closing: Arc::clone(&self.closing),
            background_completion_channel: Arc::clone(&self.background_completion_channel),
            background_completion_notify: Arc::clone(&self.background_completion_notify),
            background_completion_shutdown: self.background_completion_shutdown.clone(),
            background_completion_monitor: Arc::clone(&self.background_completion_monitor),
            background_completion_owners: Arc::clone(&self.background_completion_owners),
            background_completion_monitor_owner: false,
        }
    }

    pub(super) async fn start_background_completion_monitor(&self) {
        let mut completion_rx = self.config.exec_sessions.subscribe_completion();
        let controller = self.clone_for_background_completion_monitor();
        let shutdown = self.background_completion_shutdown.clone();
        let monitor = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => break,
                    result = completion_rx.recv() => match result {
                        Ok(event) => {
                            if let Err(error) = controller.handle_exec_session_completion(event).await {
                                tracing::warn!(error = %error, "Background completion handling failed");
                            }
                        }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(skipped, "Background completion monitor lagged; reconciling records");
                            if let Err(error) = controller.refresh_background_processes().await {
                                tracing::warn!(error = %error, "Background completion reconciliation failed");
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    },
                }
            }
        });
        let mut monitor_slot = self.background_completion_monitor.lock().await;
        if let Some(previous) = monitor_slot.replace(monitor) {
            previous.abort();
            let _ = previous.await;
        }
    }

    pub(super) async fn stop_background_completion_monitor(&self) {
        self.background_completion_shutdown.cancel();
        let monitor = self.background_completion_monitor.lock().await.take();
        if let Some(monitor) = monitor {
            monitor.abort();
            let _ = monitor.await;
        }
    }

    async fn handle_exec_session_completion(&self, event: ExecSessionCompletionEvent) -> Result<()> {
        if self.shutdown_requested.load(Ordering::Relaxed) {
            return Ok(());
        }
        if !event.managed_background {
            return Ok(());
        }

        let record_id = {
            let state = self.state.read().await;
            let Some(record_id) = state
                .background_children
                .values()
                .find(|record| record.exec_session_id == event.session_id.as_str())
                .map(|record| record.id.clone())
            else {
                // The session may be a user-managed background command or a
                // stale completion from a previous controller incarnation.
                return Ok(());
            };
            record_id
        };

        let Some(snapshot) = self.config.exec_sessions.snapshot_session(event.session_id.as_str()).await.ok() else {
            return Ok(());
        };
        let respawn = self.update_background_record_state(&record_id, Some(snapshot)).await?;
        if let Some((agent_name, stable_id, restart_attempts)) = respawn {
            self.ensure_background_record_running(
                agent_name.as_str(),
                Some(stable_id.as_str()),
                restart_attempts,
                None,
            )
            .await?;
        }
        self.refresh_background_archive_metadata(&record_id).await?;
        self.save_background_state().await?;

        self.publish_terminal_background_completion(&record_id, event.session_id.as_str(), Some(event.exit_code))
            .await
    }

    async fn publish_terminal_background_completion(
        &self,
        record_id: &str,
        expected_exec_session_id: &str,
        exit_code: Option<i32>,
    ) -> Result<()> {
        if self.shutdown_requested.load(Ordering::Relaxed) {
            return Ok(());
        }

        let completion = {
            let mut state = self.state.write().await;
            let (task_id, status, summary, error, session_id, exec_session_id, archive_path, transcript_path) = {
                let Some(record) = state.background_children.get(record_id) else {
                    return Ok(());
                };
                if record.exec_session_id != expected_exec_session_id
                    || !matches!(record.status, BackgroundSubprocessStatus::Stopped | BackgroundSubprocessStatus::Error)
                {
                    return Ok(());
                }
                (
                    record.id.clone(),
                    record.status,
                    record.summary.clone(),
                    record.error.clone(),
                    record.session_id.clone(),
                    record.exec_session_id.clone(),
                    record.archive_path.clone(),
                    record.transcript_path.clone(),
                )
            };

            let identity = format!("{task_id}:{expected_exec_session_id}");
            if state.background_completion_identities.iter().any(|seen| seen == &identity) {
                return Ok(());
            }
            if state.background_completion_identities.len() >= BACKGROUND_COMPLETION_IDENTITY_CAPACITY {
                state.background_completion_identities.pop_front();
            }
            state.background_completion_identities.push_back(identity);

            BackgroundCompletionEvent {
                task_id,
                status,
                summary,
                error,
                session_id,
                exec_session_id,
                archive_path,
                transcript_path,
                termination_requested: state
                    .background_children
                    .get(record_id)
                    .is_some_and(|record| record.termination_requested),
                exit_code,
            }
        };

        self.background_completion_channel.lock().publish(completion);
        self.background_completion_notify.notify_one();
        Ok(())
    }

    /// Returns status entries for all tracked background subprocesses.
    pub async fn background_status_entries(&self) -> Vec<BackgroundSubprocessEntry> {
        let state = self.state.read().await;
        state
            .background_children
            .values()
            .map(BackgroundRecord::build_status_entry)
            .collect()
    }

    /// Returns a snapshot of a background subprocess including its preview output.
    pub async fn background_snapshot(&self, target: &str) -> Result<BackgroundSubprocessSnapshot> {
        let _ = self.refresh_background_processes().await?;

        let entry = {
            let state = self.state.read().await;
            state
                .background_children
                .get(target)
                .ok_or_else(|| anyhow!("Unknown background subprocess {target}"))?
                .build_status_entry()
        };

        let preview = if entry.exec_session_id.is_empty() {
            String::new()
        } else {
            match self
                .config
                .exec_sessions
                .read_session_output(&entry.exec_session_id, false)
                .await
            {
                Ok(Some(output)) => extract_tail_lines(&output, SUBAGENT_PREVIEW_LINES),
                Ok(None) | Err(_) => {
                    if let Some(path) = entry.transcript_path.as_ref().or(entry.archive_path.as_ref()) {
                        load_archive_preview(path).await.unwrap_or_default()
                    } else {
                        String::new()
                    }
                }
            }
        };

        Ok(BackgroundSubprocessSnapshot { entry, preview })
    }

    /// Returns whether background subagents are enabled in the configuration.
    #[must_use]
    pub fn background_subagents_enabled(&self) -> bool {
        self.config.vt_cfg.subagents.background.enabled
    }

    /// Returns the configured default background agent name, if any.
    #[must_use]
    pub fn configured_default_background_agent(&self) -> Option<&str> {
        self.config
            .vt_cfg
            .subagents
            .background
            .default_agent
            .as_deref()
            .map(str::trim)
            .filter(|agent| !agent.is_empty())
    }

    /// Toggles the default background subagent between running and stopped.
    pub async fn toggle_default_background_subagent(&self) -> Result<BackgroundSubprocessEntry> {
        if !self.background_subagents_enabled() {
            bail!("Background subagents are disabled by configuration");
        }

        let agent_name = self
            .configured_default_background_agent()
            .ok_or_else(|| anyhow!("No default background subagent is configured"))?
            .to_string();
        let target_id = background_record_id(agent_name.as_str());
        let should_stop = {
            let state = self.state.read().await;
            state
                .background_children
                .get(&target_id)
                .is_some_and(|record| record.desired_enabled && record.status.is_active())
        };

        if should_stop {
            self.graceful_stop_background(&target_id).await
        } else {
            self.ensure_background_record_running(agent_name.as_str(), Some(target_id.as_str()), 0, None)
                .await
        }
    }

    /// Restarts background subagents that were previously enabled but are no longer running.
    pub async fn restore_background_subagents(&self) -> Result<Vec<BackgroundSubprocessEntry>> {
        let desired_records = {
            let state = self.state.read().await;
            state
                .background_children
                .values()
                .filter(|record| record.desired_enabled)
                .map(|record| {
                    (
                        record.id.clone(),
                        record.agent_name.clone(),
                        record.exec_session_id.clone(),
                        record.restart_attempts,
                    )
                })
                .collect::<Vec<_>>()
        };

        for (record_id, agent_name, exec_session_id, restart_attempts) in desired_records {
            let is_live = !exec_session_id.is_empty()
                && self
                    .config
                    .exec_sessions
                    .snapshot_session(&exec_session_id)
                    .await
                    .ok()
                    .is_some_and(|snapshot| exec_session_is_running(&snapshot));

            if is_live || !self.config.vt_cfg.subagents.background.auto_restore {
                continue;
            }
            tracing::info!(
                agent_name = agent_name.as_str(),
                record_id = record_id.as_str(),
                "Restoring background subagent subprocess"
            );
            self.ensure_background_record_running(
                agent_name.as_str(),
                Some(record_id.as_str()),
                restart_attempts,
                None,
            )
            .await?;
        }

        self.refresh_background_processes().await
    }

    /// Refreshes the state of all background subprocesses and respawns as needed.
    pub async fn refresh_background_processes(&self) -> Result<Vec<BackgroundSubprocessEntry>> {
        let record_ids = {
            let state = self.state.read().await;
            state.background_children.keys().cloned().collect::<Vec<_>>()
        };

        let mut changed = false;
        for record_id in record_ids {
            let (snapshot_target, before_status, before_error, before_summary, before_desired_enabled) = {
                let state = self.state.read().await;
                let record = state.background_children.get(&record_id);
                (
                    record.map(|r| r.exec_session_id.clone()),
                    record.map(|r| r.status),
                    record.and_then(|r| r.error.clone()),
                    record.and_then(|r| r.summary.clone()),
                    record.is_some_and(|r| r.desired_enabled),
                )
            };

            let snapshot = if let Some(exec_session_id) = snapshot_target.as_ref()
                && !exec_session_id.is_empty()
            {
                self.config.exec_sessions.snapshot_session(exec_session_id).await.ok()
            } else {
                None
            };

            let respawn = self.update_background_record_state(&record_id, snapshot).await?;

            if let Some((agent_name, stable_id, restart_attempts)) = respawn {
                self.ensure_background_record_running(
                    agent_name.as_str(),
                    Some(stable_id.as_str()),
                    restart_attempts,
                    None,
                )
                .await?;
            }

            let changed_this_record = {
                let state = self.state.read().await;
                state.background_children.get(&record_id).is_some_and(|r| {
                    r.status != before_status.unwrap_or(BackgroundSubprocessStatus::Starting)
                        || r.error != before_error
                        || r.summary != before_summary
                        || r.desired_enabled != before_desired_enabled
                })
            };
            changed |= changed_this_record;

            self.refresh_background_archive_metadata(&record_id).await?;
        }

        if changed {
            self.save_background_state().await?;
        }
        Ok(self.background_status_entries().await)
    }

    async fn update_background_record_state(
        &self,
        record_id: &str,
        snapshot: Option<crate::tools::types::VTCodeExecSession>,
    ) -> Result<Option<(String, String, u8)>> {
        let termination_requested = if let Some(snapshot) = snapshot.as_ref() {
            self.config.exec_sessions.termination_requested(snapshot.id.as_str()).await
        } else {
            false
        };
        let mut state = self.state.write().await;
        let Some(record) = state.background_children.get_mut(record_id) else {
            return Ok(None);
        };
        let Some(snapshot) = snapshot else {
            return Self::handle_missing_background_snapshot(record, &self.config);
        };

        // A previous execution may finish while the stable task is restarting.
        if record.exec_session_id != snapshot.id.as_str() {
            return Ok(None);
        }
        record.updated_at = Utc::now();
        record.exit_code = snapshot.exit_code;
        record.termination_requested |= termination_requested;
        record.pid = snapshot.child_pid;
        record.started_at = snapshot.started_at.or(record.started_at);

        match snapshot.lifecycle_state {
            Some(crate::tools::types::VTCodeSessionLifecycleState::Running) => {
                // A graceful stop sets `desired_enabled=false` optimistically
                // while SIGTERM drains. Do not resurrect `Stopped` back to
                // `Running` during that grace window; the `Exited` arm below
                // finalizes once the backend confirms exit.
                if !record.desired_enabled && matches!(record.status, BackgroundSubprocessStatus::Stopped) {
                    return Ok(None);
                }
                record.status = BackgroundSubprocessStatus::Running;
                record.ended_at = None;
                record.error = None;
            }
            Some(crate::tools::types::VTCodeSessionLifecycleState::Exited) | None => {
                record.ended_at.get_or_insert(Utc::now());
                // A clean `exit 0` is successful completion, not a crash.
                // It must not trigger auto-restore and must surface as
                // `Stopped` so the runloop/drawer agree with the exec
                // session's `exited (0)` status.
                if matches!(snapshot.exit_code, Some(0)) {
                    record.desired_enabled = false;
                    record.status = BackgroundSubprocessStatus::Stopped;
                    record.summary = Some("Background subprocess completed successfully".to_string());
                    record.error = None;
                    return Ok(None);
                }
                if record.desired_enabled
                    && self.config.vt_cfg.subagents.background.auto_restore
                    && record.restart_attempts < 1
                {
                    let next_restart_attempt = record.restart_attempts.saturating_add(1);
                    record.restart_attempts = next_restart_attempt;
                    record.status = BackgroundSubprocessStatus::Starting;
                    tracing::warn!(
                        agent_name = record.agent_name.as_str(),
                        record_id = record.id.as_str(),
                        attempt = next_restart_attempt,
                        "Background subprocess exited unexpectedly; scheduling restart"
                    );
                    return Ok(Some((record.agent_name.clone(), record.id.clone(), next_restart_attempt)));
                }
                Self::mark_background_record_stopped_or_error(record, &snapshot, &self.config);
            }
        }

        Ok(None)
    }

    fn handle_missing_background_snapshot(
        record: &mut BackgroundRecord,
        config: &SubagentControllerConfig,
    ) -> Result<Option<(String, String, u8)>> {
        if record.desired_enabled && config.vt_cfg.subagents.background.auto_restore {
            if record.restart_attempts < 1 {
                let next_restart_attempt = record.restart_attempts.saturating_add(1);
                record.restart_attempts = next_restart_attempt;
                record.status = BackgroundSubprocessStatus::Starting;
                tracing::warn!(
                    agent_name = record.agent_name.as_str(),
                    record_id = record.id.as_str(),
                    attempt = next_restart_attempt,
                    "Background subprocess is missing; scheduling restart"
                );
                return Ok(Some((record.agent_name.clone(), record.id.clone(), next_restart_attempt)));
            }
            record.status = BackgroundSubprocessStatus::Error;
            record.error = Some("Background subprocess is not running".to_string());
            record.ended_at.get_or_insert(Utc::now());
        } else if !record.desired_enabled {
            record.status = BackgroundSubprocessStatus::Stopped;
            record.ended_at.get_or_insert(Utc::now());
        }
        Ok(None)
    }

    fn mark_background_record_stopped_or_error(
        record: &mut BackgroundRecord,
        snapshot: &crate::tools::types::VTCodeExecSession,
        _config: &SubagentControllerConfig,
    ) {
        // Defensive: a retained `Some(0)` snapshot must never surface as
        // `Error`. This covers paths that bypass the early clean-exit return
        // above (e.g. restart budget already exhausted).
        if matches!(snapshot.exit_code, Some(0)) {
            record.desired_enabled = false;
            record.status = BackgroundSubprocessStatus::Stopped;
            record.summary = Some("Background subprocess completed successfully".to_string());
            record.error = None;
            record.ended_at.get_or_insert(Utc::now());
            return;
        }
        if record.desired_enabled {
            record.status = BackgroundSubprocessStatus::Error;
            record.summary = None;
            record.error = Some(match snapshot.exit_code {
                Some(exit_code) => format!("Background subprocess exited with code {exit_code}"),
                None => "Background subprocess exited unexpectedly".to_string(),
            });
        } else {
            record.status = BackgroundSubprocessStatus::Stopped;
            record.summary = Some("Background subprocess stopped".to_string());
            record.error = None;
        }
    }

    /// Blocks until one of the target background subprocesses reaches a
    /// terminal state (`Stopped`/`Error`) or the timeout expires.
    ///
    /// This is the background counterpart to the delegated
    /// [`SubagentController::wait`]: managed subprocesses previously had no
    /// model-visible wait path, so the main orchestrator could only observe
    /// completion via manual `/subprocesses` polling or the Local Agents
    /// drawer. Unknown ids resolve to `Ok(None)` (fail-closed) rather than
    /// an error so the unified `agent action=wait` dispatcher can race this
    /// alongside the delegated wait without hallucinating completion.
    pub async fn wait_for_background(
        &self,
        targets: &[String],
        timeout_ms: Option<u64>,
    ) -> Result<Option<BackgroundSubprocessEntry>> {
        if targets.is_empty() {
            return Ok(None);
        }
        let mut completion_rx = self.subscribe_background_completions();
        let _ = self.refresh_background_processes().await?;
        for target in targets {
            if let Ok(entry) = self.background_status_for(target).await
                && matches!(entry.status, BackgroundSubprocessStatus::Stopped | BackgroundSubprocessStatus::Error)
            {
                return Ok(Some(entry));
            }
        }
        let known = {
            let state = self.state.read().await;
            targets.iter().any(|target| state.background_children.contains_key(target))
        };
        if !known {
            return Ok(None);
        }

        let timeout = std::time::Duration::from_millis(
            timeout_ms.unwrap_or_else(|| self.config.vt_cfg.subagents.default_timeout_seconds.saturating_mul(1000)),
        );
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            tokio::select! {
                result = completion_rx.recv() => {
                    match result {
                        Ok(event) if targets.iter().any(|target| target == &event.task_id || target == &event.exec_session_id) => {
                            for target in targets {
                                if let Ok(entry) = self.background_status_for(target).await
                                    && matches!(entry.status, BackgroundSubprocessStatus::Stopped | BackgroundSubprocessStatus::Error)
                                {
                                    return Ok(Some(entry));
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            let _ = self.refresh_background_processes().await?;
                            for target in targets {
                                if let Ok(entry) = self.background_status_for(target).await
                                    && matches!(entry.status, BackgroundSubprocessStatus::Stopped | BackgroundSubprocessStatus::Error)
                                {
                                    return Ok(Some(entry));
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => return Ok(None),
                    }
                }
                _ = tokio::time::sleep(remaining) => {
                    let _ = self.refresh_background_processes().await?;
                    for target in targets {
                        if let Ok(entry) = self.background_status_for(target).await
                            && matches!(entry.status, BackgroundSubprocessStatus::Stopped | BackgroundSubprocessStatus::Error)
                        {
                            return Ok(Some(entry));
                        }
                    }
                    return Ok(None);
                }
            }
        }
    }

    /// Gracefully stops a background subprocess by setting its desired state to disabled.
    pub async fn graceful_stop_background(&self, target: &str) -> Result<BackgroundSubprocessEntry> {
        let (agent_name, exec_session_id) = {
            let mut state = self.state.write().await;
            let record = state
                .background_children
                .get_mut(target)
                .ok_or_else(|| anyhow!("Unknown background subprocess {target}"))?;
            record.termination_requested = true;
            record.desired_enabled = false;
            record.status = BackgroundSubprocessStatus::Stopped;
            record.summary = Some("Background subprocess stopped".to_string());
            record.error = None;
            record.updated_at = Utc::now();
            record.ended_at = Some(Utc::now());
            (record.agent_name.clone(), record.exec_session_id.clone())
        };

        tracing::info!(
            agent_name = agent_name.as_str(),
            record_id = target,
            exec_session_id = exec_session_id.as_str(),
            "Gracefully stopping background subagent subprocess"
        );

        if !exec_session_id.is_empty() {
            let _ = self.config.exec_sessions.terminate_session(&exec_session_id).await;
            let _ = self.config.exec_sessions.prune_exited_session(&exec_session_id).await;
        }

        self.refresh_background_archive_metadata(target).await?;
        self.save_background_state().await?;
        self.background_status_for(target).await
    }

    /// Force-cancels a background subprocess, closing its exec session immediately.
    pub async fn force_cancel_background(&self, target: &str) -> Result<BackgroundSubprocessEntry> {
        let (agent_name, exec_session_id) = {
            let mut state = self.state.write().await;
            let record = state
                .background_children
                .get_mut(target)
                .ok_or_else(|| anyhow!("Unknown background subprocess {target}"))?;
            record.termination_requested = true;
            record.desired_enabled = false;
            record.status = BackgroundSubprocessStatus::Stopped;
            record.summary = Some("Background subprocess stopped".to_string());
            record.error = None;
            record.updated_at = Utc::now();
            record.ended_at = Some(Utc::now());
            (record.agent_name.clone(), record.exec_session_id.clone())
        };

        tracing::info!(
            agent_name = agent_name.as_str(),
            record_id = target,
            exec_session_id = exec_session_id.as_str(),
            "Force cancelling background subagent subprocess"
        );

        if !exec_session_id.is_empty() {
            let _ = self.config.exec_sessions.close_session(&exec_session_id).await;
        }

        self.refresh_background_archive_metadata(target).await?;
        self.save_background_state().await?;
        self.publish_terminal_background_completion(target, &exec_session_id, None)
            .await?;
        self.background_status_for(target).await
    }

    /// Returns a thread snapshot for a tracked child subagent by its target id.
    pub async fn snapshot_for_thread(&self, target: &str) -> Result<SubagentThreadSnapshot> {
        let (
            id,
            session_id,
            parent_thread_id,
            agent_name,
            display_label,
            status,
            background,
            created_at,
            updated_at,
            archive_path,
            transcript_path,
            effective_config,
            thread_handle,
            archive_metadata,
            stored_messages,
            recent_events,
        ) = {
            let state = self.state.read().await;
            let record = state
                .children
                .get(target)
                .ok_or_else(|| anyhow!("Unknown subagent id {target}"))?;
            (
                record.id.clone(),
                record.session_id.clone(),
                record.parent_thread_id.clone(),
                record.spec.name.clone(),
                record.display_label.clone(),
                record.status,
                record.background,
                record.created_at,
                record.updated_at,
                record.archive_path.clone(),
                record.transcript_path.clone(),
                record.effective_config.clone(),
                record.thread_handle.clone(),
                record.archive_metadata.clone(),
                record.stored_messages.clone(),
                record
                    .thread_handle
                    .as_ref()
                    .map(ThreadRuntimeHandle::recent_events)
                    .unwrap_or_default(),
            )
        };

        let effective_config = effective_config
            .ok_or_else(|| anyhow!("Subagent {target} does not have a captured runtime configuration yet"))?;
        let snapshot = match thread_handle {
            Some(handle) => handle.snapshot(),
            None => {
                let archive_listing = match archive_path.as_ref() {
                    Some(path) if tokio::fs::metadata(path).await.is_ok() => load_session_listing(path).await.ok(),
                    _ => None,
                };
                let metadata = archive_listing
                    .as_ref()
                    .map(|listing| listing.snapshot.metadata.clone())
                    .or(archive_metadata)
                    .or_else(|| {
                        Some(crate::core::threads::build_thread_archive_metadata(
                            &self.config.workspace_root,
                            effective_config.agent.default_model.as_str(),
                            effective_config.agent.provider.as_str(),
                            effective_config.agent.theme.as_str(),
                            effective_config.agent.reasoning_effort.as_str(),
                        ))
                    });
                ThreadSnapshot {
                    thread_id: ThreadId::new(session_id.clone()),
                    metadata,
                    archive_listing,
                    messages: stored_messages,
                    loaded_skills: Vec::new(),
                    turn_in_flight: false,
                }
            }
        };

        Ok(SubagentThreadSnapshot {
            id,
            session_id,
            parent_thread_id,
            agent_name,
            display_label,
            status,
            background,
            created_at,
            updated_at,
            archive_path,
            transcript_path,
            effective_config,
            snapshot,
            recent_events,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subagents::tests::{read_only_test_spec, test_background_record, test_controller_config};

    #[tokio::test]
    async fn program_status_stale_snapshot_does_not_settle_restarted_background_task() {
        let temp = tempfile::TempDir::new().unwrap();
        let controller =
            SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
                .await
                .unwrap();
        let spec = read_only_test_spec("demo");
        let record =
            test_background_record(&spec, "stable-task", BackgroundSubprocessStatus::Running, true, "exec-current");
        let updated_at = record.updated_at;
        controller
            .state
            .write()
            .await
            .background_children
            .insert(record.id.clone(), record);
        let snapshot = serde_json::from_value(serde_json::json!({
            "id":"exec-old", "backend":"pipe", "command":"ignored", "args":[], "lifecycle_state":"exited", "exit_code":19
        })).unwrap();
        assert!(
            controller
                .update_background_record_state("stable-task", Some(snapshot))
                .await
                .unwrap()
                .is_none()
        );
        let state = controller.state.read().await;
        let record = &state.background_children["stable-task"];
        assert_eq!(record.status, BackgroundSubprocessStatus::Running);
        assert_eq!(record.exec_session_id, "exec-current");
        assert_eq!(record.exit_code, None);
        assert_eq!(record.updated_at, updated_at);
        assert!(!record.termination_requested);
    }

    #[test]
    fn program_status_completion_evidence_survives_persistence() {
        let spec = read_only_test_spec("demo");
        let mut record =
            test_background_record(&spec, "stable-task", BackgroundSubprocessStatus::Stopped, false, "exec-terminated");
        record.exit_code = Some(137);
        record.termination_requested = true;
        let persisted = record.into_persisted();
        let bytes = serde_json::to_vec(&persisted).unwrap();
        let decoded: PersistedBackgroundRecord = serde_json::from_slice(&bytes).unwrap();
        let restored = BackgroundRecord::from_persisted(decoded).build_status_entry();
        assert_eq!(restored.exit_code, Some(137));
        assert!(restored.termination_requested);
    }
}
