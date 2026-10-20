//! Bounded session-exit work. The caller emits the terminal event before entering this phase.
use super::super::metrics::capture_code_change_snapshot;
use super::session_bootstrap::EXIT_BACKGROUND_SHUTDOWN_TIMEOUT;
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::git::FileStat;
use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::welcome::SessionBootstrap;
use hashbrown::HashMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, timeout};
use vtcode_core::core::agent::snapshots::{SnapshotManager, SnapshotTurnDiagnostics};
use vtcode_core::core::threads::ThreadRuntimeHandle;
use vtcode_core::exec::events::ThreadCompletionSubtype;
use vtcode_core::hooks::{LifecycleHookEngine, SessionEndReason};
use vtcode_core::llm::provider as uni;
use vtcode_core::primary_agent::ActivePrimaryAgentState;
use vtcode_core::skills::types::Skill;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::session_archive::{self, SessionArchive};
use vtcode_ui::tui::app::{InlineHandle, InlineSession};

use super::*;

/// Tight cap for user-driven teardown (exit/cancel/`/new`): the shell return
/// or fresh prompt is waiting, so background shutdown must not park it.
/// Normal completion keeps the full 2s budget.
const FAST_BACKGROUND_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(750);

fn is_fast_teardown(reason: SessionEndReason) -> bool {
    matches!(reason, SessionEndReason::Exit | SessionEndReason::Cancelled | SessionEndReason::NewSession)
}

pub(super) struct SessionTeardownContext<'a> {
    pub harness_emitter: Option<&'a HarnessEventEmitter>,
    pub checkpoint_manager: Option<&'a SnapshotManager>,
    pub tool_registry: &'a ToolRegistry,
    pub workspace: &'a Path,
    pub session_stats: &'a SessionStats,
    pub subtype: ThreadCompletionSubtype,
    pub session_end_reason: SessionEndReason,
}

#[derive(Default)]
pub(super) struct SessionTeardownOutput {
    pub harness_finish_error: Option<anyhow::Error>,
    pub end_code_changes: Option<HashMap<PathBuf, FileStat>>,
}

/// Publish a branch's result before another branch can exhaust the shared deadline.
pub(super) async fn retain_completed_result<T>(slot: &mut T, work: impl Future<Output = T>) {
    *slot = work.await;
}

pub(super) async fn drain_session_teardown(context: SessionTeardownContext<'_>, output: &mut SessionTeardownOutput) {
    let SessionTeardownContext {
        harness_emitter,
        checkpoint_manager,
        tool_registry,
        workspace,
        session_stats,
        subtype,
        session_end_reason,
    } = context;
    // Independent, individually-bounded teardown steps run concurrently:
    // their worst-case caps overlap (max) instead of adding (sum) the way
    // the previous sequential chain did. Every await carries a timeout; a
    // timeout only skips waiting (the OS reaps children at process exit),
    // it never leaks the terminal.
    // `/new` skips the end code-change snapshot: the delta only feeds the
    // exit summary, which `/new` bypasses via `continue`, so spawning `git`
    // here is pure latency on the fresh-prompt path.
    let is_new_session = matches!(session_end_reason, SessionEndReason::NewSession);
    let exec_shutdown_timeout = if is_fast_teardown(session_end_reason) {
        FAST_BACKGROUND_SHUTDOWN_TIMEOUT
    } else {
        EXIT_BACKGROUND_SHUTDOWN_TIMEOUT
    };
    tokio::join!(
        retain_completed_result(&mut output.harness_finish_error, async {
            let emitter = harness_emitter?;
            // Fire one best-effort session-completion notification, mirroring
            // the per-turn outcome helper. Reuses the same
            // completion_success/failure gates as turn completion.
            if timeout(
                Duration::from_millis(250),
                super::super::notifications::emit_session_completion_notification(subtype, session_stats.total_turns()),
            )
            .await
            .is_err()
            {
                tracing::debug!(target: "vtcode.harness", "session completion notification timed out during exit");
            }
            // Always attempt exporter finalization after the terminal event.
            // Optional exporters are isolated inside `HarnessEventEmitter`,
            // while a canonical persistence error is retained and returned
            // only after `finish()` has had a chance to drain/close the sink.
            match timeout(Duration::from_millis(500), emitter.finish()).await {
                Ok(Ok(())) => None,
                Ok(Err(error)) => {
                    tracing::error!(
                        target: "vtcode.harness",
                        phase = "canonical_finish",
                        error = %error,
                        "failed to finalize canonical session persistence"
                    );
                    Some(error.context("failed to finalize canonical session persistence"))
                }
                Err(_elapsed) => {
                    tracing::warn!(
                        target: "vtcode.harness",
                        phase = "canonical_finish",
                        "canonical session persistence finish timed out during exit; continuing teardown"
                    );
                    None
                }
            }
        }),
        async {
            if let Some(manager) = checkpoint_manager {
                let session_id = tool_registry.harness_context_snapshot().session_id;
                match timeout(Duration::from_millis(500), manager.complete_session_navigation(&session_id)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        tracing::debug!(%error, "checkpoint navigation trim failed after thread completion")
                    }
                    Err(_elapsed) => {
                        tracing::debug!(%session_id, "checkpoint navigation trim timed out during exit")
                    }
                }
            }
        },
        async {
            // Best-effort backstop: a Ctrl+C exit from idle (no turn
            // running) can leave exec/PTY sessions alive because
            // turn-level cancellation never ran. Terminate them before
            // teardown so no child outlives the TUI. `Immediate` PTY mode
            // (group SIGKILL, no SIGTERM grace window) because the user
            // asked to leave. `/new` shares this path so the previous
            // session's children cannot leak into the fresh session or
            // accumulate into a slower final process exit.
            if matches!(
                session_end_reason,
                SessionEndReason::Exit
                    | SessionEndReason::Cancelled
                    | SessionEndReason::Error
                    | SessionEndReason::NewSession
            ) {
                // Empty registries return immediately inside `terminate_all_*`
                // without touching backends, so the idle `/new` hot path pays
                // only a map snapshot + pipe-manager check.
                match timeout(exec_shutdown_timeout, tool_registry.terminate_all_exec_sessions_for_exit_async()).await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        tracing::warn!(%error, "failed to terminate exec sessions during session exit");
                    }
                    Err(_elapsed) => {
                        tracing::warn!("timed out terminating exec sessions during session exit; continuing teardown");
                    }
                }
            }
        },
        retain_completed_result(&mut output.end_code_changes, async {
            if is_new_session {
                None
            } else {
                capture_code_change_snapshot(workspace, "end").await
            }
        }),
    );
}

/// Await filesystem cleanup on the blocking pool before starting the next lifecycle phase.
/// Bounded so a slow disk cannot park `/new` or process exit; retention
/// catches strays on the next startup.
pub(super) async fn cleanup_completed_artifacts(workspace: &Path, turn_run_id: &str, session_id: &str) {
    let workspace = workspace.to_path_buf();
    let turn_run_id = turn_run_id.to_owned();
    let session_id = session_id.to_owned();
    let cleanup_task = tokio::task::spawn_blocking(move || {
        cleanup_completed_artifacts_blocking(&workspace, &turn_run_id, &session_id);
    });
    match timeout(Duration::from_secs(1), cleanup_task).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(%error, "completed session artifact cleanup task failed");
        }
        Err(_elapsed) => {
            tracing::warn!("completed session artifact cleanup timed out after 1s; continuing teardown");
        }
    }
}

/// # Blocking
/// Reads and removes session files and renames completed tracker artifacts.
fn cleanup_completed_artifacts_blocking(workspace: &Path, turn_run_id: &str, session_id: &str) {
    // Empty shells (0 turns, terminal) are not worth keeping: they pollute
    // `.vtcode/sessions/` and hide real sessions. Runs on every close path
    // (including NewSession / resume continues) after `emitter.finish()`
    // has dropped the liveness lock. Best-effort; retention catches strays.
    if let Err(error) = vtcode_memory::evict_zero_turn_completed_store(workspace, turn_run_id) {
        tracing::debug!(target: "vtcode.harness", error = %error, "zero-turn session store cleanup failed");
    }
    // A fully-checked task tracker is archived so it cannot leak into the
    // next session's memory envelope.
    {
        if let Err(error) =
            vtcode_core::core::agent::harness_artifacts::archive_completed_current_task(workspace, session_id)
        {
            tracing::debug!(%error, "completed task tracker archive failed after thread completion");
        }
    }
}

pub(super) async fn finalize_persistent_memory(
    finalize_config: vtcode_core::config::types::AgentConfig,
    finalize_vt_cfg: Option<VTCodeConfig>,
    finalize_messages: Arc<Vec<vtcode_core::llm::provider::Message>>,
    finalize_session_id: String,
) {
    let mut finalize_task = tokio::spawn(async move {
        vtcode_core::persistent_memory::finalize_persistent_memory(
            &finalize_config,
            finalize_vt_cfg.as_ref(),
            &finalize_messages,
            &finalize_session_id,
        )
        .await
    });
    match timeout(Duration::from_secs(5), &mut finalize_task).await {
        Ok(Ok(Ok(_))) => {}
        Ok(Ok(Err(err))) => {
            tracing::warn!("Failed to update persistent memory at session finalization: {}", err);
        }
        Ok(Err(join_error)) => {
            tracing::warn!("Persistent memory finalization task failed: {}", join_error);
        }
        Err(_elapsed) => {
            tracing::info!("Persistent memory finalization continues in the background");
            // Detached by design after the 5 s wait, but not silent:
            // an observer task logs the eventual outcome instead of
            // dropping the JoinHandle and losing any error.
            tokio::spawn(async move {
                match finalize_task.await {
                    Ok(Ok(_)) => {}
                    Ok(Err(err)) => {
                        tracing::warn!("Background persistent memory finalization failed: {}", err);
                    }
                    Err(join_error) => {
                        tracing::warn!(
                            "Background persistent memory finalization task panicked or was aborted: {}",
                            join_error
                        );
                    }
                }
            });
        }
    }
}

pub(super) async fn shutdown_subagents(tool_registry: &ToolRegistry) {
    if let Some(controller) = tool_registry.subagent_controller() {
        // Bounded on every teardown path, not just fast exits: nested
        // close_tree walks can stall on contended locks, the OS reaps any
        // remainder at process exit, and `/new` recreates the controller
        // anyway. This intentionally tightens the previous unconditional 2s
        // budget to the 750ms fast cap.
        let budget = FAST_BACKGROUND_SHUTDOWN_TIMEOUT;
        if timeout(budget, controller.signal_shutdown()).await.is_err() {
            tracing::warn!("timed out shutting down subagent controller during session exit");
        }
    }
}

/// State the session-exit tail reads from the iteration it ends.
///
/// Borrowed rather than owned so the iteration keeps ownership of every value
/// it still drops afterwards. The two by-value fields are what
/// `finalize_session` consumes owned; neither has drop side effects.
pub(super) struct SessionTailContext<'a> {
    pub config: &'a mut CoreAgentConfig,
    pub config_watcher: &'a mut SimpleConfigWatcher,
    pub vt_cfg: &'a mut Option<VTCodeConfig>,
    pub renderer: &'a mut AnsiRenderer,
    pub session: &'a mut InlineSession,
    pub handle: &'a InlineHandle,
    pub ctrl_c_state: &'a CtrlCState,
    pub session_archive: &'a mut Option<SessionArchive>,
    pub lifecycle_hooks: &'a Option<LifecycleHookEngine>,
    pub checkpoint_manager: &'a Option<SnapshotManager>,
    pub context_manager: &'a ContextManager,
    pub tool_registry: &'a ToolRegistry,
    pub runtime: &'a AgentRuntime,
    pub provider_client: &'a dyn uni::LLMProvider,
    pub session_bootstrap: &'a SessionBootstrap,
    pub thread_handle: &'a ThreadRuntimeHandle,
    pub turn_run_id: &'a TurnRunId,
    pub turn_id: &'a str,
    pub harness_emitter: &'a Option<HarnessEventEmitter>,
    pub active_primary_agent: &'a ActivePrimaryAgentState,
    pub loaded_skills: &'a Arc<RwLock<HashMap<String, Skill>>>,
    pub session_stats: &'a SessionStats,
    pub session_started_at: Instant,
    pub session_end_reason: SessionEndReason,
    pub full_auto: bool,
    pub last_approved_plan_summary_status: Option<ExecutionSummaryStatus>,
    pub last_turn_result: &'a Option<RunLoopTurnLoopResult>,
    pub last_turn_response_was_fallback: bool,
    pub last_turn_diagnostics: Option<SnapshotTurnDiagnostics>,
    pub linked_directories: Vec<LinkedDirectory>,
    pub start_code_changes: &'a Option<HashMap<PathBuf, FileStat>>,
    pub agent_touched_paths: &'a mut BTreeSet<PathBuf>,
    pub async_mcp_manager: &'a Option<Arc<AsyncMcpManager>>,
    pub next_session_primary_agent: &'a mut Option<String>,
    pub resume_state: &'a mut Option<ResumeSession>,
    pub pending_session_start_trigger: &'a mut Option<SessionStartTrigger>,
}

/// Exit tail for one session: terminal event, the bounded maintenance group,
/// TUI/terminal restoration, the exit summary, and the resume/`/new` handoff
/// decision ([`ControlFlow::Continue`] restarts the unified loop,
/// [`ControlFlow::Break`] ends it).
pub(super) async fn run_session_tail(context: SessionTailContext<'_>) -> Result<ControlFlow<()>> {
    let SessionTailContext {
        config,
        config_watcher,
        vt_cfg,
        renderer,
        session,
        handle,
        ctrl_c_state,
        session_archive,
        lifecycle_hooks,
        checkpoint_manager,
        context_manager,
        tool_registry,
        runtime,
        provider_client,
        session_bootstrap,
        thread_handle,
        turn_run_id,
        turn_id,
        harness_emitter,
        active_primary_agent,
        loaded_skills,
        session_stats,
        session_started_at,
        session_end_reason,
        full_auto,
        last_approved_plan_summary_status,
        last_turn_result,
        last_turn_response_was_fallback,
        last_turn_diagnostics,
        linked_directories,
        start_code_changes,
        agent_touched_paths,
        async_mcp_manager,
        next_session_primary_agent,
        resume_state,
        pending_session_start_trigger,
    } = context;

    // Tell the TUI to tear down at the START of the session tail, not at
    // the end. The worker leaves the alternate screen, drains, and restores
    // escape modes concurrently with the persistence work below (harness
    // finish, exec termination, archive write, hooks, MCP), so a typed
    // `exit`/`/quit` no longer keeps the fullscreen visible through the
    // whole tail. `finalize_session` still sends its own shutdown as an
    // idempotent backstop and joins the worker. Sends to a closed TUI
    // channel are no-ops, so this is safe on every end reason, including
    // NewSession/resume, which recreate the TUI afterwards.
    handle.shutdown();
    let session_tail_started = Instant::now();
    let deadline = ctrl_c_state
        .exit_deadline()
        .unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_millis(1500));
    if let Some(archive) = session_archive.as_mut() {
        archive.set_primary_agent(active_primary_agent.active().name());
        let skill_names = tokio::time::timeout_at(deadline, loaded_skills.read())
            .await
            .map(|skills| skills.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        archive.set_loaded_skills(skill_names);
        archive.set_continuation_metadata(session_stats.budget_limit().map(|(max_budget_usd, actual_cost_usd)| {
            session_archive::SessionContinuationMetadata::budget_limit(
                max_budget_usd,
                actual_cost_usd,
                crate::agent::runloop::unified::turn::compaction::has_latest_memory_envelope(
                    &config.workspace,
                    thread_handle.thread_id().as_str(),
                ),
            )
        }));
    }
    let (outcome_code, subtype) = resolve_thread_completion_status(
        &session_end_reason,
        session_stats.budget_limit().is_some(),
        last_approved_plan_summary_status,
        last_turn_result.as_ref(),
        last_turn_response_was_fallback,
    );
    let terminal_event_error = if let Some(emitter) = harness_emitter.as_ref() {
        let harness_snapshot = tool_registry.harness_context_snapshot();
        let result = subtype
            .is_success()
            .then(|| latest_assistant_result_text(&runtime.state.messages))
            .flatten();
        let total_cost_usd = session_stats.total_cost_usd().and_then(serde_json::Number::from_f64);
        let event = crate::agent::runloop::unified::inline_events::harness::thread_completed_event(
            turn_run_id.0.clone(),
            harness_snapshot.session_id,
            subtype,
            outcome_code,
            result,
            session_stats.stop_reason().map(str::to_string),
            session_stats.total_usage(),
            total_cost_usd,
            session_stats.total_turns(),
        );
        // The terminal event is enqueued synchronously; exporter
        // finalization happens in the concurrent group below.
        emitter
            .emit(event)
            .err()
            .map(|error| error.context("failed to emit canonical thread.completed event"))
    } else {
        None
    };

    tracing::debug!(target: "vtcode.shutdown", boundary = "teardown_started", accepted_exit_elapsed_ms = ?ctrl_c_state.exit_elapsed_ms(), elapsed_ms = session_tail_started.elapsed().as_millis() as u64);
    let final_response = latest_assistant_result_text(&runtime.state.messages);
    if matches!(session_end_reason, SessionEndReason::NewSession) {
        *next_session_primary_agent = Some(active_primary_agent.active().name().to_owned());
    }
    agent_touched_paths.extend(context_manager.tracked_instruction_activity_paths());
    session_bootstrap::poll_config_reload(
        config_watcher,
        &mut *vt_cfg,
        config,
        renderer,
        "Configuration reloaded during idle period",
    )?;
    // One deadline owns every independent cleanup branch. Atomic archive
    // workers retain ownership if we stop waiting; terminal restoration and
    // the postamble run outside this best-effort maintenance group.
    let mut teardown_output = SessionTeardownOutput::default();
    let mut finalization_output = None;
    let maintenance = tokio::time::timeout_at(deadline, async {
            tokio::join!(
                async {
                    drain_session_teardown(SessionTeardownContext {
                        harness_emitter: harness_emitter.as_ref(), checkpoint_manager: checkpoint_manager.as_ref(),
                        tool_registry, workspace: &config.workspace, session_stats,
                        subtype, session_end_reason,
                    }, &mut teardown_output).await;
                    cleanup_completed_artifacts(&config.workspace, &turn_run_id.0, &tool_registry.harness_context_snapshot().session_id).await;
                    tracing::debug!(target: "vtcode.shutdown", boundary = "persistence_finished", elapsed_ms = session_tail_started.elapsed().as_millis() as u64);
                },
                async {
                    match finalize_session(
                        renderer, lifecycle_hooks.as_ref(), turn_id, session_end_reason,
                        session_archive, session_stats, last_turn_diagnostics, &runtime.state.messages,
                        linked_directories, async_mcp_manager.as_deref(), handle, session, &mut finalization_output,
                    ).await {
                        Ok(output) => finalization_output = Some(output),
                        Err(error) => { tracing::error!(%error, "failed to finalize session"); }
                    }
                },
                shutdown_subagents(tool_registry),
                async {
                    if !matches!(session_end_reason, SessionEndReason::Exit | SessionEndReason::Cancelled) {
                        finalize_persistent_memory(config.clone(), vt_cfg.clone(), runtime.state.messages.clone(), turn_run_id.0.clone()).await;
                    }
                },
            )
        }).await;
    if maintenance.is_err() {
        tracing::warn!(target: "vtcode.shutdown", boundary = "maintenance_deadline", elapsed_ms = session_tail_started.elapsed().as_millis() as u64, "session maintenance deadline expired");
    }
    // A timed-out archive or hook must never bypass restoration or suppress
    // the summary. Join the already-stopping TUI only for remaining time.
    let tui_closed = session
        .wait_for_exit(deadline.saturating_duration_since(tokio::time::Instant::now()))
        .await;
    tracing::debug!(target: "vtcode.shutdown", boundary = "tui_wait_finished", tui_closed, elapsed_ms = session_tail_started.elapsed().as_millis() as u64);
    let _ = vtcode_ui::tui::panic_hook::restore_tui_keep_raw_mode();
    vtcode_ui::tui::panic_hook::finish_deferred_raw_mode_restore();
    vtcode_core::utils::transcript::clear_inline_handle();
    vtcode_core::ui::set_tui_mode(false);
    tracing::debug!(target: "vtcode.shutdown", boundary = "terminal_restored", elapsed_ms = session_tail_started.elapsed().as_millis() as u64);
    let SessionTeardownOutput { harness_finish_error, end_code_changes } = teardown_output;
    let teardown_error = terminal_event_error.or(harness_finish_error);
    let code_change_delta = compute_session_code_change_delta(start_code_changes.as_ref(), end_code_changes.as_ref());
    if let Some(next_resume) = resume_state.as_ref() {
        if let Some(error) = teardown_error {
            return Err(error);
        }
        refresh_runtime_debug_context_for_next_session(config.workspace.as_path(), Some(next_resume)).await?;
        return Ok(ControlFlow::Continue(()));
    }
    if matches!(session_end_reason, SessionEndReason::NewSession) {
        if let Some(error) = teardown_error {
            return Err(error);
        }
        session_bootstrap::poll_config_reload(
            config_watcher,
            &mut *vt_cfg,
            config,
            renderer,
            "Configuration reloaded due to file changes",
        )?;

        refresh_runtime_debug_context_for_next_session(config.workspace.as_path(), None).await?;
        tracing::info!(
            teardown_elapsed_ms = session_tail_started.elapsed().as_millis() as u64,
            "new session teardown completed"
        );
        *resume_state = None;
        *pending_session_start_trigger = Some(SessionStartTrigger::NewSession);
        return Ok(ControlFlow::Continue(()));
    }

    let finalization_succeeded = finalization_output.is_some();
    let resume_identifier = finalization_output
        .as_ref()
        .and_then(|output| output.archive_path.as_ref())
        .and_then(|path| path.file_stem())
        .and_then(|stem| stem.to_str());
    let trust_label = match session_bootstrap.acp_workspace_trust {
        Some(vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode::FullAuto) => "full auto",
        Some(vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy) => "tools policy",
        None if full_auto => "full auto",
        None => "tools policy",
    };
    let provider_label = {
        let label = crate::agent::runloop::unified::session_setup::resolve_provider_label(config, vt_cfg.as_ref());
        if label.is_empty() {
            provider_client.name().to_string()
        } else {
            label
        }
    };
    let reasoning_label = vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.reasoning_effort.as_str().to_string())
        .unwrap_or_else(|| config.reasoning_effort.as_str().to_string());
    let (code_additions, code_deletions) = code_change_delta.map(|d| (d.additions, d.deletions)).unwrap_or((0, 0));
    if !finalization_succeeded {
        let _ = vtcode_ui::tui::panic_hook::restore_tui();
    }
    let session_total_usage = session_stats.total_usage();
    print_exit_summary(ExitData {
        app_name: vtcode_core::config::constants::app::DISPLAY_NAME,
        version: env!("CARGO_PKG_VERSION"),
        model: &config.model,
        provider: &provider_label,
        trust_label,
        reasoning: &reasoning_label,
        session_duration: session_started_at.elapsed(),
        prompt_tokens: session_total_usage.input_tokens,
        completion_tokens: session_total_usage.output_tokens,
        cached_tokens: session_total_usage.cached_input_tokens,
        cache_creation_tokens: session_total_usage.cache_creation_tokens,
        cache_hit_rate_percent: session_total_usage.cache_hit_rate().map(|rate| rate * 100.0),
        code_additions,
        code_deletions,
        final_response: final_response.as_deref(),
        resume_identifier,
        budget_limit: session_stats.budget_limit(),
        total_cost_usd: session_stats.total_cost_usd(),
        end_reason_label: session_end_reason.as_str(),
        first_call_composition: session_stats.first_call_composition(),
        session_end_reason,
    });
    tracing::debug!(target: "vtcode.shutdown", boundary = "postamble_finished", accepted_exit_elapsed_ms = ?ctrl_c_state.exit_elapsed_ms(), elapsed_ms = session_tail_started.elapsed().as_millis() as u64);
    if let Some(error) = teardown_error {
        return Err(error);
    }

    if matches!(session_end_reason, SessionEndReason::Error) {
        return Err(anyhow::anyhow!(
            "{}",
            session_stats
                .turn_stall_reason()
                .unwrap_or("Session ended with an execution error.")
        ));
    }
    Ok(ControlFlow::Break(()))
}

#[cfg(test)]
mod tests {
    use super::cleanup_completed_artifacts;
    use vtcode_core::core::agent::harness_artifacts::current_task_path;

    #[tokio::test]
    async fn deadline_preserves_completed_results_and_canonical_errors() {
        use super::{SessionTeardownOutput, retain_completed_result};
        use crate::agent::runloop::unified::turn::finalization::FinalizationOutput;
        let mut teardown = SessionTeardownOutput::default();
        let mut archive = None;
        let mut unfinished = None::<usize>;
        let work = async {
            tokio::join!(
                retain_completed_result(&mut teardown.harness_finish_error, async {
                    Some(anyhow::anyhow!("canonical write failed"))
                }),
                retain_completed_result(&mut teardown.end_code_changes, async {
                    Some(super::HashMap::from_iter([(
                        "changed.rs".into(),
                        super::FileStat { additions: 17, deletions: 3 },
                    )]))
                }),
                retain_completed_result(&mut archive, async {
                    Some(FinalizationOutput { archive_path: Some("saved-session.json".into()) })
                }),
                retain_completed_result(&mut unfinished, std::future::pending()),
            );
        };
        assert!(tokio::time::timeout(std::time::Duration::from_millis(20), work).await.is_err());
        assert_eq!(teardown.harness_finish_error.unwrap().to_string(), "canonical write failed");
        assert_eq!(
            teardown.end_code_changes.unwrap().get(std::path::Path::new("changed.rs")),
            Some(&super::FileStat { additions: 17, deletions: 3 })
        );
        assert_eq!(archive.unwrap().archive_path.unwrap(), std::path::PathBuf::from("saved-session.json"));
        assert!(unfinished.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cleanup_archives_completed_tracker_before_returning() {
        let workspace = tempfile::tempdir().expect("workspace");
        let task_path = current_task_path(workspace.path());
        std::fs::create_dir_all(task_path.parent().expect("tasks directory")).expect("create tasks");
        let content = "# Work\n- [x] first\n- [X] second\n";
        std::fs::write(&task_path, content).expect("write tracker");

        cleanup_completed_artifacts(workspace.path(), "run-id", "session-id").await;

        assert!(!task_path.exists(), "cleanup must finish before the next lifecycle phase");
        let archives = std::fs::read_dir(task_path.parent().expect("tasks directory").join("archive"))
            .expect("archive directory")
            .map(|entry| entry.expect("archive entry").path())
            .collect::<Vec<_>>();
        assert_eq!(archives.len(), 1);
        assert!(
            archives[0]
                .file_name()
                .expect("archive name")
                .to_string_lossy()
                .starts_with("current_task-session-id-")
        );
        assert_eq!(std::fs::read_to_string(&archives[0]).expect("archived tracker"), content);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cleanup_preserves_unfinished_tracker() {
        let workspace = tempfile::tempdir().expect("workspace");
        let task_path = current_task_path(workspace.path());
        std::fs::create_dir_all(task_path.parent().expect("tasks directory")).expect("create tasks");
        let content = "# Work\n- [x] first\n- [ ] second\n";
        std::fs::write(&task_path, content).expect("write tracker");

        cleanup_completed_artifacts(workspace.path(), "run-id", "session-id").await;

        assert_eq!(std::fs::read_to_string(&task_path).expect("unfinished tracker"), content);
        assert!(!task_path.parent().expect("tasks directory").join("archive").exists());
    }

    #[test]
    fn fast_teardown_covers_exit_cancel_and_new_session() {
        use super::{FAST_BACKGROUND_SHUTDOWN_TIMEOUT, is_fast_teardown};
        use vtcode_core::hooks::SessionEndReason;

        // Asymmetric: fast reasons must map to tight cap, slow reasons keep
        // full budget. Drive via helper so mapping itself is pinned.
        for reason in [
            SessionEndReason::Exit,
            SessionEndReason::Cancelled,
            SessionEndReason::NewSession,
        ] {
            assert!(is_fast_teardown(reason), "{reason:?} should be fast");
        }
        for reason in [SessionEndReason::Completed, SessionEndReason::Error] {
            assert!(!is_fast_teardown(reason), "{reason:?} should keep full budget");
        }
        // Fast cap must be strictly tighter than the normal 2s budget so
        // `/new` and exit never park on nested close_tree walks.
        assert!(
            FAST_BACKGROUND_SHUTDOWN_TIMEOUT < super::super::session_bootstrap::EXIT_BACKGROUND_SHUTDOWN_TIMEOUT,
            "fast shutdown must be tighter than normal"
        );
    }
}
