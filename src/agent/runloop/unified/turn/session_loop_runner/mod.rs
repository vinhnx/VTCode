mod archive;
mod execution_policy;
mod metrics;
mod plan_seed;

use super::*;
use crate::agent::runloop::git::compute_session_code_change_delta;
use crate::agent::runloop::unified::plan_mode_state::render_plan_mode_next_step_hint;
use crate::agent::runloop::unified::postamble::{ExitSummaryData, print_exit_summary};
use crate::agent::runloop::unified::turn::turn_loop::TurnLoopOutcome;
use crate::agent::runloop::welcome::SessionBootstrap;
use crate::updater::{InlineUpdateOutcome, display_update_notice, run_inline_update_prompt};
use vtcode_config::loader::SimpleConfigWatcher;
use vtcode_core::core::agent::features::FeatureSet;

const PLAN_APPROVED_EXECUTION_DIRECTIVE: &str = "Plan was approved. Start implementation immediately: execute the plan step by step beginning with the first pending step. Do not ask for another implementation confirmation.";
const PLAN_APPROVED_EXECUTION_INPUT: &str = "Implement the approved plan now.";
use archive::{
    create_session_archive, refresh_runtime_debug_context_for_next_session, workspace_archive_label,
};
use execution_policy::{
    build_partial_timeout_messages, effective_max_tool_calls_for_turn,
    resolve_effective_turn_timeout_secs,
};
use metrics::{
    TurnExecutionMetrics, capture_code_change_snapshot, emit_turn_execution_metrics,
    estimate_history_bytes,
};
use plan_seed::load_active_plan_seed;
use tokio::sync::mpsc;

#[derive(Clone)]
struct TurnHistoryCheckpoint {
    baseline_len: usize,
    #[cfg(debug_assertions)]
    prefix_fingerprint: u64,
}

impl TurnHistoryCheckpoint {
    fn capture(history: &[vtcode_core::llm::provider::Message]) -> Self {
        Self {
            baseline_len: history.len(),
            #[cfg(debug_assertions)]
            prefix_fingerprint: Self::prefix_fingerprint(history),
        }
    }

    fn rollback(&self, history: &mut Vec<vtcode_core::llm::provider::Message>) {
        #[cfg(debug_assertions)]
        self.assert_append_only(history);
        history.truncate(self.baseline_len);
    }

    #[cfg(debug_assertions)]
    fn assert_append_only(&self, history: &[vtcode_core::llm::provider::Message]) {
        debug_assert!(
            history.len() >= self.baseline_len,
            "turn history rollback requires append-only growth after checkpoint"
        );
        debug_assert_eq!(
            Self::prefix_fingerprint(&history[..self.baseline_len]),
            self.prefix_fingerprint,
            "turn history rollback requires the pre-checkpoint prefix to remain unchanged"
        );
    }

    #[cfg(debug_assertions)]
    fn prefix_fingerprint(history: &[vtcode_core::llm::provider::Message]) -> u64 {
        use std::hash::{Hash, Hasher};

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(history)
            .unwrap_or_default()
            .hash(&mut hasher);
        hasher.finish()
    }
}

fn live_reload_preserves_session_config(
    initial_vt_cfg: Option<&VTCodeConfig>,
    runtime_cfg: &CoreAgentConfig,
) -> bool {
    let Some(initial_vt_cfg) = initial_vt_cfg else {
        return true;
    };

    let mut reloaded_vt_cfg =
        vtcode_core::config::loader::ConfigManager::load_from_workspace(&runtime_cfg.workspace)
            .ok()
            .map(|manager| manager.config().clone());
    crate::agent::agents::apply_runtime_overrides(reloaded_vt_cfg.as_mut(), runtime_cfg);

    let Some(reloaded_vt_cfg) = reloaded_vt_cfg else {
        return false;
    };

    let Ok(initial_value) = serde_json::to_value(initial_vt_cfg) else {
        return false;
    };
    let Ok(reloaded_value) = serde_json::to_value(reloaded_vt_cfg) else {
        return false;
    };

    initial_value == reloaded_value
}

async fn force_reload_workspace_config_for_execution(
    workspace: &std::path::Path,
    runtime_cfg: &CoreAgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
    tool_registry: &mut vtcode_core::tools::registry::ToolRegistry,
    async_mcp_manager: Option<&crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager>,
) -> Result<()> {
    crate::agent::runloop::unified::turn::workspace::refresh_vt_config(
        workspace,
        runtime_cfg,
        vt_cfg,
    )
    .await?;

    if let Some(cfg) = vt_cfg.as_ref() {
        crate::agent::runloop::unified::turn::workspace::apply_workspace_config_to_registry(
            tool_registry,
            cfg,
        )?;

        if let Some(mcp_manager) = async_mcp_manager {
            let desired_policy =
                crate::agent::runloop::unified::async_mcp_manager::approval_policy_from_human_in_the_loop(
                    cfg.security.human_in_the_loop,
                );
            if mcp_manager.approval_policy() != desired_policy {
                mcp_manager.set_approval_policy(desired_policy);
            }
        }
    }

    Ok(())
}

struct ExitHeaderDisplay {
    provider_label: String,
    reasoning_label: String,
    context_window_size: usize,
    mode_label: String,
    tools_count: usize,
    editing_mode: vtcode_tui::EditingMode,
    autonomous_mode: bool,
    full_auto: bool,
}

fn build_exit_header_context_fast(
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
    display: ExitHeaderDisplay,
) -> vtcode_tui::InlineHeaderContext {
    use vtcode_core::config::constants::ui;

    let trust_label = match session_bootstrap.acp_workspace_trust {
        Some(vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode::FullAuto) => {
            "full_auto"
        }
        Some(vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy) => {
            "tools_policy"
        }
        None if display.full_auto || display.autonomous_mode => "full auto",
        None => "tools policy",
    };

    vtcode_tui::InlineHeaderContext {
        app_name: vtcode_core::config::constants::app::DISPLAY_NAME.to_string(),
        provider: format!("{}{}", ui::HEADER_PROVIDER_PREFIX, display.provider_label),
        model: format!("{}{}", ui::HEADER_MODEL_PREFIX, config.model),
        context_window_size: Some(display.context_window_size),
        version: env!("CARGO_PKG_VERSION").to_string(),
        search_tools: Some(crate::agent::runloop::ui::build_search_tools_badge(
            &config.workspace,
        )),
        editor_context: None,
        git: format!(
            "{}{}",
            ui::HEADER_GIT_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        ),
        mode: display.mode_label,
        reasoning: format!("{}{}", ui::HEADER_REASONING_PREFIX, display.reasoning_label),
        reasoning_stage: None,
        workspace_trust: format!("{}{}", ui::HEADER_TRUST_PREFIX, trust_label),
        tools: format!("{}{}", ui::HEADER_TOOLS_PREFIX, display.tools_count),
        mcp: format!(
            "{}{}",
            ui::HEADER_MCP_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        ),
        highlights: Vec::new(),
        editing_mode: display.editing_mode,
        autonomous_mode: display.autonomous_mode,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_single_agent_loop_unified_impl(
    config: &CoreAgentConfig,
    initial_vt_cfg: Option<VTCodeConfig>,
    _skip_confirmations: bool,
    full_auto: bool,
    plan_mode: bool,
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
    let live_reload_enabled =
        live_reload_preserves_session_config(initial_vt_cfg.as_ref(), &config);
    if !live_reload_enabled {
        tracing::debug!(
            "Configuration live reload disabled because startup overrides cannot be reproduced from workspace config"
        );
    }
    let mut vt_cfg = initial_vt_cfg.or_else(|| config_watcher.load_config());
    let mut idle_config = extract_idle_config(vt_cfg.as_ref());

    loop {
        let session_started_at = Instant::now();
        let start_code_changes = capture_code_change_snapshot(&config.workspace, "start").await;
        let resume_request = resume_state.take();
        let resume_ref = resume_request.as_ref();
        let thread_manager = vtcode_core::core::threads::ThreadManager::new();
        let archive_metadata = vtcode_core::core::threads::build_thread_archive_metadata(
            &config.workspace,
            &config.model,
            &config.provider,
            &config.theme,
            config.reasoning_effort.as_str(),
        )
        .with_debug_log_path(
            crate::main_helpers::runtime_debug_log_path()
                .map(|path| path.to_string_lossy().to_string()),
        );
        let reserved_archive_id = crate::main_helpers::runtime_archive_session_id();
        let prepared_resume = if let Some(resume) = resume_ref {
            Some(
                vtcode_core::core::threads::prepare_archived_session(
                    resume.listing().clone(),
                    config.workspace.clone(),
                    archive_metadata.clone(),
                    resume.intent().clone(),
                    if resume.is_fork() {
                        reserved_archive_id.clone()
                    } else {
                        None
                    },
                )
                .await?,
            )
        } else {
            None
        };
        let (thread_handle, session_archive) = if let Some(prepared) = prepared_resume {
            (
                thread_manager
                    .start_thread_with_identifier(prepared.thread_id.clone(), prepared.bootstrap),
                Some(prepared.archive),
            )
        } else {
            let thread_id = if let Some(identifier) = reserved_archive_id.clone() {
                identifier
            } else {
                vtcode_core::utils::session_archive::reserve_session_archive_identifier(
                    &workspace_archive_label(&config.workspace),
                    None,
                )
                .await?
            };
            let bootstrap =
                vtcode_core::core::threads::ThreadBootstrap::new(Some(archive_metadata.clone()));
            let archive =
                create_session_archive(archive_metadata.clone(), Some(thread_id.clone())).await?;
            (
                thread_manager.start_thread_with_identifier(thread_id, bootstrap),
                Some(archive),
            )
        };
        crate::main_helpers::set_runtime_archive_session_id(Some(
            thread_handle.thread_id().to_string(),
        ));
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
        let turn_run_id = TurnRunId(thread_handle.thread_id().to_string());
        let effective_log_path: Option<String> = harness_config
            .event_log_path
            .as_ref()
            .filter(|path| !path.trim().is_empty())
            .cloned()
            .or_else(|| default_harness_log_dir().map(|dir| dir.to_string_lossy().into_owned()));
        let harness_emitter: Option<HarnessEventEmitter> =
            effective_log_path.as_deref().and_then(|path| {
                let resolved = resolve_event_log_path(path, &turn_run_id);
                HarnessEventEmitter::new(resolved).ok()
            });
        if let Some(emitter) = harness_emitter.as_ref() {
            let open_responses_config = vt_cfg
                .as_ref()
                .map(|cfg| cfg.agent.open_responses.clone())
                .unwrap_or_default();
            let features = FeatureSet::from_config(vt_cfg.as_ref());
            if features.open_responses.emit_events {
                let or_path = effective_log_path.as_ref().map(|base| {
                    let parent = std::path::Path::new(base.as_str())
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
        let steering_sender = if steering_receiver.is_none() {
            let (sender, receiver) = mpsc::unbounded_channel();
            *steering_receiver = Some(receiver);
            Some(sender)
        } else {
            None
        };
        let ui_setup = initialize_session_ui(
            &config,
            vt_cfg.as_ref(),
            &mut session_state,
            resume_ref,
            session_archive,
            full_auto,
            _skip_confirmations,
            steering_sender,
        )
        .await?;
        let mut renderer = ui_setup.renderer;
        let mut session = ui_setup.session;
        let handle = ui_setup.handle;
        let mut header_context = ui_setup.header_context;
        let mut ide_context_bridge = ui_setup.ide_context_bridge;
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
        let _file_palette_task_guard = ui_setup.file_palette_task_guard;
        let _startup_update_task_guard = ui_setup.startup_update_task_guard;
        let startup_update_cached_notice = ui_setup.startup_update_cached_notice;
        let mut startup_update_notice_rx = ui_setup.startup_update_notice_rx;
        let SessionState {
            session_bootstrap,
            mut provider_client,
            mut tool_registry,
            tools,
            tool_catalog,
            mut conversation_history,
            execution,
            metadata,
            async_mcp_manager,
            mut mcp_panel_state,
            loaded_skills,
            ..
        } = session_state;
        let decision_ledger = metadata.decision_ledger;
        let traj = metadata.trajectory;
        let telemetry = metadata.telemetry;
        let error_recovery = metadata.error_recovery;
        let tool_result_cache = execution.tool_result_cache;
        let tool_permission_cache = execution.tool_permission_cache;
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
        if plan_mode {
            transition_to_plan_mode(&tool_registry, &mut session_stats, &handle, true, true).await;
            render_plan_mode_next_step_hint(&mut renderer)?;
        }
        let mut linked_directories: Vec<LinkedDirectory> = Vec::with_capacity(4);
        let mut model_picker_state: Option<ModelPickerState> = None;
        let mut palette_state: Option<ActivePalette> = None;
        let mut last_forced_redraw = Instant::now();
        let mut input_status_state = InputStatusState::default();
        let mut prefer_latest_queued_input_once = false;
        crate::agent::runloop::unified::status_line::update_ide_context_source(
            &mut input_status_state,
            crate::agent::runloop::unified::session_setup::ide_context_status_label_from_bridge(
                &context_manager,
                config.workspace.as_path(),
                vt_cfg.as_ref(),
                ide_context_bridge.as_ref(),
            ),
        );
        let mut queued_inputs: VecDeque<String> = VecDeque::with_capacity(8);
        let mut ctrl_c_notice_displayed = false;
        let mut mcp_catalog_initialized = tool_registry.mcp_client().is_some();
        let mut last_known_mcp_tools: Vec<String> = Vec::with_capacity(16);
        let mut pending_mcp_refresh = false;
        let mut last_mcp_refresh = std::time::Instant::now();
        let startup_update_requested_restart =
            if let Some(notice) = startup_update_cached_notice.as_ref() {
                display_update_notice(
                    &handle,
                    &mut header_context,
                    renderer.should_use_unicode_formatting(),
                    notice,
                );
                matches!(
                    run_inline_update_prompt(
                        &mut renderer,
                        &handle,
                        &mut session,
                        &ctrl_c_state,
                        &ctrl_c_notify,
                        config.workspace.as_path(),
                        notice,
                    )
                    .await?,
                    InlineUpdateOutcome::RestartRequested
                )
            } else {
                false
            };

        if startup_update_requested_restart {
            session_end_reason = SessionEndReason::Completed;
        }

        if !startup_update_requested_restart {
            loop {
                let interaction_outcome = {
                    let mut interaction_turn_metadata_cache = None;
                    let mut interaction_ctx = crate::agent::runloop::unified::turn::session::interaction_loop::InteractionLoopContext {
                    renderer: &mut renderer,
                    session: &mut session,
                    handle: &handle,
                    header_context: &mut header_context,
                    ide_context_bridge: &mut ide_context_bridge,
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
                    error_recovery: &error_recovery,
                    last_forced_redraw: &mut last_forced_redraw,
                    turn_metadata_cache: &mut interaction_turn_metadata_cache,
                    harness_config: harness_config.clone(),
                    steering_receiver,
                    startup_update_notice_rx: &mut startup_update_notice_rx,
                };

                    let mut interaction_state =
                    crate::agent::runloop::unified::turn::session::interaction_loop::InteractionState {
                        input_status_state: &mut input_status_state,
                        queued_inputs: &mut queued_inputs,
                        prefer_latest_queued_input_once: &mut prefer_latest_queued_input_once,
                        model_picker_state: &mut model_picker_state,
                        palette_state: &mut palette_state,
                        last_known_mcp_tools: &mut last_known_mcp_tools,
                        pending_mcp_refresh: &mut pending_mcp_refresh,
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
                let next_turn_input = match interaction_outcome {
                    InteractionOutcome::Exit { reason } => {
                        session_end_reason = reason;
                        break;
                    }
                    InteractionOutcome::Resume { resume_session } => {
                        resume_state = Some(*resume_session);
                        session_end_reason = SessionEndReason::Completed;
                        break;
                    }
                    InteractionOutcome::DirectToolHandled => {
                        // Explicit `run ...` / `!cmd` interactions are direct command mode:
                        // render the tool output and wait for the next user input instead of
                        // fabricating an autonomous follow-up turn.
                        continue;
                    }
                    InteractionOutcome::Continue { input } => input,
                    InteractionOutcome::PlanApproved { auto_accept } => {
                        let plan_seed = load_active_plan_seed(&tool_registry).await;
                        crate::agent::runloop::unified::plan_mode_state::transition_to_edit_mode(
                            &tool_registry,
                            &mut session_stats,
                            &handle,
                            true,
                        )
                        .await;
                        handle.set_skip_confirmations(auto_accept);
                        renderer.line(MessageStyle::Info, "Executing approved plan...")?;

                        if let Err(err) = force_reload_workspace_config_for_execution(
                            config.workspace.as_path(),
                            &config,
                            &mut vt_cfg,
                            &mut tool_registry,
                            async_mcp_manager.as_deref(),
                        )
                        .await
                        {
                            tracing::warn!(
                                "Failed to reload workspace configuration at plan approval: {}",
                                err
                            );
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Failed to reload configuration: {}", err),
                            )?;
                        }

                        let mut execution_directive = PLAN_APPROVED_EXECUTION_DIRECTIVE.to_string();
                        if let Some(seed) = plan_seed {
                            execution_directive.push_str("\n\nApproved plan context:\n");
                            execution_directive.push_str(&seed);
                        }
                        conversation_history.push(vtcode_core::llm::provider::Message::system(
                            execution_directive,
                        ));
                        PLAN_APPROVED_EXECUTION_INPUT.to_string()
                    }
                };
                if next_turn_input.trim().is_empty() {
                    continue;
                }
                let mut working_history = std::mem::take(&mut conversation_history);
                let timeout_secs = resolve_effective_turn_timeout_secs(
                    resolve_timeout(
                        vt_cfg
                            .as_ref()
                            .map(|cfg| cfg.optimization.agent_execution.max_execution_time_secs),
                    ),
                    harness_config.max_tool_wall_clock_secs,
                );
                let turn_started_at = Instant::now();
                let mut attempts: usize = 0;
                let history_snapshot_bytes = estimate_history_bytes(&working_history);
                let turn_history_checkpoint = TurnHistoryCheckpoint::capture(&working_history);
                let mut turn_metadata_cache = None;
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
                    let execution_history_len_before_attempt =
                        tool_registry.execution_history_len();
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
                        &error_recovery,
                        &mut harness_state,
                        harness_emitter.as_ref(),
                        &mut config,
                        vt_cfg.as_ref(),
                        &mut turn_metadata_cache,
                        &mut provider_client,
                        &traj,
                        full_auto,
                        steering_receiver,
                    );

                    let result = timeout(
                        Duration::from_secs(timeout_secs),
                        crate::agent::runloop::unified::turn::run_turn_loop(
                            &mut working_history,
                            turn_loop_ctx,
                        ),
                    )
                    .await;

                    match result {
                        Ok(inner) => break inner,
                        Err(_) => {
                            let active_pty_sessions_before_cancel =
                                tool_registry.active_pty_sessions();
                            let attempted_tool_calls = harness_state.tool_calls;
                            let timed_out_phase = harness_state.phase;
                            if let Err(err) =
                                tool_registry.terminate_all_exec_sessions_async().await
                            {
                                tracing::warn!(error = %err, "Failed to terminate all exec sessions after turn timeout");
                            }
                            let execution_history_len_after_attempt =
                                tool_registry.execution_history_len();
                            handle.set_input_status(None, None);
                            input_status_state.left = None;
                            input_status_state.right = None;

                            if ctrl_c_state.is_exit_requested()
                                || ctrl_c_state.is_cancel_requested()
                            {
                                let interrupted_result = if ctrl_c_state.is_exit_requested() {
                                    RunLoopTurnLoopResult::Exit
                                } else {
                                    RunLoopTurnLoopResult::Cancelled
                                };
                                turn_history_checkpoint.rollback(&mut working_history);
                                break Ok(TurnLoopOutcome {
                                    result: interrupted_result,
                                    turn_modified_files: std::collections::BTreeSet::new(),
                                });
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
                            turn_history_checkpoint.rollback(&mut working_history);
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
                        handle.set_input_status(None, None);
                        let _ = renderer.line_if_not_empty(MessageStyle::Output);
                        tracing::error!("Turn execution error: {}", err);
                        let _ = renderer.line(MessageStyle::Error, &format!("Error: {}", err));
                        TurnLoopOutcome {
                            result: RunLoopTurnLoopResult::Aborted,
                            turn_modified_files: std::collections::BTreeSet::new(),
                        }
                    }
                };
                conversation_history = working_history;
                let outcome_result = outcome.result.clone();
                let turn_elapsed = turn_started_at.elapsed();
                let show_turn_timer = vt_cfg
                    .as_ref()
                    .map(|cfg| cfg.ui.show_turn_timer)
                    .unwrap_or(true);
                if let Err(err) = crate::agent::runloop::unified::turn::apply_turn_outcome(
                    outcome,
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
                emit_turn_execution_metrics(TurnExecutionMetrics {
                    attempts_made: attempts.saturating_add(1),
                    retry_count: attempts,
                    history_snapshot_bytes,
                    timeout_secs,
                    elapsed_ms: turn_elapsed.as_millis(),
                    outcome: match &outcome_result {
                        RunLoopTurnLoopResult::Completed => "completed",
                        RunLoopTurnLoopResult::Aborted => "aborted",
                        RunLoopTurnLoopResult::Cancelled => "cancelled",
                        RunLoopTurnLoopResult::Exit => "exit",
                        RunLoopTurnLoopResult::Blocked { .. } => "blocked",
                    },
                });

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
                    let skill_names: Vec<String> =
                        loaded_skills.read().await.keys().cloned().collect();

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
                match &outcome_result {
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
                                Some(
                                    "Turn blocked due to repeated failing tool behavior."
                                        .to_string(),
                                )
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
        }
        if let Some(archive) = session_archive.as_mut() {
            let skill_names: Vec<String> = loaded_skills.read().await.keys().cloned().collect();
            archive.set_loaded_skills(skill_names);
        }
        if let Some(emitter) = harness_emitter.as_ref() {
            emitter.finish_open_responses();
        }
        let finalization_output = match finalize_session(
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
            Ok(output) => Some(output),
            Err(err) => {
                tracing::error!("Failed to finalize session: {}", err);
                renderer
                    .line(
                        MessageStyle::Error,
                        &format!("Failed to finalize session: {}", err),
                    )
                    .ok();
                None
            }
        };
        if let Some(next_resume) = resume_state.as_ref() {
            refresh_runtime_debug_context_for_next_session(
                config.workspace.as_path(),
                Some(next_resume),
            )
            .await?;
            continue;
        }
        if matches!(session_end_reason, SessionEndReason::NewSession) {
            if live_reload_enabled && config_watcher.should_reload() {
                vt_cfg = config_watcher.load_config();
                crate::agent::agents::apply_runtime_overrides(vt_cfg.as_mut(), &config);
                idle_config = extract_idle_config(vt_cfg.as_ref());
                tracing::debug!("Configuration reloaded due to file changes");
            }

            refresh_runtime_debug_context_for_next_session(config.workspace.as_path(), None)
                .await?;
            resume_state = None;
            _consecutive_idle_cycles = 0;
            continue;
        }
        if live_reload_enabled && config_watcher.should_reload() {
            vt_cfg = config_watcher.load_config();
            crate::agent::agents::apply_runtime_overrides(vt_cfg.as_mut(), &config);
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

        let end_code_changes = capture_code_change_snapshot(&config.workspace, "end").await;
        let code_change_delta = compute_session_code_change_delta(
            start_code_changes.as_ref(),
            end_code_changes.as_ref(),
        );
        let telemetry_snapshot = match telemetry.get_snapshot() {
            Ok(snapshot) => snapshot,
            Err(err) => {
                tracing::warn!(
                    "Failed to capture telemetry snapshot for postamble: {}",
                    err
                );
                vtcode_core::core::telemetry::TelemetryStats::default()
            }
        };
        let finalization_succeeded = finalization_output.is_some();
        let resume_identifier = finalization_output
            .and_then(|output| output.archive_path)
            .and_then(|path| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(|stem| stem.to_string())
            });
        let provider_name = provider_client.name().to_string();
        let reasoning_label = vt_cfg
            .as_ref()
            .map(|cfg| cfg.agent.reasoning_effort.as_str().to_string())
            .unwrap_or_else(|| config.reasoning_effort.as_str().to_string());
        let mode_label = match (config.ui_surface, full_auto) {
            (vtcode_core::config::types::UiSurfacePreference::Inline, true) => "auto".to_string(),
            (vtcode_core::config::types::UiSurfacePreference::Inline, false) => {
                "inline".to_string()
            }
            (vtcode_core::config::types::UiSurfacePreference::Alternate, _) => "alt".to_string(),
            (vtcode_core::config::types::UiSurfacePreference::Auto, true) => "auto".to_string(),
            (vtcode_core::config::types::UiSurfacePreference::Auto, false) => "std".to_string(),
        };
        let tools_count = tools.read().await.len();
        let provider_label = if config.provider.trim().is_empty() {
            provider_name.clone()
        } else {
            config.provider.clone()
        };
        let header_context = Some(build_exit_header_context_fast(
            &config,
            &session_bootstrap,
            ExitHeaderDisplay {
                provider_label,
                reasoning_label,
                context_window_size: provider_client.effective_context_size(&config.model),
                mode_label,
                tools_count,
                editing_mode: if session_stats.is_plan_mode() {
                    vtcode_tui::EditingMode::Plan
                } else {
                    vtcode_tui::EditingMode::Edit
                },
                autonomous_mode: session_stats.is_autonomous_mode(),
                full_auto,
            },
        ));
        if !finalization_succeeded {
            let _ = vtcode_tui::panic_hook::restore_tui();
        }
        print_exit_summary(ExitSummaryData {
            total_session_time: session_started_at.elapsed(),
            code_changes: code_change_delta,
            telemetry: telemetry_snapshot,
            header_context,
            resume_identifier,
        });
        break;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        TurnHistoryCheckpoint, archive::NextRuntimeArchiveId,
        archive::next_runtime_archive_id_request, archive::workspace_archive_label,
        build_partial_timeout_messages, effective_max_tool_calls_for_turn,
        resolve_effective_turn_timeout_secs,
    };
    use crate::agent::agents::ResumeSession;
    use crate::agent::runloop::unified::run_loop_context::TurnPhase;
    use chrono::Utc;
    use std::path::{Path, PathBuf};
    use vtcode_core::core::threads::ArchivedSessionIntent;
    use vtcode_core::llm::provider::MessageRole;
    use vtcode_core::utils::session_archive::{
        SessionArchiveMetadata, SessionListing, SessionMessage, SessionSnapshot,
    };

    fn resume_session(intent: ArchivedSessionIntent) -> ResumeSession {
        let listing = SessionListing {
            path: PathBuf::from("/tmp/session-source.json"),
            snapshot: SessionSnapshot {
                metadata: SessionArchiveMetadata::new(
                    "workspace",
                    "/tmp/workspace",
                    "model",
                    "provider",
                    "theme",
                    "medium",
                ),
                started_at: Utc::now(),
                ended_at: Utc::now(),
                total_messages: 1,
                distinct_tools: Vec::new(),
                transcript: Vec::new(),
                messages: vec![SessionMessage::new(MessageRole::User, "hello")],
                progress: None,
                error_logs: Vec::new(),
            },
        };

        ResumeSession::from_listing(&listing, intent)
    }

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
    fn zero_tool_call_limit_stays_unlimited_in_all_modes() {
        assert_eq!(effective_max_tool_calls_for_turn(0, true), 0);
        assert_eq!(effective_max_tool_calls_for_turn(0, false), 0);
    }

    #[test]
    fn edit_mode_keeps_configured_tool_call_limit() {
        assert_eq!(effective_max_tool_calls_for_turn(32, false), 32);
    }

    #[test]
    fn plan_mode_requesting_partial_timeout_includes_autonomous_recovery_note() {
        let (timeout_message, timeout_error_message) =
            build_partial_timeout_messages(660, TurnPhase::Requesting, 25, 0, true, true);
        assert!(timeout_message.contains("Autonomous recovery"));
        assert!(timeout_error_message.contains("Autonomous recovery"));
    }

    #[test]
    fn edit_mode_requesting_partial_timeout_omits_autonomous_recovery_note() {
        let (timeout_message, timeout_error_message) =
            build_partial_timeout_messages(660, TurnPhase::Requesting, 25, 0, false, true);
        assert!(!timeout_message.contains("Autonomous recovery"));
        assert!(!timeout_error_message.contains("Autonomous recovery"));
    }

    #[test]
    fn turn_history_checkpoint_truncates_appended_messages() {
        let mut history = vec![
            vtcode_core::llm::provider::Message::user("before".to_string()),
            vtcode_core::llm::provider::Message::assistant("baseline".to_string()),
        ];
        let checkpoint = TurnHistoryCheckpoint::capture(&history);

        history.push(vtcode_core::llm::provider::Message::assistant(
            "during turn".to_string(),
        ));
        history.push(vtcode_core::llm::provider::Message::tool_response(
            "call-1".to_string(),
            "{\"ok\":true}".to_string(),
        ));

        checkpoint.rollback(&mut history);

        assert_eq!(history.len(), 2);
        assert_eq!(history[0].content.as_text(), "before");
        assert_eq!(history[1].content.as_text(), "baseline");
    }

    #[test]
    fn turn_history_checkpoint_preserves_preexisting_history_prefix() {
        let mut history = vec![
            vtcode_core::llm::provider::Message::system("system".to_string()),
            vtcode_core::llm::provider::Message::user("request".to_string()),
            vtcode_core::llm::provider::Message::assistant("response".to_string()),
        ];
        let expected_prefix = history.clone();
        let checkpoint = TurnHistoryCheckpoint::capture(&history);

        history.push(vtcode_core::llm::provider::Message::assistant(
            "retryable append".to_string(),
        ));

        checkpoint.rollback(&mut history);

        assert_eq!(history, expected_prefix);
    }

    #[test]
    fn requesting_timeout_without_tool_activity_omits_autonomous_recovery_note() {
        let (timeout_message, timeout_error_message) =
            build_partial_timeout_messages(660, TurnPhase::Requesting, 0, 0, true, false);
        assert!(!timeout_message.contains("Autonomous recovery"));
        assert!(!timeout_error_message.contains("Autonomous recovery"));
    }

    #[test]
    fn workspace_archive_label_uses_directory_name() {
        assert_eq!(workspace_archive_label(Path::new("/tmp/demo")), "demo");
    }

    #[test]
    fn next_runtime_archive_id_request_reuses_existing_id_for_resume() {
        let resume = resume_session(ArchivedSessionIntent::ResumeInPlace);

        assert_eq!(
            next_runtime_archive_id_request(Path::new("/tmp/workspace"), Some(&resume)),
            NextRuntimeArchiveId::Existing("session-source".to_string())
        );
    }

    #[test]
    fn next_runtime_archive_id_request_reserves_for_fork_and_new_session() {
        let resume = resume_session(ArchivedSessionIntent::ForkNewArchive {
            custom_suffix: Some("branch".to_string()),
        });

        assert_eq!(
            next_runtime_archive_id_request(Path::new("/tmp/workspace"), Some(&resume)),
            NextRuntimeArchiveId::Reserve {
                workspace_label: "workspace".to_string(),
                custom_suffix: Some("branch".to_string()),
            }
        );
        assert_eq!(
            next_runtime_archive_id_request(Path::new("/tmp/workspace"), None),
            NextRuntimeArchiveId::Reserve {
                workspace_label: "workspace".to_string(),
                custom_suffix: None,
            }
        );
    }
}
