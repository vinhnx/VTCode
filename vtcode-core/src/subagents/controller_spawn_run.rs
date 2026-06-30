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
    pub async fn spawn(&self, request: SpawnAgentRequest) -> Result<SubagentStatusEntry> {
        let mut request = request;
        let delegation = self
            .prepare_delegation_context(
                request.agent_type.clone(),
                &mut request.items,
                &mut request.model,
                "spawn_agent",
            )
            .await?;
        let spec = self
            .resolve_requested_spec(delegation.requested_agent.as_deref())
            .await?;
        let prompt = self.prepare_delegation_prompt(
            &spec,
            &delegation,
            &mut request.model,
            &request.message,
            &request.items,
            "spawn_agent",
            "spawning the subagent",
            "Ignoring subagent model override because the current user turn did not explicitly request that model",
        )?;
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

    pub async fn spawn_background_subprocess(
        &self,
        request: SpawnBackgroundSubprocessRequest,
    ) -> Result<BackgroundSubprocessEntry> {
        if self.config.managed_background_runtime {
            bail!("managed background subprocesses cannot launch nested background subprocesses");
        }
        if !self.config.vt_cfg.subagents.background.enabled {
            bail!("Background subagents are disabled by configuration");
        }

        let mut request = request;
        let delegation = self
            .prepare_delegation_context(
                request.agent_type.clone(),
                &mut request.items,
                &mut request.model,
                "spawn_background_subprocess",
            )
            .await?;
        let spec = self
            .resolve_requested_spec(delegation.requested_agent.as_deref())
            .await?;
        if !spec.background {
            bail!(
                "spawn_background_subprocess requires an agent with `background: true`; '{}' is a normal delegated child agent. Use spawn_agent instead.",
                spec.name
            );
        }
        let prompt = self.prepare_delegation_prompt(
            &spec,
            &delegation,
            &mut request.model,
            &request.message,
            &request.items,
            "spawn_background_subprocess",
            "launching the background subprocess",
            "Ignoring background subprocess model override because the current user turn did not explicitly request that model",
        )?;
        let desired_max_turns =
            normalize_background_child_max_turns(request.max_turns.or(spec.max_turns), true);
        let desired_model_override = request.model.clone().or_else(|| spec.model.clone());
        let desired_reasoning_override = request.reasoning_effort.clone().or_else(|| {
            spec.reasoning_effort
                .as_ref()
                .map(|e| e.as_str().to_string())
        });

        let record_id = background_record_id(spec.name.as_str());
        let _ = self.refresh_background_processes().await?;
        {
            let state = self.state.read().await;
            if let Some(record) = state.background_children.get(&record_id)
                && record.desired_enabled
                && record.status.is_active()
            {
                let conflicts = Self::active_background_launch_conflicts(
                    record,
                    prompt.as_str(),
                    desired_max_turns,
                    desired_model_override.as_deref(),
                    desired_reasoning_override.as_deref(),
                );
                if !conflicts.is_empty() {
                    bail!(
                        "spawn_background_subprocess found active background subprocess '{}' with different {}. Stop or restart the existing subprocess before changing its launch settings.",
                        spec.name,
                        conflicts.join(", "),
                    );
                }
                return Ok(record.build_status_entry());
            }
        }

        self.ensure_background_record_running(
            spec.name.as_str(),
            Some(record_id.as_str()),
            0,
            Some(BackgroundLaunchOverrides {
                prompt: Some(prompt),
                max_turns: request.max_turns,
                model_override: request.model,
                reasoning_override: request.reasoning_effort,
            }),
        )
        .await
    }

    pub async fn spawn_custom(
        &self,
        spec: SubagentSpec,
        request: SpawnAgentRequest,
    ) -> Result<SubagentStatusEntry> {
        if !spec.is_subagent() {
            bail!(
                "custom subagent spawn only supports subagent-capable specs; '{}' is primary-only",
                spec.name
            );
        }

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
            // Collect notify handles from child records.
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

            // Register notified() futures BEFORE checking terminal status.
            // This prevents a Tokio Notify race condition: if apply_result()
            // calls notify_waiters() between a status check and future
            // creation, the notification is permanently lost. By registering
            // futures first, any concurrent notification either:
            //   (a) arrives before we poll the future → stored as a permit,
            //       select! returns immediately, loop re-checks status, or
            //   (b) arrives after we start waiting → wakes the future normally.
            let wait_any = select_all(
                notifies
                    .into_iter()
                    .map(|notify| Box::pin(async move { notify.notified().await }))
                    .collect::<Vec<_>>(),
            );
            tokio::pin!(wait_any);

            // Now check if any target is already terminal.
            for target in targets {
                if let Ok(entry) = self.status_for(target).await
                    && entry.status.is_terminal()
                {
                    return Ok(Some(entry));
                }
            }

            let sleep = tokio::time::sleep_until(deadline);
            tokio::pin!(sleep);

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
            .ok_or_else(|| anyhow!("Unknown subagent id {target}"))?;
        Ok(record.build_status_entry())
    }

    pub(super) async fn spawn_child_ids_for_parent(&self, parent_thread_id: &str) -> Vec<String> {
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

    pub(super) async fn collect_spawn_subtree_ids(
        &self,
        root_thread_id: &str,
    ) -> Result<Vec<String>> {
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

    pub(super) async fn reopen_single(&self, target: &str) -> Result<bool> {
        let mut state = self.state.write().await;
        let record = state
            .children
            .get_mut(target)
            .ok_or_else(|| anyhow!("Unknown subagent id {target}"))?;
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

    pub(super) async fn close_single(&self, target: &str) -> Result<SubagentStatusEntry> {
        let mut state = self.state.write().await;
        let record = state
            .children
            .get_mut(target)
            .ok_or_else(|| anyhow!("Unknown subagent id {target}"))?;
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

    pub(super) async fn background_status_for(
        &self,
        target: &str,
    ) -> Result<BackgroundSubprocessEntry> {
        let state = self.state.read().await;
        let record = state
            .background_children
            .get(target)
            .ok_or_else(|| anyhow!("Unknown background subprocess {target}"))?;
        Ok(record.build_status_entry())
    }

    pub(super) async fn ensure_background_record_running(
        &self,
        agent_name: &str,
        stable_id: Option<&str>,
        restart_attempts: u8,
        overrides: Option<BackgroundLaunchOverrides>,
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
        let prompt = overrides
            .as_ref()
            .and_then(|overrides| overrides.prompt.clone())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| (!previous_prompt.trim().is_empty()).then_some(previous_prompt))
            .or_else(|| spec.initial_prompt.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                format!(
                    "You are the VT Code background subagent `{}`. Summarize readiness briefly, inspect the workspace at a high level, then remain idle until the process is terminated.",
                    spec.name
                )
            });
        let max_turns = normalize_background_child_max_turns(
            overrides
                .as_ref()
                .and_then(|overrides| overrides.max_turns)
                .or(previous_max_turns)
                .or(spec.max_turns),
            true,
        );
        let model_override = overrides
            .as_ref()
            .and_then(|overrides| overrides.model_override.clone())
            .or(previous_model_override)
            .or_else(|| spec.model.clone());
        let reasoning_override = overrides
            .as_ref()
            .and_then(|overrides| overrides.reasoning_override.clone())
            .or(previous_reasoning_override)
            .or_else(|| {
                spec.reasoning_effort
                    .as_ref()
                    .map(|e| e.as_str().to_string())
            });

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

        let launch = build_background_launch_spec(
            &self.config.workspace_root,
            spec.name.as_str(),
            parent_session_id.as_str(),
            session_id.as_str(),
            prompt.as_str(),
            max_turns,
            model_override.as_deref(),
            reasoning_override.as_deref(),
        )?;
        let metadata = if launch.use_pty {
            self.config
                .exec_sessions
                .create_pty_session(
                    exec_session_id.clone().into(),
                    launch.command,
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
        } else {
            self.config
                .exec_sessions
                .create_pipe_session(
                    exec_session_id.clone().into(),
                    launch.command,
                    self.config.workspace_root.clone(),
                    hashbrown::HashMap::new(),
                )
                .await
        }
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
                .ok_or_else(|| anyhow!("Unknown background subprocess {record_id}"))?;
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

    pub(super) async fn refresh_background_archive_metadata(&self, target: &str) -> Result<()> {
        let session_id = {
            let state = self.state.read().await;
            state
                .background_children
                .get(target)
                .map(|record| record.session_id.clone())
                .ok_or_else(|| anyhow!("Unknown background subprocess {target}"))?
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

    /// Signal that the program is shutting down. Subsequent calls to
    /// `save_background_state` will be skipped.
    pub fn signal_shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::Relaxed);
    }

    pub(super) async fn save_background_state(&self) -> Result<()> {
        if self.shutdown_requested.load(Ordering::Relaxed) {
            return Ok(());
        }
        let records = {
            let state = self.state.read().await;
            state
                .background_children
                .values()
                .cloned()
                .map(BackgroundRecord::into_persisted)
                .collect()
        };
        persist_background_state(&self.config.workspace_root, records).await
    }

    pub(super) async fn find_spec(&self, candidate: &str) -> Option<SubagentSpec> {
        self.state
            .read()
            .await
            .discovered
            .effective
            .iter()
            .find(|spec| spec.is_subagent() && spec.matches_name(candidate))
            .cloned()
    }

    pub(super) async fn resolve_requested_spec(
        &self,
        requested: Option<&str>,
    ) -> Result<SubagentSpec> {
        let requested = requested.unwrap_or("default");
        self.find_spec(requested)
            .await
            .ok_or_else(|| anyhow!("Unknown subagent type {requested}"))
    }

    pub(super) async fn prepare_delegation_context(
        &self,
        requested_agent: Option<String>,
        items: &mut Vec<SubagentInputItem>,
        model: &mut Option<String>,
        tool_name: &'static str,
    ) -> Result<PreparedDelegationContext> {
        let state = self.state.read().await;
        sanitize_subagent_input_items(items);
        *model = normalize_requested_model_override(model.take(), &state.turn_hints.current_input);
        let requested_agent = if let Some(agent_type) = requested_agent {
            Some(agent_type)
        } else {
            match state.turn_hints.explicit_mentions.as_slice() {
                [] => None,
                [single] => Some(single.clone()),
                mentions => {
                    bail!(
                        "{} omitted agent_type, but the user explicitly selected multiple agents: {}. Specify agent_type explicitly.",
                        tool_name,
                        mentions.join(", ")
                    );
                }
            }
        };
        Ok(PreparedDelegationContext {
            requested_agent,
            explicit_mentions: state.turn_hints.explicit_mentions.clone(),
            explicit_request: state.turn_hints.explicit_request,
            current_input: state.turn_hints.current_input.clone(),
        })
    }

    pub(super) fn prepare_delegation_prompt(
        &self,
        spec: &SubagentSpec,
        delegation: &PreparedDelegationContext,
        model: &mut Option<String>,
        message: &Option<String>,
        items: &[SubagentInputItem],
        tool_name: &'static str,
        launch_phrase: &'static str,
        ignored_model_warning: &'static str,
    ) -> Result<String> {
        if let Some(explicit) = delegation.explicit_mentions.first()
            && delegation.explicit_mentions.len() == 1
            && !spec.matches_name(explicit)
        {
            bail!(
                "{} requested agent_type '{}', but the user explicitly selected '{}'. Use the selected agent or ask the user to clarify.",
                tool_name,
                spec.name,
                explicit
            );
        }
        if !spec.is_read_only()
            && !delegation.explicit_request
            && delegation.requested_agent.is_none()
        {
            bail!(
                "{} cannot launch write-capable agent '{}' without an explicit delegation signal from the current user turn. Ask the user to mention the agent, say 'delegate'/'spawn', or request parallel work.",
                tool_name,
                spec.name
            );
        }
        if spec.is_read_only()
            && !self.config.vt_cfg.subagents.auto_delegate_read_only
            && !delegation.explicit_request
        {
            bail!(
                "{} cannot proactively launch read-only agent '{}' because `subagents.auto_delegate_read_only` is disabled and the current user turn did not explicitly request delegation.",
                tool_name,
                spec.name
            );
        }
        if let Some(requested_model) = model.as_deref()
            && !contains_explicit_model_request(&delegation.current_input, requested_model)
        {
            tracing::warn!(
                agent_name = spec.name.as_str(),
                requested_model = requested_model.trim(),
                "{ignored_model_warning}"
            );
            *model = None;
        }
        let prompt = request_prompt(message, items)
            .or_else(|| spec.initial_prompt.clone())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("{tool_name} requires a task message or items"))?;
        if delegated_task_requires_clarification(&prompt) {
            bail!(
                "{} task for '{}' is too vague ('{}'). Ask the user for a specific delegated task before {}.",
                tool_name,
                spec.name,
                prompt.trim(),
                launch_phrase
            );
        }
        Ok(prompt)
    }

    pub(super) fn active_background_launch_conflicts(
        record: &BackgroundRecord,
        prompt: &str,
        max_turns: Option<usize>,
        model_override: Option<&str>,
        reasoning_override: Option<&str>,
    ) -> Vec<&'static str> {
        let mut conflicts = Vec::new();
        if record.prompt != prompt {
            conflicts.push("prompt");
        }
        if record.max_turns != max_turns {
            conflicts.push("max_turns");
        }
        if record.model_override.as_deref() != model_override {
            conflicts.push("model");
        }
        if record.reasoning_override.as_deref() != reasoning_override {
            conflicts.push("reasoning_effort");
        }
        conflicts
    }

    pub(super) async fn spawn_with_spec(
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
        // Create a worktree for isolation if requested.
        let worktree_path = if spec.isolation == Some(vtcode_config::IsolationMode::Worktree) {
            let wm = crate::git::WorktreeManager::new(&self.config.workspace_root);
            let wt_name = format!(
                "{}-{}",
                sanitize_component(spec.name.as_str()),
                Utc::now().format("%Y%m%dT%H%M%S")
            );
            Some(wm.create(&wt_name).with_context(|| {
                format!("Failed to create worktree for subagent '{}'", spec.name)
            })?)
        } else {
            None
        };

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
            bail!("Subagent concurrency limit reached (max_concurrent={effective_max_concurrent})");
        }
        let is_background_child = background;
        let child_max_turns =
            normalize_background_child_max_turns(max_turns.or(spec.max_turns), is_background_child);
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
            background: is_background_child,
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
            queued_prompts: VecDeque::from([prompt]),
            max_turns: child_max_turns,
            model_override,
            reasoning_override,
            thread_handle: None,
            handle: None,
            notify,
            worktree_path,
        };
        state.children.insert(id.clone(), entry);
        drop(state);

        self.launch_child(id.as_str()).await?;
        self.status_for(&id).await
    }

    pub(super) async fn restart_child(&self, target: &str) -> Result<()> {
        let has_queued_input = {
            let mut state = self.state.write().await;
            let record = state
                .children
                .get_mut(target)
                .ok_or_else(|| anyhow!("Unknown subagent id {target}"))?;
            if record.queued_prompts.is_empty()
                && let Some(prompt) = record.last_prompt.clone()
            {
                record.queued_prompts.push_back(prompt);
            }
            !record.queued_prompts.is_empty()
        };
        if !has_queued_input {
            bail!("Subagent {target} has no queued input");
        }
        self.launch_child(target).await
    }
}
