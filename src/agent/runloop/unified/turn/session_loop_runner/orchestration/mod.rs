use std::collections::VecDeque;
use std::time::Instant;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;
use vtcode_commons::ui_protocol::ActivityState;
use vtcode_config::loader::SimpleConfigWatcher;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runtime::AgentRuntime;
use vtcode_core::core::agent::session::AgentSessionState;
use vtcode_core::core::agent::steering::SteeringMessage;
use vtcode_core::core::interfaces::session::PlanningEntrySource;
use vtcode_core::exec::events::ThreadCompletionSubtype;
use vtcode_core::hooks::{SessionEndReason, SessionStartTrigger};
use vtcode_core::llm::provider::MessageRole;
use vtcode_core::session::SessionId;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::session_archive;
use vtcode_core::utils::session_archive::SessionMessage;
use vtcode_ui::tui::app::ArchivedPromptEntry;

use super::super::{CancelGuard, TerminalCleanupGuard};
use super::archive::refresh_runtime_debug_context_for_next_session;
use super::blocked_handoff::write_blocked_handoff_after_checkpoint;
use super::handoff::{
    append_approved_plan_execution_input, apply_primary_agent_tool_policy_overrides,
    build_approved_plan_execution_prompt, report_plan_approval_selection_failure, select_approved_plan_execution_agent,
};
use super::metrics::{capture_code_change_snapshot, estimate_history_bytes};
use super::plan_seed::load_active_plan_seed;
use super::support::{
    ExecutionSummaryStatus, RefusedTurnRollback, approved_plan_execution_summary, build_unrelated_dirty_worktree_note,
    build_withdrawn_turn_changes_note, checkpoint_session_archive_start, checkpoint_unavailable_notice,
    force_reload_workspace_config_for_execution, format_workspace_relative_paths, latest_assistant_result_text,
    prepare_transient_turn_notes, prompt_startup_planning_workflow, remove_transient_system_notes,
    take_pending_resumed_user_prompt,
};
use super::turn_tail::{TurnPersistenceTail, complete_turn_persistence_tail};
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::git::{compute_session_code_change_delta, normalize_workspace_path};
use crate::agent::runloop::model_picker::ModelPickerState;
use crate::agent::runloop::unified::inline_events::harness::harness_event;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::planning_workflow_state::{
    render_planning_workflow_next_step_hint, transition_to_planning_workflow,
};
use crate::agent::runloop::unified::postamble::{ExitData, print_exit_summary};
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
use crate::agent::runloop::unified::session_setup::{
    SessionState, apply_post_hydration_ui, hydrate_session_runtime, initialize_session_critical, initialize_session_ui,
    run_session_start_hooks, spawn_signal_handler,
};
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::turn::background_completion::PendingBackgroundCompletions;
use crate::agent::runloop::unified::turn::context::TurnLoopResult as RunLoopTurnLoopResult;
use crate::agent::runloop::unified::turn::finalization::finalize_session;
use crate::agent::runloop::unified::turn::primary_agent_runtime::{
    PrimaryAgentRuntimeSyncContext, sync_primary_agent_permissions, sync_primary_agent_runtime,
};
use crate::agent::runloop::unified::turn::turn_loop::TurnLoopOutcome;
use crate::agent::runloop::unified::turn::turn_loop_helpers::{
    effective_max_tool_calls_for_approved_plan_execution, effective_max_tool_calls_for_turn,
    resolve_safety_tool_call_limits,
};
use crate::agent::runloop::unified::workspace_links::LinkedDirectory;
use crate::updater::{InlineUpdateOutcome, display_update_notice, run_inline_update_prompt};

mod session_bootstrap;
mod session_teardown;

pub(crate) use session_bootstrap::BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX;
pub(super) use session_bootstrap::resolve_thread_completion_status;
use session_bootstrap::{
    apply_startup_plan_agent_selection, background_completion_continuation_prompt, load_archived_prompts_for_history,
    persist_primary_agent, record_plan_selection_failure_tail,
};

#[cfg_attr(feature = "profiling", hotpath::measure)]
pub(crate) async fn run_single_agent_loop_unified_impl(
    config: &CoreAgentConfig,
    initial_vt_cfg: Option<VTCodeConfig>,
    skip_confirmations: bool,
    full_auto: bool,
    primary_agent_explicitly_configured: bool,
    planning_entry_source: PlanningEntrySource,
    resume: Option<ResumeSession>,
    steering_receiver: &mut Option<mpsc::UnboundedReceiver<SteeringMessage>>,
) -> Result<()> {
    let _terminal_cleanup_guard = TerminalCleanupGuard::new();

    let mut config = config.clone();
    let mut session_skip_confirmations = skip_confirmations;
    let mut resume_state = resume;
    let mut config_watcher = SimpleConfigWatcher::new_with_user_config_paths(config.workspace.clone());
    config_watcher.set_check_interval(15);
    config_watcher.set_debounce_duration(500);
    if let Some(initial_config) = initial_vt_cfg.as_ref() {
        config_watcher.set_last_known_config(initial_config.clone());
    }
    let mut vt_cfg = initial_vt_cfg.or_else(|| config_watcher.load_config());
    let mut pending_session_start_trigger = None;
    let mut next_session_primary_agent: Option<String> = None;

    loop {
        let session_started_at = Instant::now();
        let resume_request = resume_state.take();
        let resume_ref = resume_request.as_ref();
        let session_trigger = pending_session_start_trigger.take().unwrap_or_else(|| {
            if resume_ref.is_some() {
                SessionStartTrigger::Resume
            } else {
                SessionStartTrigger::Startup
            }
        });
        let active_thread_label = resume_ref.map_or("main", ResumeSession::thread_label);
        let archive_metadata = vtcode_core::core::threads::build_thread_archive_metadata(
            &config.workspace,
            &config.model,
            &config.provider,
            &config.theme,
            config.reasoning_effort.as_str(),
        )
        .with_debug_log_path(
            crate::main_helpers::runtime_debug_log_path().map(|path| path.to_string_lossy().to_string()),
        );
        let reserved_archive_id = crate::main_helpers::runtime_archive_session_id();
        let history_enabled = session_archive::history_persistence_enabled();
        // Overlap the `git diff` start snapshot with archive/thread prep:
        // both are independent blocking I/O and previously summed
        // (≈120ms snapshot + 10-100ms archive reserve) on the `/new`
        // fresh-prompt path.
        let (start_code_changes, thread_bootstrap) = tokio::join!(
            capture_code_change_snapshot(&config.workspace, "start"),
            session_bootstrap::prepare_session_thread(
                &config,
                vt_cfg.as_ref(),
                resume_ref,
                archive_metadata,
                reserved_archive_id,
                history_enabled,
            )
        );
        let session_bootstrap::SessionThreadBootstrap { thread_id, bootstrap, mut session_archive } = thread_bootstrap?;
        let thread_handle =
            vtcode_core::core::threads::ThreadManager::new().start_thread_with_identifier(thread_id, bootstrap);
        crate::main_helpers::set_runtime_archive_session_id(Some(thread_handle.thread_id().to_string()));
        if let Some(archive) = session_archive.as_ref()
            && let Err(err) = checkpoint_session_archive_start(archive, &thread_handle).await
        {
            tracing::warn!("Failed to checkpoint session archive at startup: {}", err);
        }
        let session_setup_phase = vtcode_commons::startup_trace::phase_started();
        let session_primary_agent_override = next_session_primary_agent.take();
        // Static-first paint: typeable shell before ToolRegistry/discovery.
        let steering_sender_for_shell = if steering_receiver.is_none() {
            let (sender, receiver) = mpsc::unbounded_channel();
            *steering_receiver = Some(receiver);
            Some(sender)
        } else {
            None
        };
        let (settings_sender, shell_settings_receiver) = mpsc::unbounded_channel();
        let session_critical_phase = vtcode_commons::startup_trace::phase_started();
        let thread_id_for_critical = thread_handle.thread_id().to_string();
        let (mut shell, critical_result) = {
            let shell_future = crate::agent::runloop::unified::session_setup::initialize_session_shell(
                &config,
                vt_cfg.as_ref(),
                crate::agent::runloop::unified::session_setup::SessionUiLaunchOptions {
                    session_archive: None,
                    full_auto,
                    skip_confirmations,
                    steering_sender: steering_sender_for_shell,
                    settings_sender: settings_sender.clone(),
                },
            );
            let critical = initialize_session_critical(
                &config,
                vt_cfg.as_ref(),
                full_auto,
                primary_agent_explicitly_configured,
                resume_ref,
                thread_id_for_critical.as_str(),
                session_primary_agent_override.as_deref(),
            );
            tokio::pin!(shell_future, critical);
            let (shell, ready) = tokio::select! {
                result = &mut critical => (shell_future.await?, Some(result)),
                result = &mut shell_future => (result?, None),
            };
            let result = match ready {
                Some(result) => Some(result),
                None => {
                    crate::agent::runloop::unified::stop_requests::await_initialization(
                        &shell.ctrl_c_state,
                        &shell.ctrl_c_notify,
                        critical,
                    )
                    .await
                }
            };
            (shell, result)
        };
        let Some(critical_result) = critical_result else {
            crate::agent::runloop::unified::stop_requests::finish_initialization_exit(
                &shell.handle,
                &mut shell.session,
                &shell.ctrl_c_state,
                crate::agent::runloop::unified::stop_requests::InitializationExitContext {
                    emitter: None,
                    session_id: thread_handle.thread_id().as_str(),
                    config: &config,
                    full_auto,
                    started: session_started_at,
                },
            )
            .await;
            return Ok(());
        };
        let mut settings_receiver = shell_settings_receiver;
        let mut session_state = critical_result?;
        vtcode_commons::startup_trace::record_phase("session_setup_critical", session_critical_phase);
        // Persist the active primary agent ("mode") so a future resume restores
        // it instead of falling back to the config default.
        persist_primary_agent(&mut session_archive, &session_state.active_primary_agent);
        let harness_config = vt_cfg.as_ref().map(|cfg| cfg.agent.harness.clone()).unwrap_or_default();
        let turn_run_id = TurnRunId(thread_handle.thread_id().to_string());
        let harness_emitter =
            super::harness::initialize_harness(&config.workspace, vt_cfg.as_ref(), &config.model, &turn_run_id).await?;

        // Once canonical persistence is open, every fallible operation must
        // finalize it before leaving this session iteration. The normal path
        // emits `thread.completed`; this macro emits an error terminal event
        // and drains the canonical sink for early error exits.
        macro_rules! harness_try {
            ($expression:expr) => {{
                match $expression {
                    Ok(value) => value,
                    Err(error) => {
                        let error: anyhow::Error = error.into();
                        if let Some(emitter) = harness_emitter.as_ref() {
                            emitter.finish_after_unexpected_exit().await;
                        }
                        return Err(error);
                    }
                }
            }};
        }

        let session_ui_phase = vtcode_commons::startup_trace::phase_started();
        let ui_setup = initialize_session_ui(
            &config,
            vt_cfg.as_ref(),
            thread_handle.thread_id().as_str(),
            &mut session_state,
            session_trigger,
            resume_ref,
            shell,
            crate::agent::runloop::unified::stop_requests::InitializationExitContext {
                emitter: harness_emitter.as_ref(),
                session_id: thread_handle.thread_id().as_str(),
                config: &config,
                full_auto,
                started: session_started_at,
            },
            crate::agent::runloop::unified::session_setup::SessionUiLaunchOptions {
                session_archive,
                full_auto,
                skip_confirmations,
                steering_sender: None,
                settings_sender: settings_sender.clone(),
            },
        )
        .await;
        let Some(mut ui_setup) = harness_try!(ui_setup) else {
            return Ok(());
        };
        let initialization_progress = ui_setup
            .handle
            .resume_progress(vtcode_commons::ui_protocol::ProgressPhase::Initializing);
        vtcode_commons::startup_trace::record_phase("session_setup_ui", session_ui_phase);

        macro_rules! initialization_wait {
            ($future:expr) => {{
                let state = ui_setup.ctrl_c_state.clone();
                let notify = ui_setup.ctrl_c_notify.clone();
                match crate::agent::runloop::unified::stop_requests::await_initialization(&state, &notify, $future)
                    .await
                {
                    Some(result) => harness_try!(result),
                    None => {
                        crate::agent::runloop::unified::stop_requests::finish_initialization_exit(
                            &ui_setup.handle,
                            &mut ui_setup.session,
                            &state,
                            crate::agent::runloop::unified::stop_requests::InitializationExitContext {
                                emitter: harness_emitter.as_ref(),
                                session_id: &turn_run_id.0,
                                config: &config,
                                full_auto,
                                started: session_started_at,
                            },
                        )
                        .await;
                        return Ok(());
                    }
                }
            }};
        }
        initialization_wait!(crate::agent::runloop::unified::session_setup::complete_session_registry(
            &mut session_state,
            &config,
            vt_cfg.as_ref(),
            full_auto,
            primary_agent_explicitly_configured,
            resume_ref,
            thread_handle.thread_id().as_str(),
            session_primary_agent_override.as_deref(),
        ));

        // Retention walks the session store and may rmtree dozens of dirs.
        // Scheduled only after first paint is available so a large archive
        // cannot delay the first frame; still best-effort background.
        {
            let workspace = config.workspace.clone();
            let vt_cfg = vt_cfg.clone();
            let turn_run_id = turn_run_id.clone();
            tokio::spawn(async move {
                super::harness::run_harness_retention(&workspace, vt_cfg.as_ref(), &turn_run_id).await;
            });
        }

        // Deferred hydration runs after the TUI first frame is available.
        // The interaction loop must not dispatch a model turn until this
        // completes; setup failures abort with the historical setup error.
        let session_hydrate_phase = vtcode_commons::startup_trace::phase_started();
        initialization_wait!(hydrate_session_runtime(
            &mut session_state,
            &mut ui_setup.context_manager,
            &config,
            vt_cfg.as_ref(),
            full_auto,
            primary_agent_explicitly_configured,
            resume_ref,
            thread_handle.thread_id().as_str(),
            session_primary_agent_override.as_deref(),
        ));
        vtcode_commons::startup_trace::record_phase("session_setup_hydrate", session_hydrate_phase);

        // Re-drive UI surfaces that depend on hydrated session state.
        let post_hydrate_guard =
            harness_try!(apply_post_hydration_ui(vt_cfg.as_ref(), full_auto, &session_state, &mut ui_setup,));
        if let Some(guard) = post_hydrate_guard {
            // Prefer the post-hydrate refresh task when a controller appeared
            // after first paint.
            ui_setup.background_subprocess_task_guard = Some(guard);
        }

        // Session-start hooks run only after hydration so they observe the
        // fully initialized tool registry. For `/new` the fresh prompt is
        // waiting, so bound the total: per-command timeouts default to 60s
        // and run sequentially, which can park `/new` for 10s+ on a slow
        // hook. Timeout skips remaining hooks (warn, continue) instead of
        // aborting the session — startup keeps full budgets.
        if matches!(session_trigger, SessionStartTrigger::NewSession) {
            initialization_wait!(async {
                match tokio::time::timeout(
                    Duration::from_secs(2),
                    run_session_start_hooks(&ui_setup.lifecycle_hooks, &mut ui_setup.renderer, &mut session_state),
                )
                .await
                {
                    Ok(result) => {
                        harness_try!(result);
                    }
                    Err(_elapsed) => {
                        tracing::warn!(
                            "session-start hooks timed out on /new fast path; continuing without remaining hooks"
                        );
                    }
                }
                Ok::<(), anyhow::Error>(())
            });
        } else {
            initialization_wait!(run_session_start_hooks(
                &ui_setup.lifecycle_hooks,
                &mut ui_setup.renderer,
                &mut session_state
            ));
        }

        drop(initialization_progress);
        vtcode_commons::startup_trace::record_phase("session_setup", session_setup_phase);
        if matches!(session_trigger, SessionStartTrigger::NewSession) {
            tracing::info!(
                bootstrap_elapsed_ms = session_started_at.elapsed().as_millis() as u64,
                "new session bootstrap completed"
            );
        }
        let mut renderer = ui_setup.renderer;
        let mut session = ui_setup.session;
        let handle = ui_setup.handle;

        // Load archived prompts from recent sessions into the history picker.
        // Runs in the background so the TUI starts immediately.
        {
            let handle = handle.clone();
            tokio::spawn(async move {
                load_archived_prompts_for_history(&handle).await;
            });
        }

        let mut header_context = ui_setup.header_context;
        let ctrl_c_state = ui_setup.ctrl_c_state;
        let ctrl_c_notify = ui_setup.ctrl_c_notify;
        let input_activity_counter = ui_setup.input_activity_counter;
        let checkpoint_manager = ui_setup.checkpoint_manager;
        let mut session_archive = ui_setup.session_archive;
        let mut lifecycle_hooks = ui_setup.lifecycle_hooks;
        let mut context_manager = ui_setup.context_manager;
        let mut default_placeholder = ui_setup.default_placeholder;
        let mut follow_up_placeholder = ui_setup.follow_up_placeholder;
        let mut next_checkpoint_turn = ui_setup.next_checkpoint_turn;
        let mut session_end_reason = ui_setup.session_end_reason;
        let mut turn_id = turn_run_id.0.clone();
        let _file_palette_task_guard = ui_setup.file_palette_task_guard;
        let _background_subprocess_task_guard = ui_setup.background_subprocess_task_guard;
        let _startup_update_task_guard = ui_setup.startup_update_task_guard;
        let _editor_open_coordinator_task_guard = ui_setup.editor_open_coordinator_task_guard;
        let _settings_task_guard = ui_setup.settings_task_guard;
        let editor_open_sender = ui_setup.editor_open_sender;
        let editor_open_dispatcher = ui_setup.editor_open_dispatcher;
        let startup_update_cached_notice = ui_setup.startup_update_cached_notice;
        let mut startup_update_notice_rx = ui_setup.startup_update_notice_rx;
        let SessionState {
            session_bootstrap,
            mut provider_client,
            tool_registry: tool_registry_opt,
            tools,
            tool_catalog,
            conversation_history,
            execution,
            metadata,
            async_mcp_manager,
            mut mcp_panel_state,
            loaded_skills,
            mut active_primary_agent,
            ..
        } = session_state;
        // `complete_session_registry` already ran after first paint; the
        // interaction loop requires the concrete registry.
        let mut tool_registry = tool_registry_opt.expect("tool registry completed before interaction");
        // `initialize_session_ui` may move the archive through setup. Persist
        // again after extracting the live state so every subsequent switch is
        // anchored to the same archive metadata instance.
        persist_primary_agent(&mut session_archive, &active_primary_agent);
        let decision_ledger = metadata.decision_ledger;
        let traj = metadata.trajectory;
        let telemetry = metadata.telemetry;
        let error_recovery = metadata.error_recovery;
        let max_tool_loops = vt_cfg
            .as_ref()
            .map(|cfg| cfg.tools.max_tool_loops)
            .unwrap_or(vtcode_config::constants::tool_limits::DEFAULT_MAX_TOOL_LOOPS);
        let effective_model = crate::agent::runloop::unified::turn::turn_processing::resolve_effective_request_model(
            &config.model,
            active_primary_agent.active(),
        );
        let max_context_tokens = vtcode_core::compaction::effective_context_budget(
            vt_cfg.as_ref(),
            provider_client.as_ref(),
            &effective_model,
        );
        let mut runtime = AgentRuntime::new(
            AgentSessionState::new(
                SessionId::generate().into_inner(),
                config.max_conversation_turns,
                max_tool_loops,
                max_context_tokens,
            ),
            None,
            steering_receiver.take(),
        );
        runtime.state.messages = conversation_history.into();
        let durable_session_id = tool_registry.harness_context_snapshot().session_id;
        if let Some(envelope) = vtcode_core::compaction::memory_envelope::load_latest_memory_envelope_async(
            config.workspace.as_path(),
            &durable_session_id,
        )
        .await
        {
            if let Err(error) = runtime.restore_follow_up_state(envelope.pending_intents, envelope.applied_intent_ids) {
                tracing::warn!(%error, "durable steering queue is full; pending intents were not replayed");
            }
        }
        if resume_ref.is_some()
            && let Some(pending_prompt) = take_pending_resumed_user_prompt(runtime.state.messages_mut())
        {
            let (_, runtime_steering) = runtime.split_mut();
            if let Err(error) = runtime_steering.try_queue_follow_up_input(pending_prompt) {
                tracing::warn!(%error, "Unable to queue resumed user prompt");
            }
        } else if resume_ref.is_some() {
            use crate::agent::runloop::unified::turn::tool_outcomes::helpers as tracker_continue;
            let auto_continue_enabled = crate::agent::runloop::unified::stop_requests::stop_outcome(&ctrl_c_state)
                .is_none()
                && tracker_continue::tracker_auto_continue_enabled(vt_cfg.as_ref());
            let cross_turn_turns = tracker_continue::tracker_cross_turn_turns(vt_cfg.as_ref());
            let incomplete = if auto_continue_enabled && cross_turn_turns > 0 {
                tracker_continue::incomplete_tracker_items(&tool_registry).await
            } else {
                None
            };
            // Planning-blocked resume: auto-queue plan continuation when the
            // blocked-handoff summary is planning-related and recoverable, and
            // no plan is approval-ready. Require planning context so long
            // diagnostic summaries that merely quote "budget exhausted" cannot
            // trigger plan auto-queue.
            let resume_blocked_summary =
                vtcode_core::core::agent::blocked_handoff::read_current_blocked_handoff(config.workspace.as_path())
                    .map(|info| info.blocker_summary);
            let planning_resume = auto_continue_enabled
                && cross_turn_turns > 0
                && tool_registry.is_planning_active()
                && resume_blocked_summary.as_deref().is_some_and(|summary| {
                    let lower = summary.to_ascii_lowercase();
                    lower.contains("planning") && tracker_continue::plan_mode_recoverable_block(summary)
                });
            if planning_resume {
                let plan_state = tool_registry.planning_workflow_state();
                let plan_ready =
                    crate::agent::runloop::unified::planning_workflow::persisted_plan_is_ready(&plan_state).await;
                if !plan_ready {
                    let follow_up = tracker_continue::plan_mode_continue_follow_up();
                    let directive = tracker_continue::plan_mode_resume_directive();
                    {
                        let messages = std::sync::Arc::make_mut(&mut runtime.state.messages);
                        messages.push(vtcode_core::llm::provider::Message::system(directive));
                    }
                    let (_, runtime_steering) = runtime.split_mut();
                    match runtime_steering.try_queue_follow_up_input(follow_up) {
                        Ok(()) => {
                            let _ = renderer.line(
                                MessageStyle::Info,
                                "[i] Resumed planning session with recoverable blocked handoff; auto-continuing without manual `continue`.",
                            );
                        }
                        Err(error) => {
                            tracing::warn!(%error, "Unable to queue plan resume continuation");
                        }
                    }
                }
            } else if !tool_registry.is_planning_active()
                && tracker_continue::should_queue_tracker_resume_continuation(
                    auto_continue_enabled,
                    cross_turn_turns,
                    incomplete.as_deref(),
                )
            {
                let incomplete = incomplete.unwrap_or_default();
                // Resume with open TODO/tracker steps: auto-queue one continuation
                // turn instead of waiting for the user to type continue.
                let follow_up = tracker_continue::tracker_continue_follow_up(&incomplete);
                let directive = tracker_continue::tracker_continue_directive(
                    tracker_continue::TRACKER_RESUME_DIRECTIVE_LABEL,
                    &incomplete,
                );
                {
                    let messages = std::sync::Arc::make_mut(&mut runtime.state.messages);
                    messages.push(vtcode_core::llm::provider::Message::system(directive));
                }
                let (_, runtime_steering) = runtime.split_mut();
                match runtime_steering.try_queue_follow_up_input(follow_up) {
                    Ok(()) => {
                        let _ = renderer.line(
                            MessageStyle::Info,
                            "[i] Resumed session has incomplete task_tracker steps; auto-continuing without manual `continue`.",
                        );
                    }
                    Err(error) => {
                        tracing::warn!(%error, "Unable to queue tracker resume continuation");
                    }
                }
            }
        }
        let tool_result_cache = execution.tool_result_cache;
        let tool_permission_cache = execution.tool_permission_cache;
        let permissions_state = execution.permissions_state;
        let approval_recorder = execution.approval_recorder;
        let safety_validator = execution.safety_validator;
        let circuit_breaker = execution.circuit_breaker;
        let tool_health_tracker = execution.tool_health_tracker;
        let rate_limiter = execution.rate_limiter;
        let validation_cache = execution.validation_cache;
        let autonomous_executor = execution.autonomous_executor;
        let cancel_token = CancellationToken::new();
        let _cancel_guard = CancelGuard(cancel_token.clone());
        let _signal_handler = spawn_signal_handler(
            ctrl_c_state.clone(),
            ctrl_c_notify.clone(),
            async_mcp_manager.clone(),
            cancel_token.clone(),
        );
        let mut session_stats = SessionStats::default();
        session_stats.circuit_breaker = circuit_breaker.clone();
        session_stats.tool_health_tracker = tool_health_tracker.clone();
        session_stats.rate_limiter = rate_limiter.clone();
        session_stats.validation_cache = validation_cache.clone();
        session_stats.set_prompt_cache_lineage_id(
            thread_handle
                .snapshot()
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.prompt_cache_lineage_id.clone()),
        );
        session_stats.set_prompt_cache_profile(
            thread_handle
                .snapshot()
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.budget_limit_continuation())
                .map(|_| vtcode_core::llm::provider::PromptCacheProfile::BudgetContinuation),
        );
        session_stats.vim_mode_enabled = vt_cfg.as_ref().is_some_and(|cfg| cfg.ui.vim_mode);
        let mut plan_session =
            crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
        if planning_entry_source.should_auto_enter() {
            transition_to_planning_workflow(
                &tool_registry,
                &mut session_stats,
                &mut plan_session,
                &handle,
                planning_entry_source,
                Some(active_primary_agent.active().name().to_string()),
                vt_cfg.as_ref().map(|cfg| cfg.default_primary_agent.clone()),
                true,
                true,
            )
            .await;
            apply_startup_plan_agent_selection(&mut active_primary_agent, &tool_registry, &config, &handle).await;
            harness_try!(render_planning_workflow_next_step_hint(&mut renderer));
            // No researching indicator here: startup entry has no request yet.
        } else if planning_entry_source.requires_startup_prompt() && resume_ref.is_none() {
            let should_enter = harness_try!(
                prompt_startup_planning_workflow(&handle, &mut session, &ctrl_c_state, &ctrl_c_notify).await
            );
            if should_enter {
                transition_to_planning_workflow(
                    &tool_registry,
                    &mut session_stats,
                    &mut plan_session,
                    &handle,
                    planning_entry_source,
                    Some(active_primary_agent.active().name().to_string()),
                    vt_cfg.as_ref().map(|cfg| cfg.default_primary_agent.clone()),
                    true,
                    true,
                )
                .await;
                apply_startup_plan_agent_selection(&mut active_primary_agent, &tool_registry, &config, &handle).await;
                harness_try!(render_planning_workflow_next_step_hint(&mut renderer));
                // No researching indicator here: no request exists yet.
            }
        }
        let mut linked_directories: Vec<LinkedDirectory> = Vec::with_capacity(4);
        let mut model_picker_state: Option<ModelPickerState> = None;
        let mut palette_state: Option<ActivePalette> = None;
        let mut last_forced_redraw = Instant::now();
        let mut input_status_state = InputStatusState::default();
        let mut dismissed_memory_cleanup_fingerprint: Option<(usize, usize)> = None;
        let mut prefer_latest_queued_input_once = false;
        let mut queued_inputs: VecDeque<crate::agent::runloop::unified::inline_events::QueuedInput> =
            VecDeque::with_capacity(8);
        let mut background_completion_receiver = tool_registry
            .subagent_controller()
            .map(|controller| controller.subscribe_parent_background_completions());
        let exec_session_manager = tool_registry.exec_session_manager();
        let mut exec_completion_receiver = Some(exec_session_manager.subscribe_completion());
        let exec_completion_notify = Some(exec_session_manager.completion_notify());
        let mut pending_background_completions = PendingBackgroundCompletions::default();
        let (webmcp_prompt_sender, webmcp_prompt_receiver) = crate::agent::runloop::unified::webmcp::prompt_channel();
        let mut webmcp_prompt_receiver = Some(webmcp_prompt_receiver);
        let mut webmcp_bridge = None;
        let mut agent_touched_paths = std::collections::BTreeSet::new();
        let mut ctrl_c_notice_displayed = false;
        let mut inline_prompt_cost_notice_shown = false;
        let mut mcp_catalog_initialized = tool_registry.mcp_client().is_some();
        let mut last_known_mcp_tools: Vec<String> = Vec::with_capacity(16);
        let mut pending_mcp_refresh = false;
        let mut last_mcp_refresh = Instant::now();
        let startup_update_requested_restart = if let Some(notice) = startup_update_cached_notice.as_ref() {
            display_update_notice(&handle, &mut header_context, renderer.should_use_unicode_formatting(), notice);
            let update_outcome = harness_try!(
                run_inline_update_prompt(
                    &mut renderer,
                    &handle,
                    &mut session,
                    &ctrl_c_state,
                    &ctrl_c_notify,
                    config.workspace.as_path(),
                    notice,
                )
                .await
            );
            matches!(update_outcome, InlineUpdateOutcome::RestartRequested)
        } else {
            false
        };

        if startup_update_requested_restart {
            session_end_reason = SessionEndReason::Completed;
        }

        // Show release notes on first launch after update
        if !startup_update_requested_restart
            && let Some((ref version, ref highlights)) = session_bootstrap.release_highlights
        {
            crate::updater::display_release_notes(&handle, version, highlights);
            crate::updater::record_current_version_seen();
        }

        let mut cross_turn_tracker = crate::agent::runloop::unified::run_loop_context::CrossTurnTracker::new();
        let mut approved_plan_execution_turn = false;
        let mut pending_approved_plan_execution_input = false;
        let mut last_approved_plan_summary_status: Option<ExecutionSummaryStatus> = None;
        let mut last_turn_result: Option<RunLoopTurnLoopResult> = None;
        let mut last_turn_response_was_fallback = false;
        let mut last_turn_diagnostics = None;

        if !startup_update_requested_restart {
            loop {
                let mut executing_approved_plan = approved_plan_execution_turn;
                approved_plan_execution_turn = false;
                use crate::agent::runloop::unified::turn::session::interaction_loop::InteractionOutcome;

                // The approval turn may have ended before the handoff state
                // was fully applied (for example after recovery or a stale
                // primary-agent catalog). Re-establish the execution boundary
                // before the queued implementation turn can build its request.
                // This prevents a read-only `plan` agent from surviving the
                // approval and denying the first shell/edit call.
                if executing_approved_plan {
                    if tool_registry.is_planning_active() {
                        let plan = match crate::agent::runloop::unified::planning_workflow::load_plan_text_for_approval(
                            &tool_registry,
                        )
                        .await
                        {
                            Ok(plan) => plan,
                            Err(error) => {
                                pending_approved_plan_execution_input = false;
                                harness_try!(renderer.line(
                                    MessageStyle::Error,
                                    &format!("Approved-plan execution is blocked: {error}"),
                                ));
                                continue;
                            }
                        };
                        if let Err(error) =
                            crate::agent::runloop::unified::planning_workflow::complete_approved_plan_handoff(
                                &tool_registry,
                                &mut plan_session,
                                &handle,
                                plan,
                                crate::agent::runloop::unified::planning_workflow::PlanExecutionTarget::build(
                                    crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Current,
                                    session_skip_confirmations,
                                ),
                            )
                            .await
                        {
                            harness_try!(
                                renderer
                                    .line(MessageStyle::Error, &format!("Approved-plan execution is blocked: {error}"))
                            );
                            pending_approved_plan_execution_input = false;
                            continue;
                        }
                    }
                    let configured_default = vt_cfg
                        .as_ref()
                        .map(|cfg| cfg.default_primary_agent.as_str())
                        .filter(|name| !name.trim().is_empty());
                    let current_agent = active_primary_agent.active().name().to_string();
                    // Selection failure must stay recoverable so an approved
                    // plan is not lost to a hard session abort.
                    let execution_agent = match select_approved_plan_execution_agent(
                        &mut active_primary_agent,
                        &tool_registry,
                        &config.workspace,
                        Some(current_agent.as_str()),
                        configured_default,
                    )
                    .await
                    {
                        Ok(agent) => agent,
                        Err(err) => {
                            let msg = report_plan_approval_selection_failure(&current_agent, &err);
                            harness_try!(renderer.line(MessageStyle::Error, &msg));
                            pending_approved_plan_execution_input = false;
                            let session_id = tool_registry.harness_context_snapshot().session_id;
                            record_plan_selection_failure_tail(
                                &mut runtime,
                                &mut session_archive,
                                &session_stats,
                                &loaded_skills,
                                next_checkpoint_turn,
                                config.workspace.as_path(),
                                &session_id,
                                vt_cfg.as_ref(),
                                harness_config.max_tool_wall_clock_secs,
                            )
                            .await;
                            continue;
                        }
                    };
                    if current_agent != execution_agent {
                        harness_try!(renderer.line(
                            MessageStyle::Info,
                            &format!("Approved plan requires a write-capable agent; switching to {execution_agent}."),
                        ));
                    }
                    if let Err(err) = force_reload_workspace_config_for_execution(
                        config.workspace.as_path(),
                        &config,
                        &mut vt_cfg,
                        &mut tool_registry,
                        async_mcp_manager.as_deref(),
                    )
                    .await
                    {
                        tracing::warn!(error = %err, "Failed to reload workspace configuration before approved-plan execution");
                        harness_try!(
                            renderer.line(MessageStyle::Error, &format!("Failed to reload configuration: {err}"))
                        );
                    }
                    sync_primary_agent_permissions(&mut vt_cfg, active_primary_agent.active());
                    apply_primary_agent_tool_policy_overrides(&tool_registry, active_primary_agent.active()).await;
                    let mut runtime_sync = PrimaryAgentRuntimeSyncContext {
                        config: &config,
                        vt_cfg: vt_cfg.as_ref(),
                        thread_id: &turn_run_id.0,
                        active_primary_agent: active_primary_agent.active(),
                        lifecycle_hooks: &mut lifecycle_hooks,
                        async_mcp_manager: async_mcp_manager.as_ref(),
                        tool_registry: &mut tool_registry,
                        tools: &tools,
                        tool_catalog: &tool_catalog,
                        mcp_catalog_initialized: &mut mcp_catalog_initialized,
                        pending_mcp_refresh: &mut pending_mcp_refresh,
                        provider_client: &*provider_client,
                    };
                    harness_try!(sync_primary_agent_runtime(&mut runtime_sync).await);
                    let display = active_primary_agent.active().display_name.clone();
                    let color = active_primary_agent.active().color.clone().filter(|c| !c.trim().is_empty());
                    handle.set_primary_agent(Some(display), color);
                }

                if let Some(controller) = tool_registry.subagent_controller() {
                    controller.set_parent_messages(&runtime.state.messages).await;
                }

                if crate::agent::runloop::unified::stop_requests::stop_outcome(&ctrl_c_state).is_none()
                    && !matches!(last_turn_result, Some(RunLoopTurnLoopResult::Cancelled | RunLoopTurnLoopResult::Exit))
                    && pending_background_completions.should_schedule_continuation(
                        !queued_inputs.is_empty() || !session.events.is_empty(),
                        runtime.has_pending_follow_up_inputs(),
                    )
                {
                    match runtime.try_queue_follow_up_input(background_completion_continuation_prompt()) {
                        Ok(()) => pending_background_completions.mark_continuation_queued(),
                        Err(error) => tracing::warn!(%error, "Unable to queue background completion continuation"),
                    }
                }

                let mut active_task_follow_up = false;
                let can_resume = !ctrl_c_state.is_cancel_handled()
                    && crate::agent::runloop::unified::stop_requests::stop_outcome(&ctrl_c_state).is_none();
                let interaction_outcome = if pending_approved_plan_execution_input && can_resume {
                    // An approved-plan handoff is an internal state transition,
                    // not ordinary user steering. Consume it directly so a
                    // full or reordered steering FIFO cannot leave the newly
                    // selected build agent waiting for another `continue`.
                    pending_approved_plan_execution_input = false;
                    let (input, prompt_message_index) = append_approved_plan_execution_input(&mut runtime);
                    let turn_id = SessionId::generate().into_inner();
                    InteractionOutcome::Continue {
                        input,
                        prompt_message_index: Some(prompt_message_index),
                        turn_id,
                    }
                } else if let Some(input) = can_resume.then(|| runtime.run_until_idle()).flatten() {
                    active_task_follow_up = true;
                    let turn_id = SessionId::generate().into_inner();
                    InteractionOutcome::Continue { input, prompt_message_index: None, turn_id }
                } else {
                    let mut interaction_turn_metadata_cache = None;
                    let background_completion_notify = tool_registry
                        .subagent_controller()
                        .map(|controller| controller.background_completion_notify());
                    let (session_state, runtime_steering) = runtime.split_mut();
                    let mut interaction_ctx =
                        crate::agent::runloop::unified::turn::session::interaction_loop::InteractionLoopContext {
                            thread_id: &turn_run_id.0,
                            active_thread_label,
                            thread_handle: &thread_handle,
                            renderer: &mut renderer,
                            session: &mut session,
                            handle: &handle,
                            header_context: &mut header_context,
                            ctrl_c_state: &ctrl_c_state,
                            ctrl_c_notify: &ctrl_c_notify,
                            input_activity_counter: &input_activity_counter,
                            config: &mut config,
                            vt_cfg: &mut vt_cfg,
                            provider_client: &mut provider_client,
                            session_bootstrap: &session_bootstrap,
                            async_mcp_manager: &async_mcp_manager,
                            tool_registry: &mut tool_registry,
                            tools: &tools,
                            tool_catalog: &tool_catalog,
                            conversation_history: std::sync::Arc::make_mut(&mut session_state.messages),
                            agent_touched_paths: &mut agent_touched_paths,
                            decision_ledger: &decision_ledger,
                            context_manager: &mut context_manager,
                            active_primary_agent: &mut active_primary_agent,
                            session_stats: &mut session_stats,
                            plan_session: &mut plan_session,
                            mcp_panel_state: &mut mcp_panel_state,
                            linked_directories: &mut linked_directories,
                            lifecycle_hooks: &mut lifecycle_hooks,
                            full_auto,
                            skip_confirmations: session_skip_confirmations,
                            approval_recorder: &approval_recorder,
                            tool_permission_cache: &tool_permission_cache,
                            permissions_state: &permissions_state,
                            loaded_skills: &loaded_skills,
                            default_placeholder: &mut default_placeholder,
                            follow_up_placeholder: &mut follow_up_placeholder,
                            checkpoint_manager: checkpoint_manager.as_ref(),
                            tool_result_cache: &tool_result_cache,
                            traj: &traj,
                            harness_emitter: harness_emitter.as_ref(),
                            safety_validator: &safety_validator,
                            circuit_breaker: &circuit_breaker,
                            tool_health_tracker: &tool_health_tracker,
                            rate_limiter: &rate_limiter,
                            telemetry: &telemetry,
                            autonomous_executor: &autonomous_executor,
                            error_recovery: &error_recovery,
                            last_forced_redraw: &mut last_forced_redraw,
                            turn_metadata_cache: &mut interaction_turn_metadata_cache,
                            harness_config: harness_config.clone(),
                            runtime_steering,
                            webmcp_prompt_receiver: &mut webmcp_prompt_receiver,
                            webmcp_prompt_sender: &webmcp_prompt_sender,
                            webmcp_bridge: &mut webmcp_bridge,
                            startup_update_notice_rx: &mut startup_update_notice_rx,
                            editor_open_sender: &editor_open_sender,
                            editor_open_dispatcher: editor_open_dispatcher.clone(),
                            background_completion_notify,
                            exec_completion_notify: exec_completion_notify.clone(),
                            background_completion_receiver: &mut background_completion_receiver,
                            exec_completion_receiver: &mut exec_completion_receiver,
                            pending_background_completions: &mut pending_background_completions,
                        };

                    let mut interaction_state =
                        crate::agent::runloop::unified::turn::session::interaction_loop::InteractionState {
                            input_status_state: &mut input_status_state,
                            dismissed_memory_cleanup_fingerprint: &mut dismissed_memory_cleanup_fingerprint,
                            queued_inputs: &mut queued_inputs,
                            prefer_latest_queued_input_once: &mut prefer_latest_queued_input_once,
                            model_picker_state: &mut model_picker_state,
                            palette_state: &mut palette_state,
                            last_known_mcp_tools: &mut last_known_mcp_tools,
                            pending_mcp_refresh: &mut pending_mcp_refresh,
                            mcp_catalog_initialized: &mut mcp_catalog_initialized,
                            last_mcp_refresh: &mut last_mcp_refresh,
                            ctrl_c_notice_displayed: &mut ctrl_c_notice_displayed,
                            inline_prompt_cost_notice_shown: &mut inline_prompt_cost_notice_shown,
                        };

                    harness_try!(
                        crate::agent::runloop::unified::turn::session::interaction_loop::run_interaction_loop(
                            &mut interaction_ctx,
                            &mut interaction_state,
                        )
                        .await
                    )
                };
                // User-driven mode switches are applied inside the interaction
                // loop. Capture them before any turn outcome can block or
                // hand off so an archive resume never falls back to Duck/Plan.
                persist_primary_agent(&mut session_archive, &active_primary_agent);
                if input_status_state.is_blocked {
                    input_status_state.is_blocked = false;
                    handle.set_placeholder(default_placeholder.clone());
                    handle.set_activity_state(ActivityState::Idle);
                }
                let turn_progress =
                    handle.resume_progress(vtcode_commons::ui_protocol::ProgressPhase::PreparingContext);
                let preparation_started_at = Instant::now();
                let (next_turn_input, completed_turn_prompt_message_index) = match interaction_outcome {
                    InteractionOutcome::Exit { reason } => {
                        session_end_reason = reason;
                        break;
                    }
                    InteractionOutcome::Resume { resume_session } => {
                        resume_state = Some(*resume_session);
                        session_end_reason = SessionEndReason::Completed;
                        break;
                    }
                    InteractionOutcome::BackgroundCompletionReady => continue,
                    InteractionOutcome::DirectToolHandled => continue,
                    InteractionOutcome::DirectBackgroundToolHandled { completion_identity } => {
                        pending_background_completions.suppress_autonomous_continuation(completion_identity);
                        continue;
                    }
                    InteractionOutcome::Continue { input, prompt_message_index, turn_id: next_turn_id } => {
                        turn_id = next_turn_id;
                        (input, prompt_message_index)
                    }
                    InteractionOutcome::PlanApproved { target } => {
                        // This approval path starts the implementation turn in
                        // the same outer iteration, so mark it before the
                        // HarnessTurnState is constructed below. The queued
                        // approval path sets the equivalent flag on the next
                        // iteration.
                        executing_approved_plan = true;
                        let plan = match crate::agent::runloop::unified::planning_workflow::load_plan_text_for_approval(
                            &tool_registry,
                        )
                        .await
                        {
                            Ok(plan) => plan,
                            Err(error) => {
                                handle.set_activity_state(ActivityState::Idle);
                                harness_try!(renderer.line(
                                    MessageStyle::Error,
                                    &format!("Approved-plan execution is blocked: {error}. The plan was retained; please retry approval."),
                                ));
                                transition_to_planning_workflow(
                                    &tool_registry,
                                    &mut session_stats,
                                    &mut plan_session,
                                    &handle,
                                    PlanningEntrySource::AgentSelection,
                                    Some(active_primary_agent.active().name().to_string()),
                                    vt_cfg.as_ref().map(|cfg| cfg.default_primary_agent.clone()),
                                    false,
                                    false,
                                )
                                .await;
                                continue;
                            }
                        };
                        if let Err(error) =
                            crate::agent::runloop::unified::planning_workflow::complete_approved_plan_handoff(
                                &tool_registry,
                                &mut plan_session,
                                &handle,
                                plan,
                                target,
                            )
                            .await
                        {
                            handle.set_activity_state(ActivityState::Idle);
                            harness_try!(renderer.line(
                                MessageStyle::Error,
                                &format!("Approved-plan execution is blocked: {error}. The plan was retained; please retry approval."),
                            ));
                            transition_to_planning_workflow(
                                &tool_registry,
                                &mut session_stats,
                                &mut plan_session,
                                &handle,
                                PlanningEntrySource::AgentSelection,
                                Some(active_primary_agent.active().name().to_string()),
                                vt_cfg.as_ref().map(|cfg| cfg.default_primary_agent.clone()),
                                false,
                                false,
                            )
                            .await;
                            continue;
                        }
                        crate::agent::runloop::unified::planning_workflow::resolve_plan_approval(
                            &mut plan_session,
                            harness_emitter.as_ref(),
                            &turn_run_id.0,
                            &turn_id,
                            crate::agent::runloop::unified::planning_workflow::plan_approval_decision_for_target(
                                target,
                            ),
                            false,
                        );
                        let execution_context = target.execution_context;
                        let skip_confirmations = target.skip_confirmations;
                        let fresh_context = matches!(
                            execution_context,
                            crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Fresh
                        );
                        if fresh_context {
                            handle.set_activity_state(ActivityState::PreparingFreshExecutionThread);
                        }
                        let plan_seed = load_active_plan_seed(&tool_registry).await;
                        if fresh_context && plan_seed.is_none() {
                            handle.set_activity_state(ActivityState::Idle);
                            harness_try!(renderer.line(
                                MessageStyle::Error,
                                "Fresh execution could not start because the approved plan was not found. The plan was retained; please retry approval.",
                            ));
                            transition_to_planning_workflow(
                                &tool_registry,
                                &mut session_stats,
                                &mut plan_session,
                                &handle,
                                PlanningEntrySource::AgentSelection,
                                Some(active_primary_agent.active().name().to_string()),
                                vt_cfg.as_ref().map(|cfg| cfg.default_primary_agent.clone()),
                                false,
                                false,
                            )
                            .await;
                            continue;
                        }
                        let plan_seed = if fresh_context {
                            load_active_plan_seed(&tool_registry).await.or(plan_seed)
                        } else {
                            plan_seed
                        };
                        let current_context_budget = vtcode_core::compaction::effective_context_budget(
                            vt_cfg.as_ref(),
                            provider_client.as_ref(),
                            &crate::agent::runloop::unified::turn::turn_processing::resolve_effective_request_model(
                                &config.model,
                                active_primary_agent.active(),
                            ),
                        );
                        let previous_context_usage_percent =
                            context_manager.context_usage_percent(current_context_budget);
                        let configured_default = vt_cfg
                            .as_ref()
                            .map(|cfg| cfg.default_primary_agent.as_str())
                            .filter(|name| !name.trim().is_empty());
                        let requested_agent = target.agent_name();
                        let resolved_execution_agent = match select_approved_plan_execution_agent(
                            &mut active_primary_agent,
                            &tool_registry,
                            &config.workspace,
                            Some(requested_agent),
                            configured_default,
                        )
                        .await
                        {
                            Ok(agent) => agent,
                            Err(err) if fresh_context => {
                                handle.set_activity_state(ActivityState::Idle);
                                harness_try!(renderer.line(
                                    MessageStyle::Error,
                                    &format!("Fresh execution could not select a build agent: {err}"),
                                ));
                                transition_to_planning_workflow(
                                    &tool_registry,
                                    &mut session_stats,
                                    &mut plan_session,
                                    &handle,
                                    PlanningEntrySource::AgentSelection,
                                    Some(active_primary_agent.active().name().to_string()),
                                    vt_cfg.as_ref().map(|cfg| cfg.default_primary_agent.clone()),
                                    false,
                                    false,
                                )
                                .await;
                                let session_id = tool_registry.harness_context_snapshot().session_id;
                                record_plan_selection_failure_tail(
                                    &mut runtime,
                                    &mut session_archive,
                                    &session_stats,
                                    &loaded_skills,
                                    next_checkpoint_turn,
                                    config.workspace.as_path(),
                                    &session_id,
                                    vt_cfg.as_ref(),
                                    harness_config.max_tool_wall_clock_secs,
                                )
                                .await;
                                continue;
                            }
                            Err(err) => {
                                let msg = report_plan_approval_selection_failure(requested_agent, &err);
                                harness_try!(renderer.line(MessageStyle::Error, &msg));
                                let session_id = tool_registry.harness_context_snapshot().session_id;
                                record_plan_selection_failure_tail(
                                    &mut runtime,
                                    &mut session_archive,
                                    &session_stats,
                                    &loaded_skills,
                                    next_checkpoint_turn,
                                    config.workspace.as_path(),
                                    &session_id,
                                    vt_cfg.as_ref(),
                                    harness_config.max_tool_wall_clock_secs,
                                )
                                .await;
                                continue;
                            }
                        };
                        if requested_agent != resolved_execution_agent.as_str() {
                            tracing::warn!(
                                requested_agent = ?requested_agent,
                                resolved_agent = ?resolved_execution_agent,
                                "Approved plan requested a non-executable primary agent; using a write-capable agent"
                            );
                            harness_try!(renderer.line(
                                MessageStyle::Info,
                                &format!(
                                    "Approved plan requires a write-capable agent; switching to {}.",
                                    resolved_execution_agent
                                ),
                            ));
                        }
                        if let Err(err) = force_reload_workspace_config_for_execution(
                            config.workspace.as_path(),
                            &config,
                            &mut vt_cfg,
                            &mut tool_registry,
                            async_mcp_manager.as_deref(),
                        )
                        .await
                        {
                            tracing::warn!("Failed to reload workspace configuration at plan approval: {}", err);
                            harness_try!(
                                renderer.line(MessageStyle::Error, &format!("Failed to reload configuration: {err}"))
                            );
                        }

                        sync_primary_agent_permissions(&mut vt_cfg, active_primary_agent.active());
                        apply_primary_agent_tool_policy_overrides(&tool_registry, active_primary_agent.active()).await;
                        let mut runtime_sync = PrimaryAgentRuntimeSyncContext {
                            config: &config,
                            vt_cfg: vt_cfg.as_ref(),
                            thread_id: &turn_run_id.0,
                            active_primary_agent: active_primary_agent.active(),
                            lifecycle_hooks: &mut lifecycle_hooks,
                            async_mcp_manager: async_mcp_manager.as_ref(),
                            tool_registry: &mut tool_registry,
                            tools: &tools,
                            tool_catalog: &tool_catalog,
                            mcp_catalog_initialized: &mut mcp_catalog_initialized,
                            pending_mcp_refresh: &mut pending_mcp_refresh,
                            provider_client: &*provider_client,
                        };
                        if let Err(err) = sync_primary_agent_runtime(&mut runtime_sync).await {
                            if fresh_context {
                                handle.set_activity_state(ActivityState::Idle);
                                harness_try!(renderer.line(
                                    MessageStyle::Error,
                                    &format!("Fresh execution could not restore the build runtime: {err}"),
                                ));
                                transition_to_planning_workflow(
                                    &tool_registry,
                                    &mut session_stats,
                                    &mut plan_session,
                                    &handle,
                                    PlanningEntrySource::AgentSelection,
                                    Some(active_primary_agent.active().name().to_string()),
                                    vt_cfg.as_ref().map(|cfg| cfg.default_primary_agent.clone()),
                                    false,
                                    false,
                                )
                                .await;
                                continue;
                            }
                            tracing::error!(error = %err, "approved-plan runtime restoration failed");
                            session_end_reason = SessionEndReason::Error;
                            break;
                        }
                        if fresh_context {
                            handle.set_activity_state(ActivityState::RestoringApprovedPlan);
                            runtime.clear_pending_follow_up_inputs();
                            runtime.state.clear_conversation_history();
                            context_manager.reset_for_fresh_execution();
                            session_stats.reset_for_fresh_execution();
                            let build_tool_limit = effective_max_tool_calls_for_approved_plan_execution(
                                harness_config.max_tool_calls_per_turn,
                            );
                            let max_session_turns = vt_cfg
                                .as_ref()
                                .map(|cfg| cfg.agent.max_conversation_turns)
                                .unwrap_or(vtcode_config::constants::defaults::DEFAULT_MAX_CONVERSATION_TURNS);
                            let (max_per_turn, max_per_session) =
                                resolve_safety_tool_call_limits(build_tool_limit, max_session_turns, false);
                            safety_validator.reset_for_fresh_execution(max_per_turn, max_per_session);
                            crate::agent::runloop::unified::planning_workflow::emit_context_reset(
                                harness_emitter.as_ref(),
                                turn_run_id.0.clone(),
                                turn_id.clone(),
                                previous_context_usage_percent,
                            );
                        }
                        let execution_display = active_primary_agent.active().display_name.clone();
                        let execution_color =
                            active_primary_agent.active().color.clone().filter(|c| !c.trim().is_empty());
                        handle.set_primary_agent(Some(execution_display), execution_color);
                        session_skip_confirmations = skip_confirmations;
                        handle.set_skip_confirmations(skip_confirmations);
                        if fresh_context {
                            handle.set_activity_state(ActivityState::StartingBuild);
                        }
                        harness_try!(renderer.line(MessageStyle::Info, "Executing approved plan..."));

                        let execution_directive =
                            build_approved_plan_execution_prompt(execution_context, plan_seed.as_deref());
                        runtime
                            .state
                            .messages_mut()
                            .push(vtcode_core::llm::provider::Message::system(execution_directive));
                        handle.set_activity_state(ActivityState::Building);
                        let (input, prompt_message_index) = append_approved_plan_execution_input(&mut runtime);
                        (input, Some(prompt_message_index))
                    }
                };
                if next_turn_input.trim().is_empty() {
                    continue;
                }
                if let Some(emitter) = harness_emitter.as_ref() {
                    use vtcode_core::exec::events::InputOrigin;
                    let origin = if executing_approved_plan {
                        InputOrigin::PlanApproval
                    } else if crate::agent::runloop::unified::turn::is_internal_harness_follow_up(&next_turn_input) {
                        InputOrigin::Continuation
                    } else if active_task_follow_up {
                        InputOrigin::Correction
                    } else {
                        InputOrigin::User
                    };
                    let task_id = emitter.begin_task_turn(&turn_id, &next_turn_input, origin)?;
                    if let Some(validator) = emitter.decision_validator() {
                        tool_registry.set_decision_evidence_validator(validator);
                    }
                    tool_registry.set_harness_task(Some(task_id));
                }
                let (session_state, runtime_steering) = runtime.split_mut();
                let working_history = std::sync::Arc::make_mut(&mut session_state.messages);
                let refused_turn_rollback = RefusedTurnRollback::capture(
                    working_history,
                    completed_turn_prompt_message_index,
                    &next_turn_input,
                );
                macro_rules! preparation_wait {
                    ($future:expr) => {{
                        match crate::agent::runloop::unified::stop_requests::await_with_stop(
                            &ctrl_c_state,
                            &ctrl_c_notify,
                            $future,
                        )
                        .await
                        {
                            Some(value) => value,
                            None => {
                                if let Some(emitter) = harness_emitter.as_ref() {
                                    let _ = emitter.emit(
                                        crate::agent::runloop::unified::inline_events::harness::turn_failed_event(
                                            "turn cancelled during preparation",
                                            None,
                                        ),
                                    );
                                }
                                // Preparation has not admitted tools. Keep the user's
                                // request as a draft, and require a new submission.
                                handle.set_placeholder(Some(
                                    vtcode_config::constants::ui::CHAT_INPUT_PLACEHOLDER_INTERRUPTED.to_owned(),
                                ));
                                handle.set_activity_state(ActivityState::Idle);
                                last_turn_result = Some(RunLoopTurnLoopResult::Cancelled);
                                if ctrl_c_state.is_exit_requested() {
                                    session_end_reason = SessionEndReason::Exit;
                                    break;
                                }
                                ctrl_c_state.mark_cancel_handled();
                                session_end_reason = SessionEndReason::Cancelled;
                                continue;
                            }
                        }
                    }};
                }
                let workspace_buf = config.workspace.clone();
                let touched_clone = agent_touched_paths.clone();
                let dirty_worktree_task = tokio::task::spawn_blocking(move || {
                    build_unrelated_dirty_worktree_note(&workspace_buf, &touched_clone)
                });
                let checkpoint_started_at = Instant::now();
                let _prompt_checkpoint_lease = if let Some(manager) = checkpoint_manager.as_ref() {
                    handle.set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::SavingCheckpoint);
                    let prefix = completed_turn_prompt_message_index
                        .unwrap_or(working_history.len())
                        .min(working_history.len());
                    let conversation: Vec<_> = working_history[..prefix].iter().map(SessionMessage::from).collect();
                    let session_id = tool_registry.harness_context_snapshot().session_id;
                    // A missing lease (cooperative cancellation) shares the
                    // unavailable-checkpoint recovery below: retain the prompt
                    // and retry instead of panicking on a supposedly
                    // impossible arm.
                    let lease = match preparation_wait!(manager.begin_prompt_with_cancellation(
                        next_checkpoint_turn,
                        &session_id,
                        &next_turn_input,
                        &conversation,
                        CancellationToken::new()
                    ))
                    .and_then(|lease| lease.context("checkpoint preparation cancelled"))
                    {
                        Ok(lease) => lease,
                        Err(err) => {
                            // Drain the independent worker before retrying this prompt.
                            let _ = preparation_wait!(dirty_worktree_task);
                            tracing::warn!(error = %err, "Checkpoint unavailable; prompt retained in input");
                            let message = checkpoint_unavailable_notice(&format!("{err:#}"));
                            let _ = renderer.line(MessageStyle::Info, message);
                            // The prompt message was already appended to history by the
                            // interaction loop (or the approved-plan handoff). Remove it
                            // so a retry does not duplicate, and restore the text.
                            if let Some(index) = completed_turn_prompt_message_index {
                                if index < working_history.len() {
                                    working_history.truncate(index);
                                }
                                handle.set_input(next_turn_input.clone());
                                handle.force_redraw();
                            } else if working_history.last().is_some_and(|message| {
                                message.role == MessageRole::User
                                    && message.content.as_text().trim() == next_turn_input.trim()
                            }) {
                                working_history.pop();
                                handle.set_input(next_turn_input.clone());
                                handle.force_redraw();
                            }
                            continue;
                        }
                    };
                    if let Some(emitter) = harness_emitter.as_ref() {
                        let _ = emitter.emit(harness_event(
                            vtcode_core::exec::events::HarnessEventKind::SnapshotCreated,
                            Some(format!("Before prompt {next_checkpoint_turn} snapshot saved")),
                            None,
                            None,
                            None,
                        ));
                    }
                    next_checkpoint_turn = next_checkpoint_turn.saturating_add(1);
                    Some(lease)
                } else {
                    None
                };
                tracing::debug!(target: "vtcode.response_latency", operation_id = turn_progress.operation().id(),
                    checkpoint_ms = checkpoint_started_at.elapsed().as_secs_f64() * 1000.0, "checkpoint preparation complete");
                handle.set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::PreparingContext);
                let unrelated_dirty_note = match preparation_wait!(dirty_worktree_task) {
                    Ok(Ok(Some(note))) => Some(note),
                    Ok(Err(err)) => {
                        tracing::warn!(
                            error = %err,
                            "Failed to inspect unrelated dirty worktree entries before turn"
                        );
                        None
                    }
                    Err(error) => {
                        tracing::warn!(%error, "Dirty worktree inspection worker failed");
                        None
                    }
                    Ok(Ok(None)) => None,
                };
                let transient_system_notes = preparation_wait!(prepare_transient_turn_notes(
                    config.workspace.as_path(),
                    &tool_registry,
                    unrelated_dirty_note,
                    pending_background_completions.take_transient_note(),
                ));
                working_history.extend(
                    transient_system_notes
                        .iter()
                        .cloned()
                        .map(vtcode_core::llm::provider::Message::system),
                );
                tracing::debug!(target: "vtcode.response_latency", operation_id = turn_progress.operation().id(),
                    preparation_ms = preparation_started_at.elapsed().as_secs_f64() * 1000.0,
                    accepted_to_prepared_ms = turn_progress.operation().started_at().elapsed().as_secs_f64() * 1000.0,
                    "turn preparation complete");
                let turn_started_at = Instant::now();
                let history_snapshot_bytes = estimate_history_bytes(working_history);
                let mut turn_metadata_cache = None;
                let planning_active = tool_registry.is_planning_active();
                // Cross-turn tracking data and aborted diagnostics are
                // extracted before harness_state goes out of scope.
                let (
                    turn_result,
                    cross_turn_read_sigs,
                    cross_turn_written,
                    cross_turn_shell_cmd,
                    cross_turn_failed_shell_key,
                    cross_turn_out_of_band_progress,
                    session_limit_granted,
                    aborted_turn_diagnostics,
                ) = {
                    let mut auto_finish_planning_attempted = false;
                    let max_tool_calls_per_turn = if executing_approved_plan {
                        effective_max_tool_calls_for_approved_plan_execution(harness_config.max_tool_calls_per_turn)
                    } else {
                        effective_max_tool_calls_for_turn(harness_config.max_tool_calls_per_turn, planning_active)
                    };
                    tool_registry.begin_patch_recovery_turn();
                    let mut harness_state = HarnessTurnState::new(
                        TurnRunId(turn_run_id.0.clone()),
                        TurnId(turn_id.clone()),
                        max_tool_calls_per_turn,
                        harness_config.max_tool_wall_clock_secs,
                        harness_config.max_tool_retries,
                    );
                    harness_state.set_approved_plan_execution(executing_approved_plan);
                    let mut turn_loop_ctx = crate::agent::runloop::unified::turn::TurnLoopContext::new(
                        &mut renderer,
                        &handle,
                        &mut session,
                        &mut session_stats,
                        &mut plan_session,
                        &mut auto_finish_planning_attempted,
                        &mut mcp_panel_state,
                        &tool_result_cache,
                        &approval_recorder,
                        &decision_ledger,
                        &mut tool_registry,
                        &tools,
                        &tool_catalog,
                        &ctrl_c_state,
                        &ctrl_c_notify,
                        &mut context_manager,
                        &mut last_forced_redraw,
                        &mut input_status_state,
                        lifecycle_hooks.as_ref(),
                        &default_placeholder,
                        &tool_permission_cache,
                        &permissions_state,
                        &safety_validator,
                        &circuit_breaker,
                        &tool_health_tracker,
                        &rate_limiter,
                        &telemetry,
                        &autonomous_executor,
                        &error_recovery,
                        &mut harness_state,
                        harness_emitter.as_ref(),
                        &mut config,
                        None,
                        &mut turn_metadata_cache,
                        &mut provider_client,
                        &traj,
                        &active_primary_agent,
                        session_skip_confirmations,
                        full_auto,
                        runtime_steering,
                    );
                    turn_loop_ctx.live_vt_cfg = Some(&mut vt_cfg);
                    let thread_id_owned = thread_handle.thread_id().to_string();
                    turn_loop_ctx.settings =
                        Some(crate::agent::runloop::unified::turn::turn_loop::ActiveSettingsContext {
                            receiver: &mut settings_receiver,
                            header_context: &mut header_context,
                            session_bootstrap: &session_bootstrap,
                            thread_id: thread_id_owned.as_str(),
                            thread_handle: &thread_handle,
                        });

                    let primary_agent_snapshot = active_primary_agent.active().clone();
                    let result =
                        crate::agent::runloop::unified::turn::run_turn_loop(working_history, turn_loop_ctx).await;

                    // Prompt overlays can receive queued mode events while the
                    // turn owns the input surface. Restore the write-capable
                    // agent selected at prompt time before handling the turn
                    // outcome; legitimate handoffs are applied below from the
                    // explicit outcome, not through leaked UI input.
                    if active_primary_agent.active() != &primary_agent_snapshot {
                        active_primary_agent.restore_snapshot(primary_agent_snapshot);
                    }

                    // Preserve authoritative turn state even when the turn
                    // loop fails before it can construct a normal outcome.
                    let aborted_turn_diagnostics = result
                        .is_err()
                        .then(|| harness_state.snapshot_turn_diagnostics(Default::default(), 0));
                    (
                        result,
                        harness_state
                            .seen_successful_readonly_signatures
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>(),
                        harness_state.recently_written_files.clone(),
                        harness_state.last_admitted_shell_command_signature.clone(),
                        harness_state.last_failed_shell_key().map(str::to_owned),
                        harness_state.has_out_of_band_tool_progress(),
                        harness_state.has_session_limit_grant(),
                        aborted_turn_diagnostics,
                    )
                };
                drop(turn_progress);
                let outcome = match turn_result {
                    Ok(outcome) => outcome,
                    Err(err) => {
                        crate::agent::runloop::unified::status_line::clear_input_status(
                            &handle,
                            &mut input_status_state,
                        );
                        handle.set_activity_state(ActivityState::Idle);
                        let _ = renderer.line_if_not_empty(MessageStyle::Output);
                        handle.program_status(vtcode_commons::program_status::ProgramStatusUpdate::Outcome(
                            vtcode_commons::program_status::ProgramState::Error,
                        ));
                        tracing::error!("Turn execution error: {}", err);
                        let _ = renderer.line(MessageStyle::Error, &format!("Error: {err}"));
                        TurnLoopOutcome {
                            result: RunLoopTurnLoopResult::Aborted,
                            turn_modified_files: std::collections::BTreeSet::new(),
                            turn_touched_files: std::collections::BTreeSet::new(),
                            turn_diagnostics: aborted_turn_diagnostics.unwrap_or_default(),
                            pending_primary_agent: None,
                            pending_plan_execution_target: None,
                            plan_approved_execution_pending: false,
                            final_response_was_fallback: false,
                            refused: false,
                        }
                    }
                };
                if session_limit_granted && !matches!(&outcome.result, RunLoopTurnLoopResult::Blocked { .. }) {
                    // A successful grant resumes the pending Build turn. Do
                    // not leave the previous blocked placeholder/state visible
                    // or let a grant-in-flight turn be finalized as no response.
                    input_status_state.is_blocked = false;
                    handle.set_placeholder(default_placeholder.clone());
                    handle.set_activity_state(ActivityState::Idle);
                }
                // A refused request must not stay in model-visible history:
                // the next request would replay it and be refused again. Roll
                // back before any post-turn history edits, checkpointing, or
                // persistence; the refusal notice already reached the
                // transcript and the harness event stream.
                let turn_refused = outcome.refused;
                let refused_turn_rolled_back = turn_refused && refused_turn_rollback.apply(working_history);
                if refused_turn_rolled_back {
                    // The rollback removed the turn's tool calls, not their
                    // effects on disk; name the files it changed so the next
                    // turn does not reason from stale contents.
                    if let Some(note) =
                        build_withdrawn_turn_changes_note(config.workspace.as_path(), &outcome.turn_modified_files)
                    {
                        working_history.push(vtcode_core::llm::provider::Message::system(note));
                    }
                } else {
                    remove_transient_system_notes(working_history, &transient_system_notes);
                }

                // Cross-turn loop detection: fingerprint this turn's actions and
                // inject a warning if a loop or stuck pattern is detected. The
                // warning describes activity from a rolled-back refused turn,
                // which the model no longer sees, so it is not injected then.
                if let Some(cross_turn_warning) = cross_turn_tracker.seal_turn_with_progress(
                    &cross_turn_read_sigs,
                    &cross_turn_written,
                    cross_turn_shell_cmd.as_deref(),
                    cross_turn_failed_shell_key.as_deref(),
                    cross_turn_out_of_band_progress,
                    planning_active,
                ) && !refused_turn_rolled_back
                {
                    tracing::warn!(warning = %cross_turn_warning, "Cross-turn loop detector triggered");
                    working_history.push(vtcode_core::llm::provider::Message::system(cross_turn_warning));
                }

                agent_touched_paths.extend(
                    outcome
                        .turn_modified_files
                        .iter()
                        .map(|path| normalize_workspace_path(config.workspace.as_path(), path)),
                );
                agent_touched_paths.extend(context_manager.tracked_instruction_activity_paths());
                let outcome_result = outcome.result.clone();
                let execution_modified_files = outcome.turn_modified_files.clone();
                let switch_primary_agent = outcome.pending_primary_agent.clone();
                let plan_execution_target = outcome.pending_plan_execution_target;
                let has_primary_agent_switch = switch_primary_agent.is_some() || plan_execution_target.is_some();
                let plan_execution_context = plan_execution_target.map_or(
                    crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Current,
                    |target| target.execution_context,
                );
                let plan_skip_confirmations = plan_execution_target.is_some_and(|target| target.skip_confirmations);
                let plan_approved_execution_pending = outcome.plan_approved_execution_pending;
                let final_response_was_fallback = outcome.final_response_was_fallback;
                last_turn_result = Some(outcome_result.clone());
                last_turn_response_was_fallback = final_response_was_fallback;
                let turn_elapsed = turn_started_at.elapsed();
                let mut turn_diagnostics = outcome.turn_diagnostics.clone();
                turn_diagnostics.elapsed_ms = turn_elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
                last_turn_diagnostics = Some(turn_diagnostics.clone());
                let show_turn_timer = vt_cfg.as_ref().map(|cfg| cfg.ui.show_turn_timer).unwrap_or(true);
                let harness_snapshot = tool_registry.harness_context_snapshot();
                if let Err(err) = crate::agent::runloop::unified::turn::apply_turn_outcome(
                    outcome,
                    crate::agent::runloop::unified::turn::TurnOutcomeContext {
                        conversation_history: std::sync::Arc::make_mut(&mut runtime.state.messages),
                        completed_turn_prompt: Some(next_turn_input.as_str()),
                        completed_turn_prompt_message_index,
                        renderer: &mut renderer,
                        handle: &handle,
                        ctrl_c_state: &ctrl_c_state,
                        default_placeholder: &default_placeholder,
                        checkpoint_manager: None,
                        next_checkpoint_turn: &mut next_checkpoint_turn,
                        session_end_reason: &mut session_end_reason,
                        turn_elapsed,
                        show_turn_timer,
                        session_id: &harness_snapshot.session_id,
                        runtime_turn_id: Some(&turn_id),
                        harness_emitter: harness_emitter.as_ref(),
                    },
                )
                .await
                {
                    tracing::error!("Failed to apply turn outcome: {}", err);
                    renderer
                        .line(MessageStyle::Error, &format!("Failed to finalize turn: {err}"))
                        .ok();
                }
                if executing_approved_plan {
                    handle.set_activity_state(ActivityState::Idle);
                }
                // Plan-mode "switch to build/auto agent" handoff: perform the
                // primary-agent switch now so the chosen agent executes the plan.
                // This mirrors the `PlanApproved` handoff in the interaction loop
                // (session.rs): mutate `active_primary_agent` and refresh the TUI
                // handle display. The full `handle_select_primary_agent` requires
                // `InteractionLoopContext`, which is unavailable here because the
                // plan-confirmation popup is rendered inside the turn loop rather
                // than the inline interaction loop.
                //
                // Plan *entry* (`start_planning` confirmation) also lands here via
                // `pending_primary_agent = "plan"`. That destination is the plan
                // agent itself, not an approved-plan execution agent, so it uses
                // direct selection instead of the write-capable resolver.
                let requested_agent_for_handoff = plan_execution_target
                    .map(|target| target.agent_name().to_owned())
                    .or(switch_primary_agent);
                if let Some(requested_agent) = requested_agent_for_handoff {
                    let plan_entry =
                        crate::agent::runloop::unified::turn::turn_loop::is_plan_entry_handoff(&requested_agent)
                            && plan_execution_target.is_none();
                    if plan_entry {
                        use crate::agent::runloop::unified::planning_workflow_state::PLAN_PRIMARY_AGENT_NAME;
                        use crate::agent::runloop::unified::turn::primary_agent_runtime::{
                            builtin_primary_agent_specs, load_primary_agent_specs,
                        };
                        let specs = match load_primary_agent_specs(&tool_registry, &config.workspace).await {
                            Ok(specs) if !specs.is_empty() => specs,
                            _ => builtin_primary_agent_specs(),
                        };
                        match active_primary_agent.select_from_specs(&specs, PLAN_PRIMARY_AGENT_NAME) {
                            Ok(active) => {
                                let agent_display = active.display_name.clone();
                                let color = active.color.clone().filter(|c| !c.trim().is_empty());
                                apply_primary_agent_tool_policy_overrides(
                                    &tool_registry,
                                    active_primary_agent.active(),
                                )
                                .await;
                                sync_primary_agent_permissions(&mut vt_cfg, active_primary_agent.active());
                                let mut runtime_sync = PrimaryAgentRuntimeSyncContext {
                                    config: &config,
                                    vt_cfg: vt_cfg.as_ref(),
                                    thread_id: &turn_run_id.0,
                                    active_primary_agent: active_primary_agent.active(),
                                    lifecycle_hooks: &mut lifecycle_hooks,
                                    async_mcp_manager: async_mcp_manager.as_ref(),
                                    tool_registry: &mut tool_registry,
                                    tools: &tools,
                                    tool_catalog: &tool_catalog,
                                    mcp_catalog_initialized: &mut mcp_catalog_initialized,
                                    pending_mcp_refresh: &mut pending_mcp_refresh,
                                    provider_client: &*provider_client,
                                };
                                if let Err(err) = sync_primary_agent_runtime(&mut runtime_sync).await {
                                    tracing::error!(
                                        target: "vtcode.planning_workflow",
                                        switch_path = "plan_entry",
                                        requested_agent = %requested_agent,
                                        resolved_agent = %agent_display,
                                        error = %err,
                                        "Plan-entry runtime sync failed; header will still show Plan and planning stays active"
                                    );
                                    harness_try!(renderer.line(
                                        MessageStyle::Warning,
                                        &format!("Plan mode is active, but runtime sync failed: {err}"),
                                    ));
                                }
                                handle.set_primary_agent(Some(agent_display), color);
                                tracing::info!(
                                    target: "vtcode.planning_workflow",
                                    switch_path = "plan_entry",
                                    "Switched primary agent to plan after confirmed planning entry"
                                );
                                persist_primary_agent(&mut session_archive, &active_primary_agent);
                            }
                            Err(err) => {
                                tracing::warn!(
                                    target: "vtcode.planning_workflow",
                                    switch_path = "plan_entry",
                                    requested_agent = %requested_agent,
                                    error = %err,
                                    "Could not select plan primary agent after planning entry; planning stays active"
                                );
                                harness_try!(renderer.line(
                                    MessageStyle::Warning,
                                    &format!("Could not select plan primary agent after planning entry: {err}"),
                                ));
                            }
                        }
                    } else {
                        let configured_default = vt_cfg
                            .as_ref()
                            .map(|cfg| cfg.default_primary_agent.as_str())
                            .filter(|name| !name.trim().is_empty());
                        // Selection failure must stay recoverable: the plan is
                        // already approved, so aborting the session here would
                        // leave a half-switched state with no retry path. Run
                        // the turn-persistence tail before `continue` so a
                        // finished turn is checkpointed here, not deferred.
                        let execution_agent = match select_approved_plan_execution_agent(
                            &mut active_primary_agent,
                            &tool_registry,
                            &config.workspace,
                            Some(requested_agent.as_str()),
                            configured_default,
                        )
                        .await
                        {
                            Ok(agent) => agent,
                            Err(err) => {
                                let msg = report_plan_approval_selection_failure(&requested_agent, &err);
                                harness_try!(renderer.line(MessageStyle::Error, &msg));
                                // This site runs after the turn's work. Persist
                                // the real turn tail before abandoning the
                                // iteration so the just-finished turn is
                                // checkpointed (not deferred to the next turn).
                                persist_primary_agent(&mut session_archive, &active_primary_agent);
                                complete_turn_persistence_tail(TurnPersistenceTail {
                                    outcome: "aborted",
                                    history_snapshot_bytes,
                                    timeout_secs: harness_config.max_tool_wall_clock_secs,
                                    elapsed_ms: turn_elapsed.as_millis(),
                                    blocked_turn: false,
                                    turn_diagnostics: Some(turn_diagnostics),
                                    runtime: &mut runtime,
                                    session_archive: &mut session_archive,
                                    next_checkpoint_turn,
                                    session_stats: &session_stats,
                                    loaded_skills: &loaded_skills,
                                    workspace: config.workspace.as_path(),
                                    session_id: &harness_snapshot.session_id,
                                    vt_cfg: vt_cfg.as_ref(),
                                })
                                .await;
                                continue;
                            }
                        };
                        if execution_agent != requested_agent {
                            tracing::warn!(
                                target: "vtcode.planning_workflow",
                                switch_path = "plan_approval",
                                requested_agent = %requested_agent,
                                resolved_agent = %execution_agent,
                                "Approved plan requested a non-executable primary agent; using a write-capable agent"
                            );
                            harness_try!(renderer.line(
                                MessageStyle::Info,
                                &format!(
                                    "Approved plan requires a write-capable agent; switching to {}.",
                                    execution_agent
                                ),
                            ));
                        }
                        // The approval choice, rather than the destination agent
                        // name, owns confirmation policy. This keeps a manual
                        // Execute/Switch Build handoff prompting even if an
                        // earlier agent or fallback happens to be named `auto`.
                        session_skip_confirmations = plan_skip_confirmations;
                        handle.set_skip_confirmations(session_skip_confirmations);
                        sync_primary_agent_permissions(&mut vt_cfg, active_primary_agent.active());
                        apply_primary_agent_tool_policy_overrides(&tool_registry, active_primary_agent.active()).await;
                        let mut runtime_sync = PrimaryAgentRuntimeSyncContext {
                            config: &config,
                            vt_cfg: vt_cfg.as_ref(),
                            thread_id: &turn_run_id.0,
                            active_primary_agent: active_primary_agent.active(),
                            lifecycle_hooks: &mut lifecycle_hooks,
                            async_mcp_manager: async_mcp_manager.as_ref(),
                            tool_registry: &mut tool_registry,
                            tools: &tools,
                            tool_catalog: &tool_catalog,
                            mcp_catalog_initialized: &mut mcp_catalog_initialized,
                            pending_mcp_refresh: &mut pending_mcp_refresh,
                            provider_client: &*provider_client,
                        };
                        if let Err(err) = sync_primary_agent_runtime(&mut runtime_sync).await {
                            tracing::error!(
                                target: "vtcode.planning_workflow",
                                switch_path = "plan_approval",
                                agent = %execution_agent,
                                error = %err,
                                "Approved-plan runtime sync failed; plan remains approved and can be retried"
                            );
                            harness_try!(renderer.line(
                                MessageStyle::Error,
                                &format!("Approved plan is ready, but mode switch failed: {err}"),
                            ));
                        }
                        let agent_display = active_primary_agent.active().display_name.clone();
                        let color = active_primary_agent.active().color.clone().filter(|c| !c.trim().is_empty());
                        handle.set_primary_agent(Some(agent_display), color);
                        tracing::info!(
                            target: "vtcode.planning_workflow",
                            switch_path = "plan_approval",
                            agent = %execution_agent,
                            "Switched primary agent after plan approval"
                        );
                        persist_primary_agent(&mut session_archive, &active_primary_agent);
                    }
                }
                if plan_approved_execution_pending && !has_primary_agent_switch {
                    session_skip_confirmations = plan_skip_confirmations;
                    handle.set_skip_confirmations(session_skip_confirmations);
                }
                if plan_approved_execution_pending {
                    let fresh_context = matches!(
                        plan_execution_context,
                        crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Fresh
                    );
                    if fresh_context {
                        handle.set_activity_state(ActivityState::PreparingFreshExecutionThread);
                    }
                    let mut plan_seed = load_active_plan_seed(&tool_registry).await;
                    if fresh_context && plan_seed.is_none() {
                        handle.set_activity_state(ActivityState::Idle);
                        harness_try!(renderer.line(
                            MessageStyle::Error,
                            "Fresh execution could not start because the approved plan was not found. The plan was retained; please retry approval.",
                        ));
                        continue;
                    }
                    if fresh_context {
                        let current_context_budget = vtcode_core::compaction::effective_context_budget(
                            vt_cfg.as_ref(),
                            provider_client.as_ref(),
                            &crate::agent::runloop::unified::turn::turn_processing::resolve_effective_request_model(
                                &config.model,
                                active_primary_agent.active(),
                            ),
                        );
                        let previous_context_usage_percent =
                            context_manager.context_usage_percent(current_context_budget);
                        plan_seed = load_active_plan_seed(&tool_registry).await.or(plan_seed.take());
                        handle.set_activity_state(ActivityState::RestoringApprovedPlan);
                        runtime.clear_pending_follow_up_inputs();
                        runtime.state.clear_conversation_history();
                        context_manager.reset_for_fresh_execution();
                        session_stats.reset_for_fresh_execution();
                        let build_tool_limit = effective_max_tool_calls_for_approved_plan_execution(
                            harness_config.max_tool_calls_per_turn,
                        );
                        let max_session_turns = vt_cfg
                            .as_ref()
                            .map(|cfg| cfg.agent.max_conversation_turns)
                            .unwrap_or(vtcode_config::constants::defaults::DEFAULT_MAX_CONVERSATION_TURNS);
                        let (max_per_turn, max_per_session) =
                            resolve_safety_tool_call_limits(build_tool_limit, max_session_turns, false);
                        safety_validator.reset_for_fresh_execution(max_per_turn, max_per_session);
                        crate::agent::runloop::unified::planning_workflow::emit_context_reset(
                            harness_emitter.as_ref(),
                            turn_run_id.0.clone(),
                            turn_id.clone(),
                            previous_context_usage_percent,
                        );
                        handle.set_activity_state(ActivityState::StartingBuild);
                    }
                    approved_plan_execution_turn = true;
                    pending_approved_plan_execution_input = true;
                    let execution_directive =
                        build_approved_plan_execution_prompt(plan_execution_context, plan_seed.as_deref());
                    runtime
                        .state
                        .messages_mut()
                        .push(vtcode_core::llm::provider::Message::system(execution_directive));
                    handle.set_activity_state(ActivityState::Building);
                    persist_primary_agent(&mut session_archive, &active_primary_agent);
                }
                if executing_approved_plan {
                    let summary = approved_plan_execution_summary(
                        &tool_registry,
                        &outcome_result,
                        final_response_was_fallback,
                        !execution_modified_files.is_empty(),
                    )
                    .await;
                    last_approved_plan_summary_status = Some(summary.status);
                    let changed_files =
                        format_workspace_relative_paths(config.workspace.as_path(), &execution_modified_files);
                    let _ = renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Execution summary: {}; changed files: {changed_files}; verification: see the final response and task tracker; blockers: {}.",
                            summary.status.as_str(),
                            summary.blocker.as_deref().unwrap_or("none")
                        ),
                    );
                }
                // Stop can arrive after the provider finishes. Do not hold
                // exit behind cache locks or checkpoint maintenance; archive
                // workers retain ownership of their atomic writes.
                let checkpoint_outcome = match crate::agent::runloop::unified::stop_requests::await_with_stop(
                    &ctrl_c_state,
                    &ctrl_c_notify,
                    async {
                        vtcode_core::tools::cache::FILE_CACHE.check_pressure_and_evict().await;
                        tool_result_cache.write().await.check_pressure_and_evict();
                        let blocked_turn = matches!(&outcome_result, RunLoopTurnLoopResult::Blocked { .. });
                        persist_primary_agent(&mut session_archive, &active_primary_agent);
                        complete_turn_persistence_tail(TurnPersistenceTail {
                            outcome: match &outcome_result {
                                RunLoopTurnLoopResult::Completed { .. } => "completed",
                                RunLoopTurnLoopResult::Aborted => "aborted",
                                RunLoopTurnLoopResult::Cancelled => "cancelled",
                                RunLoopTurnLoopResult::Exit => "exit",
                                RunLoopTurnLoopResult::Blocked { .. } => "blocked",
                            },
                            history_snapshot_bytes,
                            timeout_secs: harness_config.max_tool_wall_clock_secs,
                            elapsed_ms: turn_elapsed.as_millis(),
                            blocked_turn,
                            turn_diagnostics: Some(turn_diagnostics),
                            runtime: &mut runtime,
                            session_archive: &mut session_archive,
                            next_checkpoint_turn,
                            session_stats: &session_stats,
                            loaded_skills: &loaded_skills,
                            workspace: config.workspace.as_path(),
                            session_id: &harness_snapshot.session_id,
                            vt_cfg: vt_cfg.as_ref(),
                        })
                        .await
                    },
                )
                .await
                {
                    Some(checkpoint) => checkpoint,
                    None => {
                        if ctrl_c_state.is_exit_requested() {
                            session_end_reason = SessionEndReason::Exit;
                            break;
                        }
                        ctrl_c_state.mark_cancel_handled();
                        session_end_reason = SessionEndReason::Cancelled;
                        last_turn_result = Some(RunLoopTurnLoopResult::Cancelled);
                        handle.set_activity_state(ActivityState::Idle);
                        continue;
                    }
                };
                // Tracker-aware outer auto-continue after checkpoint/persistence:
                // incomplete tracker work + recoverable turn end → queue the next
                // turn instead of nudging the user. Verification blocks keep their
                // existing recovery path first (handled below).
                // Set when tracker auto-queue was eligible but could not
                // resume (queue full / cross-turn budget exhausted) while
                // incomplete tracker steps remain. The exhausted-path info
                // line is the single user-facing nudge; suppress the generic
                // blocked-handoff "Type continue" stack and blocked placeholder
                // for this recoverable budget end.
                let mut tracker_auto_continue_exhausted = false;
                {
                    use crate::agent::runloop::unified::turn::tool_outcomes::helpers as tracker_continue;
                    let planning_active = tool_registry.is_planning_active();
                    let tracker_kill_switch =
                        !matches!(outcome_result, RunLoopTurnLoopResult::Cancelled | RunLoopTurnLoopResult::Exit)
                            && crate::agent::runloop::unified::stop_requests::stop_outcome(&ctrl_c_state).is_none()
                            && tracker_continue::tracker_auto_continue_enabled(vt_cfg.as_ref());
                    let is_verification_block = matches!(&outcome_result, RunLoopTurnLoopResult::Blocked { reason }
                    if reason.as_deref().is_some_and(|r| {
                        r.contains(
                            crate::agent::runloop::unified::turn::turn_loop::PENDING_VERIFICATION_BLOCK_REASON,
                        )
                    }));
                    let turn_completed = matches!(&outcome_result, RunLoopTurnLoopResult::Completed { .. });
                    let blocked_reason = match &outcome_result {
                        RunLoopTurnLoopResult::Blocked { reason } => reason.as_deref(),
                        _ => None,
                    };
                    let incomplete = if tracker_kill_switch && !planning_active {
                        let probe = tracker_continue::probe_tracker_incomplete(&tool_registry).await;
                        // Complete clears the cache so auto-queue stops after tracker finishes.
                        session_stats.apply_tracker_probe(probe).map(|items| items.to_vec())
                    } else {
                        None
                    };
                    // Progress-reset: any newly completed tracker step restores
                    // the cross-turn auto-continue episode budget plus the
                    // verification auto-recovery turn budget (failures preserved
                    // so a never-passing suite still escalates). Long-running
                    // work that keeps completing steps must not stall on stale
                    // verification misses for new work.
                    if tracker_kill_switch
                        && let Some(completed) = tracker_continue::tracker_completed_count(&tool_registry).await
                        && session_stats.note_tracker_completed_count(completed)
                    {
                        session_stats.reset_tracker_continuation_budget();
                        session_stats.reset_verification_auto_recovery_turns();
                    }
                    let max_turns = tracker_continue::tracker_cross_turn_turns(vt_cfg.as_ref());
                    // A rolled-back refused turn left no final text; the latest
                    // assistant message belongs to an earlier turn.
                    let final_text = if refused_turn_rolled_back {
                        None
                    } else {
                        latest_assistant_result_text(&runtime.state.messages)
                    };
                    // Adoption gate: a fresh informational turn must not be
                    // pulled into unrelated workspace tracker work. Completed
                    // turns only auto-queue tracker work adopted by this
                    // session (progressive-work request, recent tool activity,
                    // or follow-up). Blocked recoverable ends keep their own
                    // classifier; explicit resume bypasses this entirely.
                    let tracker_adoption_allowed =
                        crate::agent::runloop::unified::turn::context::tracker_continuation_adoption_allowed(
                            &runtime.state.messages,
                        );
                    let final_text_is_safety_handoff =
                        vtcode_core::core::agent::completion::tracker_final_text_is_safety_handoff(
                            final_text.as_deref().unwrap_or(""),
                        );
                    let final_text_requires_user_input =
                        vtcode_core::core::agent::completion::tracker_final_text_requires_user_input(
                            final_text.as_deref().unwrap_or(""),
                        );
                    let should_queue = tracker_continue::should_queue_tracker_auto_continue(
                        tracker_kill_switch,
                        planning_active,
                        turn_completed,
                        blocked_reason,
                        is_verification_block,
                        incomplete.as_deref(),
                        max_turns,
                        final_text_is_safety_handoff,
                        final_text_requires_user_input,
                    ) && (!turn_completed || tracker_adoption_allowed);
                    // Plan-mode outer auto-continue: only recoverable *blocked*
                    // planning ends (budget/safety-cap/tool-free recovery) queue
                    // another turn. Completed planning turns may be interview or
                    // approval handoffs and must wait for the user. Never auto-approves.
                    // Empty-fallback guard: deterministic `PLANNING_COMPLETED_FALLBACK_RESPONSE`
                    // turns (no LLM synthesis, no tools) increment a consecutive
                    // counter; after MAX_PLAN_EMPTY_FALLBACK_AUTO_CONTINUE the
                    // gate closes so 32 empty turns cannot re-queue forever
                    // (session-vtcode-20260921T045723Z).
                    let plan_auto_continue_enabled = planning_active && tracker_kill_switch;
                    let plan_state = tool_registry.planning_workflow_state();
                    let plan_ready_for_approval = planning_active
                        && crate::agent::runloop::unified::planning_workflow::persisted_plan_is_ready(&plan_state)
                            .await;
                    let final_text_is_empty_fallback =
                        final_text.as_deref().is_some_and(tracker_continue::is_plan_empty_fallback_text);
                    if final_text_is_empty_fallback {
                        session_stats.record_plan_empty_fallback();
                    } else {
                        session_stats.reset_plan_empty_fallbacks();
                    }
                    let should_queue_plan = tracker_continue::should_queue_plan_mode_auto_continue(
                        plan_auto_continue_enabled,
                        planning_active,
                        plan_ready_for_approval,
                        turn_completed,
                        blocked_reason,
                        is_verification_block,
                        max_turns,
                        session_stats.consecutive_plan_empty_fallbacks(),
                    );
                    if should_queue_plan {
                        let follow_up = tracker_continue::plan_mode_continue_follow_up();
                        let directive = tracker_continue::plan_mode_auto_continue_directive();
                        let budget_remaining = session_stats.plan_continuation_turns() < max_turns;
                        let queued = budget_remaining
                            && crate::agent::runloop::unified::stop_requests::stop_outcome(&ctrl_c_state).is_none()
                            && match runtime.try_queue_follow_up_input(follow_up) {
                                Ok(()) => {
                                    session_stats.record_plan_continuation_turn_with_limit(max_turns);
                                    std::sync::Arc::make_mut(&mut runtime.state.messages)
                                        .push(vtcode_core::llm::provider::Message::system(directive));
                                    // Queued auto-continue stays quiet: the next turn starts
                                    // immediately, so a TUI info line is noise. Exhausted /
                                    // queue-full paths below still inform the user.
                                    tracing::debug!(
                                        plan_turn = session_stats.plan_continuation_turns(),
                                        max_turns,
                                        "Queued plan-mode auto-continue without TUI echo"
                                    );
                                    true
                                }
                                Err(err) => {
                                    tracing::warn!(
                                        %err,
                                        "Plan-mode auto-continue queue full; falling through to turn end"
                                    );
                                    let _ = renderer.line(
                                        MessageStyle::Info,
                                        "[i] Plan-mode auto-continue could not resume automatically; planning remains active. Type `continue` to resume planning.",
                                    );
                                    tracker_auto_continue_exhausted = true;
                                    false
                                }
                            };
                        if queued {
                            if matches!(session_end_reason, SessionEndReason::Exit) {
                                break;
                            }
                            continue;
                        }
                        if !budget_remaining {
                            let _ = renderer.line(
                                MessageStyle::Info,
                                "[i] Plan-mode auto-continue budget exhausted; planning remains active. Type `continue` to resume planning.",
                            );
                            tracker_auto_continue_exhausted = true;
                        }
                        if planning_active && !plan_ready_for_approval {
                            let _ = renderer
                                .line(MessageStyle::Info, &tracker_continue::plan_progress_line("", false, 0, 0));
                        }
                    } else if should_queue {
                        let incomplete = incomplete.unwrap_or_default();
                        let (follow_up, directive) = if incomplete.is_empty() {
                            let reason = blocked_reason.unwrap_or("recoverable block");
                            (
                                tracker_continue::recoverable_blocked_continue_follow_up(reason),
                                tracker_continue::recoverable_blocked_auto_continue_directive(reason),
                            )
                        } else {
                            (
                                tracker_continue::tracker_continue_follow_up(&incomplete),
                                tracker_continue::tracker_continue_directive(
                                    tracker_continue::TRACKER_AUTO_CONTINUE_DIRECTIVE_LABEL,
                                    &incomplete,
                                ),
                            )
                        };
                        let budget_remaining = session_stats.tracker_continuation_turns() < max_turns;
                        let queued = budget_remaining
                            && crate::agent::runloop::unified::stop_requests::stop_outcome(&ctrl_c_state).is_none()
                            && match runtime.try_queue_follow_up_input(follow_up) {
                                Ok(()) => {
                                    session_stats.record_tracker_continuation_turn_with_limit(max_turns);
                                    std::sync::Arc::make_mut(&mut runtime.state.messages)
                                        .push(vtcode_core::llm::provider::Message::system(directive));
                                    // Queued auto-continue stays quiet: the next turn starts
                                    // immediately, so a TUI info line is noise (notably the
                                    // plan-accept → build handoff with pending tracker
                                    // steps). Exhausted / queue-full paths below still
                                    // inform the user.
                                    tracing::debug!(
                                        tracker_turn = session_stats.tracker_continuation_turns(),
                                        max_turns,
                                        incomplete = incomplete.len(),
                                        "Queued tracker auto-continue without TUI echo"
                                    );
                                    true
                                }
                                Err(err) => {
                                    tracing::warn!(%err, "Tracker auto-continue queue full; falling through to turn end");
                                    let _ = renderer.line(
                                        MessageStyle::Info,
                                        if incomplete.is_empty() {
                                            "[i] Blocked-end auto-continue could not resume automatically. Type `continue` to retry the request."
                                        } else {
                                            "[i] Tracker auto-continue could not resume automatically; incomplete tracker steps remain. Type `continue` to resume remaining steps."
                                        },
                                    );
                                    tracker_auto_continue_exhausted = true;
                                    false
                                }
                            };
                        if queued {
                            if matches!(session_end_reason, SessionEndReason::Exit) {
                                break;
                            }
                            continue;
                        }
                        if !budget_remaining {
                            let _ = renderer.line(
                                MessageStyle::Info,
                                if incomplete.is_empty() {
                                    "[i] Blocked-end auto-continue budget exhausted. Type `continue` to retry the request."
                                } else {
                                    "[i] Tracker auto-continue budget exhausted; incomplete tracker steps remain. Type `continue` to resume remaining steps."
                                },
                            );
                            tracker_auto_continue_exhausted = true;
                        }
                    } else if planning_active && plan_ready_for_approval && turn_completed {
                        session_stats.reset_plan_continuation_budget();
                        let _ =
                            renderer.line(MessageStyle::Info, &tracker_continue::plan_progress_line("", true, 0, 0));
                    } else if planning_active && !plan_ready_for_approval && !turn_completed && !should_queue_plan {
                        // Blocked planning without auto-queue: compact status only.
                        // When the empty-fallback cap fired, name it explicitly so
                        // the user knows why auto-continue stopped instead of
                        // seeing 32 silent `research/synthesis` lines.
                        if session_stats.consecutive_plan_empty_fallbacks()
                            >= tracker_continue::MAX_PLAN_EMPTY_FALLBACK_AUTO_CONTINUE
                        {
                            let _ = renderer.line(
                                MessageStyle::Info,
                                &format!(
                                    "[i] Plan-mode auto-continue stopped after {} empty turns with no synthesis; planning remains active. Type `continue` or re-state the request to resume.",
                                    session_stats.consecutive_plan_empty_fallbacks()
                                ),
                            );
                        }
                        let _ =
                            renderer.line(MessageStyle::Info, &tracker_continue::plan_progress_line("", false, 0, 0));
                    } else if !planning_active && incomplete.as_ref().is_none_or(|items| items.is_empty()) {
                        // Tracker work cleared (or none) outside planning: reset the
                        // episode budgets. Planning ends must not silently restore the
                        // shared plan/tracker continuation budget.
                        session_stats.reset_tracker_continuation_budget();
                        session_stats.reset_plan_continuation_budget();
                    }
                }
                // A refusal is terminal for its request, not a stall to resume:
                // no blocked handoff, no stall reason for `continue` to replay.
                if let RunLoopTurnLoopResult::Blocked { reason } = &outcome_result
                    && !turn_refused
                {
                    use crate::agent::runloop::unified::turn::tool_outcomes::helpers as verification_gate;

                    let base = reason.as_deref().unwrap_or("Turn blocked due to repeated failing behavior.");
                    let is_verification_block = base
                        .contains(crate::agent::runloop::unified::turn::turn_loop::PENDING_VERIFICATION_BLOCK_REASON);
                    // Recoverable tracker/plan budget ends that already printed
                    // the exhausted auto-continue info line must not stack a
                    // second "Type continue" blocked-handoff nudge.
                    let suppress_blocked_nudge = tracker_auto_continue_exhausted
                        && !is_verification_block
                        && verification_gate::tracker_auto_continue_is_recoverable_block(Some(base));
                    let max_failures = verification_gate::verification_max_consecutive_failures(vt_cfg.as_ref());
                    let escalated = session_stats.verification_consecutive_failures() >= max_failures;
                    // Autonomous cross-turn recovery for verification blocks:
                    // queue another turn with a project-aware verifier directive
                    // instead of forcing the user to type `continue`. Bounded by
                    // the configured cross-turn budget, and skipped once the
                    // never-passing suite escalated: more turns cannot fix a
                    // verifier that keeps failing, so surface the manual
                    // handoff (enriched with the failure log below) instead.
                    // This mirrors Codex's Stop-hook red-green loop (test gate
                    // feeds failures back as continued work) rather than a
                    // human-gated stop.
                    if is_verification_block
                        && !escalated
                        && session_stats.record_verification_auto_recovery_turn_with_limit(
                            verification_gate::verification_cross_turn_turns(vt_cfg.as_ref()),
                        )
                    {
                        let attempt = session_stats.verification_auto_recovery_turns();
                        let max = verification_gate::verification_cross_turn_turns(vt_cfg.as_ref());
                        let default_verifier = verification_gate::resolve_harness_verifier_command(
                            vt_cfg.as_ref(),
                            config.workspace.as_path(),
                        );
                        let directive = vtcode_core::tools::tool_intent::verification_recovery_directive(
                            default_verifier.as_deref(),
                            attempt,
                            max,
                        );
                        let follow_up =
                            super::blocked_handoff::verification_auto_recovery_follow_up(default_verifier.as_deref());
                        // Queue first: on a full queue the turn must fall
                        // through to the manual blocked handoff without leaving
                        // an orphan recovery directive in history.
                        match runtime.try_queue_follow_up_input(follow_up) {
                            Ok(()) => {
                                std::sync::Arc::make_mut(&mut runtime.state.messages)
                                    .push(vtcode_core::llm::provider::Message::system(directive));
                                let _ = renderer.line(
                                    MessageStyle::Info,
                                    &super::blocked_handoff::verification_auto_recovery_status_line(
                                        default_verifier.as_deref(),
                                        attempt,
                                        max,
                                    ),
                                );
                                session_stats.mark_turn_stalled(
                                    true,
                                    reason.clone().or_else(|| {
                                        Some(
                                            "Turn blocked waiting for verification; auto-recovery turn scheduled."
                                                .to_string(),
                                        )
                                    }),
                                );
                                // No `suppress_next_follow_up_prompt`: the
                                // queued input bypasses the interaction loop
                                // (`run_until_idle`), so no suppression is
                                // consumed here; setting it would leak into the
                                // next genuine user `continue` and delay its
                                // stalled-recovery handling by one prompt.
                                if matches!(session_end_reason, SessionEndReason::Exit) {
                                    break;
                                }
                                continue;
                            }
                            Err(err) => {
                                tracing::warn!(error = %err, "Verification auto-recovery queue full; writing blocked handoff");
                                session_stats.reset_verification_recovery_episode();
                            }
                        }
                    }
                    let base_owned: String = if is_verification_block {
                        let verifier = verification_gate::resolve_harness_verifier_command(
                            vt_cfg.as_ref(),
                            config.workspace.as_path(),
                        );
                        let attempt = session_stats.verification_auto_recovery_turns();
                        let max = verification_gate::verification_cross_turn_turns(vt_cfg.as_ref());
                        super::blocked_handoff::verification_exhausted_handoff_reason(
                            base,
                            verifier.as_deref(),
                            attempt,
                            max,
                            session_stats.last_verification_failure().filter(|_| escalated),
                        )
                    } else {
                        base.to_string()
                    };
                    let summary = super::blocked_handoff::blocker_summary_with_diagnostics(
                        &base_owned,
                        last_turn_diagnostics.as_ref(),
                        &session_stats.sorted_tools(),
                    );
                    if !suppress_blocked_nudge {
                        write_blocked_handoff_after_checkpoint(
                            &config.workspace,
                            &harness_snapshot.session_id,
                            &summary,
                            checkpoint_outcome.blocked_handoff_resume(),
                            &mut renderer,
                            harness_emitter.as_ref(),
                            Some(&handle),
                            tool_registry.is_planning_active(),
                        );
                    } else {
                        // Keep forensics artifacts without the user-nudge stack.
                        super::blocked_handoff::persist_blocked_handoff_quiet(
                            &config.workspace,
                            &harness_snapshot.session_id,
                            &summary,
                            checkpoint_outcome.blocked_handoff_resume(),
                            tool_registry.is_planning_active(),
                        );
                    }
                }
                match &outcome_result {
                    RunLoopTurnLoopResult::Completed { .. } => {
                        let handoff_resolved =
                            vtcode_core::core::agent::blocked_handoff::clear_current_blocked_handoff_for_session(
                                &config.workspace,
                                &harness_snapshot.session_id,
                            );
                        match handoff_resolved {
                            Ok(true) => {
                                if let Some(emitter) = harness_emitter.as_ref() {
                                    let _ = emitter.emit(harness_event(
                                        vtcode_core::exec::events::HarnessEventKind::BlockedHandoffResolved,
                                        Some("Blocked handoff resolved".to_owned()),
                                        None,
                                        None,
                                        None,
                                    ));
                                }
                            }
                            Ok(false) => {}
                            Err(err) => tracing::warn!(error = %err, "Failed to resolve current blocked handoff"),
                        }
                        session_stats.mark_turn_stalled(false, None);
                        session_stats.reset_verification_recovery_episode();
                    }
                    RunLoopTurnLoopResult::Aborted => {
                        session_stats
                            .mark_turn_stalled(true, Some("Turn aborted due to an execution error.".to_string()));
                    }
                    RunLoopTurnLoopResult::Blocked { .. } if turn_refused => {
                        handle.set_placeholder(Some(
                            "Request declined · Rephrase it, or use /model to switch models...".to_string(),
                        ));
                        // Blocked status makes the next input restore the
                        // default placeholder.
                        input_status_state.is_blocked = true;
                        session_stats.mark_turn_stalled(false, None);
                    }
                    RunLoopTurnLoopResult::Blocked { reason } => {
                        // Plan-mode QoL: a blocked placeholder that still says
                        // `continue` re-blocks on the next turn. Name the mode
                        // switch explicitly; the transcript lines written above
                        // carry the full reasoning. Never auto-switch here.
                        // Plan-mode guidance wins over the tracker-exhausted
                        // hint because it names the actionable mode switch.
                        if tool_registry.is_planning_active() {
                            handle.set_placeholder(Some(
                                "Plan blocked (read-only) · `continue` to keep planning, `/mode build` to implement..."
                                    .to_string(),
                            ));
                        } else if tracker_auto_continue_exhausted {
                            handle.set_placeholder(Some(
                                "Tracker auto-continue exhausted · Type 'continue' to resume remaining steps..."
                                    .to_string(),
                            ));
                        } else {
                            handle.set_placeholder(Some(
                                "Turn blocked · Type 'continue' to retry or describe changes...".to_string(),
                            ));
                        }
                        input_status_state.is_blocked = true;
                        session_stats.mark_turn_stalled(
                            true,
                            reason
                                .clone()
                                .or_else(|| Some("Turn blocked due to repeated failing tool behavior.".to_string())),
                        );
                        if !renderer.supports_inline_ui()
                            && session_stats.auto_permission_prompt_fallback_active()
                            && session_stats.last_auto_permission_denial().is_some()
                        {
                            session_end_reason = SessionEndReason::Error;
                            break;
                        }
                    }
                    _ => {
                        session_stats.mark_turn_stalled(false, None);
                        // Cancelled/Exit ends the stall episode: a later
                        // verification block starts with a fresh auto-recovery
                        // budget and failure count instead of inheriting them.
                        session_stats.reset_verification_recovery_episode();
                    }
                }
                if matches!(session_end_reason, SessionEndReason::Exit) {
                    break;
                }
                continue;
            }
        }
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
            next_session_primary_agent = Some(active_primary_agent.active().name().to_owned());
        }
        agent_touched_paths.extend(context_manager.tracked_instruction_activity_paths());
        session_bootstrap::poll_config_reload(
            &mut config_watcher,
            &mut vt_cfg,
            &config,
            &mut renderer,
            "Configuration reloaded during idle period",
        )?;
        // One deadline owns every independent cleanup branch. Atomic archive
        // workers retain ownership if we stop waiting; terminal restoration and
        // the postamble run outside this best-effort maintenance group.
        let mut teardown_output = session_teardown::SessionTeardownOutput::default();
        let mut finalization_output = None;
        let maintenance = tokio::time::timeout_at(deadline, async {
            tokio::join!(
                async {
                    session_teardown::drain_session_teardown(session_teardown::SessionTeardownContext {
                        harness_emitter: harness_emitter.as_ref(), checkpoint_manager: checkpoint_manager.as_ref(),
                        tool_registry: &tool_registry, workspace: &config.workspace, session_stats: &session_stats,
                        subtype, session_end_reason,
                    }, &mut teardown_output).await;
                    session_teardown::cleanup_completed_artifacts(&config.workspace, &turn_run_id.0, &tool_registry.harness_context_snapshot().session_id).await;
                    tracing::debug!(target: "vtcode.shutdown", boundary = "persistence_finished", elapsed_ms = session_tail_started.elapsed().as_millis() as u64);
                },
                async {
                    match finalize_session(
                        &mut renderer, lifecycle_hooks.as_ref(), &turn_id, session_end_reason,
                        &mut session_archive, &session_stats, last_turn_diagnostics, &runtime.state.messages,
                        linked_directories, async_mcp_manager.as_deref(), &handle, &mut session, &mut finalization_output,
                    ).await {
                        Ok(output) => finalization_output = Some(output),
                        Err(error) => { tracing::error!(%error, "failed to finalize session"); }
                    }
                },
                session_teardown::shutdown_subagents(&tool_registry),
                async {
                    if !matches!(session_end_reason, SessionEndReason::Exit | SessionEndReason::Cancelled) {
                        session_teardown::finalize_persistent_memory(config.clone(), vt_cfg.clone(), runtime.state.messages.clone(), turn_run_id.0.clone()).await;
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
        let session_teardown::SessionTeardownOutput { harness_finish_error, end_code_changes } = teardown_output;
        let teardown_error = terminal_event_error.or(harness_finish_error);
        let code_change_delta =
            compute_session_code_change_delta(start_code_changes.as_ref(), end_code_changes.as_ref());
        if let Some(next_resume) = resume_state.as_ref() {
            if let Some(error) = teardown_error {
                return Err(error);
            }
            refresh_runtime_debug_context_for_next_session(config.workspace.as_path(), Some(next_resume)).await?;
            continue;
        }
        if matches!(session_end_reason, SessionEndReason::NewSession) {
            if let Some(error) = teardown_error {
                return Err(error);
            }
            session_bootstrap::poll_config_reload(
                &mut config_watcher,
                &mut vt_cfg,
                &config,
                &mut renderer,
                "Configuration reloaded due to file changes",
            )?;

            refresh_runtime_debug_context_for_next_session(config.workspace.as_path(), None).await?;
            tracing::info!(
                teardown_elapsed_ms = session_tail_started.elapsed().as_millis() as u64,
                "new session teardown completed"
            );
            resume_state = None;
            pending_session_start_trigger = Some(SessionStartTrigger::NewSession);
            continue;
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
            let label = crate::agent::runloop::unified::session_setup::resolve_provider_label(&config, vt_cfg.as_ref());
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
            app_name: "VT Code",
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
        break;
    }
    Ok(())
}
