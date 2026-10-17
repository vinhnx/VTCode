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
    pub(super) async fn launch_child(&self, child_id: &str) -> Result<()> {
        let permit = Arc::clone(&self.admission)
            .try_acquire_owned()
            .context("Subagent concurrency limit reached")?;
        self.launch_child_reserved(child_id, permit).await
    }

    pub(super) async fn launch_child_reserved(
        &self,
        child_id: &str,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<()> {
        self.ensure_ordinary_delegation_allowed()?;
        // Record the handle under the same lock as admission so concurrent
        // shutdown cannot miss a newly launched child.
        let mut state = self.state.write().await;
        anyhow::ensure!(
            !self.shutdown_requested.load(Ordering::Acquire) && !self.closing.load(Ordering::Acquire),
            "Subagent controller is shutting down"
        );
        let record = state
            .children
            .get_mut(child_id)
            .ok_or_else(|| anyhow!("Unknown subagent id {child_id}"))?;
        anyhow::ensure!(
            record.handle.as_ref().is_none_or(|handle| handle.is_finished()),
            "Subagent already has a running launch"
        );
        record.status = SubagentStatus::Queued;
        record.updated_at = Utc::now();

        // Install the task handle before releasing the admission lock.
        let controller = self.clone();
        let target = child_id.to_string();
        let handle = tokio::spawn(async move {
            let _permit = permit;
            Box::pin(controller.child_loop(&target)).await;

            // After child_loop completes, reconcile worktree if needed.
            // This runs on the owned controller clone so it does not affect
            // the Send-ness of child_loop's future.
            let worktree_info = {
                let state = controller.state.read().await;
                state
                    .children
                    .get(&target)
                    .and_then(|record| record.worktree_path.as_ref().map(|p| (p.clone(), record.spec.name.clone())))
            };

            if let Some((wt_path, wt_name)) = worktree_info
                && controller.config.vt_cfg.automation.loop_engine.reconcile_on_complete
            {
                controller.run_worktree_reconciliation(&target, &wt_path, &wt_name).await;
            }
        });

        record.handle = Some(handle);
        Ok(())
    }

    async fn child_loop(&self, child_id: &str) {
        loop {
            let request = {
                let mut state = self.state.write().await;
                let Some(record) = state.children.get_mut(child_id) else {
                    return;
                };
                record.dequeue_run()
            };
            let Some(request) = request else {
                let mut state = self.state.write().await;
                if let Some(record) = state.children.get_mut(child_id) {
                    record.updated_at = Utc::now();
                }
                return;
            };

            let execute = Box::pin(self.run_child_once(
                child_id,
                request.prompt,
                request.max_turns,
                request.model_override,
                request.reasoning_override,
            ))
            .await;

            let (has_more_work, hook_payload) = {
                let mut state = self.state.write().await;
                let Some(record) = state.children.get_mut(child_id) else {
                    return;
                };
                record.updated_at = Utc::now();
                let has_more_work = record.apply_result(execute);
                let hook_payload = (!has_more_work).then(|| record.build_hook_payload());
                (has_more_work, hook_payload)
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

            if has_more_work {
                continue;
            }

            // The child is terminal and no queued work remains. Tear down its
            // child-scoped controller so any running grandchildren are aborted
            // instead of continuing detached after the child reports done.
            let nested = {
                let state = self.state.read().await;
                state.children.get(child_id).and_then(|record| {
                    record
                        .child_controller
                        .clone()
                        .map(|controller| (controller, record.session_id.clone()))
                })
            };
            if let Some((controller, session_id)) = nested {
                let ids = controller.spawn_child_ids_for_parent(&session_id).await;
                for id in ids {
                    if let Err(err) = controller.close(&id).await {
                        tracing::warn!(child_id, node_id = id.as_str(), error = %err, "Failed to close nested subagent subtree on child completion");
                    }
                }
            }

            {
                let mut state = self.state.write().await;
                if let Some(record) = state.children.get_mut(child_id) {
                    record.updated_at = Utc::now();
                }
            }
            return;
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
        let (spec, session_id, bootstrap_messages, display_label, background, worktree_path, existing_child_controller) = {
            let mut state = self.state.write().await;
            let record = state
                .children
                .get_mut(child_id)
                .ok_or_else(|| anyhow!("Unknown subagent id {child_id}"))?;
            record.status = SubagentStatus::Running;
            record.updated_at = Utc::now();
            (
                record.spec.clone(),
                record.session_id.clone(),
                record.stored_messages.clone(),
                record.display_label.clone(),
                record.background,
                record.worktree_path.clone(),
                record.child_controller.clone(),
            )
        };

        // Use the worktree path as the effective workspace root if the
        // subagent was spawned with isolation=worktree.
        let effective_workspace = worktree_path.as_deref().unwrap_or(&self.config.workspace_root);

        // This child may delegate further only when a grandchild (depth + 2)
        // still fits inside `subagents.max_depth` AND the child is write-capable.
        // A read-only child never receives delegation tools, so it also needs
        // no child-scoped controller and must keep the delegation tools denied.
        let allow_nested_delegation =
            !spec.is_read_only() && self.config.depth.saturating_add(2) <= self.config.vt_cfg.subagents.max_depth;

        let (resolved_model, child_reasoning_effort, child_cfg) = prepare_child_runtime_config(
            &self.config.vt_cfg,
            &spec,
            self.config.parent_model.as_str(),
            self.config.parent_provider.as_str(),
            self.config.parent_reasoning_effort,
            max_turns,
            model_override.as_deref(),
            reasoning_override.as_deref(),
            allow_nested_delegation,
            resolve_effective_subagent_model,
        )?;
        let parent_session_id = self.parent_session_id.read().await.clone();

        let archive_metadata = build_subagent_archive_metadata(
            effective_workspace,
            child_cfg.agent.default_model.as_str(),
            child_cfg.agent.provider.as_str(),
            child_cfg.agent.theme.as_str(),
            child_reasoning_effort.as_str(),
            parent_session_id.as_str(),
            !bootstrap_messages.is_empty(),
        );
        let bootstrap = ThreadBootstrap::new(Some(archive_metadata.clone())).with_messages(bootstrap_messages.clone());
        let archive = if let Some(listing) = find_session_by_identifier(&session_id).await? {
            SessionArchive::resume_from_listing(&listing, archive_metadata.clone())
        } else {
            SessionArchive::new_with_identifier(archive_metadata.clone(), session_id.clone()).await?
        };
        checkpoint_subagent_archive_start(&archive, &bootstrap_messages).await?;
        let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
            agent_type_for_spec(&spec),
            resolved_model,
            self.config.api_key.clone(),
            effective_workspace.to_path_buf(),
            session_id.clone(),
            RunnerSettings {
                reasoning_effort: Some(child_reasoning_effort),
                verbosity: None,
            },
            None,
            bootstrap,
            Some(child_cfg.clone()),
            self.config.openai_chatgpt_auth.clone(),
        ))
        .await?;
        runner.set_quiet(true);
        runner.set_subagent_mode(true);
        // When this child may delegate further, attach a child-scoped
        // controller so the subagent-lifecycle tools surface in its catalog
        // and grandchild spawns inherit the incremented depth. The controller
        // is created once and reused across resumes so grandchildren remain
        // reachable for the child's lifetime. It is created with
        // `managed_background_runtime: true` so the unified `agent` tool cannot
        // launch background subprocesses from a child.
        let child_controller = if allow_nested_delegation {
            match existing_child_controller {
                Some(controller) => Some(controller),
                None => {
                    let nested_config = SubagentControllerConfig {
                        workspace_root: effective_workspace.to_path_buf(),
                        parent_session_id: session_id.clone(),
                        parent_model: child_cfg.agent.default_model.clone(),
                        parent_provider: child_cfg.agent.provider.clone(),
                        parent_reasoning_effort: child_reasoning_effort,
                        api_key: self.config.api_key.clone(),
                        vt_cfg: child_cfg.clone(),
                        openai_chatgpt_auth: self.config.openai_chatgpt_auth.clone(),
                        depth: self.config.depth.saturating_add(1),
                        workspace_gated: self.config.workspace_gated,
                        exec_sessions: self.config.exec_sessions.clone(),
                        pty_manager: self.config.pty_manager.clone(),
                        managed_background_runtime: true,
                    };
                    match SubagentController::new(nested_config).await {
                        Ok(controller) => Some(std::sync::Arc::new(controller)),
                        Err(err) => {
                            // Fail closed: without a controller the child keeps
                            // the current non-nested toolset and cannot delegate.
                            tracing::warn!(
                                child_id,
                                error = %err,
                                "Failed to create nested subagent controller; child delegation disabled"
                            );
                            None
                        }
                    }
                }
            }
        } else {
            None
        };
        // Refresh parent context on both a newly created and a reused
        // controller so a resumed child's grandchildren fork from the latest
        // bootstrap messages.
        if let Some(controller) = child_controller.as_ref() {
            controller.set_parent_messages(&bootstrap_messages).await;
            runner.set_subagent_controller(controller.clone());
        }
        let thread_handle = runner.thread_handle();
        let archive_path = archive.path().to_path_buf();

        {
            let mut state = self.state.write().await;
            let record = state
                .children
                .get_mut(child_id)
                .ok_or_else(|| anyhow!("Unknown subagent id {child_id}"))?;
            record.archive_metadata = Some(archive_metadata.clone());
            record.archive_path = Some(archive_path.clone());
            record.effective_config = Some(child_cfg.clone());
            record.thread_handle = Some(thread_handle.clone());
            if let Some(controller) = child_controller.clone() {
                record.child_controller = Some(controller);
            }
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

        // Fail closed on tool exposure: only expose the delegation tools when
        // the child-scoped controller is actually attached. If controller
        // creation failed the child keeps the non-nested toolset even though
        // its config deny-list may omit the delegation tools.
        let nested_tools_enabled = allow_nested_delegation && child_controller.is_some();
        let filtered_tools =
            filter_child_tools(&spec, runner.build_universal_tools().await?, spec.is_read_only(), nested_tools_enabled);
        let allowed_tools = filtered_tools
            .iter()
            .map(|tool| tool.function_name().to_string())
            .collect::<Vec<_>>();
        runner.set_tool_definitions_override(filtered_tools);
        runner.enable_full_auto(&allowed_tools).await;

        let memory_appendix =
            load_memory_appendix_async(&self.config.workspace_root, spec.name.as_str(), spec.memory).await?;
        let mut task = Task::new(format!("subagent-{}", spec.name), format!("Subagent {}", spec.name), prompt);
        task.instructions = Some(compose_subagent_instructions(&spec, memory_appendix));

        let results = Box::pin(runner.execute_task(&task, &[])).await?;
        let messages = runner.session_messages();
        let transcript_path = persist_child_archive(&archive, &messages, spec.name.as_str()).await?;

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
