// ─── Module Structure ───────────────────────────────────────────────────────

mod background;
mod config;
mod constants;
mod discovery;
mod model;
mod prompt;
mod types;

// ─── Re-exports ─────────────────────────────────────────────────────────────

pub use background::{
    background_record_id, build_background_subagent_command, extract_tail_lines,
    load_archive_preview, subagent_display_label,
};
pub use config::{
    build_child_config, compose_subagent_instructions, filter_child_tools,
    normalize_child_max_turns, prepare_child_runtime_config,
};
pub use model::{agent_type_for_spec, load_memory_appendix};
pub use prompt::{
    contains_explicit_delegation_request, contains_explicit_model_request,
    delegated_task_requires_clarification, extract_explicit_agent_mentions,
    normalize_requested_model_override, request_prompt, sanitize_subagent_input_items,
};
pub use types::{
    BackgroundRecord, BackgroundSubprocessEntry, BackgroundSubprocessSnapshot,
    BackgroundSubprocessStatus, ChildRecord, ChildRunResult, ControllerState,
    PersistedBackgroundRecord, PersistedBackgroundState, SendInputRequest, SpawnAgentRequest,
    StatusEntryBuilder, SubagentInputItem, SubagentStatus, SubagentStatusEntry,
    SubagentThreadSnapshot, TurnDelegationHints,
};

// ─── Public Utilities ───────────────────────────────────────────────────────

pub fn is_subagent_tool(name: &str) -> bool {
    SUBAGENT_TOOL_NAMES.contains(&name)
}

// ─── Controller ─────────────────────────────────────────────────────────────

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use futures::future::select_all;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
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

// ─── Controller Config ─────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SubagentControllerConfig {
    pub workspace_root: PathBuf,
    pub parent_session_id: String,
    pub parent_model: String,
    pub parent_provider: String,
    pub parent_reasoning_effort: ReasoningEffortLevel,
    pub api_key: String,
    pub vt_cfg: VTCodeConfig,
    pub openai_chatgpt_auth: Option<OpenAIChatGptAuthHandle>,
    pub depth: usize,
    pub exec_sessions: ExecSessionManager,
    pub pty_manager: PtyManager,
}

#[derive(Clone)]
pub struct SubagentController {
    config: Arc<SubagentControllerConfig>,
    parent_session_id: Arc<RwLock<String>>,
    lifecycle_hooks: Option<LifecycleHookEngine>,
    state: Arc<RwLock<ControllerState>>,
}

impl SubagentController {
    pub async fn new(config: SubagentControllerConfig) -> Result<Self> {
        let discovered = discover_controller_subagents(&config.workspace_root).await?;
        let lifecycle_hooks = LifecycleHookEngine::new_with_session(
            config.workspace_root.clone(),
            &config.vt_cfg.hooks,
            SessionStartTrigger::Startup,
            config.parent_session_id.clone(),
            config.vt_cfg.permissions.default_mode,
        )?;
        let background_children = load_background_state(&config.workspace_root)?
            .records
            .into_iter()
            .map(|record| (record.id.clone(), BackgroundRecord::from_persisted(record)))
            .collect();
        Ok(Self {
            parent_session_id: Arc::new(RwLock::new(config.parent_session_id.clone())),
            lifecycle_hooks,
            config: Arc::new(config),
            state: Arc::new(RwLock::new(ControllerState {
                discovered,
                parent_messages: Vec::new(),
                turn_hints: TurnDelegationHints::default(),
                children: std::collections::BTreeMap::new(),
                background_children,
            })),
        })
    }

    pub async fn reload(&self) -> Result<()> {
        let discovered = discover_controller_subagents(&self.config.workspace_root).await?;
        self.state.write().await.discovered = discovered;
        Ok(())
    }

    pub async fn set_parent_messages(&self, messages: &[Message]) {
        let cloned = messages.to_vec();
        self.state.write().await.parent_messages = cloned;
    }

    pub async fn set_turn_delegation_hints_from_input(&self, input: &str) -> Vec<String> {
        let mut state = self.state.write().await;
        let explicit_mentions =
            extract_explicit_agent_mentions(input, state.discovered.effective.as_slice());
        let explicit_request =
            contains_explicit_delegation_request(input, explicit_mentions.as_slice());
        state.turn_hints = TurnDelegationHints {
            explicit_mentions: explicit_mentions.clone(),
            explicit_request,
            current_input: input.to_string(),
        };
        explicit_mentions
    }

    pub async fn clear_turn_delegation_hints(&self) {
        self.state.write().await.turn_hints = TurnDelegationHints::default();
    }

    pub async fn set_parent_session_id(&self, session_id: impl Into<String>) {
        *self.parent_session_id.write().await = session_id.into();
    }

    pub async fn effective_specs(&self) -> Vec<SubagentSpec> {
        self.state.read().await.discovered.effective.clone()
    }

    pub async fn shadowed_specs(&self) -> Vec<SubagentSpec> {
        self.state.read().await.discovered.shadowed.clone()
    }

    pub async fn status_entries(&self) -> Vec<SubagentStatusEntry> {
        let state = self.state.read().await;
        state
            .children
            .values()
            .map(ChildRecord::build_status_entry)
            .collect()
    }

    pub async fn background_status_entries(&self) -> Vec<BackgroundSubprocessEntry> {
        let state = self.state.read().await;
        state
            .background_children
            .values()
            .map(BackgroundRecord::build_status_entry)
            .collect()
    }

    pub async fn background_snapshot(&self, target: &str) -> Result<BackgroundSubprocessSnapshot> {
        let _ = self.refresh_background_processes().await?;

        let entry = {
            let state = self.state.read().await;
            state
                .background_children
                .get(target)
                .ok_or_else(|| anyhow!("Unknown background subprocess {}", target))?
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
                        load_archive_preview(path).unwrap_or_default()
                    } else {
                        String::new()
                    }
                }
            }
        };

        Ok(BackgroundSubprocessSnapshot { entry, preview })
    }

    #[must_use]
    pub fn background_subagents_enabled(&self) -> bool {
        self.config.vt_cfg.subagents.background.enabled
    }

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
            self.ensure_background_record_running(agent_name.as_str(), Some(target_id.as_str()), 0)
                .await
        }
    }

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
            )
            .await?;
        }

        self.refresh_background_processes().await
    }

    pub async fn refresh_background_processes(&self) -> Result<Vec<BackgroundSubprocessEntry>> {
        let record_ids = {
            let state = self.state.read().await;
            state
                .background_children
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };

        for record_id in record_ids {
            let snapshot_target = {
                let state = self.state.read().await;
                state
                    .background_children
                    .get(&record_id)
                    .map(|record| record.exec_session_id.clone())
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
                )
                .await?;
            }

            self.refresh_background_archive_metadata(&record_id).await?;
        }

        self.save_background_state().await?;
        Ok(self.background_status_entries().await)
    }

    async fn update_background_record_state(
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

    fn mark_background_record_stopped_or_error(
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

    pub async fn graceful_stop_background(
        &self,
        target: &str,
    ) -> Result<BackgroundSubprocessEntry> {
        let (agent_name, exec_session_id) = {
            let mut state = self.state.write().await;
            let record = state
                .background_children
                .get_mut(target)
                .ok_or_else(|| anyhow!("Unknown background subprocess {}", target))?;
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

    pub async fn force_cancel_background(&self, target: &str) -> Result<BackgroundSubprocessEntry> {
        let (agent_name, exec_session_id) = {
            let mut state = self.state.write().await;
            let record = state
                .background_children
                .get_mut(target)
                .ok_or_else(|| anyhow!("Unknown background subprocess {}", target))?;
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
                .ok_or_else(|| anyhow!("Unknown subagent id {}", target))?;
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
            anyhow!(
                "Subagent {} does not have a captured runtime configuration yet",
                target
            )
        })?;
        let snapshot = match thread_handle {
            Some(handle) => handle.snapshot(),
            None => {
                let archive_listing = match archive_path.as_ref() {
                    Some(path) if path.exists() => load_session_listing(path).ok(),
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

    pub async fn spawn(&self, request: SpawnAgentRequest) -> Result<SubagentStatusEntry> {
        let mut request = request;
        let (requested_agent, explicit_mentions, explicit_request, current_input) = {
            let state = self.state.read().await;
            sanitize_subagent_input_items(&mut request.items);
            request.model = normalize_requested_model_override(
                request.model.take(),
                &state.turn_hints.current_input,
            );
            let requested_agent = if let Some(agent_type) = request.agent_type.as_deref() {
                Some(agent_type.to_string())
            } else {
                match state.turn_hints.explicit_mentions.as_slice() {
                    [] => None,
                    [single] => Some(single.clone()),
                    mentions => {
                        bail!(
                            "spawn_agent omitted agent_type, but the user explicitly selected multiple agents: {}. Specify agent_type explicitly.",
                            mentions.join(", ")
                        );
                    }
                }
            };
            (
                requested_agent,
                state.turn_hints.explicit_mentions.clone(),
                state.turn_hints.explicit_request,
                state.turn_hints.current_input.clone(),
            )
        };
        let spec = self
            .resolve_requested_spec(requested_agent.as_deref())
            .await?;
        if let Some(explicit) = explicit_mentions.first()
            && explicit_mentions.len() == 1
            && !spec.matches_name(explicit)
        {
            bail!(
                "spawn_agent requested agent_type '{}', but the user explicitly selected '{}'. Use the selected agent or ask the user to clarify.",
                spec.name,
                explicit
            );
        }
        if !spec.is_read_only() && !explicit_request {
            bail!(
                "spawn_agent cannot launch write-capable agent '{}' without an explicit delegation signal from the current user turn. Ask the user to mention the agent, say 'delegate'/'spawn', or request parallel work.",
                spec.name
            );
        }
        if spec.is_read_only()
            && !self.config.vt_cfg.subagents.auto_delegate_read_only
            && !explicit_request
        {
            bail!(
                "spawn_agent cannot proactively launch read-only agent '{}' because `subagents.auto_delegate_read_only` is disabled and the current user turn did not explicitly request delegation.",
                spec.name
            );
        }
        if let Some(requested_model) = request.model.as_deref()
            && !contains_explicit_model_request(&current_input, requested_model)
        {
            tracing::warn!(
                agent_name = spec.name.as_str(),
                requested_model = requested_model.trim(),
                "Ignoring subagent model override because the current user turn did not explicitly request that model"
            );
            request.model = None;
        }
        let prompt = request_prompt(&request.message, &request.items)
            .or_else(|| spec.initial_prompt.clone())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("spawn_agent requires a task message or items"))?;
        if delegated_task_requires_clarification(&prompt) {
            bail!(
                "spawn_agent task for '{}' is too vague ('{}'). Ask the user for a specific delegated task before spawning the subagent.",
                spec.name,
                prompt.trim()
            );
        }
        self.spawn_with_spec(
            spec,
            prompt,
            request.fork_context,
            request.background,
            request.max_turns,
            request.model,
            request.reasoning_effort,
        )
        .await
    }

    pub async fn spawn_custom(
        &self,
        spec: SubagentSpec,
        request: SpawnAgentRequest,
    ) -> Result<SubagentStatusEntry> {
        if !spec.is_read_only() {
            bail!(
                "custom subagent spawn only supports read-only specs; '{}' exposes write-capable behavior",
                spec.name
            );
        }

        let mut request = request;
        sanitize_subagent_input_items(&mut request.items);

        let prompt = request_prompt(&request.message, &request.items)
            .or_else(|| spec.initial_prompt.clone())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("custom subagent spawn requires a task message or items"))?;
        if delegated_task_requires_clarification(&prompt) {
            bail!(
                "custom subagent task for '{}' is too vague ('{}'). Provide a specific delegated task before spawning the subagent.",
                spec.name,
                prompt.trim()
            );
        }

        self.spawn_with_spec(
            spec,
            prompt,
            request.fork_context,
            request.background,
            request.max_turns,
            request.model,
            request.reasoning_effort,
        )
        .await
    }

    pub async fn send_input(&self, request: SendInputRequest) -> Result<SubagentStatusEntry> {
        let prompt = request_prompt(&request.message, &request.items)
            .ok_or_else(|| anyhow!("send_input requires a message or items"))?;

        let maybe_restart = {
            let mut state = self.state.write().await;
            let record = state
                .children
                .get_mut(&request.target)
                .ok_or_else(|| anyhow!("Unknown subagent id {}", request.target))?;

            if record.status == SubagentStatus::Closed {
                bail!("Subagent {} is closed", request.target);
            }

            record.updated_at = Utc::now();
            record.last_prompt = Some(prompt.clone());

            if request.interrupt {
                if let Some(handle) = record.handle.take() {
                    handle.abort();
                }
                record.status = SubagentStatus::Queued;
                record.queued_prompts.clear();
                record.queued_prompts.push_back(prompt.clone());
                true
            } else if matches!(
                record.status,
                SubagentStatus::Running | SubagentStatus::Queued
            ) {
                record.status = SubagentStatus::Waiting;
                record.queued_prompts.push_back(prompt.clone());
                false
            } else {
                record.status = SubagentStatus::Queued;
                record.queued_prompts.push_back(prompt.clone());
                true
            }
        };

        if maybe_restart {
            self.restart_child(&request.target).await?;
        }

        self.status_for(&request.target).await
    }

    pub async fn resume(&self, target: &str) -> Result<SubagentStatusEntry> {
        let subtree_ids = self.collect_spawn_subtree_ids(target).await?;
        let mut restart_ids = Vec::new();
        for node_id in subtree_ids {
            if self.reopen_single(node_id.as_str()).await? {
                restart_ids.push(node_id);
            }
        }
        for restart_id in restart_ids {
            self.restart_child(&restart_id).await?;
        }
        self.status_for(target).await
    }

    pub async fn close(&self, target: &str) -> Result<SubagentStatusEntry> {
        let subtree_ids = self.collect_spawn_subtree_ids(target).await?;
        for node_id in subtree_ids.into_iter().rev() {
            self.close_single(node_id.as_str()).await?;
        }
        self.status_for(target).await
    }

    pub async fn wait(
        &self,
        targets: &[String],
        timeout_ms: Option<u64>,
    ) -> Result<Option<SubagentStatusEntry>> {
        for target in targets {
            if let Ok(entry) = self.status_for(target).await
                && entry.status.is_terminal()
            {
                return Ok(Some(entry));
            }
        }

        let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or_else(|| {
            self.config
                .vt_cfg
                .subagents
                .default_timeout_seconds
                .saturating_mul(1000)
        }));
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            let notifies = {
                let state = self.state.read().await;
                targets
                    .iter()
                    .filter_map(|target| {
                        state
                            .children
                            .get(target)
                            .map(|record| record.notify.clone())
                    })
                    .collect::<Vec<_>>()
            };
            if notifies.is_empty() {
                return Ok(None);
            }

            for target in targets {
                if let Ok(entry) = self.status_for(target).await
                    && entry.status.is_terminal()
                {
                    return Ok(Some(entry));
                }
            }

            let sleep = tokio::time::sleep_until(deadline);
            tokio::pin!(sleep);
            let wait_any = select_all(
                notifies
                    .into_iter()
                    .map(|notify| Box::pin(async move { notify.notified().await }))
                    .collect::<Vec<_>>(),
            );
            tokio::pin!(wait_any);

            tokio::select! {
                _ = &mut sleep => return Ok(None),
                _ = &mut wait_any => {}
            }
        }
    }

    pub async fn status_for(&self, target: &str) -> Result<SubagentStatusEntry> {
        let state = self.state.read().await;
        let record = state
            .children
            .get(target)
            .ok_or_else(|| anyhow!("Unknown subagent id {}", target))?;
        Ok(record.build_status_entry())
    }

    async fn spawn_child_ids_for_parent(&self, parent_thread_id: &str) -> Vec<String> {
        let state = self.state.read().await;
        let mut child_ids = state
            .children
            .values()
            .filter(|record| record.parent_thread_id == parent_thread_id)
            .map(|record| record.id.clone())
            .collect::<Vec<_>>();
        child_ids.sort();
        child_ids
    }

    async fn collect_spawn_subtree_ids(&self, root_thread_id: &str) -> Result<Vec<String>> {
        let mut subtree_ids = Vec::new();
        let mut stack = vec![root_thread_id.to_string()];

        while let Some(thread_id) = stack.pop() {
            subtree_ids.push(thread_id.clone());
            let child_ids = self.spawn_child_ids_for_parent(&thread_id).await;
            for child_id in child_ids.into_iter().rev() {
                stack.push(child_id);
            }
        }

        Ok(subtree_ids)
    }

    async fn reopen_single(&self, target: &str) -> Result<bool> {
        let mut state = self.state.write().await;
        let record = state
            .children
            .get_mut(target)
            .ok_or_else(|| anyhow!("Unknown subagent id {}", target))?;
        if matches!(
            record.status,
            SubagentStatus::Running | SubagentStatus::Queued
        ) {
            return Ok(false);
        }
        let prompt = record.last_prompt.clone().unwrap_or_else(|| {
            "Continue the delegated task from the existing context.".to_string()
        });
        record.status = SubagentStatus::Queued;
        record.updated_at = Utc::now();
        record.completed_at = None;
        record.error = None;
        record.summary = None;
        record.queued_prompts.push_back(prompt);
        Ok(true)
    }

    async fn close_single(&self, target: &str) -> Result<SubagentStatusEntry> {
        let mut state = self.state.write().await;
        let record = state
            .children
            .get_mut(target)
            .ok_or_else(|| anyhow!("Unknown subagent id {}", target))?;
        if record.status == SubagentStatus::Closed {
            return Ok(record.build_status_entry());
        }
        if let Some(handle) = record.handle.take() {
            handle.abort();
        }
        record.status = SubagentStatus::Closed;
        record.updated_at = Utc::now();
        record.completed_at = Some(Utc::now());
        record.notify.notify_waiters();
        Ok(record.build_status_entry())
    }

    async fn background_status_for(&self, target: &str) -> Result<BackgroundSubprocessEntry> {
        let state = self.state.read().await;
        let record = state
            .background_children
            .get(target)
            .ok_or_else(|| anyhow!("Unknown background subprocess {}", target))?;
        Ok(record.build_status_entry())
    }

    async fn ensure_background_record_running(
        &self,
        agent_name: &str,
        stable_id: Option<&str>,
        restart_attempts: u8,
    ) -> Result<BackgroundSubprocessEntry> {
        let spec = self
            .resolve_requested_spec(Some(agent_name))
            .await
            .with_context(|| format!("Failed to resolve background subagent '{agent_name}'"))?;
        let record_id = stable_id
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| background_record_id(agent_name));
        let previous_record = {
            let state = self.state.read().await;
            state.background_children.get(&record_id).map(|record| {
                (
                    record.created_at,
                    record.prompt.clone(),
                    record.max_turns,
                    record.model_override.clone(),
                    record.reasoning_override.clone(),
                )
            })
        };
        let parent_session_id = self.parent_session_id.read().await.clone();
        let session_id = format!(
            "{}-{}-{}",
            sanitize_component(parent_session_id.as_str()),
            sanitize_component(record_id.as_str()),
            Utc::now().format("%Y%m%dT%H%M%S%3fZ")
        );
        let exec_session_id = format!("exec-{session_id}");
        let (
            created_at,
            previous_prompt,
            previous_max_turns,
            previous_model_override,
            previous_reasoning_override,
        ) = previous_record.unwrap_or((Utc::now(), String::new(), None, None, None));
        let prompt = (!previous_prompt.trim().is_empty())
            .then_some(previous_prompt)
            .or_else(|| spec.initial_prompt.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                format!(
                    "You are the VT Code background subagent `{}`. Summarize readiness briefly, inspect the workspace at a high level, then remain idle until the process is terminated.",
                    spec.name
                )
            });
        let max_turns = normalize_child_max_turns(previous_max_turns.or(spec.max_turns));
        let model_override = previous_model_override.or_else(|| spec.model.clone());
        let reasoning_override =
            previous_reasoning_override.or_else(|| spec.reasoning_effort.clone());

        {
            let mut state = self.state.write().await;
            state.background_children.insert(
                record_id.clone(),
                BackgroundRecord {
                    id: record_id.clone(),
                    agent_name: spec.name.clone(),
                    display_label: subagent_display_label(&spec),
                    description: spec.description.clone(),
                    source: spec.source.label(),
                    color: spec.color.clone(),
                    session_id: session_id.clone(),
                    exec_session_id: exec_session_id.clone(),
                    desired_enabled: true,
                    status: BackgroundSubprocessStatus::Starting,
                    created_at,
                    updated_at: Utc::now(),
                    started_at: None,
                    ended_at: None,
                    pid: None,
                    prompt: prompt.clone(),
                    summary: Some("Starting background subagent".to_string()),
                    error: None,
                    archive_path: None,
                    transcript_path: None,
                    max_turns,
                    model_override: model_override.clone(),
                    reasoning_override: reasoning_override.clone(),
                    restart_attempts,
                },
            );
        }

        let command = build_background_subagent_command(
            &self.config.workspace_root,
            spec.name.as_str(),
            parent_session_id.as_str(),
            session_id.as_str(),
            prompt.as_str(),
            max_turns,
            model_override.as_deref(),
            reasoning_override.as_deref(),
        )?;
        let metadata = self
            .config
            .exec_sessions
            .create_pty_session(
                exec_session_id.clone().into(),
                command,
                self.config.workspace_root.clone(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                hashbrown::HashMap::new(),
                None,
            )
            .await
            .with_context(|| {
                format!(
                    "Failed to spawn background subprocess for subagent '{}'",
                    spec.name
                )
            })?;

        tracing::info!(
            agent_name = spec.name.as_str(),
            record_id = record_id.as_str(),
            exec_session_id = exec_session_id.as_str(),
            pid = metadata.child_pid,
            "Spawned background subagent subprocess"
        );

        {
            let mut state = self.state.write().await;
            let record = state
                .background_children
                .get_mut(&record_id)
                .ok_or_else(|| anyhow!("Unknown background subprocess {}", record_id))?;
            record.exec_session_id = exec_session_id;
            record.pid = metadata.child_pid;
            record.started_at = metadata.started_at;
            record.status = BackgroundSubprocessStatus::Running;
            record.updated_at = Utc::now();
            record.ended_at = None;
            record.error = None;
            record.summary = Some("Background subagent is running".to_string());
        }

        self.save_background_state().await?;
        self.background_status_for(&record_id).await
    }

    async fn refresh_background_archive_metadata(&self, target: &str) -> Result<()> {
        let session_id = {
            let state = self.state.read().await;
            state
                .background_children
                .get(target)
                .map(|record| record.session_id.clone())
                .ok_or_else(|| anyhow!("Unknown background subprocess {}", target))?
        };

        if let Some(listing) = find_session_by_identifier(&session_id).await? {
            let mut state = self.state.write().await;
            if let Some(record) = state.background_children.get_mut(target) {
                record.archive_path = Some(listing.path.clone());
                record.transcript_path = Some(listing.path);
            }
        }

        Ok(())
    }

    async fn save_background_state(&self) -> Result<()> {
        let records = {
            let state = self.state.read().await;
            state
                .background_children
                .values()
                .cloned()
                .map(BackgroundRecord::into_persisted)
                .collect()
        };
        persist_background_state(&self.config.workspace_root, records)
    }

    async fn find_spec(&self, candidate: &str) -> Option<SubagentSpec> {
        self.state
            .read()
            .await
            .discovered
            .effective
            .iter()
            .find(|spec| spec.matches_name(candidate))
            .cloned()
    }

    async fn resolve_requested_spec(&self, requested: Option<&str>) -> Result<SubagentSpec> {
        let requested = requested.unwrap_or("default");
        self.find_spec(requested)
            .await
            .ok_or_else(|| anyhow!("Unknown subagent type {}", requested))
    }

    async fn spawn_with_spec(
        &self,
        spec: SubagentSpec,
        prompt: String,
        fork_context: bool,
        background: bool,
        max_turns: Option<usize>,
        model_override: Option<String>,
        reasoning_override: Option<String>,
    ) -> Result<SubagentStatusEntry> {
        if !self.config.vt_cfg.subagents.enabled {
            bail!("Subagents are disabled by configuration");
        }
        if self.config.depth.saturating_add(1) > self.config.vt_cfg.subagents.max_depth {
            bail!(
                "Subagent depth limit reached (max_depth={})",
                self.config.vt_cfg.subagents.max_depth
            );
        }
        if spec.isolation.as_deref() == Some("worktree") {
            bail!("Subagent isolation=worktree is not supported in this VT Code build");
        }

        let active_count = {
            let state = self.state.read().await;
            state
                .children
                .values()
                .filter(|record| {
                    matches!(
                        record.status,
                        SubagentStatus::Queued | SubagentStatus::Running | SubagentStatus::Waiting
                    )
                })
                .count()
        };
        let effective_max_concurrent = self
            .config
            .vt_cfg
            .subagents
            .max_concurrent
            .min(SUBAGENT_HARD_CONCURRENCY_LIMIT);
        if active_count >= effective_max_concurrent {
            bail!(
                "Subagent concurrency limit reached (max_concurrent={})",
                effective_max_concurrent
            );
        }
        let child_max_turns = normalize_child_max_turns(max_turns.or(spec.max_turns));
        let (_, _, effective_config) = prepare_child_runtime_config(
            &self.config.vt_cfg,
            &spec,
            self.config.parent_model.as_str(),
            self.config.parent_provider.as_str(),
            self.config.parent_reasoning_effort,
            child_max_turns,
            model_override.as_deref(),
            reasoning_override.as_deref(),
            resolve_effective_subagent_model,
        )?;

        let id = format!(
            "agent-{}-{}",
            sanitize_component(spec.name.as_str()),
            Utc::now().format("%Y%m%dT%H%M%S%3fZ")
        );
        let parent_session_id = self.parent_session_id.read().await.clone();
        let session_id = format!(
            "{}-{}",
            sanitize_component(parent_session_id.as_str()),
            sanitize_component(id.as_str())
        );
        let display_label = subagent_display_label(&spec);
        let notify = Arc::new(Notify::new());
        let mut state = self.state.write().await;
        let initial_messages = if fork_context {
            state.parent_messages.clone()
        } else {
            Vec::new()
        };
        let entry = ChildRecord {
            id: id.clone(),
            session_id,
            parent_thread_id: parent_session_id,
            spec: spec.clone(),
            display_label,
            status: SubagentStatus::Queued,
            background: background || spec.background,
            depth: self.config.depth.saturating_add(1),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
            summary: None,
            error: None,
            archive_metadata: None,
            archive_path: None,
            transcript_path: None,
            effective_config: Some(effective_config),
            stored_messages: initial_messages,
            last_prompt: Some(prompt.clone()),
            queued_prompts: VecDeque::new(),
            thread_handle: None,
            handle: None,
            notify,
        };
        state.children.insert(id.clone(), entry);
        drop(state);

        self.launch_child(
            id.as_str(),
            prompt,
            child_max_turns,
            model_override,
            reasoning_override,
        )
        .await?;
        self.status_for(&id).await
    }

    async fn restart_child(&self, target: &str) -> Result<()> {
        let (prompt, max_turns) = {
            let mut state = self.state.write().await;
            let record = state
                .children
                .get_mut(target)
                .ok_or_else(|| anyhow!("Unknown subagent id {}", target))?;
            let prompt = record
                .queued_prompts
                .pop_front()
                .or_else(|| record.last_prompt.clone());
            let prompt =
                prompt.ok_or_else(|| anyhow!("Subagent {} has no queued input", target))?;
            (prompt, record.spec.max_turns)
        };
        self.launch_child(target, prompt, max_turns, None, None)
            .await
    }

    async fn launch_child(
        &self,
        child_id: &str,
        prompt: String,
        max_turns: Option<usize>,
        model_override: Option<String>,
        reasoning_override: Option<String>,
    ) -> Result<()> {
        let controller = self.clone();
        let target = child_id.to_string();
        let handle = tokio::spawn(async move {
            controller
                .child_loop(
                    &target,
                    prompt,
                    max_turns,
                    model_override,
                    reasoning_override,
                )
                .await;
        });
        let mut state = self.state.write().await;
        let record = state
            .children
            .get_mut(child_id)
            .ok_or_else(|| anyhow!("Unknown subagent id {}", child_id))?;
        record.handle = Some(handle);
        record.status = SubagentStatus::Queued;
        record.updated_at = Utc::now();
        Ok(())
    }

    async fn child_loop(
        &self,
        child_id: &str,
        mut prompt: String,
        max_turns: Option<usize>,
        model_override: Option<String>,
        reasoning_override: Option<String>,
    ) {
        loop {
            let execute = self
                .run_child_once(
                    child_id,
                    prompt.clone(),
                    max_turns,
                    model_override.clone(),
                    reasoning_override.clone(),
                )
                .await;

            let (next_prompt, hook_payload) = {
                let mut state = self.state.write().await;
                let Some(record) = state.children.get_mut(child_id) else {
                    return;
                };
                record.updated_at = Utc::now();
                let next_prompt = record.apply_result(execute);
                let hook_payload = next_prompt.is_none().then(|| record.build_hook_payload());
                (next_prompt, hook_payload)
            };

            if let Some((
                parent_session_id,
                child_thread_id,
                agent_name,
                display_label,
                background,
                status,
                transcript_path,
            )) = hook_payload
                && let Some(hooks) = self.lifecycle_hooks.as_ref()
                && let Err(err) = hooks
                    .run_subagent_stop(
                        &parent_session_id,
                        &child_thread_id,
                        &agent_name,
                        &display_label,
                        background,
                        &status,
                        transcript_path.as_deref(),
                    )
                    .await
            {
                tracing::warn!(
                    child_id,
                    error = %err,
                    "Failed to run subagent stop hooks"
                );
            }

            if let Some(next) = next_prompt {
                prompt = next;
                continue;
            } else {
                let mut state = self.state.write().await;
                if let Some(record) = state.children.get_mut(child_id) {
                    record.handle = None;
                    record.updated_at = Utc::now();
                }
                return;
            }
        }
    }

    async fn run_child_once(
        &self,
        child_id: &str,
        prompt: String,
        max_turns: Option<usize>,
        model_override: Option<String>,
        reasoning_override: Option<String>,
    ) -> Result<ChildRunResult> {
        let (spec, session_id, bootstrap_messages, display_label, background) = {
            let mut state = self.state.write().await;
            let record = state
                .children
                .get_mut(child_id)
                .ok_or_else(|| anyhow!("Unknown subagent id {}", child_id))?;
            record.status = SubagentStatus::Running;
            record.updated_at = Utc::now();
            (
                record.spec.clone(),
                record.session_id.clone(),
                record.stored_messages.clone(),
                record.display_label.clone(),
                record.background,
            )
        };

        let (resolved_model, child_reasoning_effort, child_cfg) = prepare_child_runtime_config(
            &self.config.vt_cfg,
            &spec,
            self.config.parent_model.as_str(),
            self.config.parent_provider.as_str(),
            self.config.parent_reasoning_effort,
            max_turns,
            model_override.as_deref(),
            reasoning_override.as_deref(),
            resolve_effective_subagent_model,
        )?;
        let parent_session_id = self.parent_session_id.read().await.clone();

        let archive_metadata = build_subagent_archive_metadata(
            &self.config.workspace_root,
            child_cfg.agent.default_model.as_str(),
            child_cfg.agent.provider.as_str(),
            child_cfg.agent.theme.as_str(),
            child_reasoning_effort.as_str(),
            parent_session_id.as_str(),
            !bootstrap_messages.is_empty(),
        );
        let bootstrap = ThreadBootstrap::new(Some(archive_metadata.clone()))
            .with_messages(bootstrap_messages.clone());
        let archive = if let Some(listing) = find_session_by_identifier(&session_id).await? {
            SessionArchive::resume_from_listing(&listing, archive_metadata.clone())
        } else {
            SessionArchive::new_with_identifier(archive_metadata.clone(), session_id.clone())
                .await?
        };
        checkpoint_subagent_archive_start(&archive, &bootstrap_messages).await?;
        let mut runner = AgentRunner::new_with_thread_bootstrap_and_config_with_openai_auth(
            agent_type_for_spec(&spec),
            resolved_model,
            self.config.api_key.clone(),
            self.config.workspace_root.clone(),
            session_id.clone(),
            RunnerSettings {
                reasoning_effort: Some(child_reasoning_effort),
                verbosity: None,
            },
            None,
            bootstrap,
            child_cfg.clone(),
            self.config.openai_chatgpt_auth.clone(),
        )
        .await?;
        runner.set_quiet(true);
        let thread_handle = runner.thread_handle();
        let archive_path = archive.path().to_path_buf();

        {
            let mut state = self.state.write().await;
            let record = state
                .children
                .get_mut(child_id)
                .ok_or_else(|| anyhow!("Unknown subagent id {}", child_id))?;
            record.archive_metadata = Some(archive_metadata.clone());
            record.archive_path = Some(archive_path.clone());
            record.effective_config = Some(child_cfg.clone());
            record.thread_handle = Some(thread_handle.clone());
        }
        if let Some(hooks) = self.lifecycle_hooks.as_ref()
            && let Err(err) = hooks
                .run_subagent_start(
                    parent_session_id.as_str(),
                    thread_handle.thread_id().as_str(),
                    spec.name.as_str(),
                    &display_label,
                    background,
                    SubagentStatus::Running.as_str(),
                    Some(archive_path.as_path()),
                )
                .await
        {
            tracing::warn!(
                child_id,
                error = %err,
                "Failed to run subagent start hooks"
            );
        }

        let filtered_tools = filter_child_tools(
            &spec,
            runner.build_universal_tools().await?,
            spec.is_read_only(),
        );
        let allowed_tools = filtered_tools
            .iter()
            .map(|tool| tool.function_name().to_string())
            .collect::<Vec<_>>();
        runner.set_tool_definitions_override(filtered_tools);
        runner.enable_full_auto(&allowed_tools).await;

        let memory_appendix =
            load_memory_appendix(&self.config.workspace_root, spec.name.as_str(), spec.memory)?;
        let mut task = Task::new(
            format!("subagent-{}", spec.name),
            format!("Subagent {}", spec.name),
            prompt,
        );
        task.instructions = Some(compose_subagent_instructions(&spec, memory_appendix));

        let results = runner.execute_task(&task, &[]).await?;
        let messages = runner.session_messages();
        let transcript_path =
            persist_child_archive(&archive, &messages, spec.name.as_str()).await?;

        Ok(ChildRunResult {
            messages,
            summary: if results.summary.trim().is_empty() {
                results.outcome.description()
            } else {
                results.summary.clone()
            },
            outcome: results.outcome,
            transcript_path,
        })
    }
}

fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn load_session_listing(
    path: &std::path::Path,
) -> Result<crate::utils::session_archive::SessionListing> {
    use anyhow::Context;
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read session archive {}", path.display()))?;
    let snapshot: crate::utils::session_archive::SessionSnapshot = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse session archive {}", path.display()))?;
    Ok(crate::utils::session_archive::SessionListing {
        path: path.to_path_buf(),
        snapshot,
    })
}

async fn checkpoint_subagent_archive_start(
    archive: &SessionArchive,
    messages: &[Message],
) -> Result<()> {
    use crate::utils::session_archive::SessionMessage;
    let recent_messages = messages
        .iter()
        .map(SessionMessage::from)
        .collect::<Vec<_>>();
    archive
        .persist_progress_async(crate::utils::session_archive::SessionProgressArgs {
            total_messages: recent_messages.len(),
            distinct_tools: Vec::new(),
            recent_messages,
            turn_number: 1,
            token_usage: None,
            max_context_tokens: None,
            loaded_skills: Some(Vec::new()),
        })
        .await?;
    Ok(())
}

async fn persist_child_archive(
    archive: &SessionArchive,
    messages: &[Message],
    agent_name: &str,
) -> Result<Option<PathBuf>> {
    use crate::utils::session_archive::SessionMessage;
    let transcript = messages
        .iter()
        .filter_map(transcript_line_from_message)
        .take(SUBAGENT_TRANSCRIPT_LINE_LIMIT)
        .collect::<Vec<_>>();
    let stored_messages = messages
        .iter()
        .map(SessionMessage::from)
        .collect::<Vec<_>>();
    let path = archive.finalize(
        transcript,
        stored_messages.len(),
        vec![agent_name.to_string()],
        stored_messages,
    )?;
    Ok(Some(path))
}

fn transcript_line_from_message(message: &Message) -> Option<String> {
    let role = message.role.to_string();
    let content = message.content.trim();
    if content.is_empty() {
        return None;
    }
    Some(format!("{role}: {content}"))
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PermissionMode;
    use crate::config::constants::models;
    use crate::config::constants::tools;
    use crate::config::models::{ModelId, Provider};
    use crate::llm::provider::ToolDefinition;
    use crate::tools::exec_session::ExecSessionManager;
    use crate::tools::registry::PtySessionManager;
    use anyhow::{Result, anyhow};
    use std::collections::BTreeMap;
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::sync::Notify;
    use vtcode_config::{SubagentMcpServer, SubagentMemoryScope, SubagentSource, SubagentSpec};

    fn test_controller_config(
        workspace_root: PathBuf,
        vt_cfg: VTCodeConfig,
    ) -> SubagentControllerConfig {
        let pty_sessions = PtySessionManager::new(workspace_root.clone(), vt_cfg.pty.clone());
        let exec_sessions = ExecSessionManager::new(workspace_root.clone(), pty_sessions.clone());
        SubagentControllerConfig {
            workspace_root,
            parent_session_id: "parent-session".to_string(),
            parent_model: models::openai::GPT_5_4.to_string(),
            parent_provider: "openai".to_string(),
            parent_reasoning_effort: ReasoningEffortLevel::Medium,
            api_key: "test-key".to_string(),
            vt_cfg,
            openai_chatgpt_auth: None,
            depth: 0,
            exec_sessions,
            pty_manager: pty_sessions.manager().clone(),
        }
    }

    fn test_child_record(
        id: &str,
        parent_thread_id: &str,
        spec: &SubagentSpec,
        status: SubagentStatus,
        depth: usize,
    ) -> ChildRecord {
        ChildRecord {
            id: id.to_string(),
            session_id: format!("session-{id}"),
            parent_thread_id: parent_thread_id.to_string(),
            spec: spec.clone(),
            display_label: subagent_display_label(spec),
            status,
            background: false,
            depth,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: status.is_terminal().then_some(Utc::now()),
            summary: None,
            error: None,
            archive_metadata: None,
            archive_path: None,
            transcript_path: None,
            effective_config: Some(VTCodeConfig::default()),
            stored_messages: Vec::new(),
            last_prompt: Some(format!("prompt-{id}")),
            queued_prompts: VecDeque::new(),
            thread_handle: None,
            handle: None,
            notify: Arc::new(Notify::new()),
        }
    }

    #[test]
    fn request_prompt_prefers_message() {
        let request = SpawnAgentRequest {
            message: Some("hello".to_string()),
            ..SpawnAgentRequest::default()
        };
        assert_eq!(
            request_prompt(&request.message, &request.items).as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn delegated_task_requires_clarification_for_vague_prompt() {
        assert!(delegated_task_requires_clarification("report"));
        assert!(delegated_task_requires_clarification("report findings"));
        assert!(!delegated_task_requires_clarification(
            "review current code changes"
        ));
    }

    #[test]
    fn resolve_subagent_model_maps_aliases() {
        let cfg = VTCodeConfig::default();
        let resolved = resolve_subagent_model(
            &cfg,
            models::anthropic::CLAUDE_SONNET_4_6,
            "anthropic",
            Some("haiku"),
            "explorer",
        )
        .expect("resolve model");
        assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_HAIKU_4_5);
    }

    #[test]
    fn resolve_subagent_model_defaults_to_parent_when_omitted() {
        let cfg = VTCodeConfig::default();
        let resolved = resolve_subagent_model(
            &cfg,
            models::ollama::GPT_OSS_120B_CLOUD,
            "ollama",
            None,
            "worker",
        )
        .expect("resolve model");
        assert_eq!(resolved.as_str(), models::ollama::GPT_OSS_120B_CLOUD);
    }

    #[test]
    fn resolve_subagent_model_accepts_dotted_claude_aliases_for_anthropic() {
        let cfg = VTCodeConfig::default();
        let resolved =
            resolve_subagent_model(&cfg, "claude-haiku-4.5", "anthropic", None, "worker")
                .expect("resolve model");
        assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_HAIKU_4_5);
    }

    #[test]
    fn resolve_subagent_model_falls_back_to_copilot_default_for_unsupported_inherit_model() {
        let cfg = VTCodeConfig::default();
        let resolved = resolve_subagent_model(&cfg, "claude-haiku-4.5", "copilot", None, "worker")
            .expect("resolve model");
        assert_eq!(
            resolved,
            ModelId::default_orchestrator_for_provider(Provider::Copilot)
        );
    }

    #[test]
    fn resolve_effective_subagent_model_uses_explicit_inherit_override() {
        let cfg = VTCodeConfig::default();
        let resolved = resolve_effective_subagent_model(
            &cfg,
            models::anthropic::CLAUDE_SONNET_4_6,
            "anthropic",
            Some("inherit"),
            Some("haiku"),
            "worker",
        )
        .expect("resolve model");
        assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_SONNET_4_6);
    }

    #[test]
    fn resolve_effective_subagent_model_falls_back_to_parent_on_invalid_override() {
        let cfg = VTCodeConfig::default();
        let resolved = resolve_effective_subagent_model(
            &cfg,
            models::ollama::GPT_OSS_120B_CLOUD,
            "ollama",
            Some("not-a-real-model"),
            None,
            "rust-engineer",
        )
        .expect("resolve model");
        assert_eq!(resolved.as_str(), models::ollama::GPT_OSS_120B_CLOUD);
    }

    #[test]
    fn resolve_subagent_small_model_rejects_cross_provider_configured_lightweight_model() {
        let mut cfg = VTCodeConfig::default();
        cfg.agent.small_model.model = models::anthropic::CLAUDE_HAIKU_4_5.to_string();

        let resolved = resolve_subagent_model(
            &cfg,
            models::openai::GPT_5_4,
            "openai",
            Some("small"),
            "worker",
        )
        .expect("resolve model");

        assert_eq!(resolved, ModelId::GPT54Mini);
    }

    #[test]
    fn resolve_effective_subagent_model_falls_back_to_spec_model_on_invalid_override() {
        let cfg = VTCodeConfig::default();
        let resolved = resolve_effective_subagent_model(
            &cfg,
            models::anthropic::CLAUDE_SONNET_4_6,
            "anthropic",
            Some("not-a-real-model"),
            Some("haiku"),
            "reviewer",
        )
        .expect("resolve model");
        assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_HAIKU_4_5);
    }

    #[test]
    fn background_record_ids_are_stable_and_sanitized() {
        assert_eq!(
            background_record_id("Rust Engineer"),
            "background-Rust-Engineer"
        );
        assert_eq!(
            background_record_id("plugin:reviewer/default"),
            "background-plugin-reviewer-default"
        );
    }

    #[test]
    fn background_subagent_command_includes_expected_flags() {
        let workspace = std::env::current_dir().expect("workspace");
        let command = build_background_subagent_command(
            &workspace,
            "rust-engineer",
            "session-parent",
            "session-child",
            "Inspect the repo",
            Some(7),
            Some("gpt-5.4-mini"),
            Some("high"),
        )
        .expect("background command");

        assert!(command.len() >= 15);
        assert_eq!(command[1], "background-subagent");
        assert!(
            command
                .windows(2)
                .any(|pair| pair == ["--agent-name", "rust-engineer"])
        );
        assert!(
            command
                .windows(2)
                .any(|pair| pair == ["--parent-session-id", "session-parent"])
        );
        assert!(
            command
                .windows(2)
                .any(|pair| pair == ["--session-id", "session-child"])
        );
        assert!(
            command
                .windows(2)
                .any(|pair| pair == ["--prompt", "Inspect the repo"])
        );
        assert!(command.windows(2).any(|pair| pair == ["--max-turns", "7"]));
        assert!(
            command
                .windows(2)
                .any(|pair| pair == ["--model-override", "gpt-5.4-mini"])
        );
        assert!(
            command
                .windows(2)
                .any(|pair| pair == ["--reasoning-override", "high"])
        );
    }

    #[test]
    fn resolve_effective_subagent_model_still_errors_on_invalid_spec_model() {
        let cfg = VTCodeConfig::default();
        let err = resolve_effective_subagent_model(
            &cfg,
            models::anthropic::CLAUDE_SONNET_4_6,
            "anthropic",
            None,
            Some("not-a-real-model"),
            "reviewer",
        )
        .expect_err("invalid spec model should fail");
        assert!(err.to_string().contains("Failed to resolve model"));
    }

    async fn wait_for_effective_model(
        controller: &SubagentController,
        target: &str,
    ) -> Result<String> {
        for _ in 0..50 {
            if let Ok(snapshot) = controller.snapshot_for_thread(target).await {
                return Ok(snapshot.effective_config.agent.default_model);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        Err(anyhow!(
            "Subagent {target} did not capture an effective runtime configuration in time"
        ))
    }

    fn read_only_test_spec(name: &str) -> SubagentSpec {
        SubagentSpec {
            name: name.to_string(),
            description: "test".to_string(),
            prompt: String::new(),
            tools: Some(vec![tools::READ_FILE.to_string()]),
            disallowed_tools: Vec::new(),
            model: None,
            color: None,
            reasoning_effort: None,
            permission_mode: Some(PermissionMode::Plan),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            max_turns: None,
            nickname_candidates: Vec::new(),
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::Builtin,
            file_path: None,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn filter_child_tools_removes_subagent_tools_in_children() {
        let defs = vec![
            ToolDefinition::function(
                tools::SPAWN_AGENT.to_string(),
                "Spawn".to_string(),
                serde_json::json!({"type": "object"}),
            ),
            ToolDefinition::function(
                tools::UNIFIED_SEARCH.to_string(),
                "Search".to_string(),
                serde_json::json!({"type": "object"}),
            ),
            ToolDefinition::function(
                tools::LIST_FILES.to_string(),
                "List".to_string(),
                serde_json::json!({"type": "object"}),
            ),
            ToolDefinition::function(
                tools::REQUEST_USER_INPUT.to_string(),
                "Ask".to_string(),
                serde_json::json!({"type": "object"}),
            ),
        ];
        let spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "explorer")
            .expect("explorer");
        let filtered = filter_child_tools(&spec, defs, true);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].function_name(), tools::UNIFIED_SEARCH);
    }

    #[test]
    fn filter_child_tools_keeps_unified_exec_for_shell_capable_agents() {
        let defs = vec![
            ToolDefinition::function(
                tools::UNIFIED_EXEC.to_string(),
                "Exec".to_string(),
                serde_json::json!({"type": "object"}),
            ),
            ToolDefinition::function(
                tools::UNIFIED_SEARCH.to_string(),
                "Search".to_string(),
                serde_json::json!({"type": "object"}),
            ),
        ];
        let spec = SubagentSpec {
            name: "shell-demo".to_string(),
            description: "test".to_string(),
            prompt: String::new(),
            tools: Some(vec![
                tools::UNIFIED_EXEC.to_string(),
                tools::UNIFIED_SEARCH.to_string(),
            ]),
            disallowed_tools: Vec::new(),
            model: None,
            color: None,
            reasoning_effort: None,
            permission_mode: None,
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            max_turns: None,
            nickname_candidates: Vec::new(),
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::Builtin,
            file_path: None,
            warnings: Vec::new(),
        };

        let filtered = filter_child_tools(&spec, defs, spec.is_read_only());
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].function_name(), tools::UNIFIED_EXEC);
        assert_eq!(filtered[1].function_name(), tools::UNIFIED_SEARCH);
    }

    #[test]
    fn build_child_config_clamps_permissions_and_intersects_allowed_tools() {
        let mut parent = VTCodeConfig::default();
        parent.permissions.default_mode = PermissionMode::Default;
        parent.permissions.allow = vec![
            tools::READ_FILE.to_string(),
            tools::UNIFIED_SEARCH.to_string(),
        ];
        parent.permissions.deny = vec![tools::UNIFIED_EXEC.to_string()];

        let mut spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "worker")
            .expect("worker");
        spec.permission_mode = Some(PermissionMode::BypassPermissions);
        spec.tools = Some(vec![
            tools::SPAWN_AGENT.to_string(),
            tools::UNIFIED_SEARCH.to_string(),
            tools::READ_FILE.to_string(),
        ]);

        let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, None);
        assert_eq!(child.permissions.default_mode, PermissionMode::Default);
        assert_eq!(
            child.permissions.allow,
            vec![
                tools::READ_FILE.to_string(),
                tools::UNIFIED_SEARCH.to_string()
            ]
        );
        assert!(
            child
                .permissions
                .deny
                .contains(&tools::UNIFIED_EXEC.to_string())
        );
        assert!(
            child
                .permissions
                .deny
                .contains(&tools::SPAWN_AGENT.to_string())
        );
    }

    #[test]
    fn build_child_config_preserves_matching_rule_and_exact_tool_ids() {
        let mut parent = VTCodeConfig::default();
        parent.permissions.allow = vec![
            "Read(/docs/**)".to_string(),
            "mcp::context7::search".to_string(),
            tools::READ_FILE.to_string(),
        ];

        let mut spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "worker")
            .expect("worker");
        spec.tools = Some(vec![
            "mcp::context7::search".to_string(),
            tools::UNIFIED_EXEC.to_string(),
            tools::READ_FILE.to_string(),
        ]);

        let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, None);

        assert_eq!(
            child.permissions.allow,
            vec![
                "Read(/docs/**)".to_string(),
                "mcp::context7::search".to_string(),
                tools::READ_FILE.to_string()
            ]
        );
    }

    #[test]
    fn build_child_config_preserves_parent_rule_shaped_allowlist() {
        let mut parent = VTCodeConfig::default();
        parent.permissions.allow = vec!["Read".to_string()];

        let mut spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "worker")
            .expect("worker");
        spec.tools = Some(vec![
            tools::READ_FILE.to_string(),
            tools::UNIFIED_SEARCH.to_string(),
            tools::UNIFIED_EXEC.to_string(),
        ]);

        let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, None);

        assert_eq!(child.permissions.allow, vec!["Read".to_string()]);
    }

    #[test]
    fn build_child_config_promotes_single_turn_budget_to_recovery_budget() {
        let parent = VTCodeConfig::default();
        let spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "worker")
            .expect("worker");

        let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, Some(1));

        assert_eq!(child.automation.full_auto.max_turns, SUBAGENT_MIN_MAX_TURNS);
    }

    #[test]
    fn build_child_config_merges_inline_mcp_provider() {
        let parent = VTCodeConfig::default();
        let mut spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "default")
            .expect("default");
        spec.mcp_servers = vec![SubagentMcpServer::Inline(BTreeMap::from([(
            "playwright".to_string(),
            serde_json::json!({
                "type": "stdio",
                "command": "npx",
                "args": ["-y", "@playwright/mcp@latest"],
            }),
        )]))];

        let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, None);
        let provider = child
            .mcp
            .providers
            .iter()
            .find(|provider| provider.name == "playwright")
            .expect("playwright provider");
        assert_eq!(provider.name, "playwright");
    }

    #[test]
    fn explicit_delegation_request_detects_mentions_and_keywords() {
        let direct_mentions = extract_explicit_agent_mentions("@agent-worker fix the issue", &[]);
        assert!(contains_explicit_delegation_request(
            "@agent-worker fix the issue",
            direct_mentions.as_slice()
        ));
        let no_mentions = extract_explicit_agent_mentions("delegate this in parallel", &[]);
        assert!(contains_explicit_delegation_request(
            "delegate this in parallel",
            no_mentions.as_slice()
        ));
        let empty_mentions = extract_explicit_agent_mentions("review the repository", &[]);
        assert!(!contains_explicit_delegation_request(
            "review the repository",
            empty_mentions.as_slice()
        ));
    }

    #[test]
    fn explicit_agent_mentions_detect_natural_language_selection() {
        let rust_engineer = read_only_test_spec("rust-engineer");
        assert_eq!(
            extract_explicit_agent_mentions(
                "use rust-engineer agent to review current code",
                &[rust_engineer]
            ),
            vec!["rust-engineer".to_string()]
        );
    }

    #[test]
    fn explicit_agent_mentions_detect_looser_subagent_selection() {
        let background_demo = read_only_test_spec("background-demo");
        assert_eq!(
            extract_explicit_agent_mentions(
                "use background-demo and run the subagent",
                &[background_demo]
            ),
            vec!["background-demo".to_string()]
        );
    }

    #[test]
    fn explicit_agent_mentions_detect_run_subagent_selection() {
        let rust_engineer = read_only_test_spec("rust-engineer");
        assert_eq!(
            extract_explicit_agent_mentions(
                "run rust-engineer subagent and review changes",
                &[rust_engineer]
            ),
            vec!["rust-engineer".to_string()]
        );
    }

    #[test]
    fn explicit_model_request_detects_aliases_and_full_ids() {
        assert!(contains_explicit_model_request(
            "delegate this using gpt-5.4-mini",
            "gpt-5.4-mini"
        ));
        assert!(contains_explicit_model_request(
            "use the worker subagent with haiku",
            "haiku"
        ));
        assert!(contains_explicit_model_request(
            "run this with the small model",
            "small"
        ));
        assert!(!contains_explicit_model_request(
            "delegate this small cleanup task",
            "small"
        ));
        assert!(!contains_explicit_model_request(
            "delegate this task",
            "gpt-5.4-mini"
        ));
    }

    #[test]
    fn normalize_requested_model_override_drops_default_like_values() {
        assert_eq!(
            normalize_requested_model_override(Some("default".to_string()), "delegate this task"),
            None
        );
        assert_eq!(
            normalize_requested_model_override(Some(" inherit ".to_string()), "delegate this task"),
            None
        );
        assert_eq!(
            normalize_requested_model_override(
                Some(" inherit ".to_string()),
                "delegate this task using inherit"
            ),
            Some("inherit".to_string())
        );
    }

    #[test]
    fn sanitize_subagent_input_items_drops_empty_fields() {
        let mut items = vec![
            SubagentInputItem {
                item_type: Some("text".to_string()),
                text: Some("  Workspace: /tmp/repo  ".to_string()),
                path: Some(String::new()),
                name: Some(" ".to_string()),
                image_url: None,
            },
            SubagentInputItem {
                item_type: Some("text".to_string()),
                text: Some("   ".to_string()),
                path: Some(String::new()),
                name: None,
                image_url: None,
            },
        ];

        sanitize_subagent_input_items(&mut items);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text.as_deref(), Some("Workspace: /tmp/repo"));
        assert!(items[0].path.is_none());
        assert!(items[0].name.is_none());
    }

    #[tokio::test]
    async fn controller_exposes_builtin_specs() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");
        let specs = controller.effective_specs().await;
        assert!(specs.iter().any(|spec| spec.name == "explorer"));
        assert!(specs.iter().any(|spec| spec.name == "worker"));
    }

    #[tokio::test]
    async fn spawn_defaults_to_single_explicit_mention() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        controller
            .set_turn_delegation_hints_from_input("@agent-explorer inspect the codebase")
            .await;

        let spawned = controller
            .spawn(SpawnAgentRequest {
                message: Some("Inspect the codebase.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");

        assert_eq!(spawned.agent_name, "explorer");
        controller.close(&spawned.id).await.expect("close");
    }

    #[tokio::test]
    async fn spawn_defaults_to_single_natural_language_selection() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        let mentions = controller
            .set_turn_delegation_hints_from_input("use explorer agent to inspect the codebase")
            .await;
        assert_eq!(mentions, vec!["explorer".to_string()]);

        let spawned = controller
            .spawn(SpawnAgentRequest {
                message: Some("Inspect the codebase.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");

        assert_eq!(spawned.agent_name, "explorer");
        controller.close(&spawned.id).await.expect("close");
    }

    #[tokio::test]
    async fn spawn_rejects_mismatched_explicit_mention() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        controller
            .set_turn_delegation_hints_from_input("@agent-explorer inspect the codebase")
            .await;

        let err = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("worker".to_string()),
                message: Some("Implement a change.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect_err("mismatched mention should fail");

        assert!(
            err.to_string()
                .contains("user explicitly selected 'explorer'")
        );
    }

    #[tokio::test]
    async fn spawn_rejects_write_capable_agent_without_explicit_request() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        let err = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("worker".to_string()),
                message: Some("Implement a change.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect_err("write-capable agent should require explicit request");

        assert!(
            err.to_string()
                .contains("cannot launch write-capable agent 'worker'")
        );
    }

    #[tokio::test]
    async fn spawn_rejects_vague_task_even_with_explicit_request() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        controller
            .set_turn_delegation_hints_from_input("run worker subagent and report")
            .await;

        let err = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("worker".to_string()),
                message: Some("report".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect_err("vague task should require clarification");

        assert!(err.to_string().contains("too vague ('report')"));
    }

    #[tokio::test]
    async fn spawn_defaults_to_write_capable_run_subagent_selection() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        let mentions = controller
            .set_turn_delegation_hints_from_input("run worker subagent and implement the change")
            .await;
        assert_eq!(mentions, vec!["worker".to_string()]);

        let spawned = controller
            .spawn(SpawnAgentRequest {
                message: Some("Implement the change.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");

        assert_eq!(spawned.agent_name, "worker");
        controller.close(&spawned.id).await.expect("close");
    }

    #[tokio::test]
    async fn spawn_rejects_read_only_agent_when_auto_delegate_is_disabled() {
        let temp = TempDir::new().expect("tempdir");
        let mut cfg = VTCodeConfig::default();
        cfg.subagents.auto_delegate_read_only = false;
        let controller =
            SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
                .await
                .expect("controller");

        let err = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("explorer".to_string()),
                message: Some("Inspect the repository.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect_err("read-only agent should require explicit delegation");

        assert!(
            err.to_string()
                .contains("cannot proactively launch read-only agent 'explorer'")
        );
    }

    #[test]
    fn load_memory_appendix_renders_compact_summary() {
        let temp = TempDir::new().expect("tempdir");
        let memory_dir = temp.path().join(".vtcode/agent-memory/reviewer");
        std::fs::create_dir_all(&memory_dir).expect("memory dir");
        std::fs::write(
            memory_dir.join("MEMORY.md"),
            "# Reviewer Memory\n\n## Preferences\n- Keep diffs surgical.\n- Run focused tests before broad checks.\n- Prefer repo docs for orientation.\n- Ask only when a decision is materially blocked.\n- Additional long-form notes that should stay out of the prompt body.\n",
        )
        .expect("write memory");

        let appendix =
            load_memory_appendix(temp.path(), "reviewer", Some(SubagentMemoryScope::Project))
                .expect("appendix")
                .expect("memory appendix");

        assert!(appendix.contains("Persistent memory file:"));
        assert!(appendix.contains("Key points:"));
        assert!(appendix.contains("Keep diffs surgical."));
        assert!(appendix.contains("Open `MEMORY.md` when exact wording or more detail matters."));
        assert!(!appendix.contains("Current MEMORY.md excerpt"));
        assert!(!appendix.contains("## Preferences"));
    }

    #[tokio::test]
    async fn spawn_ignores_model_override_without_explicit_user_model_request() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        controller
            .set_turn_delegation_hints_from_input("delegate this task")
            .await;

        let spawned = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("worker".to_string()),
                message: Some("Implement the change.".to_string()),
                model: Some(models::openai::GPT_5_4_MINI.to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");

        let effective_model = wait_for_effective_model(&controller, &spawned.id)
            .await
            .expect("effective model");
        assert_eq!(effective_model, models::openai::GPT_5_4);
        controller.close(&spawned.id).await.expect("close");
    }

    #[tokio::test]
    async fn spawn_honors_model_override_when_user_explicitly_requests_it() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        controller
            .set_turn_delegation_hints_from_input("delegate this task using gpt-5.4-mini")
            .await;

        let spawned = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("worker".to_string()),
                message: Some("Implement the change.".to_string()),
                model: Some(models::openai::GPT_5_4_MINI.to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");

        let effective_model = wait_for_effective_model(&controller, &spawned.id)
            .await
            .expect("effective model");
        assert_eq!(effective_model, models::openai::GPT_5_4_MINI);
        controller.close(&spawned.id).await.expect("close");
    }

    #[tokio::test]
    async fn spawn_captures_runtime_config_before_first_child_turn() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        controller
            .set_turn_delegation_hints_from_input("delegate this task")
            .await;

        let spawned = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("worker".to_string()),
                message: Some("Implement the change.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");

        let snapshot = controller
            .snapshot_for_thread(&spawned.id)
            .await
            .expect("snapshot");

        assert_eq!(snapshot.id, spawned.id);
        assert!(
            !snapshot
                .effective_config
                .agent
                .default_model
                .trim()
                .is_empty()
        );

        controller.close(&spawned.id).await.expect("close");
    }

    #[tokio::test]
    async fn spawn_custom_uses_explicit_spec_without_delegation_hints() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        let mut spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "explorer")
            .expect("explorer");
        spec.name = "init-grounding-explorer".to_string();
        spec.description = "VT Code /init grounding explorer.".to_string();
        spec.source = SubagentSource::ProjectVtcode;

        let spawned = controller
            .spawn_custom(
                spec,
                SpawnAgentRequest {
                    message: Some(
                        "Inspect the repository and report agent-facing findings.".to_string(),
                    ),
                    max_turns: Some(2),
                    ..SpawnAgentRequest::default()
                },
            )
            .await
            .expect("spawn");

        assert_eq!(spawned.agent_name, "init-grounding-explorer");
        assert_eq!(spawned.source, SubagentSource::ProjectVtcode.label());
        controller.close(&spawned.id).await.expect("close");
    }

    #[tokio::test]
    async fn spawn_custom_rejects_write_capable_spec() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        let spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "worker")
            .expect("worker");

        let err = controller
            .spawn_custom(
                spec,
                SpawnAgentRequest {
                    message: Some("Implement a change.".to_string()),
                    ..SpawnAgentRequest::default()
                },
            )
            .await
            .expect_err("write-capable custom spec should be rejected");

        assert!(
            err.to_string()
                .contains("custom subagent spawn only supports read-only specs")
        );
    }

    #[tokio::test]
    async fn close_marks_child_closed() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");
        controller
            .set_turn_delegation_hints_from_input("delegate this task")
            .await;
        let spawned = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("default".to_string()),
                message: Some("Summarize the repository.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");
        let closed = controller.close(&spawned.id).await.expect("close");
        assert_eq!(closed.status, SubagentStatus::Closed);
    }

    #[tokio::test]
    async fn close_is_idempotent_for_closed_agents() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");
        controller
            .set_turn_delegation_hints_from_input("delegate this task")
            .await;
        let spawned = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("default".to_string()),
                message: Some("Summarize the repository.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");

        let closed = controller.close(&spawned.id).await.expect("first close");
        let closed_again = controller.close(&spawned.id).await.expect("second close");

        assert_eq!(closed_again.status, SubagentStatus::Closed);
        assert_eq!(closed_again.updated_at, closed.updated_at);
        assert_eq!(closed_again.completed_at, closed.completed_at);
    }

    #[tokio::test]
    async fn close_and_resume_cascade_through_spawn_tree() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        let spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "explorer")
            .expect("explorer");

        {
            let mut state = controller.state.write().await;
            state.children.insert(
                "parent".to_string(),
                test_child_record("parent", "session-root", &spec, SubagentStatus::Running, 1),
            );
            state.children.insert(
                "child".to_string(),
                test_child_record("child", "parent", &spec, SubagentStatus::Running, 2),
            );
            state.children.insert(
                "grandchild".to_string(),
                test_child_record("grandchild", "child", &spec, SubagentStatus::Running, 3),
            );
        }

        let closed = controller.close("parent").await.expect("close");
        assert_eq!(closed.status, SubagentStatus::Closed);
        assert_eq!(
            controller.status_for("child").await.expect("child").status,
            SubagentStatus::Closed
        );
        assert_eq!(
            controller
                .status_for("grandchild")
                .await
                .expect("grandchild")
                .status,
            SubagentStatus::Closed
        );

        let subtree_ids = controller
            .collect_spawn_subtree_ids("parent")
            .await
            .expect("collect subtree");
        assert_eq!(
            subtree_ids,
            vec![
                "parent".to_string(),
                "child".to_string(),
                "grandchild".to_string()
            ]
        );

        let mut restart_ids = Vec::new();
        for node_id in subtree_ids {
            if controller
                .reopen_single(node_id.as_str())
                .await
                .expect("reopen subtree node")
            {
                restart_ids.push(node_id);
            }
        }

        assert_eq!(
            restart_ids,
            vec![
                "parent".to_string(),
                "child".to_string(),
                "grandchild".to_string()
            ]
        );
        assert_eq!(
            controller
                .status_for("parent")
                .await
                .expect("parent")
                .status,
            SubagentStatus::Queued
        );
        assert_eq!(
            controller.status_for("child").await.expect("child").status,
            SubagentStatus::Queued
        );
        assert_eq!(
            controller
                .status_for("grandchild")
                .await
                .expect("grandchild")
                .status,
            SubagentStatus::Queued
        );
    }

    #[tokio::test]
    async fn spawn_rejects_fourth_active_subagent() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");
        controller
            .set_turn_delegation_hints_from_input("delegate this task")
            .await;

        let spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "explorer")
            .expect("explorer");

        {
            let mut state = controller.state.write().await;
            for idx in 0..SUBAGENT_HARD_CONCURRENCY_LIMIT {
                let id = format!("active-{idx}");
                state.children.insert(
                    id.clone(),
                    ChildRecord {
                        id: id.clone(),
                        session_id: format!("session-{id}"),
                        parent_thread_id: "parent-session".to_string(),
                        spec: spec.clone(),
                        display_label: subagent_display_label(&spec),
                        status: SubagentStatus::Running,
                        background: false,
                        depth: 1,
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                        completed_at: None,
                        summary: None,
                        error: None,
                        archive_metadata: None,
                        archive_path: None,
                        transcript_path: None,
                        effective_config: None,
                        stored_messages: Vec::new(),
                        last_prompt: Some("Inspect the codebase.".to_string()),
                        queued_prompts: VecDeque::new(),
                        thread_handle: None,
                        handle: None,
                        notify: Arc::new(Notify::new()),
                    },
                );
            }
        }

        let err = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("explorer".to_string()),
                message: Some("Inspect another codepath.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect_err("fourth active subagent should be rejected");

        assert!(err.to_string().contains(&format!(
            "Subagent concurrency limit reached (max_concurrent={})",
            SUBAGENT_HARD_CONCURRENCY_LIMIT
        )));
    }

    #[tokio::test]
    async fn wait_returns_first_terminal_child() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");
        let spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "default")
            .expect("default");

        {
            let mut state = controller.state.write().await;
            for id in ["first", "second"] {
                state.children.insert(
                    id.to_string(),
                    ChildRecord {
                        id: id.to_string(),
                        session_id: format!("session-{id}"),
                        parent_thread_id: "parent-session".to_string(),
                        spec: spec.clone(),
                        display_label: subagent_display_label(&spec),
                        status: SubagentStatus::Running,
                        background: false,
                        depth: 1,
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                        completed_at: None,
                        summary: None,
                        error: None,
                        archive_metadata: None,
                        archive_path: None,
                        transcript_path: None,
                        effective_config: None,
                        stored_messages: Vec::new(),
                        last_prompt: None,
                        queued_prompts: VecDeque::new(),
                        thread_handle: None,
                        handle: None,
                        notify: Arc::new(Notify::new()),
                    },
                );
            }
        }

        let controller_clone = controller.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let mut state = controller_clone.state.write().await;
            let record = state.children.get_mut("second").expect("second child");
            record.status = SubagentStatus::Completed;
            record.summary = Some("done".to_string());
            record.completed_at = Some(Utc::now());
            record.updated_at = Utc::now();
            record.notify.notify_waiters();
        });

        let result = controller
            .wait(&["first".to_string(), "second".to_string()], Some(500))
            .await
            .expect("wait result")
            .expect("terminal child");
        assert_eq!(result.id, "second");
        assert_eq!(result.status, SubagentStatus::Completed);
    }
}
