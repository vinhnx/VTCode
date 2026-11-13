//! One session iteration of the unified agent loop.
//!
//! `run_single_agent_loop_unified_impl` owns the unified `loop` plus the state
//! that outlives a single session (resume handoffs and `/new` restarts).
//! This scope owns the session guards, critical setup and interaction loop.
//! Private helpers borrow the live state for deferred initialization and
//! follow-up restoration. The exit tail is handed to
//! [`session_teardown::run_session_tail`] once the interaction loop ends.
//!
//! Cross-iteration state is written through the borrowed [`SessionLoopState`].
//! Keep session-owned guards in this scope and preserve their declaration order
//! when extracting helpers, including canonical persistence finalization before
//! those guards are dropped on errors.

use std::ops::ControlFlow;

use super::super::harness;
use super::*;

mod corrective;
mod initialization;
mod outcome;
mod plan_approval;
mod plan_handoff;
mod preamble;
mod resume;
mod setup;
mod skip_confirmations;
mod turn_prep;

/// Cross-iteration state owned by the unified loop, borrowed for one session.
///
/// Borrowed rather than owned: the loop keeps ownership of every value, so
/// drop order and lifetimes stay exactly where the facade put them.
pub(super) struct SessionLoopState<'a> {
    pub(super) config: &'a mut CoreAgentConfig,
    pub(super) session_skip_confirmations: &'a mut bool,
    pub(super) resume_state: &'a mut Option<ResumeSession>,
    pub(super) config_watcher: &'a mut SimpleConfigWatcher,
    pub(super) vt_cfg: &'a mut Option<VTCodeConfig>,
    pub(super) pending_session_start_trigger: &'a mut Option<SessionStartTrigger>,
    pub(super) next_session_primary_agent: &'a mut Option<String>,
    pub(super) steering_receiver: &'a mut Option<mpsc::UnboundedReceiver<SteeringMessage>>,
    pub(super) host_session: &'a mut Option<InlineSession>,
    pub(super) event_sender: Option<vtcode_desktop_protocol::LiveEventSender>,
    pub(super) stop_slot: Option<crate::agent::runloop::unified::stop_requests::StopHandleSlot>,
    pub(super) terminal_enabled: bool,
    pub(super) skip_confirmations: bool,
    pub(super) full_auto: bool,
    pub(super) primary_agent_explicitly_configured: bool,
    pub(super) planning_entry_source: PlanningEntrySource,
}

/// Run one session iteration: critical setup, interaction loop, teardown.
///
/// [`ControlFlow::Continue`] restarts the unified loop (resume handoff or
/// `/new`); [`ControlFlow::Break`] ends it.
pub(super) async fn run_session_iteration(state: SessionLoopState<'_>) -> Result<ControlFlow<()>> {
    let setup = match setup::run_session_setup(state).await? {
        setup::SetupFlow::Break => return Ok(ControlFlow::Break(())),
        setup::SetupFlow::Proceed(setup) => setup,
    };
    let setup::Setup {
        config,
        session_skip_confirmations,
        resume_state,
        config_watcher,
        vt_cfg,
        pending_session_start_trigger,
        next_session_primary_agent,
        terminal_enabled,
        full_auto,
        session_started_at,
        active_thread_label,
        start_code_changes,
        thread_handle,
        harness_config,
        turn_run_id,
        harness_emitter,
        mut renderer,
        mut session,
        handle,
        mut header_context,
        ctrl_c_state,
        ctrl_c_notify,
        settings_snapshot,
        input_activity_counter,
        checkpoint_manager,
        mut session_archive,
        mut lifecycle_hooks,
        mut context_manager,
        mut default_placeholder,
        mut follow_up_placeholder,
        mut next_checkpoint_turn,
        mut session_end_reason,
        _file_palette_task_guard,
        _background_subprocess_task_guard,
        _startup_update_task_guard,
        _editor_open_coordinator_task_guard,
        _settings_task_guard,
        editor_open_sender,
        editor_open_dispatcher,
        mut startup_update_notice_rx,
        mut settings_receiver,
        session_bootstrap,
        mut provider_client,
        mut tool_registry,
        tools,
        tool_catalog,
        async_mcp_manager,
        mut mcp_panel_state,
        loaded_skills,
        mut active_primary_agent,
        decision_ledger,
        traj,
        telemetry,
        error_recovery,
        mut runtime,
        tool_result_cache,
        tool_permission_cache,
        permissions_state,
        approval_recorder,
        safety_validator,
        circuit_breaker,
        tool_health_tracker,
        rate_limiter,
        autonomous_executor,
        cancel_token: _cancel_token,
        _cancel_guard,
        _signal_handler,
        mut session_stats,
        mut plan_session,
        mut linked_directories,
        mut model_picker_state,
        mut palette_state,
        mut last_forced_redraw,
        mut input_status_state,
        mut dismissed_memory_cleanup_fingerprint,
        mut prefer_latest_queued_input_once,
        mut queued_inputs,
        mut background_completion_receiver,
        _matrix_interrupt_guard,
        exec_session_manager: _exec_session_manager,
        mut exec_completion_receiver,
        exec_completion_notify,
        mut pending_background_completions,
        mut pending_matrix_note,
        mut pending_monitor_note,
        mut displayed_matrix_revision,
        webmcp_prompt_sender,
        mut webmcp_prompt_receiver,
        mut webmcp_bridge,
        mut agent_touched_paths,
        mut ctrl_c_notice_displayed,
        mut inline_prompt_cost_notice_shown,
        mut mcp_catalog_initialized,
        mut last_known_mcp_tools,
        mut pending_mcp_refresh,
        mut last_mcp_refresh,
        startup_update_requested_restart,
    } = *setup;
    let mut turn_id = turn_run_id.0.clone();

    // Twin of the setup-internal `harness_try!`: the interaction loop below
    // still needs early-error finalization after setup returns.
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

            if preamble::run_approved_plan_preamble(preamble::Preamble {
                pending_approved_plan_execution_input: &mut pending_approved_plan_execution_input,
                renderer: &mut renderer,
                plan_session: &mut plan_session,
                active_primary_agent: &mut active_primary_agent,
                tool_registry: &mut tool_registry,
                runtime: &mut runtime,
                session_archive: &mut session_archive,
                lifecycle_hooks: &mut lifecycle_hooks,
                mcp_catalog_initialized: &mut mcp_catalog_initialized,
                pending_mcp_refresh: &mut pending_mcp_refresh,
                vt_cfg: &mut *vt_cfg,
                config: &mut *config,
                handle: &handle,
                turn_run_id: &turn_run_id,
                tools: &tools,
                tool_catalog: &tool_catalog,
                async_mcp_manager: &async_mcp_manager,
                provider_client: provider_client.as_ref(),
                session_stats: &session_stats,
                loaded_skills: &loaded_skills,
                session_skip_confirmations: &*session_skip_confirmations,
                harness_emitter: &harness_emitter,
                executing_approved_plan,
                next_checkpoint_turn,
                max_tool_wall_clock_secs: harness_config.max_tool_wall_clock_secs,
            })
            .await?
            {
                continue;
            }

            if let Some(controller) = tool_registry.subagent_controller() {
                controller.set_parent_messages(&runtime.state.messages).await;
                if let Some(projection) = controller.matrix_tracker_projection().await {
                    let revision = (
                        projection["matrix_id"].as_str().unwrap_or_default().to_owned(),
                        projection["revision"].as_u64().unwrap_or_default(),
                    );
                    if displayed_matrix_revision.as_ref() != Some(&revision) {
                        let (rows, statuses, current) =
                            crate::agent::runloop::tool_output::tracker_panel_rows(&projection);
                        handle.update_task_panel_with_statuses(
                            rows,
                            statuses,
                            current,
                            crate::agent::runloop::tool_output::tracker_panel_metadata(&projection),
                        );
                        displayed_matrix_revision = Some(revision);
                    }
                }
                if let Some(snapshot) = controller.take_matrix_completion() {
                    pending_matrix_note = Some(format!(
                        "Authoritative matrix completion: {} is {:?}. Use matrix status for task outcomes and verification evidence before reporting results.",
                        snapshot.spec.id, snapshot.lifecycle
                    ));
                }
            }

            pending_background_completions.observe_terminal_exec_results(&tool_registry);
            let background_continuation_requested = pending_background_completions.should_schedule_continuation(
                !queued_inputs.is_empty() || !session.events.is_empty(),
                runtime.has_pending_follow_up_inputs(),
            );
            if crate::agent::runloop::unified::stop_requests::stop_outcome(&ctrl_c_state).is_none()
                && !matches!(last_turn_result, Some(RunLoopTurnLoopResult::Cancelled | RunLoopTurnLoopResult::Exit))
                && (background_continuation_requested
                    || (pending_matrix_note.is_some()
                        && queued_inputs.is_empty()
                        && session.events.is_empty()
                        && !runtime.has_pending_follow_up_inputs())
                    || (pending_monitor_note.is_some()
                        && queued_inputs.is_empty()
                        && session.events.is_empty()
                        && !runtime.has_pending_follow_up_inputs()))
            {
                // Monitor-only wake gets its own prompt: the commands are
                // still running, so the completion prompt's "do not poll
                // those completed tasks" would mislead the model.
                let only_monitor_note = !background_continuation_requested
                    && pending_matrix_note.is_none()
                    && pending_monitor_note.is_some();
                let follow_up_prompt = if only_monitor_note {
                    monitor_continuation_prompt()
                } else {
                    background_completion_continuation_prompt()
                };
                match runtime.try_queue_synthetic_follow_up_input(follow_up_prompt) {
                    Ok(()) => {
                        if background_continuation_requested {
                            pending_background_completions.mark_continuation_queued();
                        }
                    }
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
                        active_thread_label: active_thread_label.as_str(),
                        thread_handle: &thread_handle,
                        renderer: &mut renderer,
                        session: &mut session,
                        handle: &handle,
                        header_context: &mut header_context,
                        ctrl_c_state: &ctrl_c_state,
                        ctrl_c_notify: &ctrl_c_notify,
                        settings_snapshot: &settings_snapshot,
                        input_activity_counter: &input_activity_counter,
                        config,
                        vt_cfg: &mut *vt_cfg,
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
                        skip_confirmations: *session_skip_confirmations,
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
                        pending_monitor_note: &mut pending_monitor_note,
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
            let turn_progress = handle.resume_progress(vtcode_commons::ui_protocol::ProgressPhase::PreparingContext);
            let preparation_started_at = Instant::now();
            let (next_turn_input, completed_turn_prompt_message_index) = match interaction_outcome {
                InteractionOutcome::Exit { reason } => {
                    session_end_reason = reason;
                    break;
                }
                InteractionOutcome::Resume { resume_session } => {
                    *resume_state = Some(*resume_session);
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
                    match plan_approval::handle_plan_approval(plan_approval::PlanApprovalGate {
                        executing_approved_plan: &mut executing_approved_plan,
                        session_end_reason: &mut session_end_reason,
                        session_skip_confirmations: &mut *session_skip_confirmations,
                        runtime: &mut runtime,
                        tool_registry: &mut tool_registry,
                        active_primary_agent: &mut active_primary_agent,
                        plan_session: &mut plan_session,
                        session_stats: &mut session_stats,
                        session_archive: &mut session_archive,
                        context_manager: &mut context_manager,
                        lifecycle_hooks: &mut lifecycle_hooks,
                        renderer: &mut renderer,
                        vt_cfg: &mut *vt_cfg,
                        config: &mut *config,
                        mcp_catalog_initialized: &mut mcp_catalog_initialized,
                        pending_mcp_refresh: &mut pending_mcp_refresh,
                        handle: &handle,
                        turn_id: &turn_id,
                        turn_run_id: &turn_run_id,
                        provider_client: provider_client.as_ref(),
                        tools: &tools,
                        tool_catalog: &tool_catalog,
                        async_mcp_manager: &async_mcp_manager,
                        loaded_skills: &loaded_skills,
                        safety_validator: &safety_validator,
                        harness_emitter: &harness_emitter,
                        next_checkpoint_turn,
                        max_tool_wall_clock_secs: harness_config.max_tool_wall_clock_secs,
                        target,
                    })
                    .await?
                    {
                        plan_approval::PlanApprovalFlow::ContinueLoop => continue,
                        plan_approval::PlanApprovalFlow::BreakLoop => break,
                        plan_approval::PlanApprovalFlow::Proceed { input, prompt_message_index } => {
                            (input, prompt_message_index)
                        }
                    }
                }
            };
            let (
                working_history,
                runtime_steering,
                refused_turn_rollback,
                transient_system_notes,
                _prompt_checkpoint_lease,
            ) = match turn_prep::run_turn_prep(
                &mut runtime,
                turn_prep::TurnPrep {
                    plan_session: &mut plan_session,
                    renderer: &mut renderer,
                    last_turn_result: &mut last_turn_result,
                    session_end_reason: &mut session_end_reason,
                    next_checkpoint_turn: &mut next_checkpoint_turn,
                    pending_background_completions: &mut pending_background_completions,
                    pending_matrix_note: &mut pending_matrix_note,
                    pending_monitor_note: &mut pending_monitor_note,
                    tool_registry: &tool_registry,
                    config: &*config,
                    handle: &handle,
                    ctrl_c_state: &ctrl_c_state,
                    ctrl_c_notify: &ctrl_c_notify,
                    harness_emitter: &harness_emitter,
                    checkpoint_manager: &checkpoint_manager,
                    agent_touched_paths: &agent_touched_paths,
                    turn_progress: &turn_progress,
                    next_turn_input: &next_turn_input,
                    thread_id: &turn_run_id.0,
                    turn_id: &turn_id,
                    executing_approved_plan,
                    active_task_follow_up,
                    completed_turn_prompt_message_index,
                    preparation_started_at,
                },
            )
            .await?
            {
                turn_prep::TurnPrepFlow::ContinueLoop => continue,
                turn_prep::TurnPrepFlow::BreakLoop => break,
                turn_prep::TurnPrepFlow::Proceed {
                    working_history,
                    runtime_steering,
                    refused_turn_rollback,
                    transient_system_notes,
                    prompt_checkpoint_lease,
                } => (
                    working_history,
                    runtime_steering,
                    refused_turn_rollback,
                    transient_system_notes,
                    prompt_checkpoint_lease,
                ),
            };
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
                aborted_turn_diagnostics,
            ) = {
                let mut auto_finish_planning_attempted = false;
                tool_registry.begin_patch_recovery_turn();
                let mut harness_state = HarnessTurnState::new(
                    TurnRunId(turn_run_id.0.clone()),
                    TurnId(turn_id.clone()),
                    harness_config.max_tool_wall_clock_secs,
                    harness_config.max_tool_retries,
                );
                harness_state.set_approved_plan_execution(executing_approved_plan);
                let effective_skip_confirmations = skip_confirmations::effective_session_skip_confirmations(
                    *session_skip_confirmations,
                    &tool_registry,
                );
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
                    config,
                    None,
                    &mut turn_metadata_cache,
                    &mut provider_client,
                    &traj,
                    &active_primary_agent,
                    effective_skip_confirmations,
                    full_auto,
                    runtime_steering,
                );
                turn_loop_ctx.live_vt_cfg = Some(&mut *vt_cfg);
                let thread_id_owned = thread_handle.thread_id().to_string();
                turn_loop_ctx.settings = Some(crate::agent::runloop::unified::turn::turn_loop::ActiveSettingsContext {
                    receiver: &mut settings_receiver,
                    header_context: &mut header_context,
                    session_bootstrap: &session_bootstrap,
                    thread_id: thread_id_owned.as_str(),
                    thread_handle: &thread_handle,
                });

                let primary_agent_snapshot = active_primary_agent.active().clone();
                let result = crate::agent::runloop::unified::turn::run_turn_loop(working_history, turn_loop_ctx).await;

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
                    aborted_turn_diagnostics,
                )
            };
            drop(turn_progress);
            match outcome::run_outcome_gate(outcome::OutcomeGate {
                turn_result,
                aborted_turn_diagnostics,
                refused_turn_rollback: &refused_turn_rollback,
                transient_system_notes: &transient_system_notes,
                cross_turn_read_sigs: &cross_turn_read_sigs,
                cross_turn_written: &cross_turn_written,
                cross_turn_shell_cmd: &cross_turn_shell_cmd,
                cross_turn_failed_shell_key: &cross_turn_failed_shell_key,
                cross_turn_out_of_band_progress,
                planning_active,
                next_turn_input: &next_turn_input,
                completed_turn_prompt_message_index,
                turn_started_at: &turn_started_at,
                turn_id: &turn_id,
                turn_run_id: &turn_run_id,
                history_snapshot_bytes,
                max_tool_wall_clock_secs: harness_config.max_tool_wall_clock_secs,
                executing_approved_plan,
                config: &*config,
                vt_cfg: &mut *vt_cfg,
                handle: &handle,
                renderer: &mut renderer,
                input_status_state: &mut input_status_state,
                default_placeholder: &default_placeholder,
                session_archive: &mut session_archive,
                session_stats: &mut session_stats,
                runtime: &mut runtime,
                tool_registry: &mut tool_registry,
                context_manager: &mut context_manager,
                agent_touched_paths: &mut agent_touched_paths,
                ctrl_c_state: &ctrl_c_state,
                ctrl_c_notify: &ctrl_c_notify,
                harness_emitter: &harness_emitter,
                session_end_reason: &mut session_end_reason,
                next_checkpoint_turn: &mut next_checkpoint_turn,
                last_turn_result: &mut last_turn_result,
                last_turn_response_was_fallback: &mut last_turn_response_was_fallback,
                last_turn_diagnostics: &mut last_turn_diagnostics,
                last_approved_plan_summary_status: &mut last_approved_plan_summary_status,
                approved_plan_execution_turn: &mut approved_plan_execution_turn,
                pending_approved_plan_execution_input: &mut pending_approved_plan_execution_input,
                active_primary_agent: &mut active_primary_agent,
                lifecycle_hooks: &mut lifecycle_hooks,
                mcp_catalog_initialized: &mut mcp_catalog_initialized,
                pending_mcp_refresh: &mut pending_mcp_refresh,
                session_skip_confirmations: &mut *session_skip_confirmations,
                tools: &tools,
                tool_catalog: &tool_catalog,
                async_mcp_manager: &async_mcp_manager,
                provider_client: provider_client.as_ref(),
                loaded_skills: &loaded_skills,
                plan_session: &plan_session,
                tool_result_cache: &tool_result_cache,
                safety_validator: &safety_validator,
                cross_turn_tracker: &mut cross_turn_tracker,
            })
            .await?
            {
                outcome::OutcomeFlow::ContinueLoop => continue,
                outcome::OutcomeFlow::BreakLoop => break,
            }
        }
    }
    session_teardown::run_session_tail(session_teardown::SessionTailContext {
        config,
        config_watcher,
        vt_cfg,
        renderer: &mut renderer,
        session: &mut session,
        handle: &handle,
        ctrl_c_state: &ctrl_c_state,
        session_archive: &mut session_archive,
        lifecycle_hooks: &lifecycle_hooks,
        checkpoint_manager: &checkpoint_manager,
        context_manager: &context_manager,
        tool_registry: &tool_registry,
        runtime: &runtime,
        provider_client: &*provider_client,
        session_bootstrap: &session_bootstrap,
        thread_handle: &thread_handle,
        turn_run_id: &turn_run_id,
        turn_id: &turn_id,
        harness_emitter: &harness_emitter,
        active_primary_agent: &active_primary_agent,
        loaded_skills: &loaded_skills,
        session_stats: &session_stats,
        session_started_at,
        session_end_reason,
        full_auto,
        last_approved_plan_summary_status,
        last_turn_result: &last_turn_result,
        last_turn_response_was_fallback,
        last_turn_diagnostics,
        linked_directories,
        start_code_changes: &start_code_changes,
        agent_touched_paths: &mut agent_touched_paths,
        async_mcp_manager: &async_mcp_manager,
        terminal_enabled,
        next_session_primary_agent,
        resume_state,
        pending_session_start_trigger,
    })
    .await
}
