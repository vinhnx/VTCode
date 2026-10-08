#![allow(
    unused_imports,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use futures::future::{BoxFuture, select_all};
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

#[allow(
    unused_imports,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::*;

impl SubagentController {
    /// Spawns a new subagent child process from a [`SpawnAgentRequest`].
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
        let spec = self.resolve_requested_spec(delegation.requested_agent.as_deref()).await?;
        let prompt = self.prepare_delegation_prompt(
            &spec,
            &delegation,
            &request.message,
            &request.items,
            "spawn_agent",
            "spawning the subagent",
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

    /// Spawns a background subprocess for a subagent marked `background: true`.
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
        let spec = self.resolve_requested_spec(delegation.requested_agent.as_deref()).await?;
        if !spec.background {
            bail!(
                "spawn_background_subprocess requires an agent with `background: true`; '{}' is a normal delegated child agent. Use spawn_agent instead.",
                spec.name
            );
        }
        let prompt = self.prepare_delegation_prompt(
            &spec,
            &delegation,
            &request.message,
            &request.items,
            "spawn_background_subprocess",
            "launching the background subprocess",
        )?;
        let desired_max_turns = normalize_background_child_max_turns(request.max_turns.or(spec.max_turns), true);
        let desired_model_override = request.model.clone().or_else(|| spec.model.clone());
        let desired_reasoning_override = request
            .reasoning_effort
            .clone()
            .or_else(|| spec.reasoning_effort.as_ref().map(|e| e.as_str().to_string()));

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

    /// Spawns a subagent with a custom [`SubagentSpec`] that must be read-only.
    pub async fn spawn_custom(&self, spec: SubagentSpec, request: SpawnAgentRequest) -> Result<SubagentStatusEntry> {
        if !spec.is_subagent() {
            bail!("custom subagent spawn only supports subagent-capable specs; '{}' is primary-only", spec.name);
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

    /// Sends additional input to a running or queued subagent.
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
            } else if matches!(record.status, SubagentStatus::Running | SubagentStatus::Queued) {
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

    /// Resumes a closed or errored subagent and its descendants by re-queuing
    /// their prompts. Cascades recursively through child-scoped controllers so
    /// grandchildren closed by `close_tree` are resumed too, not merely
    /// un-gated.
    pub async fn resume(&self, target: &str) -> Result<SubagentStatusEntry> {
        self.resume_tree(target).await
    }

    /// Recursively reopens `target` and every descendant across the whole
    /// delegation tree (boxed for async recursion, mirroring `close_tree`).
    fn resume_tree(&self, target: &str) -> BoxFuture<'static, Result<SubagentStatusEntry>> {
        let self_owned = self.clone();
        let target_owned = target.to_string();
        Box::pin(async move {
            if self_owned.shutdown_requested.load(Ordering::Relaxed) {
                bail!("Subagent controller is shutting down; cannot resume subagents");
            }
            let subtree_ids = self_owned.collect_spawn_subtree_ids(&target_owned).await?;
            let mut restart_ids = Vec::new();
            for node_id in subtree_ids.iter() {
                if self_owned.reopen_single(node_id.as_str()).await? {
                    restart_ids.push(node_id.clone());
                }
            }
            // Recursively resume each child-scoped controller's descendants,
            // but only for controllers whose owning record is inside the
            // resumed subtree (mirroring close_tree's sibling isolation).
            // Grandchildren live in the child controller's state, so they are
            // reopened through that controller to keep node ownership correct.
            let nested = self_owned.nested_controllers_in_subtree(&subtree_ids).await;
            for (controller, session_id) in nested {
                let ids = controller.spawn_child_ids_for_parent(&session_id).await;
                for id in ids {
                    if let Err(err) = controller.resume_tree(&id).await {
                        tracing::warn!(node_id = id.as_str(), error = %err, "Failed to resume nested subagent subtree");
                    }
                }
            }
            // Re-check shutdown before launching: a concurrent signal_shutdown
            // between reopen and restart would otherwise start a child after
            // shutdown aborted the subtree.
            if self_owned.shutdown_requested.load(Ordering::Relaxed) {
                bail!("Subagent controller is shutting down; cannot resume subagents");
            }
            for restart_id in restart_ids {
                self_owned.restart_child(&restart_id).await?;
            }
            self_owned.status_for(&target_owned).await
        })
    }

    /// Closes a subagent and all its descendants, aborting any in-flight work.
    pub async fn close(&self, target: &str) -> Result<SubagentStatusEntry> {
        // `close_tree` walks the full nesting tree (including grandchildren
        // spawned through child-scoped controllers) and closes bottom-up.
        self.close_tree(target).await
    }

    /// Closes `target` and every descendant across the whole delegation tree.
    ///
    /// Unlike [`Self::close`] this is recursive over child-scoped controllers,
    /// so it works for arbitrary `max_depth`. The recursion is boxed to satisfy
    /// Rust's async-fn recursion requirement.
    ///
    /// Ordering prevents a race where a still-running child spawns a new
    /// descendant after the descendant snapshot: each child-scoped controller
    /// is marked as closing (which rejects new spawns via `spawn_with_spec`)
    /// before the owning child's handle is aborted and its subtree is closed.
    fn close_tree(&self, target: &str) -> BoxFuture<'static, Result<SubagentStatusEntry>> {
        let self_owned = self.clone();
        let target_owned = target.to_string();
        Box::pin(async move {
            let subtree_ids = self_owned.collect_spawn_subtree_ids(&target_owned).await?;
            let nested = self_owned.nested_controllers_in_subtree(&subtree_ids).await;
            // Mark every child-scoped controller in the subtree as closing so
            // it rejects new grandchild spawns before we start aborting.
            for (controller, _) in &nested {
                controller.begin_close().await;
            }
            // Close the target's own subtree (deepest first) on this
            // controller. Aborting the owning child's handle first stops it
            // from spawning further descendants.
            let subtree_ids_for_rescan = subtree_ids.clone();
            for node_id in subtree_ids.into_iter().rev() {
                self_owned.close_single(node_id.as_str()).await?;
            }
            // ...then recursively close each child-scoped controller's subtree.
            // Grandchildren live in the child controller's state, not ours, so
            // they are closed through that controller to keep node ownership
            // correct. Only controllers of nodes inside the closed subtree are
            // cascaded, so closing one subagent never kills a sibling's
            // descendants.
            for (controller, session_id) in nested {
                let ids = controller.spawn_child_ids_for_parent(&session_id).await;
                for id in ids {
                    if let Err(err) = controller.close_tree(&id).await {
                        tracing::warn!(node_id = id.as_str(), error = %err, "Failed to close nested subagent subtree");
                    }
                }
            }
            // A child may have attached a freshly created controller between
            // the snapshot above and its handle abort. Re-scan and cascade
            // again so no grandchild outlives the close; the second pass is a
            // no-op in the common case because close_tree is idempotent.
            let late = self_owned.nested_controllers_in_subtree(&subtree_ids_for_rescan).await;
            for (controller, session_id) in late {
                controller.begin_close().await;
                let ids = controller.spawn_child_ids_for_parent(&session_id).await;
                for id in ids {
                    if let Err(err) = controller.close_tree(&id).await {
                        tracing::warn!(node_id = id.as_str(), error = %err, "Failed to close late nested subagent subtree");
                    }
                }
            }
            self_owned.status_for(&target_owned).await
        })
    }

    /// Blocks until one of the target subagents reaches a terminal state or the timeout expires.
    pub async fn wait(&self, targets: &[String], timeout_ms: Option<u64>) -> Result<Option<SubagentStatusEntry>> {
        for target in targets {
            if let Ok(entry) = self.status_for(target).await
                && entry.status.is_terminal()
            {
                return Ok(Some(entry));
            }
        }

        let timeout = std::time::Duration::from_millis(
            timeout_ms.unwrap_or_else(|| self.config.vt_cfg.subagents.default_timeout_seconds.saturating_mul(1000)),
        );
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            // Collect notify handles from child records.
            let notifies = {
                let state = self.state.read().await;
                targets
                    .iter()
                    .filter_map(|target| state.children.get(target).map(|record| record.notify.clone()))
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

    /// Returns the current status of a tracked subagent by its target id.
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

    pub(super) async fn collect_spawn_subtree_ids(&self, root_thread_id: &str) -> Result<Vec<String>> {
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

    /// Collects the child-scoped controllers owned by records inside `subtree_ids`,
    /// paired with each owning record's session id. Shared by `resume_tree` and
    /// `close_tree` so subtree isolation cannot diverge between them.
    async fn nested_controllers_in_subtree(&self, subtree_ids: &[String]) -> Vec<(Arc<SubagentController>, String)> {
        let subtree_set = subtree_ids.iter().collect::<std::collections::HashSet<_>>();
        let state = self.state.read().await;
        state
            .children
            .iter()
            .filter(|(id, _)| subtree_set.contains(id))
            .filter_map(|(_, record)| {
                record
                    .child_controller
                    .clone()
                    .map(|controller| (controller, record.session_id.clone()))
            })
            .collect()
    }

    pub(super) async fn reopen_single(&self, target: &str) -> Result<bool> {
        let child_controller = {
            let mut state = self.state.write().await;
            let record = state
                .children
                .get_mut(target)
                .ok_or_else(|| anyhow!("Unknown subagent id {target}"))?;
            if matches!(record.status, SubagentStatus::Running | SubagentStatus::Queued) {
                return Ok(false);
            }
            let prompt = record
                .last_prompt
                .clone()
                .unwrap_or_else(|| "Continue the delegated task from the existing context.".to_string());
            record.status = SubagentStatus::Queued;
            record.updated_at = Utc::now();
            record.completed_at = None;
            record.error = None;
            record.summary = None;
            record.queued_prompts.push_back(prompt);
            record.child_controller.clone()
        };
        // Reopening a subtree reverses the transient `begin_close` on any
        // child-scoped controller so a resumed child can delegate again (and
        // its controller resumes saving background state).
        if let Some(controller) = child_controller {
            controller.end_close().await;
        }
        Ok(true)
    }

    async fn close_single(&self, target: &str) -> Result<SubagentStatusEntry> {
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

    pub(super) async fn background_status_for(&self, target: &str) -> Result<BackgroundSubprocessEntry> {
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
        let (created_at, previous_prompt, previous_max_turns, previous_model_override, previous_reasoning_override) =
            previous_record.unwrap_or((Utc::now(), String::new(), None, None, None));
        let prompt = overrides
            .as_ref()
            .and_then(|overrides| overrides.prompt.clone())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| (!previous_prompt.trim().is_empty()).then_some(previous_prompt))
            .or_else(|| spec.initial_prompt.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                format!(
                    "You are the VT Code background subagent `{}`, started without a specific task. Inspect the workspace at a high level, reply with a short readiness summary (what the project is and what you are set up to do), then end your turn; the process keeps running until it is stopped.",
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
            .or_else(|| spec.reasoning_effort.as_ref().map(|e| e.as_str().to_string()));

        {
            let mut state = self.state.write().await;
            state.background_children.insert(
                record_id.clone(),
                BackgroundRecord {
                    exit_code: None,
                    termination_requested: false,
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
                .create_pty_session_for_managed_background(
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
                    hashbrown::HashMap::new(),
                    false,
                )
                .await
        } else {
            self.config
                .exec_sessions
                .create_pipe_session_for_managed_background(
                    exec_session_id.clone().into(),
                    launch.command,
                    self.config.workspace_root.clone(),
                    hashbrown::HashMap::new(),
                )
                .await
        }
        .with_context(|| format!("Failed to spawn background subprocess for subagent '{}'", spec.name))?;

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
            finalize_background_launch(
                record,
                exec_session_id.as_str(),
                metadata.child_pid,
                metadata.started_at,
                Utc::now(),
            );
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

    /// Marks this controller as closing so [`Self::spawn`] rejects new spawns.
    ///
    /// Used before aborting a subtree so a still-running child cannot spawn a
    /// descendant between the descendant snapshot and the handle abort. Unlike
    /// [`Self::signal_shutdown`] this uses the transient `closing` flag, which
    /// is cleared again when the subtree is reopened, so a resumed child can
    /// delegate and its controller keeps saving background state.
    pub(super) async fn begin_close(&self) {
        self.closing.store(true, Ordering::Relaxed);
    }

    /// Clears the transient close-in-progress flag. Called when a closed
    /// subtree is reopened so its child-scoped controller can delegate again.
    pub(super) async fn end_close(&self) {
        self.closing.store(false, Ordering::Relaxed);
    }

    /// Signal that the program is shutting down. Subsequent calls to
    /// `save_background_state` will be skipped. All running child handles
    /// are aborted so subagent tasks do not outlive the parent session.
    pub async fn signal_shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::Relaxed);
        self.stop_background_completion_monitor().await;
        let nested = {
            let mut state = self.state.write().await;
            let mut nested = Vec::new();
            for record in state.children.values_mut() {
                if let Some(handle) = record.handle.take() {
                    handle.abort();
                }
                record.status = SubagentStatus::Closed;
                record.completed_at = Some(Utc::now());
                record.notify.notify_waiters();
                if let Some(controller) = record.child_controller.clone() {
                    nested.push((controller, record.session_id.clone()));
                }
            }
            nested
        };
        // Mark every child-scoped controller as permanently shut down (so its
        // background state is not saved) and as closing (so a race cannot let
        // a fresh grandchild outlive shutdown), then cascade.
        for (controller, _) in &nested {
            controller.shutdown_requested.store(true, Ordering::Relaxed);
            controller.begin_close().await;
            controller.stop_background_completion_monitor().await;
        }
        // Cascade shutdown to child-scoped controllers so grandchildren tasks
        // are aborted too; otherwise their tokio tasks keep running detached.
        for (controller, session_id) in nested {
            let ids = controller.spawn_child_ids_for_parent(&session_id).await;
            for id in ids {
                if let Err(err) = controller.close_tree(&id).await {
                    tracing::warn!(node_id = id.as_str(), error = %err, "Failed to close nested subagent subtree during shutdown");
                }
            }
        }
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

    pub(super) async fn resolve_requested_spec(&self, requested: Option<&str>) -> Result<SubagentSpec> {
        let requested = requested.unwrap_or("default");
        self.find_spec(requested)
            .await
            .ok_or_else(|| anyhow!("Unknown subagent type {requested}"))
    }

    async fn prepare_delegation_context(
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
        })
    }

    fn prepare_delegation_prompt(
        &self,
        spec: &SubagentSpec,
        delegation: &PreparedDelegationContext,
        message: &Option<String>,
        items: &[SubagentInputItem],
        tool_name: &'static str,
        launch_phrase: &'static str,
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
        if !spec.is_read_only() && !delegation.explicit_request && delegation.requested_agent.is_none() {
            bail!(
                "{} cannot launch write-capable agent '{}' without an explicit delegation signal from the current user turn. Ask the user to mention the agent, say 'delegate'/'spawn', or request parallel work.",
                tool_name,
                spec.name
            );
        }
        if spec.is_read_only() && !self.config.vt_cfg.subagents.auto_delegate_read_only && !delegation.explicit_request
        {
            bail!(
                "{} cannot proactively launch read-only agent '{}' because `subagents.auto_delegate_read_only` is disabled and the current user turn did not explicitly request delegation.",
                tool_name,
                spec.name
            );
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

    fn active_background_launch_conflicts(
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
        if self.shutdown_requested.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed) {
            bail!("Subagent controller is shutting down; cannot spawn new subagents");
        }
        if self.config.depth.saturating_add(1) > self.config.vt_cfg.subagents.max_depth {
            bail!("Subagent depth limit reached (max_depth={})", self.config.vt_cfg.subagents.max_depth);
        }
        if self.config.depth > 0 && spec.isolation == Some(vtcode_config::IsolationMode::Worktree) {
            bail!(
                "Subagent '{}' requests isolation=worktree, but nested worktree isolation is not supported \
                 (child-scoped controllers operate inside the parent's worktree). Use isolation=worktree only \
                 at the root delegation level.",
                spec.name
            );
        }
        // Create a worktree for isolation if requested.
        let worktree_path = if spec.isolation == Some(vtcode_config::IsolationMode::Worktree) {
            let workspace_root = self.config.workspace_root.clone();
            let worktree_name =
                format!("{}-{}", sanitize_component(spec.name.as_str()), Utc::now().format("%Y%m%dT%H%M%S"));
            let worktree_name_for_error = worktree_name.clone();
            let worktree_result = tokio::task::spawn_blocking(move || {
                crate::git::WorktreeManager::new(workspace_root).create(&worktree_name)
            })
            .await
            .context("Worktree creation task panicked")?;
            Some(
                worktree_result
                    .with_context(|| format!("Failed to create worktree for subagent '{}'", worktree_name_for_error))?,
            )
        } else {
            None
        };

        let active_count = {
            let state = self.state.read().await;
            state
                .children
                .values()
                .filter(|record| {
                    matches!(record.status, SubagentStatus::Queued | SubagentStatus::Running | SubagentStatus::Waiting)
                })
                .count()
        };
        let effective_max_concurrent = self.config.vt_cfg.subagents.max_concurrent.min(SUBAGENT_HARD_CONCURRENCY_LIMIT);
        if active_count >= effective_max_concurrent {
            bail!("Subagent concurrency limit reached (max_concurrent={effective_max_concurrent})");
        }
        let is_background_child = background;
        let child_max_turns = normalize_background_child_max_turns(max_turns.or(spec.max_turns), is_background_child);
        let (_, _, effective_config) = prepare_child_runtime_config(
            &self.config.vt_cfg,
            &spec,
            self.config.parent_model.as_str(),
            self.config.parent_provider.as_str(),
            self.config.parent_reasoning_effort,
            child_max_turns,
            model_override.as_deref(),
            reasoning_override.as_deref(),
            !spec.is_read_only() && self.config.depth.saturating_add(2) <= self.config.vt_cfg.subagents.max_depth,
            resolve_effective_subagent_model,
        )?;

        let id = format!("agent-{}-{}", sanitize_component(spec.name.as_str()), Utc::now().format("%Y%m%dT%H%M%S%3fZ"));
        let parent_session_id = self.parent_session_id.read().await.clone();
        let session_id =
            format!("{}-{}", sanitize_component(parent_session_id.as_str()), sanitize_component(id.as_str()));
        let display_label = subagent_display_label(&spec);
        let notify = Arc::new(Notify::new());
        let mut state = self.state.write().await;
        // Re-check the close gates while the write lock is held. The earlier
        // check can race with a concurrent `close_tree` that snapshots and
        // closes the subtree between this spawn's admission and its record
        // insertion; checking after acquisition closes that window.
        if self.shutdown_requested.load(Ordering::Relaxed) || self.closing.load(Ordering::Relaxed) {
            bail!("Subagent controller is shutting down; cannot spawn new subagents");
        }
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
            child_controller: None,
        };
        state.children.insert(id.clone(), entry);
        drop(state);

        self.launch_child(id.as_str()).await?;
        self.status_for(&id).await
    }

    async fn restart_child(&self, target: &str) -> Result<()> {
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

pub(super) fn finalize_background_launch(
    record: &mut BackgroundRecord,
    expected_exec_session_id: &str,
    child_pid: Option<u32>,
    started_at: Option<chrono::DateTime<Utc>>,
    updated_at: chrono::DateTime<Utc>,
) -> bool {
    if record.exec_session_id != expected_exec_session_id
        || !matches!(record.status, BackgroundSubprocessStatus::Starting)
    {
        return false;
    }
    record.pid = child_pid;
    record.started_at = started_at;
    record.status = BackgroundSubprocessStatus::Running;
    record.updated_at = updated_at;
    record.ended_at = None;
    record.error = None;
    record.summary = Some("Background subagent is running".to_string());
    true
}
