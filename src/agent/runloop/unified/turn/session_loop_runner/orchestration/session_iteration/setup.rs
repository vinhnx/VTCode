//! Session setup for the unified agent loop.
//!
//! Builds every loop-lived value: thread/archive bootstrap, the static-first
//! shell, UI setup, registry completion, runtime construction, and the
//! interaction-loop state (queues, guards, trackers, placeholders). Fields
//! stay in declaration order so guard drop order matches the inline setup.
//!
//! [`SetupFlow::Break`] ends the iteration before the loop starts (init was
//! cancelled or exited); [`SetupFlow::Proceed`] hands ownership to the loop.

use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

use anyhow::Result;
use hashbrown::HashMap;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::{Notify, RwLock};
use tokio_util::sync::CancellationToken;
use vtcode_commons::TaskGuard;
use vtcode_config::core::agent::AgentHarnessConfig;
use vtcode_config::loader::SimpleConfigWatcher;
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::PermissionsConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::error_recovery::ErrorRecoveryState;
use vtcode_core::core::agent::runtime::AgentRuntime;
use vtcode_core::core::agent::snapshots::SnapshotManager;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::telemetry::TelemetryManager;
use vtcode_core::core::threads::ThreadRuntimeHandle;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::hooks::{LifecycleHookEngine, SessionEndReason};
use vtcode_core::llm::provider::{LLMProvider, ToolDefinition};
use vtcode_core::primary_agent::ActivePrimaryAgentState;
use vtcode_core::skills::types::Skill;
use vtcode_core::subagents::BackgroundCompletionEvent;
use vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter;
use vtcode_core::tools::autonomous_executor::AutonomousExecutor;
use vtcode_core::tools::circuit_breaker::CircuitBreaker;
use vtcode_core::tools::exec_session::ExecSessionCompletionEvent;
use vtcode_core::tools::exec_session::ExecSessionManager;
use vtcode_core::tools::health::ToolHealthTracker;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::tools::{ApprovalRecorder, ToolResultCache};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::session_archive::SessionArchive;
use vtcode_ui::tui::app::{InlineHandle, InlineHeaderContext, InlineSession};

use super::*;
use crate::agent::runloop::git::FileStat;
use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::model_picker::ModelPickerState;
use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::inline_events::QueuedInput;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::run_loop_context::TurnRunId;
use crate::agent::runloop::unified::session_settings::{SessionSettingsControl, SharedSettingsSnapshot};
use crate::agent::runloop::unified::session_setup::{EditorOpenDispatcher, EditorOpenRequestSender};
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::unified::webmcp::ActiveWebmcpBridge;
use crate::agent::runloop::unified::workspace_links::LinkedDirectory;
use crate::agent::runloop::welcome::SessionBootstrap;
use crate::updater::StartupUpdateNotice;

/// Loop-lived state built by session setup, in declaration order.
pub(super) struct Setup<'a> {
    pub config: &'a mut CoreAgentConfig,
    pub session_skip_confirmations: &'a mut bool,
    pub resume_state: &'a mut Option<ResumeSession>,
    pub config_watcher: &'a mut SimpleConfigWatcher,
    pub vt_cfg: &'a mut Option<VTCodeConfig>,
    pub pending_session_start_trigger: &'a mut Option<SessionStartTrigger>,
    pub next_session_primary_agent: &'a mut Option<String>,
    pub terminal_enabled: bool,
    pub full_auto: bool,
    pub session_started_at: Instant,
    pub active_thread_label: String,
    pub start_code_changes: Option<HashMap<PathBuf, FileStat>>,
    pub thread_handle: ThreadRuntimeHandle,
    pub harness_config: AgentHarnessConfig,
    pub turn_run_id: TurnRunId,
    pub harness_emitter: Option<HarnessEventEmitter>,
    pub renderer: AnsiRenderer,
    pub session: InlineSession,
    pub handle: InlineHandle,
    pub header_context: InlineHeaderContext,
    pub ctrl_c_state: Arc<CtrlCState>,
    pub ctrl_c_notify: Arc<Notify>,
    pub settings_snapshot: SharedSettingsSnapshot,
    pub input_activity_counter: Arc<AtomicU64>,
    pub checkpoint_manager: Option<SnapshotManager>,
    pub session_archive: Option<SessionArchive>,
    pub lifecycle_hooks: Option<LifecycleHookEngine>,
    pub context_manager: ContextManager,
    pub default_placeholder: Option<String>,
    pub follow_up_placeholder: Option<String>,
    pub next_checkpoint_turn: usize,
    pub session_end_reason: SessionEndReason,
    pub _file_palette_task_guard: TaskGuard,
    pub _background_subprocess_task_guard: Option<TaskGuard>,
    pub _startup_update_task_guard: Option<TaskGuard>,
    pub _editor_open_coordinator_task_guard: TaskGuard,
    pub _settings_task_guard: TaskGuard,
    pub editor_open_sender: EditorOpenRequestSender,
    pub editor_open_dispatcher: Arc<EditorOpenDispatcher>,
    pub startup_update_notice_rx: Option<mpsc::UnboundedReceiver<StartupUpdateNotice>>,
    pub settings_receiver: mpsc::UnboundedReceiver<SessionSettingsControl>,
    pub session_bootstrap: SessionBootstrap,
    pub provider_client: Box<dyn LLMProvider>,
    pub tool_registry: ToolRegistry,
    pub tools: Arc<RwLock<Vec<ToolDefinition>>>,
    pub tool_catalog: Arc<ToolCatalogState>,
    pub async_mcp_manager: Option<Arc<AsyncMcpManager>>,
    pub mcp_panel_state: McpPanelState,
    pub loaded_skills: Arc<RwLock<HashMap<String, Skill>>>,
    pub active_primary_agent: ActivePrimaryAgentState,
    pub decision_ledger: Arc<RwLock<DecisionTracker>>,
    pub traj: TrajectoryLogger,
    pub telemetry: Arc<TelemetryManager>,
    pub error_recovery: Arc<RwLock<ErrorRecoveryState>>,
    pub runtime: AgentRuntime,
    pub tool_result_cache: Arc<RwLock<ToolResultCache>>,
    pub tool_permission_cache: Arc<RwLock<ToolPermissionCache>>,
    pub permissions_state: Arc<RwLock<PermissionsConfig>>,
    pub approval_recorder: Arc<ApprovalRecorder>,
    pub safety_validator: Arc<ToolCallSafetyValidator>,
    pub circuit_breaker: Arc<CircuitBreaker>,
    pub tool_health_tracker: Arc<ToolHealthTracker>,
    pub rate_limiter: Arc<AdaptiveRateLimiter>,
    pub autonomous_executor: Arc<AutonomousExecutor>,
    pub cancel_token: CancellationToken,
    pub _cancel_guard: CancelGuard,
    pub _signal_handler: TaskGuard,
    pub session_stats: SessionStats,
    pub plan_session: PlanningWorkflowSessionState,
    pub linked_directories: Vec<LinkedDirectory>,
    pub model_picker_state: Option<ModelPickerState>,
    pub palette_state: Option<ActivePalette>,
    pub last_forced_redraw: Instant,
    pub input_status_state: InputStatusState,
    pub dismissed_memory_cleanup_fingerprint: Option<(usize, usize)>,
    pub prefer_latest_queued_input_once: bool,
    pub queued_inputs: VecDeque<QueuedInput>,
    pub background_completion_receiver: Option<broadcast::Receiver<BackgroundCompletionEvent>>,
    pub _matrix_interrupt_guard: Option<TaskGuard>,
    pub exec_session_manager: ExecSessionManager,
    pub exec_completion_receiver: Option<broadcast::Receiver<ExecSessionCompletionEvent>>,
    pub exec_completion_notify: Option<Arc<Notify>>,
    pub pending_background_completions: PendingBackgroundCompletions,
    pub pending_matrix_note: Option<String>,
    pub displayed_matrix_revision: Option<(String, u64)>,
    pub webmcp_prompt_sender: mpsc::Sender<String>,
    pub webmcp_prompt_receiver: Option<mpsc::Receiver<String>>,
    pub webmcp_bridge: Option<ActiveWebmcpBridge>,
    pub agent_touched_paths: BTreeSet<PathBuf>,
    pub ctrl_c_notice_displayed: bool,
    pub inline_prompt_cost_notice_shown: bool,
    pub mcp_catalog_initialized: bool,
    pub last_known_mcp_tools: Vec<String>,
    pub pending_mcp_refresh: bool,
    pub last_mcp_refresh: Instant,
    pub startup_update_requested_restart: bool,
}

/// How the session iteration proceeds after setup.
pub(super) enum SetupFlow<'a> {
    Break,
    Proceed(Box<Setup<'a>>),
}

/// Run critical session setup: thread, shell, UI, registry, runtime, and loop state.
pub(super) async fn run_session_setup<'a>(state: SessionLoopState<'a>) -> Result<SetupFlow<'a>> {
    let SessionLoopState {
        config,
        session_skip_confirmations,
        resume_state,
        config_watcher,
        vt_cfg,
        pending_session_start_trigger,
        next_session_primary_agent,
        steering_receiver,
        host_session,
        event_sender,
        stop_slot,
        terminal_enabled,
        skip_confirmations,
        full_auto,
        primary_agent_explicitly_configured,
        planning_entry_source,
    } = state;

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
    let active_thread_label = resume_ref.map_or_else(|| "main".to_owned(), |request| request.thread_label().to_owned());
    let archive_metadata = vtcode_core::core::threads::build_thread_archive_metadata(
        &config.workspace,
        &config.model,
        &config.provider,
        &config.theme,
        config.reasoning_effort.as_str(),
    )
    .with_debug_log_path(crate::main_helpers::runtime_debug_log_path().map(|path| path.to_string_lossy().to_string()));
    let reserved_archive_id = crate::main_helpers::runtime_archive_session_id();
    let history_enabled = session_archive::history_persistence_enabled();
    // Overlap the `git diff` start snapshot with archive/thread prep:
    // both are independent blocking I/O and previously summed
    // (≈120ms snapshot + 10-100ms archive reserve) on the `/new`
    // fresh-prompt path.
    let (start_code_changes, thread_bootstrap) = tokio::join!(
        capture_code_change_snapshot(&config.workspace, "start"),
        session_bootstrap::prepare_session_thread(
            config,
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
            config,
            vt_cfg.as_ref(),
            crate::agent::runloop::unified::session_setup::SessionUiLaunchOptions {
                session_archive: None,
                full_auto,
                skip_confirmations,
                steering_sender: steering_sender_for_shell,
                settings_sender: settings_sender.clone(),
                host_session: host_session.take(),
            },
        );
        let critical = initialize_session_critical(
            config,
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
    // Host adapters (desktop service) cancel running turns out-of-band: the
    // session event queue is not polled mid-turn, so publish the stop handles
    // as soon as the shell owns them.
    if let Some(slot) = stop_slot.as_ref() {
        crate::agent::runloop::unified::stop_requests::publish_stop_handles(
            slot,
            &shell.ctrl_c_state,
            &shell.ctrl_c_notify,
        );
    }
    let Some(critical_result) = critical_result else {
        crate::agent::runloop::unified::stop_requests::finish_initialization_exit(
            &shell.handle,
            &mut shell.session,
            &shell.ctrl_c_state,
            crate::agent::runloop::unified::stop_requests::InitializationExitContext {
                emitter: None,
                session_id: thread_handle.thread_id().as_str(),
                config,
                full_auto,
                started: session_started_at,
            },
        )
        .await;
        return Ok(SetupFlow::Break);
    };
    let settings_receiver = shell_settings_receiver;
    let mut session_state = critical_result?;
    vtcode_commons::startup_trace::record_phase("session_setup_critical", session_critical_phase);
    // Persist the active primary agent ("mode") so a future resume restores
    // it instead of falling back to the config default.
    persist_primary_agent(&mut session_archive, &session_state.active_primary_agent);
    let harness_config = vt_cfg.as_ref().map(|cfg| cfg.agent.harness.clone()).unwrap_or_default();
    let turn_run_id = TurnRunId(thread_handle.thread_id().to_string());
    let harness_emitter =
        harness::initialize_harness(&config.workspace, vt_cfg.as_ref(), &config.model, &turn_run_id, event_sender)
            .await?;

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
        config,
        vt_cfg.as_ref(),
        thread_handle.thread_id().as_str(),
        &mut session_state,
        session_trigger,
        resume_ref,
        shell,
        crate::agent::runloop::unified::stop_requests::InitializationExitContext {
            emitter: harness_emitter.as_ref(),
            session_id: thread_handle.thread_id().as_str(),
            config,
            full_auto,
            started: session_started_at,
        },
        crate::agent::runloop::unified::session_setup::SessionUiLaunchOptions {
            session_archive,
            full_auto,
            skip_confirmations,
            steering_sender: None,
            settings_sender: settings_sender.clone(),
            host_session: None,
        },
    )
    .await;
    let Some(mut ui_setup) = harness_try!(ui_setup) else {
        return Ok(SetupFlow::Break);
    };
    let initialization_progress = ui_setup
        .handle
        .resume_progress(vtcode_commons::ui_protocol::ProgressPhase::Initializing);
    vtcode_commons::startup_trace::record_phase("session_setup_ui", session_ui_phase);

    if let ControlFlow::Break(()) = harness_try!(
        initialization::complete_session_initialization(
            &mut session_state,
            &mut ui_setup,
            initialization::SessionInitializationContext {
                config,
                vt_cfg: vt_cfg.as_ref(),
                full_auto,
                primary_agent_explicitly_configured,
                resume_ref,
                session_id: thread_handle.thread_id().as_str(),
                session_primary_agent_override: session_primary_agent_override.as_deref(),
                session_trigger,
                harness_emitter: harness_emitter.as_ref(),
                session_started_at,
            },
        )
        .await
    ) {
        return Ok(SetupFlow::Break);
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
    let settings_snapshot = ui_setup.settings_snapshot;
    let input_activity_counter = ui_setup.input_activity_counter;
    let checkpoint_manager = ui_setup.checkpoint_manager;
    let mut session_archive = ui_setup.session_archive;
    let lifecycle_hooks = ui_setup.lifecycle_hooks;
    let context_manager = ui_setup.context_manager;
    let default_placeholder = ui_setup.default_placeholder;
    let follow_up_placeholder = ui_setup.follow_up_placeholder;
    let next_checkpoint_turn = ui_setup.next_checkpoint_turn;
    if resume_ref.is_some()
        && let Some(emitter) = harness_emitter.as_ref()
    {
        for diagnostic in harness_try!(emitter.diagnostics().await) {
            harness_try!(renderer.restore_diagnostic(&diagnostic));
        }
    }
    let mut session_end_reason = ui_setup.session_end_reason;
    let _file_palette_task_guard = ui_setup.file_palette_task_guard;
    let _background_subprocess_task_guard = ui_setup.background_subprocess_task_guard;
    let _startup_update_task_guard = ui_setup.startup_update_task_guard;
    let _editor_open_coordinator_task_guard = ui_setup.editor_open_coordinator_task_guard;
    let _settings_task_guard = ui_setup.settings_task_guard;
    let editor_open_sender = ui_setup.editor_open_sender;
    let editor_open_dispatcher = ui_setup.editor_open_dispatcher;
    let startup_update_cached_notice = ui_setup.startup_update_cached_notice;
    let startup_update_notice_rx = ui_setup.startup_update_notice_rx;
    let SessionState {
        session_bootstrap,
        provider_client,
        tool_registry: tool_registry_opt,
        tools,
        tool_catalog,
        conversation_history,
        execution,
        metadata,
        async_mcp_manager,
        mcp_panel_state,
        loaded_skills,
        mut active_primary_agent,
        ..
    } = session_state;
    // `complete_session_registry` already ran after first paint; the
    // interaction loop requires the concrete registry.
    let tool_registry = tool_registry_opt.expect("tool registry completed before interaction");
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
    let max_context_tokens =
        vtcode_core::compaction::effective_context_budget(vt_cfg.as_ref(), provider_client.as_ref(), &effective_model);
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
    tool_registry.begin_tracker_request(
        crate::agent::runloop::unified::turn::context::tracker_continuation_adoption_allowed(&runtime.state.messages),
    );
    tool_registry.restore_completion_verification_from_history(&runtime.state.messages);
    resume::restore_session_follow_ups(
        &mut runtime,
        &tool_registry,
        resume::FollowUpRestoreContext {
            workspace_root: config.workspace.as_path(),
            vt_cfg: vt_cfg.as_ref(),
            resumed: resume_ref.is_some(),
            ctrl_c_state: &ctrl_c_state,
            renderer: &mut renderer,
        },
    )
    .await;
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
        terminal_enabled,
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
    let mut plan_session = PlanningWorkflowSessionState::default();
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
        apply_startup_plan_agent_selection(&mut active_primary_agent, &tool_registry, config, &handle).await;
        harness_try!(render_planning_workflow_next_step_hint(&mut renderer));
        // No researching indicator here: startup entry has no request yet.
    } else if planning_entry_source.requires_startup_prompt() && resume_ref.is_none() {
        let should_enter =
            harness_try!(prompt_startup_planning_workflow(&handle, &mut session, &ctrl_c_state, &ctrl_c_notify).await);
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
            apply_startup_plan_agent_selection(&mut active_primary_agent, &tool_registry, config, &handle).await;
            harness_try!(render_planning_workflow_next_step_hint(&mut renderer));
            // No researching indicator here: no request exists yet.
        }
    }
    let linked_directories: Vec<LinkedDirectory> = Vec::with_capacity(4);
    let model_picker_state: Option<ModelPickerState> = None;
    let palette_state: Option<ActivePalette> = None;
    let last_forced_redraw = Instant::now();
    let input_status_state = InputStatusState::default();
    let dismissed_memory_cleanup_fingerprint: Option<(usize, usize)> = None;
    let prefer_latest_queued_input_once = false;
    if resume_ref.is_some()
        && tool_registry.is_planning_active()
        && let Some(emitter) = harness_emitter.as_ref()
    {
        let saved_recovery = harness_try!(emitter.planning_recovery().await);
        let legacy_progress = saved_recovery.is_none()
            && runtime
                .state
                .messages
                .iter()
                .any(|message| matches!(message.role, MessageRole::Assistant | MessageRole::Tool));
        if saved_recovery.is_some() || legacy_progress {
            plan_session.recovery = saved_recovery.unwrap_or_else(|| {
                crate::agent::runloop::unified::planning_workflow::recovery_state::recover_legacy_session(
                    &runtime.state.messages,
                    provider_client.name(),
                    &config.model,
                )
            });
            harness_try!(
                emitter
                    .persist_checkpoint(
                        &crate::agent::runloop::unified::planning_workflow::recovery_state::checkpoint_event(
                            &plan_session.recovery,
                            thread_handle.thread_id().as_str(),
                            "planning-resume",
                        )
                    )
                    .await
            );
            if matches!(
                plan_session.recovery.outcome,
                vtcode_core::exec::events::planning::PlanningOutcome::Synthesizing
                    | vtcode_core::exec::events::planning::PlanningOutcome::Repairing
            ) || (plan_session.recovery.outcome
                == vtcode_core::exec::events::planning::PlanningOutcome::AwaitingInput
                && !plan_session.recovery.input_prompt_shown)
            {
                if let Err(error) = runtime.try_queue_synthetic_follow_up_input(
                    crate::agent::runloop::unified::turn::tool_outcomes::helpers::plan_mode_continue_follow_up(),
                ) {
                    plan_session.recovery.outcome = vtcode_core::exec::events::planning::PlanningOutcome::AwaitingInput;
                    renderer.line(
                        MessageStyle::Warning,
                        &format!("Planning could not resume automatically. Research and draft are saved. {error}"),
                    )?;
                }
            }
        }
    }

    let queued_inputs: VecDeque<QueuedInput> = VecDeque::with_capacity(8);
    let background_completion_receiver = tool_registry
        .subagent_controller()
        .map(|controller| controller.subscribe_parent_background_completions());
    let _matrix_interrupt_guard = if let Some(controller) = tool_registry.subagent_controller() {
        if let Some(persistence) = harness_emitter.as_ref().and_then(|emitter| emitter.matrix_persistence()) {
            harness_try!(controller.set_matrix_persistence(persistence).await);
        }
        let interrupt_state = Arc::clone(&ctrl_c_state);
        // Abort the interrupt poller when this scope ends.
        let interrupt_guard = TaskGuard::with_label(
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    if interrupt_state.is_cancel_requested() || interrupt_state.is_exit_requested() {
                        if let Err(error) = controller.cancel_matrix().await {
                            tracing::warn!(%error, "matrix interruption persistence failed");
                        }
                    }
                }
            }),
            "matrix-interrupt",
        );
        Some(interrupt_guard)
    } else {
        None
    };
    let exec_session_manager = tool_registry.exec_session_manager();
    let exec_completion_receiver = Some(exec_session_manager.subscribe_completion());
    let exec_completion_notify = Some(exec_session_manager.completion_notify());
    let pending_background_completions = PendingBackgroundCompletions::default();
    let pending_matrix_note: Option<String> = None;
    let displayed_matrix_revision: Option<(String, u64)> = None;
    let (webmcp_prompt_sender, webmcp_prompt_receiver) = crate::agent::runloop::unified::webmcp::prompt_channel();
    let webmcp_prompt_receiver = Some(webmcp_prompt_receiver);
    let webmcp_bridge = None;
    let agent_touched_paths = BTreeSet::new();
    let ctrl_c_notice_displayed = false;
    let inline_prompt_cost_notice_shown = false;
    let mcp_catalog_initialized = tool_registry.mcp_client().is_some();
    if mcp_catalog_initialized {
        // The catalogue is already wired before the interaction loop starts, so
        // the readiness line would be stale; suppress it without rendering.
        session_stats.mark_mcp_ready_notice_shown();
    }
    let last_known_mcp_tools: Vec<String> = Vec::with_capacity(16);
    let pending_mcp_refresh = false;
    let last_mcp_refresh = Instant::now();
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
    Ok(SetupFlow::Proceed(Box::new(Setup {
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
        renderer,
        session,
        handle,
        header_context,
        ctrl_c_state,
        ctrl_c_notify,
        settings_snapshot,
        input_activity_counter,
        checkpoint_manager,
        session_archive,
        lifecycle_hooks,
        context_manager,
        default_placeholder,
        follow_up_placeholder,
        next_checkpoint_turn,
        session_end_reason,
        _file_palette_task_guard,
        _background_subprocess_task_guard,
        _startup_update_task_guard,
        _editor_open_coordinator_task_guard,
        _settings_task_guard,
        editor_open_sender,
        editor_open_dispatcher,
        startup_update_notice_rx,
        settings_receiver,
        session_bootstrap,
        provider_client,
        tool_registry,
        tools,
        tool_catalog,
        async_mcp_manager,
        mcp_panel_state,
        loaded_skills,
        active_primary_agent,
        decision_ledger,
        traj,
        telemetry,
        error_recovery,
        runtime,
        tool_result_cache,
        tool_permission_cache,
        permissions_state,
        approval_recorder,
        safety_validator,
        circuit_breaker,
        tool_health_tracker,
        rate_limiter,
        autonomous_executor,
        cancel_token,
        _cancel_guard,
        _signal_handler,
        session_stats,
        plan_session,
        linked_directories,
        model_picker_state,
        palette_state,
        last_forced_redraw,
        input_status_state,
        dismissed_memory_cleanup_fingerprint,
        prefer_latest_queued_input_once,
        queued_inputs,
        background_completion_receiver,
        _matrix_interrupt_guard,
        exec_session_manager,
        exec_completion_receiver,
        exec_completion_notify,
        pending_background_completions,
        pending_matrix_note,
        displayed_matrix_revision,
        webmcp_prompt_sender,
        webmcp_prompt_receiver,
        webmcp_bridge,
        agent_touched_paths,
        ctrl_c_notice_displayed,
        inline_prompt_cost_notice_shown,
        mcp_catalog_initialized,
        last_known_mcp_tools,
        pending_mcp_refresh,
        last_mcp_refresh,
        startup_update_requested_restart,
    })))
}
