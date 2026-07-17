#![allow(unused_imports)]
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
use crate::tools::exec_session::ExecSessionManager;
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

#[allow(unused_imports)]
use super::*;

impl SubagentController {
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
                    if let Some(path) = entry
                        .transcript_path
                        .as_ref()
                        .or(entry.archive_path.as_ref())
                    {
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
            self.ensure_background_record_running(
                agent_name.as_str(),
                Some(target_id.as_str()),
                0,
                None,
            )
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
            state
                .background_children
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };

        let mut changed = false;
        for record_id in record_ids {
            let (snapshot_target, before_status, before_error) = {
                let state = self.state.read().await;
                let record = state.background_children.get(&record_id);
                (
                    record.map(|r| r.exec_session_id.clone()),
                    record.map(|r| r.status),
                    record.and_then(|r| r.error.clone()),
                )
            };

            let snapshot = if let Some(exec_session_id) = snapshot_target.as_ref()
                && !exec_session_id.is_empty()
            {
                self.config
                    .exec_sessions
                    .snapshot_session(exec_session_id)
                    .await
                    .ok()
            } else {
                None
            };

            let respawn = self
                .update_background_record_state(&record_id, snapshot)
                .await?;

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

    pub(super) async fn update_background_record_state(
        &self,
        record_id: &str,
        snapshot: Option<crate::tools::types::VTCodeExecSession>,
    ) -> Result<Option<(String, String, u8)>> {
        let mut state = self.state.write().await;
        let Some(record) = state.background_children.get_mut(record_id) else {
            return Ok(None);
        };
        record.updated_at = Utc::now();

        let Some(snapshot) = snapshot else {
            return Self::handle_missing_background_snapshot(record, &self.config);
        };

        record.pid = snapshot.child_pid;
        record.started_at = snapshot.started_at.or(record.started_at);

        match snapshot.lifecycle_state {
            Some(crate::tools::types::VTCodeSessionLifecycleState::Running) => {
                record.status = BackgroundSubprocessStatus::Running;
                record.ended_at = None;
                record.error = None;
            }
            Some(crate::tools::types::VTCodeSessionLifecycleState::Exited) | None => {
                record.ended_at.get_or_insert(Utc::now());
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
                    return Ok(Some((
                        record.agent_name.clone(),
                        record.id.clone(),
                        next_restart_attempt,
                    )));
                }
                Self::mark_background_record_stopped_or_error(record, &snapshot, &self.config);
            }
        }

        Ok(None)
    }

    pub(super) fn handle_missing_background_snapshot(
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
                return Ok(Some((
                    record.agent_name.clone(),
                    record.id.clone(),
                    next_restart_attempt,
                )));
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

    pub(super) fn mark_background_record_stopped_or_error(
        record: &mut BackgroundRecord,
        snapshot: &crate::tools::types::VTCodeExecSession,
        _config: &SubagentControllerConfig,
    ) {
        if record.desired_enabled {
            record.status = BackgroundSubprocessStatus::Error;
            record.error = Some(match snapshot.exit_code {
                Some(exit_code) => format!("Background subprocess exited with code {exit_code}"),
                None => "Background subprocess exited unexpectedly".to_string(),
            });
        } else {
            record.status = BackgroundSubprocessStatus::Stopped;
        }
    }

    /// Gracefully stops a background subprocess by setting its desired state to disabled.
    pub async fn graceful_stop_background(
        &self,
        target: &str,
    ) -> Result<BackgroundSubprocessEntry> {
        let (agent_name, exec_session_id) = {
            let mut state = self.state.write().await;
            let record = state
                .background_children
                .get_mut(target)
                .ok_or_else(|| anyhow!("Unknown background subprocess {target}"))?;
            record.desired_enabled = false;
            record.status = BackgroundSubprocessStatus::Stopped;
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
            let _ = self
                .config
                .exec_sessions
                .terminate_session(&exec_session_id)
                .await;
            let _ = self
                .config
                .exec_sessions
                .prune_exited_session(&exec_session_id)
                .await;
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
            record.desired_enabled = false;
            record.status = BackgroundSubprocessStatus::Stopped;
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
            let _ = self
                .config
                .exec_sessions
                .close_session(&exec_session_id)
                .await;
        }

        self.refresh_background_archive_metadata(target).await?;
        self.save_background_state().await?;
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

        let effective_config = effective_config.ok_or_else(|| {
            anyhow!("Subagent {target} does not have a captured runtime configuration yet")
        })?;
        let snapshot = match thread_handle {
            Some(handle) => handle.snapshot(),
            None => {
                let archive_listing = match archive_path.as_ref() {
                    Some(path) if tokio::fs::metadata(path).await.is_ok() => {
                        load_session_listing(path).await.ok()
                    }
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
