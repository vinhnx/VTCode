use super::*;

impl SubagentController {
    pub(in crate::subagents) async fn background_status_for(&self, target: &str) -> Result<BackgroundSubprocessEntry> {
        let state = self.state.read().await;
        let record = state
            .background_children
            .get(target)
            .ok_or_else(|| anyhow!("Unknown background subprocess {target}"))?;
        Ok(record.build_status_entry())
    }

    pub(in crate::subagents) async fn ensure_background_record_running(
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

    pub(in crate::subagents) async fn refresh_background_archive_metadata(&self, target: &str) -> Result<()> {
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
    pub(in crate::subagents) async fn begin_close(&self) {
        self.closing.store(true, Ordering::Relaxed);
    }

    /// Clears the transient close-in-progress flag. Called when a closed
    /// subtree is reopened so its child-scoped controller can delegate again.
    pub(in crate::subagents) async fn end_close(&self) {
        self.closing.store(false, Ordering::Relaxed);
    }

    /// Signal that the program is shutting down. Subsequent calls to
    /// `save_background_state` will be skipped. All running child handles
    /// are aborted so subagent tasks do not outlive the parent session.
    pub async fn signal_shutdown(&self) {
        if let Err(error) = self.cancel_matrix().await {
            self.matrix.cancellation.read().cancel();
            tracing::warn!(%error, "failed to persist matrix shutdown cancellation");
        }
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

    pub(in crate::subagents) async fn save_background_state(&self) -> Result<()> {
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
}
