use super::*;
use crate::agent::runloop::unified::plan_mode_state::render_plan_mode_next_step_hint;
use crate::agent::runloop::unified::run_loop_context::TurnPhase;

const PLAN_MODE_MIN_TOOL_CALLS_PER_TURN: usize = 48;

fn resolve_effective_turn_timeout_secs(
    configured_turn_timeout_secs: u64,
    max_tool_wall_clock_secs: u64,
) -> u64 {
    // Keep turn timeout aligned with harness wall-clock budget to avoid aborting
    // valid long-running tool+request cycles mid-turn.
    //
    // The buffer must cover at least one LLM-attempt timeout window so a turn that
    // reaches the harness wall-clock budget can still complete its in-flight request.
    // Keep this formula aligned with turn_processing::llm_request::llm_attempt_timeout_secs.
    let llm_attempt_grace_secs = (configured_turn_timeout_secs / 5).clamp(30, 120);
    let buffer_secs = 60_u64.max(llm_attempt_grace_secs);
    let min_for_harness = max_tool_wall_clock_secs.saturating_add(buffer_secs);
    configured_turn_timeout_secs.max(min_for_harness)
}

fn effective_max_tool_calls_for_turn(configured_limit: usize, plan_mode_active: bool) -> usize {
    if plan_mode_active {
        configured_limit.max(PLAN_MODE_MIN_TOOL_CALLS_PER_TURN)
    } else {
        configured_limit
    }
}

fn build_partial_timeout_messages(
    timeout_secs: u64,
    timed_out_phase: TurnPhase,
    attempted_tool_calls: usize,
    active_pty_sessions_before_cancel: usize,
    plan_mode_active: bool,
    had_tool_activity: bool,
) -> (String, String) {
    let timed_out_during_request = matches!(timed_out_phase, TurnPhase::Requesting);
    let mut timeout_note = if timed_out_during_request {
        "Tool activity exists and timeout occurred during LLM requesting; retry is skipped to avoid re-running tools.".to_string()
    } else {
        "Tool activity was detected in this attempt; retry is skipped to avoid duplicate execution."
            .to_string()
    };

    let include_continue_nudge = plan_mode_active && had_tool_activity && timed_out_during_request;
    if include_continue_nudge {
        timeout_note.push_str(" Nudge with \"continue\" to resume from the stalled turn.");
    }

    let renderer_message = format!(
        "Turn timed out after {} seconds in phase {:?}. PTY sessions cancelled; {} (calls={}, active_pty_before_cancel={})",
        timeout_secs,
        timed_out_phase,
        timeout_note,
        attempted_tool_calls,
        active_pty_sessions_before_cancel
    );

    let mut error_message = format!(
        "Turn timed out after {} seconds in phase {:?} after partial tool execution (calls={}, active_pty_before_cancel={})",
        timeout_secs, timed_out_phase, attempted_tool_calls, active_pty_sessions_before_cancel
    );
    if include_continue_nudge {
        error_message.push_str(" Nudge with \"continue\" to resume from the stalled turn.");
    }

    (renderer_message, error_message)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_single_agent_loop_unified_impl(
    config: &CoreAgentConfig,
    _vt_cfg: Option<VTCodeConfig>,
    _skip_confirmations: bool,
    full_auto: bool,
    plan_mode: bool,
    team_context: Option<vtcode_core::agent_teams::TeamContext>,
    resume: Option<ResumeSession>,
    steering_receiver: &mut Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
) -> Result<()> {
    let _terminal_cleanup_guard = TerminalCleanupGuard::new();

    let mut config = config.clone();
    let mut resume_state = resume;
    let mut _consecutive_idle_cycles = 0;
    let mut last_activity_time: Option<Instant> = None;
    let mut config_watcher = SimpleConfigWatcher::new(config.workspace.clone());
    config_watcher.set_check_interval(15);
    config_watcher.set_debounce_duration(500);
    let mut vt_cfg = config_watcher.load_config();
    let mut idle_config = extract_idle_config(vt_cfg.as_ref());

    loop {
        let resume_request = resume_state.take();
        let resume_ref = resume_request.as_ref();

        let session_id = resume_ref
            .map(|resume| SessionId::from_string(resume.identifier.clone()))
            .unwrap_or_default();
        let _session_created_at = Utc::now();
        let _session_state_path = session_path(Path::new(&config.workspace), &session_id);
        let _session_trigger = if resume_ref.is_some() {
            SessionStartTrigger::Resume
        } else {
            SessionStartTrigger::Startup
        };
        let mut session_state =
            initialize_session(&config, vt_cfg.as_ref(), full_auto, resume_ref).await?;
        let harness_config = vt_cfg
            .as_ref()
            .map(|cfg| cfg.agent.harness.clone())
            .unwrap_or_default();
        let turn_run_id = TurnRunId(SessionId::new().0);
        let harness_emitter: Option<HarnessEventEmitter> =
            harness_config.event_log_path.as_ref().and_then(|path| {
                let resolved = resolve_event_log_path(path, &turn_run_id);
                HarnessEventEmitter::new(resolved).ok()
            });
        if let Some(emitter) = harness_emitter.as_ref() {
            let open_responses_config = vt_cfg
                .as_ref()
                .map(|cfg| cfg.agent.open_responses.clone())
                .unwrap_or_default();
            if open_responses_config.enabled {
                let or_path = harness_config.event_log_path.as_ref().map(|base| {
                    let parent = std::path::Path::new(base)
                        .parent()
                        .unwrap_or(std::path::Path::new("."));
                    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
                    parent.join(format!(
                        "open-responses-{}-{}.jsonl",
                        turn_run_id.0, timestamp
                    ))
                });
                let _ =
                    emitter.enable_open_responses(open_responses_config, &config.model, or_path);
            }
            let _ = emitter.emit(ThreadEvent::ThreadStarted(ThreadStartedEvent {
                thread_id: turn_run_id.0.clone(),
            }));
        }
        let ui_setup = initialize_session_ui(
            &config,
            vt_cfg.as_ref(),
            &mut session_state,
            resume_ref,
            full_auto,
            _skip_confirmations,
        )
        .await?;
        let mut renderer = ui_setup.renderer;
        let mut session = ui_setup.session;
        let handle = ui_setup.handle;
        let ctrl_c_state = ui_setup.ctrl_c_state;
        let ctrl_c_notify = ui_setup.ctrl_c_notify;
        let checkpoint_manager = ui_setup.checkpoint_manager;
        let mut session_archive = ui_setup.session_archive;
        let lifecycle_hooks = ui_setup.lifecycle_hooks;
        let mut context_manager = ui_setup.context_manager;
        let mut default_placeholder = ui_setup.default_placeholder;
        let mut follow_up_placeholder = ui_setup.follow_up_placeholder;
        let mut next_checkpoint_turn = ui_setup.next_checkpoint_turn;
        let mut session_end_reason = ui_setup.session_end_reason;
        let _ui_redraw_batcher = ui_setup.ui_redraw_batcher;
        let SessionState {
            session_bootstrap,
            mut provider_client,
            mut tool_registry,
            tools,
            tool_catalog,
            mut conversation_history,
            decision_ledger,
            trajectory: traj,
            async_mcp_manager,
            mut mcp_panel_state,
            tool_result_cache,
            tool_permission_cache,
            approval_recorder,
            loaded_skills,
            safety_validator,
            circuit_breaker,
            tool_health_tracker,
            rate_limiter,
            validation_cache,
            telemetry,
            autonomous_executor,
            ..
        } = session_state;
        let cancel_token = CancellationToken::new();
        let _cancel_guard = CancelGuard(cancel_token.clone());
        let _signal_handler = spawn_signal_handler(
            ctrl_c_state.clone(),
            ctrl_c_notify.clone(),
            async_mcp_manager.clone(),
            cancel_token.clone(),
        );
        let mut session_stats = SessionStats::default();
        session_stats.team_context = team_context.clone();
        session_stats.circuit_breaker = circuit_breaker.clone();
        session_stats.tool_health_tracker = tool_health_tracker.clone();
        session_stats.rate_limiter = rate_limiter.clone();
        session_stats.validation_cache = validation_cache.clone();
        if plan_mode {
            transition_to_plan_mode(&tool_registry, &mut session_stats, &handle, true, true).await;
            render_plan_mode_next_step_hint(&mut renderer)?;
        }
        let mut linked_directories: Vec<LinkedDirectory> = Vec::with_capacity(4);
        let mut model_picker_state: Option<ModelPickerState> = None;
        let mut palette_state: Option<ActivePalette> = None;
        let mut last_forced_redraw = Instant::now();
        let mut input_status_state = InputStatusState::default();
        let mut queued_inputs: VecDeque<String> = VecDeque::with_capacity(8);
        let mut ctrl_c_notice_displayed = false;
        let mut mcp_catalog_initialized = tool_registry.mcp_client().is_some();
        let mut last_known_mcp_tools: Vec<String> = Vec::with_capacity(16);
        let mut last_mcp_refresh = std::time::Instant::now();
        loop {
            let interaction_outcome = {
                let mut interaction_ctx = crate::agent::runloop::unified::turn::session::interaction_loop::InteractionLoopContext {
                    renderer: &mut renderer,
                    session: &mut session,
                    handle: &handle,
                    ctrl_c_state: &ctrl_c_state,
                    ctrl_c_notify: &ctrl_c_notify,
                    config: &mut config,
                    vt_cfg: &mut vt_cfg,
                    provider_client: &mut provider_client,
                    session_bootstrap: &session_bootstrap,
                    async_mcp_manager: &async_mcp_manager,
                    tool_registry: &mut tool_registry,
                    tools: &tools,
                    tool_catalog: &tool_catalog,
                    conversation_history: &mut conversation_history,
                    decision_ledger: &decision_ledger,
                    context_manager: &mut context_manager,
                    session_stats: &mut session_stats,
                    mcp_panel_state: &mut mcp_panel_state,
                    linked_directories: &mut linked_directories,
                    lifecycle_hooks: lifecycle_hooks.as_ref(),
                    full_auto,
                    approval_recorder: &approval_recorder,
                    tool_permission_cache: &tool_permission_cache,
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
                    error_recovery: &session_state.error_recovery,
                    last_forced_redraw: &mut last_forced_redraw,
                    harness_config: harness_config.clone(),
                    steering_receiver,
                };

                let mut interaction_state =
                    crate::agent::runloop::unified::turn::session::interaction_loop::InteractionState {
                        input_status_state: &mut input_status_state,
                        queued_inputs: &mut queued_inputs,
                        model_picker_state: &mut model_picker_state,
                        palette_state: &mut palette_state,
                        last_known_mcp_tools: &mut last_known_mcp_tools,
                        mcp_catalog_initialized: &mut mcp_catalog_initialized,
                        last_mcp_refresh: &mut last_mcp_refresh,
                        ctrl_c_notice_displayed: &mut ctrl_c_notice_displayed,
                    };

                crate::agent::runloop::unified::turn::session::interaction_loop::run_interaction_loop(
                    &mut interaction_ctx,
                    &mut interaction_state,
                ).await?
            };
            use crate::agent::runloop::unified::turn::session::interaction_loop::InteractionOutcome;
            let _input = match interaction_outcome {
                InteractionOutcome::Exit { reason } => {
                    session_end_reason = reason;
                    break;
                }
                InteractionOutcome::Resume { resume_session } => {
                    resume_state = Some(*resume_session);
                    session_end_reason = SessionEndReason::Completed;
                    break;
                }
                InteractionOutcome::Continue { input } => input,
                InteractionOutcome::PlanApproved {
                    auto_accept,
                    clear_context,
                } => {
                    handle.set_editing_mode(vtcode_core::ui::tui::EditingMode::Edit);
                    handle.set_skip_confirmations(auto_accept);
                    if auto_accept {
                        renderer.line(
                            vtcode_core::utils::ansi::MessageStyle::Info,
                            "Auto-accept mode enabled for this session.",
                        )?;
                    }
                    if clear_context {
                        conversation_history.clear();
                        {
                            let mut ledger = decision_ledger.write().await;
                            *ledger = vtcode_core::core::decision_tracker::DecisionTracker::new();
                        }
                        session_stats =
                            crate::agent::runloop::unified::state::SessionStats::default();
                        vtcode_core::utils::transcript::clear();
                        renderer.clear_screen();
                        renderer.line(
                            vtcode_core::utils::ansi::MessageStyle::Info,
                            "Cleared conversation history.",
                        )?;
                    }
                    continue;
                }
            };
            let mut working_history = conversation_history.clone();
            let timeout_secs = resolve_effective_turn_timeout_secs(
                resolve_timeout(
                    vt_cfg
                        .as_ref()
                        .map(|cfg| cfg.optimization.agent_execution.max_execution_time_secs),
                ),
                harness_config.max_tool_wall_clock_secs,
            );
            let turn_started_at = Instant::now();
            let mut attempts = 0;
            let mut history_backup: Option<Vec<vtcode_core::llm::provider::Message>> = None;
            let outcome = match loop {
                let mut auto_exit_plan_mode_attempted = false;
                let plan_mode_active = session_stats.is_plan_mode();
                let max_tool_calls_per_turn = effective_max_tool_calls_for_turn(
                    harness_config.max_tool_calls_per_turn,
                    plan_mode_active,
                );
                let mut harness_state = HarnessTurnState::new(
                    TurnRunId(turn_run_id.0.clone()),
                    TurnId(SessionId::new().0),
                    max_tool_calls_per_turn,
                    harness_config.max_tool_wall_clock_secs,
                    harness_config.max_tool_retries,
                );
                let history_for_turn = if attempts == 0 {
                    std::mem::take(&mut working_history)
                } else {
                    history_backup
                        .take()
                        .expect("history_backup must be set after first attempt")
                };
                let execution_history_len_before_attempt = tool_registry.execution_history_len();
                let turn_loop_ctx = crate::agent::runloop::unified::turn::TurnLoopContext::new(
                    &mut renderer,
                    &handle,
                    &mut session,
                    &mut session_stats,
                    &mut auto_exit_plan_mode_attempted,
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
                    &safety_validator,
                    &circuit_breaker,
                    &tool_health_tracker,
                    &rate_limiter,
                    &telemetry,
                    &autonomous_executor,
                    &session_state.error_recovery,
                    &mut harness_state,
                    harness_emitter.as_ref(),
                    &mut config,
                    vt_cfg.as_ref(),
                    &mut provider_client,
                    &traj,
                    full_auto,
                    steering_receiver,
                );

                let result = timeout(
                    Duration::from_secs(timeout_secs),
                    crate::agent::runloop::unified::turn::run_turn_loop(
                        history_for_turn,
                        turn_loop_ctx,
                        &mut session_end_reason,
                    ),
                )
                .await;

                match result {
                    Ok(inner) => break inner,
                    Err(_) => {
                        let active_pty_sessions_before_cancel = tool_registry.active_pty_sessions();
                        let attempted_tool_calls = harness_state.tool_calls;
                        let timed_out_phase = harness_state.phase;
                        tool_registry.terminate_all_pty_sessions();
                        let execution_history_len_after_attempt =
                            tool_registry.execution_history_len();
                        handle.set_input_status(None, None);
                        input_status_state.left = None;
                        input_status_state.right = None;

                        if ctrl_c_state.is_exit_requested() || ctrl_c_state.is_cancel_requested() {
                            let interrupted_result = if ctrl_c_state.is_exit_requested() {
                                RunLoopTurnLoopResult::Exit
                            } else {
                                RunLoopTurnLoopResult::Cancelled
                            };
                            let recovered_history = history_backup
                                .clone()
                                .or_else(|| {
                                    if working_history.is_empty() {
                                        None
                                    } else {
                                        Some(working_history.clone())
                                    }
                                })
                                .unwrap_or_else(|| conversation_history.clone());
                            break Ok(TurnLoopOutcome {
                                result: interrupted_result,
                                working_history: recovered_history,
                                turn_modified_files: std::collections::BTreeSet::new(),
                            });
                        }

                        if history_backup.is_none() {
                            history_backup = Some(conversation_history.clone());
                        }
                        let had_tool_activity = execution_history_len_after_attempt
                            > execution_history_len_before_attempt
                            || active_pty_sessions_before_cancel > 0
                            || attempted_tool_calls > 0;
                        attempts += 1;
                        if had_tool_activity {
                            let (timeout_message, timeout_error_message) =
                                build_partial_timeout_messages(
                                    timeout_secs,
                                    timed_out_phase,
                                    attempted_tool_calls,
                                    active_pty_sessions_before_cancel,
                                    plan_mode,
                                    had_tool_activity,
                                );
                            renderer.line(MessageStyle::Error, &timeout_message)?;
                            break Err(anyhow::Error::msg(timeout_error_message));
                        }
                        if attempts >= 2 {
                            renderer.line(
                                  MessageStyle::Error,
                                  &format!(
                                      "Turn timed out after {} seconds. PTY sessions cancelled; stopping turn.",
                                    timeout_secs
                                ),
                            )?;
                            break Err(anyhow::anyhow!(
                                "Turn timed out after {} seconds",
                                timeout_secs
                            ));
                        }
                        renderer.line(
                            MessageStyle::Error,
                            &format!(
                                "Turn timed out after {} seconds. PTY sessions cancelled; retrying once.",
                                timeout_secs
                            ),
                        )?;
                    }
                }
            } {
                Ok(outcome) => outcome,
                Err(err) => {
                    tracing::error!("Turn execution error: {}", err);
                    handle.set_input_status(None, None);
                    let _ = renderer.line_if_not_empty(MessageStyle::Output);
                    let _ = renderer.line(MessageStyle::Error, &format!("Error: {}", err));
                    TurnLoopOutcome {
                        result: RunLoopTurnLoopResult::Aborted,
                        working_history: history_backup
                            .or_else(|| {
                                if working_history.is_empty() {
                                    None
                                } else {
                                    Some(working_history)
                                }
                            })
                            .unwrap_or_else(|| conversation_history.clone()),
                        turn_modified_files: std::collections::BTreeSet::new(),
                    }
                }
            };
            let turn_elapsed = turn_started_at.elapsed();
            let show_turn_timer = vt_cfg
                .as_ref()
                .map(|cfg| cfg.ui.show_turn_timer)
                .unwrap_or(true);
            if let Err(err) = crate::agent::runloop::unified::turn::apply_turn_outcome(
                &outcome,
                crate::agent::runloop::unified::turn::TurnOutcomeContext {
                    conversation_history: &mut conversation_history,
                    renderer: &mut renderer,
                    handle: &handle,
                    ctrl_c_state: &ctrl_c_state,
                    default_placeholder: &default_placeholder,
                    checkpoint_manager: checkpoint_manager.as_ref(),
                    next_checkpoint_turn: &mut next_checkpoint_turn,
                    session_end_reason: &mut session_end_reason,
                    turn_elapsed,
                    show_turn_timer,
                },
            )
            .await
            {
                tracing::error!("Failed to apply turn outcome: {}", err);
                renderer
                    .line(
                        MessageStyle::Error,
                        &format!("Failed to finalize turn: {}", err),
                    )
                    .ok();
            }

            if session_stats.take_context_clear_request() {
                conversation_history.clear();
                {
                    let mut ledger = decision_ledger.write().await;
                    *ledger = vtcode_core::core::decision_tracker::DecisionTracker::new();
                }
                session_stats = crate::agent::runloop::unified::state::SessionStats::default();
                vtcode_core::utils::transcript::clear();
                renderer.clear_screen();
                renderer.line(MessageStyle::Info, "Cleared conversation history.")?;
                handle.set_editing_mode(vtcode_core::ui::tui::EditingMode::Edit);
                handle.set_skip_confirmations(true);
            }
            last_activity_time = Some(Instant::now());
            vtcode_core::tools::cache::FILE_CACHE
                .check_pressure_and_evict()
                .await;
            tool_result_cache.write().await.check_pressure_and_evict();
            if let Some(archive) = session_archive.as_ref() {
                let mut recent_messages: Vec<SessionMessage> = conversation_history
                    .iter()
                    .rev()
                    .take(RECENT_MESSAGE_LIMIT)
                    .map(SessionMessage::from)
                    .collect();
                recent_messages.reverse();

                let progress_turn = next_checkpoint_turn.saturating_sub(1).max(1);
                let distinct_tools = session_stats.sorted_tools();
                let skill_names: Vec<String> = loaded_skills.read().await.keys().cloned().collect();

                if let Err(err) = archive
                    .persist_progress_async(SessionProgressArgs {
                        total_messages: conversation_history.len(),
                        distinct_tools: distinct_tools.clone(),
                        recent_messages,
                        turn_number: progress_turn,
                        token_usage: None,
                        max_context_tokens: None,
                        loaded_skills: Some(skill_names),
                    })
                    .await
                {
                    tracing::warn!("Failed to persist session progress: {}", err);
                }
            }
            match &outcome.result {
                RunLoopTurnLoopResult::Aborted => {
                    session_stats.mark_turn_stalled(
                        true,
                        Some("Turn aborted due to an execution error.".to_string()),
                    );
                }
                RunLoopTurnLoopResult::Blocked { reason } => {
                    session_stats.mark_turn_stalled(
                        true,
                        reason.clone().or_else(|| {
                            Some("Turn blocked due to repeated failing tool behavior.".to_string())
                        }),
                    );
                }
                _ => {
                    session_stats.mark_turn_stalled(false, None);
                }
            }
            if matches!(session_end_reason, SessionEndReason::Exit) {
                break;
            }
            continue;
        }
        if let Some(archive) = session_archive.as_mut() {
            let skill_names: Vec<String> = loaded_skills.read().await.keys().cloned().collect();
            archive.set_loaded_skills(skill_names);
        }
        if let Some(emitter) = harness_emitter.as_ref() {
            emitter.finish_open_responses();
        }
        if let Err(err) = finalize_session(
            &mut renderer,
            lifecycle_hooks.as_ref(),
            session_end_reason,
            &mut session_archive,
            &session_stats,
            &conversation_history,
            linked_directories,
            async_mcp_manager.as_deref(),
            &handle,
        )
        .await
        {
            tracing::error!("Failed to finalize session: {}", err);
            renderer
                .line(
                    MessageStyle::Error,
                    &format!("Failed to finalize session: {}", err),
                )
                .ok();
        }
        if resume_state.is_some() {
            continue;
        }
        if matches!(session_end_reason, SessionEndReason::NewSession) {
            if config_watcher.should_reload() {
                vt_cfg = config_watcher.load_config();
                idle_config = extract_idle_config(vt_cfg.as_ref());
                tracing::debug!("Configuration reloaded due to file changes");
            }

            resume_state = None;
            _consecutive_idle_cycles = 0;
            continue;
        }
        if config_watcher.should_reload() {
            vt_cfg = config_watcher.load_config();
            idle_config = extract_idle_config(vt_cfg.as_ref());
            tracing::debug!("Configuration reloaded during idle period");
        }
        if idle_config.enabled
            && let Some(last_activity) = last_activity_time
        {
            let idle_duration = last_activity.elapsed().as_millis() as u64;
            if idle_duration >= idle_config.timeout_ms {
                _consecutive_idle_cycles += 1;
                if idle_config.backoff_ms > 0 {
                    if _consecutive_idle_cycles >= idle_config.max_cycles {
                        sleep(Duration::from_millis(idle_config.backoff_ms * 2)).await;
                        _consecutive_idle_cycles = 0;
                    } else {
                        sleep(Duration::from_millis(idle_config.backoff_ms)).await;
                    }
                }
            } else {
                _consecutive_idle_cycles = 0;
            }
        }
        break;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        TurnPhase, build_partial_timeout_messages, effective_max_tool_calls_for_turn,
        resolve_effective_turn_timeout_secs,
    };

    #[test]
    fn turn_timeout_respects_tool_wall_clock_budget() {
        // Default turn timeout (300s) should be lifted above default tool wall clock (600s).
        assert_eq!(resolve_effective_turn_timeout_secs(300, 600), 660);
    }

    #[test]
    fn turn_timeout_keeps_higher_configured_value() {
        assert_eq!(resolve_effective_turn_timeout_secs(900, 600), 900);
    }

    #[test]
    fn turn_timeout_includes_full_llm_attempt_grace() {
        assert_eq!(resolve_effective_turn_timeout_secs(360, 360), 432);
    }

    #[test]
    fn turn_timeout_expands_buffer_for_large_configs() {
        assert_eq!(resolve_effective_turn_timeout_secs(600, 600), 720);
    }

    #[test]
    fn plan_mode_applies_tool_call_floor() {
        assert_eq!(effective_max_tool_calls_for_turn(32, true), 48);
        assert_eq!(effective_max_tool_calls_for_turn(64, true), 64);
    }

    #[test]
    fn edit_mode_keeps_configured_tool_call_limit() {
        assert_eq!(effective_max_tool_calls_for_turn(32, false), 32);
    }

    #[test]
    fn plan_mode_requesting_partial_timeout_includes_continue_nudge() {
        let (timeout_message, timeout_error_message) =
            build_partial_timeout_messages(660, TurnPhase::Requesting, 25, 0, true, true);
        assert!(timeout_message.contains("Nudge with \"continue\""));
        assert!(timeout_error_message.contains("Nudge with \"continue\""));
    }

    #[test]
    fn edit_mode_requesting_partial_timeout_omits_continue_nudge() {
        let (timeout_message, timeout_error_message) =
            build_partial_timeout_messages(660, TurnPhase::Requesting, 25, 0, false, true);
        assert!(!timeout_message.contains("Nudge with \"continue\""));
        assert!(!timeout_error_message.contains("Nudge with \"continue\""));
    }

    #[test]
    fn requesting_timeout_without_tool_activity_omits_continue_nudge() {
        let (timeout_message, timeout_error_message) =
            build_partial_timeout_messages(660, TurnPhase::Requesting, 0, 0, true, false);
        assert!(!timeout_message.contains("Nudge with \"continue\""));
        assert!(!timeout_error_message.contains("Nudge with \"continue\""));
    }
}
