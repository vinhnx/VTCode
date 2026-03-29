mod archive;
mod execution_policy;
mod metrics;
mod plan_seed;

use super::*;
use crate::agent::runloop::git::{
    DirtyWorktreeStatus, compute_session_code_change_delta, git_dirty_worktree_entries,
    normalize_workspace_path, workspace_relative_display,
};
use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::unified::plan_mode_state::render_plan_mode_next_step_hint;
use crate::agent::runloop::unified::postamble::{ExitSummaryData, print_exit_summary};
use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
use crate::agent::runloop::unified::turn::turn_loop::TurnLoopOutcome;
use crate::agent::runloop::unified::turn::turn_loop::{
    POST_TOOL_TIMEOUT_RECOVERY_REASON, prepare_post_tool_tool_free_recovery,
};
use crate::agent::runloop::welcome::SessionBootstrap;
use crate::updater::{InlineUpdateOutcome, display_update_notice, run_inline_update_prompt};
use std::sync::Arc;
use vtcode_config::loader::SimpleConfigWatcher;
use vtcode_core::core::agent::features::FeatureSet;
use vtcode_core::core::agent::runtime::AgentRuntime;
use vtcode_core::core::agent::session::AgentSessionState;
use vtcode_core::core::interfaces::session::PlanModeEntrySource;
use vtcode_tui::app::{
    InlineHandle, InlineListItem, InlineListSelection, InlineSession, ListOverlayRequest,
    TransientRequest, TransientSubmission,
};

const PLAN_APPROVED_EXECUTION_DIRECTIVE: &str = "Plan was approved. Start implementation immediately: execute the plan step by step beginning with the first pending step. Do not ask for another implementation confirmation.";
const PLAN_APPROVED_EXECUTION_INPUT: &str = "Implement the approved plan now.";
const STARTUP_PLAN_MODE_ENTER_ACTION: &str = "plan_mode:start_enter";
const STARTUP_PLAN_MODE_STAY_ACTION: &str = "plan_mode:start_stay";
use archive::{
    create_session_archive, refresh_runtime_debug_context_for_next_session, workspace_archive_label,
};
use execution_policy::{
    build_partial_timeout_messages, effective_max_tool_calls_for_turn,
    resolve_effective_turn_timeout_secs, should_attempt_requesting_timeout_recovery,
};
use metrics::{
    TurnExecutionMetrics, capture_code_change_snapshot, emit_turn_execution_metrics,
    estimate_history_bytes,
};
use plan_seed::load_active_plan_seed;
use tokio::sync::{Notify, mpsc};

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

#[derive(Clone)]
struct PendingTimeoutRecovery {
    reason: String,
    mode: RecoveryMode,
}

fn remove_transient_system_notes(
    history: &mut Vec<vtcode_core::llm::provider::Message>,
    notes: &[String],
) {
    for note in notes.iter().rev() {
        if let Some(index) = history.iter().rposition(|message| {
            message.role == vtcode_core::llm::provider::MessageRole::System
                && message.content.as_text() == note.as_str()
        }) {
            let _ = history.remove(index);
        }
    }
}

fn build_tracked_file_freshness_note(
    workspace: &std::path::Path,
    stale_paths: &[std::path::PathBuf],
) -> Option<String> {
    if stale_paths.is_empty() {
        return None;
    }

    let display_paths = stale_paths
        .iter()
        .map(|path| format!("- {}", workspace_relative_display(workspace, path)))
        .collect::<Vec<_>>()
        .join("\n");

    Some(format!(
        "Freshness note: the following files changed on disk after VT Code last read them:\n{display_paths}\nRe-read these files before relying on earlier content because disk content is newer than the agent's prior read snapshot."
    ))
}

fn build_unrelated_dirty_worktree_note(
    workspace: &std::path::Path,
    agent_touched_paths: &std::collections::BTreeSet<std::path::PathBuf>,
) -> Result<Option<String>> {
    let Some(entries) = git_dirty_worktree_entries(workspace)? else {
        return Ok(None);
    };

    let display_paths = entries
        .into_iter()
        .filter(|entry| {
            entry.status == DirtyWorktreeStatus::Modified
                && !agent_touched_paths.contains(&entry.path)
        })
        .map(|entry| format!("- {}", workspace_relative_display(workspace, &entry.path)))
        .collect::<Vec<_>>();

    if display_paths.is_empty() {
        return Ok(None);
    }

    Ok(Some(format!(
        "Workspace note: the following files already have unrelated user modifications before this turn:\n{}\nTreat these files as user-owned changes. Do not edit, format, revert, or overwrite them unless the user explicitly asks to work on those files.",
        display_paths.join("\n")
    )))
}

fn latest_assistant_result_text(
    messages: &[vtcode_core::llm::provider::Message],
) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == vtcode_core::llm::provider::MessageRole::Assistant)
        .map(|message| message.content.as_text().trim().to_string())
        .filter(|text| !text.is_empty())
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

fn prepare_resume_bootstrap_without_archive(
    resume: &ResumeSession,
    mut metadata: vtcode_core::utils::session_archive::SessionArchiveMetadata,
    reserved_archive_id: Option<String>,
) -> (vtcode_core::core::threads::ThreadBootstrap, String) {
    let source_metadata = &resume.snapshot().metadata;
    let is_compatible = metadata.workspace_path == source_metadata.workspace_path
        && metadata.provider == source_metadata.provider
        && metadata.model == source_metadata.model;
    if is_compatible && let Some(lineage_id) = source_metadata.prompt_cache_lineage_id.as_ref() {
        metadata.prompt_cache_lineage_id = Some(lineage_id.clone());
    }
    if resume.is_fork() {
        metadata.parent_session_id = Some(resume.identifier());
        metadata.fork_mode = Some(if resume.summarize_fork() {
            vtcode_core::utils::session_archive::SessionForkMode::Summarized
        } else {
            vtcode_core::utils::session_archive::SessionForkMode::FullCopy
        });
    }

    let mut bootstrap = resume.bootstrap().clone();
    bootstrap.metadata = Some(metadata);
    if resume.is_fork() {
        bootstrap.archive_listing = None;
    }

    let thread_id = match resume.intent() {
        vtcode_core::core::threads::ArchivedSessionIntent::ResumeInPlace => resume.identifier(),
        vtcode_core::core::threads::ArchivedSessionIntent::ForkNewArchive { .. } => {
            reserved_archive_id.unwrap_or_else(|| {
                vtcode_core::utils::session_archive::generate_session_archive_identifier(
                    &workspace_archive_label(std::path::Path::new(
                        &resume.snapshot().metadata.workspace_path,
                    )),
                    resume.custom_suffix().map(str::to_owned),
                )
            })
        }
    };

    (bootstrap, thread_id)
}

async fn checkpoint_session_archive_start(
    archive: &vtcode_core::utils::session_archive::SessionArchive,
    thread_handle: &vtcode_core::core::threads::ThreadRuntimeHandle,
) -> Result<()> {
    let snapshot = thread_handle.snapshot();
    let recent_messages = snapshot.messages.iter().map(SessionMessage::from).collect();
    archive
        .persist_progress_async(SessionProgressArgs {
            total_messages: snapshot.messages.len(),
            distinct_tools: Vec::new(),
            recent_messages,
            turn_number: 1,
            token_usage: None,
            max_context_tokens: None,
            loaded_skills: Some(snapshot.loaded_skills),
        })
        .await?;
    Ok(())
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
    editing_mode: vtcode_tui::app::EditingMode,
    autonomous_mode: bool,
    full_auto: bool,
}

fn build_exit_header_context_fast(
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
    display: ExitHeaderDisplay,
) -> vtcode_tui::app::InlineHeaderContext {
    use vtcode_core::config::constants::ui;

    let trust_label = match session_bootstrap.acp_workspace_trust {
        Some(vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode::FullAuto) => {
            "full_auto"
        }
        Some(vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy) => {
            "tools_policy"
        }
        None if display.full_auto => "full auto",
        None => "tools policy",
    };

    vtcode_tui::app::InlineHeaderContext {
        app_name: vtcode_core::config::constants::app::DISPLAY_NAME.to_string(),
        provider: format!("{}{}", ui::HEADER_PROVIDER_PREFIX, display.provider_label),
        model: format!("{}{}", ui::HEADER_MODEL_PREFIX, config.model),
        context_window_size: Some(display.context_window_size),
        version: env!("CARGO_PKG_VERSION").to_string(),
        search_tools: Some(crate::agent::runloop::ui::build_search_tools_badge(
            &config.workspace,
        )),
        persistent_memory: None,
        pr_review: None,
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
        subagent_badges: Vec::new(),
        editing_mode: display.editing_mode,
        autonomous_mode: display.autonomous_mode,
    }
}

async fn prompt_startup_plan_mode(
    handle: &InlineHandle,
    session: &mut InlineSession,
    ctrl_c_state: &Arc<crate::agent::runloop::unified::state::CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<bool> {
    let overlay = TransientRequest::List(ListOverlayRequest {
        title: "Enter Plan Mode?".to_string(),
        lines: vec![
            "Your configuration sets default editing mode to Plan.".to_string(),
            "Plan Mode is read-only and blocks mutating tools.".to_string(),
        ],
        footer_hint: Some("You can toggle later with `/plan`.".to_string()),
        items: vec![
            InlineListItem {
                title: "Enter Plan Mode".to_string(),
                subtitle: Some("Switch to read-only planning.".to_string()),
                badge: Some("Recommended".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    STARTUP_PLAN_MODE_ENTER_ACTION.to_string(),
                )),
                search_value: None,
            },
            InlineListItem {
                title: "Stay in Edit Mode".to_string(),
                subtitle: Some("Continue in edit mode.".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    STARTUP_PLAN_MODE_STAY_ACTION.to_string(),
                )),
                search_value: None,
            },
        ],
        selected: Some(InlineListSelection::ConfigAction(
            STARTUP_PLAN_MODE_ENTER_ACTION.to_string(),
        )),
        search: None,
        hotkeys: Vec::new(),
    });

    let outcome = show_overlay_and_wait(
        handle,
        session,
        overlay,
        ctrl_c_state,
        ctrl_c_notify,
        |submission| match submission {
            TransientSubmission::Selection(InlineListSelection::ConfigAction(action))
                if action == STARTUP_PLAN_MODE_ENTER_ACTION =>
            {
                Some(true)
            }
            TransientSubmission::Selection(InlineListSelection::ConfigAction(action))
                if action == STARTUP_PLAN_MODE_STAY_ACTION =>
            {
                Some(false)
            }
            TransientSubmission::Selection(_) => Some(false),
            _ => None,
        },
    )
    .await?;

    Ok(matches!(outcome, OverlayWaitOutcome::Submitted(true)))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_single_agent_loop_unified_impl(
    config: &CoreAgentConfig,
    initial_vt_cfg: Option<VTCodeConfig>,
    skip_confirmations: bool,
    full_auto: bool,
    plan_mode_entry_source: PlanModeEntrySource,
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
        let active_thread_label = resume_ref.map_or("main", ResumeSession::thread_label);
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
        let history_enabled = vtcode_core::utils::session_archive::history_persistence_enabled();
        let summarized_fork_provider = if resume_ref.is_some_and(|resume| resume.summarize_fork()) {
            Some(
                crate::agent::runloop::unified::session_setup::create_provider_client(
                    &config,
                    vt_cfg.as_ref(),
                )?,
            )
        } else {
            None
        };
        let (thread_handle, session_archive) = if let Some(resume) = resume_ref {
            if history_enabled {
                let mut prepared = vtcode_core::core::threads::prepare_archived_session(
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
                .await?;
                if let Some(provider) = summarized_fork_provider.as_deref() {
                    prepared.bootstrap.messages =
                        crate::agent::runloop::unified::turn::compaction::build_summarized_fork_history(
                            provider,
                            &config.model,
                            &resume.identifier(),
                            &prepared.thread_id,
                            &config.workspace,
                            vt_cfg.as_ref(),
                            resume.history(),
                        )
                        .await?;
                }
                (
                    thread_manager.start_thread_with_identifier(
                        prepared.thread_id.clone(),
                        prepared.bootstrap,
                    ),
                    Some(prepared.archive),
                )
            } else {
                let (mut bootstrap, thread_id) = prepare_resume_bootstrap_without_archive(
                    resume,
                    archive_metadata.clone(),
                    reserved_archive_id.clone(),
                );
                if let Some(provider) = summarized_fork_provider.as_deref() {
                    bootstrap.messages =
                        crate::agent::runloop::unified::turn::compaction::build_summarized_fork_history(
                            provider,
                            &config.model,
                            &resume.identifier(),
                            &thread_id,
                            &config.workspace,
                            vt_cfg.as_ref(),
                            resume.history(),
                        )
                        .await?;
                }
                (
                    thread_manager.start_thread_with_identifier(thread_id, bootstrap),
                    None,
                )
            }
        } else {
            let thread_id = if let Some(identifier) = reserved_archive_id.clone() {
                identifier
            } else if history_enabled {
                vtcode_core::utils::session_archive::reserve_session_archive_identifier(
                    &workspace_archive_label(&config.workspace),
                    None,
                )
                .await?
            } else {
                vtcode_core::utils::session_archive::generate_session_archive_identifier(
                    &workspace_archive_label(&config.workspace),
                    None,
                )
            };
            let bootstrap =
                vtcode_core::core::threads::ThreadBootstrap::new(Some(archive_metadata.clone()));
            let archive = if history_enabled {
                Some(
                    create_session_archive(archive_metadata.clone(), Some(thread_id.clone()))
                        .await?,
                )
            } else {
                None
            };
            (
                thread_manager.start_thread_with_identifier(thread_id, bootstrap),
                archive,
            )
        };
        crate::main_helpers::set_runtime_archive_session_id(Some(
            thread_handle.thread_id().to_string(),
        ));
        if let Some(archive) = session_archive.as_ref()
            && let Err(err) = checkpoint_session_archive_start(archive, &thread_handle).await
        {
            tracing::warn!("Failed to checkpoint session archive at startup: {}", err);
        }
        let _session_trigger = if resume_ref.is_some() {
            SessionStartTrigger::Resume
        } else {
            SessionStartTrigger::Startup
        };
        let mut session_state = initialize_session(
            &config,
            vt_cfg.as_ref(),
            full_auto,
            resume_ref,
            thread_handle.thread_id().as_str(),
        )
        .await?;
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
            skip_confirmations,
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
        let input_activity_counter = ui_setup.input_activity_counter;
        let checkpoint_manager = ui_setup.checkpoint_manager;
        let mut session_archive = ui_setup.session_archive;
        let lifecycle_hooks = ui_setup.lifecycle_hooks;
        let mut context_manager = ui_setup.context_manager;
        let mut default_placeholder = ui_setup.default_placeholder;
        let mut follow_up_placeholder = ui_setup.follow_up_placeholder;
        let mut next_checkpoint_turn = ui_setup.next_checkpoint_turn;
        let mut session_end_reason = ui_setup.session_end_reason;
        let _file_palette_task_guard = ui_setup.file_palette_task_guard;
        let _background_subprocess_task_guard = ui_setup.background_subprocess_task_guard;
        let _startup_update_task_guard = ui_setup.startup_update_task_guard;
        let startup_update_cached_notice = ui_setup.startup_update_cached_notice;
        let mut startup_update_notice_rx = ui_setup.startup_update_notice_rx;
        let SessionState {
            session_bootstrap,
            mut provider_client,
            mut tool_registry,
            tools,
            tool_catalog,
            conversation_history,
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
        let max_tool_loops = vt_cfg
            .as_ref()
            .map(|cfg| cfg.tools.max_tool_loops)
            .filter(|limit| *limit > 0)
            .unwrap_or(vtcode_core::config::constants::defaults::DEFAULT_MAX_TOOL_LOOPS);
        let max_context_tokens = vt_cfg
            .as_ref()
            .map(|cfg| cfg.context.max_context_tokens)
            .unwrap_or_else(vtcode_config::context::default_max_context_tokens);
        let mut runtime = AgentRuntime::new(
            AgentSessionState::new(
                SessionId::new().0,
                config.max_conversation_turns,
                max_tool_loops,
                max_context_tokens,
            ),
            None,
            steering_receiver.take(),
        );
        runtime.state.messages = conversation_history;
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
        session_stats.set_prompt_cache_lineage_id(
            thread_handle
                .snapshot()
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.prompt_cache_lineage_id.clone()),
        );
        session_stats.vim_mode_enabled = vt_cfg.as_ref().is_some_and(|cfg| cfg.ui.vim_mode);
        if plan_mode_entry_source.should_auto_enter() {
            transition_to_plan_mode(
                &tool_registry,
                &mut session_stats,
                &handle,
                plan_mode_entry_source,
                true,
                true,
            )
            .await;
            render_plan_mode_next_step_hint(&mut renderer)?;
        } else if plan_mode_entry_source.requires_startup_prompt() && resume_ref.is_none() {
            let should_enter =
                prompt_startup_plan_mode(&handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
                    .await?;
            if should_enter {
                transition_to_plan_mode(
                    &tool_registry,
                    &mut session_stats,
                    &handle,
                    plan_mode_entry_source,
                    true,
                    true,
                )
                .await;
                render_plan_mode_next_step_hint(&mut renderer)?;
            }
        }
        if !session_stats.is_plan_mode() {
            session_stats.set_autonomous_mode(vt_cfg.as_ref().is_some_and(|cfg| {
                cfg.permissions.default_mode == vtcode_core::config::PermissionMode::Auto
            }));
        }
        header_context.autonomous_mode = session_stats.is_autonomous_mode();
        handle.set_autonomous_mode(session_stats.is_autonomous_mode());
        let mut linked_directories: Vec<LinkedDirectory> = Vec::with_capacity(4);
        let mut model_picker_state: Option<ModelPickerState> = None;
        let mut palette_state: Option<ActivePalette> = None;
        let mut last_forced_redraw = Instant::now();
        let mut input_status_state = InputStatusState::default();
        let mut dismissed_memory_cleanup_fingerprint: Option<(usize, usize)> = None;
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
        let mut agent_touched_paths = std::collections::BTreeSet::new();
        let mut ctrl_c_notice_displayed = false;
        let mut inline_prompt_cost_notice_shown = false;
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
                use crate::agent::runloop::unified::turn::session::interaction_loop::InteractionOutcome;

                if let Some(controller) = tool_registry.subagent_controller() {
                    controller
                        .set_parent_messages(&runtime.state.messages)
                        .await;
                }

                let interaction_outcome = if let Some(input) = runtime.run_until_idle() {
                    InteractionOutcome::Continue {
                        input,
                        prompt_message_index: None,
                    }
                } else {
                    let mut interaction_turn_metadata_cache = None;
                    let (session_state, runtime_steering) = runtime.split_mut();
                    let mut interaction_ctx = crate::agent::runloop::unified::turn::session::interaction_loop::InteractionLoopContext {
                    thread_id: &turn_run_id.0,
                    active_thread_label,
                    renderer: &mut renderer,
                    session: &mut session,
                    handle: &handle,
                    header_context: &mut header_context,
                    ide_context_bridge: &mut ide_context_bridge,
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
                    conversation_history: &mut session_state.messages,
                    agent_touched_paths: &mut agent_touched_paths,
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
                    runtime_steering,
                    startup_update_notice_rx: &mut startup_update_notice_rx,
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

                    crate::agent::runloop::unified::turn::session::interaction_loop::run_interaction_loop(
                    &mut interaction_ctx,
                    &mut interaction_state,
                ).await?
                };
                let (next_turn_input, completed_turn_prompt_message_index) =
                    match interaction_outcome {
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
                        InteractionOutcome::Continue {
                            input,
                            prompt_message_index,
                        } => (input, prompt_message_index),
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

                            let mut execution_directive =
                                PLAN_APPROVED_EXECUTION_DIRECTIVE.to_string();
                            if let Some(seed) = plan_seed {
                                execution_directive.push_str("\n\nApproved plan context:\n");
                                execution_directive.push_str(&seed);
                            }
                            runtime.state.messages.push(
                                vtcode_core::llm::provider::Message::system(execution_directive),
                            );
                            (PLAN_APPROVED_EXECUTION_INPUT.to_string(), None)
                        }
                    };
                if next_turn_input.trim().is_empty() {
                    continue;
                }
                let (session_state, runtime_steering) = runtime.split_mut();
                let mut working_history = std::mem::take(&mut session_state.messages);
                let mut transient_system_notes = Vec::with_capacity(2);
                if let Some(note) = {
                    let stale_paths = tool_registry.edited_file_monitor().stale_tracked_paths();
                    build_tracked_file_freshness_note(config.workspace.as_path(), &stale_paths)
                } {
                    transient_system_notes.push(note.clone());
                    working_history.push(vtcode_core::llm::provider::Message::system(note));
                }
                match build_unrelated_dirty_worktree_note(
                    config.workspace.as_path(),
                    &agent_touched_paths,
                ) {
                    Ok(Some(note)) => {
                        transient_system_notes.push(note.clone());
                        working_history.push(vtcode_core::llm::provider::Message::system(note));
                    }
                    Ok(None) => {}
                    Err(err) => {
                        tracing::warn!(
                            error = %err,
                            "Failed to inspect unrelated dirty worktree entries before turn"
                        );
                    }
                }
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
                let mut timeout_recovery_attempted = false;
                let mut pending_timeout_recovery: Option<PendingTimeoutRecovery> = None;
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
                    let applying_timeout_recovery = pending_timeout_recovery.is_some();
                    if let Some(recovery) = pending_timeout_recovery.take() {
                        harness_state.activate_recovery_with_mode(recovery.reason, recovery.mode);
                    }
                    harness_state.set_turn_timeout_secs(timeout_secs);
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
                        skip_confirmations,
                        full_auto,
                        runtime_steering,
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
                                let continuing_with_recovery =
                                    should_attempt_requesting_timeout_recovery(
                                        timed_out_phase,
                                        had_tool_activity,
                                        timeout_recovery_attempted,
                                    );
                                let (timeout_message, timeout_error_message) =
                                    build_partial_timeout_messages(
                                        timeout_secs,
                                        timed_out_phase,
                                        attempted_tool_calls,
                                        active_pty_sessions_before_cancel,
                                        continuing_with_recovery,
                                    );
                                renderer.line(MessageStyle::Error, &timeout_message)?;
                                if continuing_with_recovery {
                                    match crate::agent::runloop::unified::turn::compaction::compact_history_for_recovery_in_place(
                                        crate::agent::runloop::unified::turn::compaction::CompactionContext::new(
                                            provider_client.as_ref(),
                                            &config.model,
                                            &turn_run_id.0,
                                            &turn_run_id.0,
                                            config.workspace.as_path(),
                                            vt_cfg.as_ref(),
                                            lifecycle_hooks.as_ref(),
                                            harness_emitter.as_ref(),
                                        ),
                                        crate::agent::runloop::unified::turn::compaction::CompactionState::new(
                                            &mut working_history,
                                            &mut session_stats,
                                            &mut context_manager,
                                        ),
                                        turn_history_checkpoint.baseline_len,
                                    ).await {
                                        Ok(Some(outcome)) => {
                                            renderer.line(
                                                MessageStyle::Info,
                                                &format!(
                                                    "Compacted earlier history before the recovery pass ({} -> {} messages).",
                                                    outcome.original_len, outcome.compacted_len
                                                ),
                                            )?;
                                        }
                                        Ok(None) => {
                                            renderer.line(
                                                MessageStyle::Info,
                                                "No earlier history was compacted before the recovery pass.",
                                            )?;
                                        }
                                        Err(err) => {
                                            tracing::warn!(
                                                error = %err,
                                                "Failed to compact earlier history before timeout recovery"
                                            );
                                            renderer.line(
                                                MessageStyle::Info,
                                                "Recovery compaction failed; continuing with the existing history for one final tool-free pass.",
                                            )?;
                                        }
                                    }
                                    prepare_post_tool_tool_free_recovery(
                                        &mut working_history,
                                        POST_TOOL_TIMEOUT_RECOVERY_REASON,
                                    );
                                    timeout_recovery_attempted = true;
                                    pending_timeout_recovery = Some(PendingTimeoutRecovery {
                                        reason: POST_TOOL_TIMEOUT_RECOVERY_REASON.to_string(),
                                        mode: RecoveryMode::ToolFreeSynthesis,
                                    });
                                    continue;
                                }
                                break Err(anyhow::Error::msg(timeout_error_message));
                            }
                            if applying_timeout_recovery {
                                renderer.line(
                                    MessageStyle::Error,
                                    &format!(
                                        "Turn timed out after {} seconds during the compacted recovery pass. PTY sessions cancelled; stopping turn.",
                                        timeout_secs
                                    ),
                                )?;
                                break Err(anyhow::anyhow!(
                                    "Turn timed out after {} seconds during the compacted recovery pass",
                                    timeout_secs
                                ));
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
                remove_transient_system_notes(&mut working_history, &transient_system_notes);
                agent_touched_paths.extend(
                    outcome
                        .turn_modified_files
                        .iter()
                        .map(|path| normalize_workspace_path(config.workspace.as_path(), path)),
                );
                agent_touched_paths.extend(context_manager.tracked_instruction_activity_paths());
                runtime.state.messages = working_history;
                let outcome_result = outcome.result.clone();
                let turn_elapsed = turn_started_at.elapsed();
                let show_turn_timer = vt_cfg
                    .as_ref()
                    .map(|cfg| cfg.ui.show_turn_timer)
                    .unwrap_or(true);
                let harness_snapshot = tool_registry.harness_context_snapshot();
                if let Err(err) = crate::agent::runloop::unified::turn::apply_turn_outcome(
                    outcome,
                    crate::agent::runloop::unified::turn::TurnOutcomeContext {
                        conversation_history: &mut runtime.state.messages,
                        completed_turn_prompt: Some(next_turn_input.as_str()),
                        completed_turn_prompt_message_index,
                        renderer: &mut renderer,
                        handle: &handle,
                        ctrl_c_state: &ctrl_c_state,
                        default_placeholder: &default_placeholder,
                        checkpoint_manager: checkpoint_manager.as_ref(),
                        next_checkpoint_turn: &mut next_checkpoint_turn,
                        session_end_reason: &mut session_end_reason,
                        turn_elapsed,
                        show_turn_timer,
                        workspace: &config.workspace,
                        session_id: &harness_snapshot.session_id,
                        harness_emitter: harness_emitter.as_ref(),
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
                    let mut recent_messages: Vec<SessionMessage> = runtime
                        .state
                        .messages
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
                            total_messages: runtime.state.messages.len(),
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
                        if !renderer.supports_inline_ui()
                            && session_stats.auto_mode_prompt_fallback_active()
                            && session_stats.last_auto_mode_denial().is_some()
                        {
                            session_end_reason = SessionEndReason::Error;
                            break;
                        }
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
            let harness_snapshot = tool_registry.harness_context_snapshot();
            let (outcome_code, subtype) =
                session_end_reason.thread_completion_status(session_stats.budget_limit().is_some());
            let result = subtype
                .is_success()
                .then(|| latest_assistant_result_text(&runtime.state.messages))
                .flatten();
            let total_cost_usd = session_stats
                .total_cost_usd()
                .and_then(serde_json::Number::from_f64);
            let event =
                crate::agent::runloop::unified::inline_events::harness::thread_completed_event(
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
            if let Err(err) = emitter.emit(event) {
                tracing::debug!(error = %err, "harness thread.completed event emission failed");
            }
        }
        if let Some(emitter) = harness_emitter.as_ref() {
            emitter.finish_open_responses();
        }
        agent_touched_paths.extend(context_manager.tracked_instruction_activity_paths());
        if let Err(err) = vtcode_core::persistent_memory::finalize_persistent_memory(
            &config,
            vt_cfg.as_ref(),
            &runtime.state.messages,
        )
        .await
        {
            tracing::warn!(
                "Failed to update persistent memory at session finalization: {}",
                err
            );
        }

        let finalization_output = match finalize_session(
            &mut renderer,
            lifecycle_hooks.as_ref(),
            session_end_reason,
            &mut session_archive,
            &session_stats,
            &runtime.state.messages,
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
        let provider_label = {
            let label = crate::agent::runloop::unified::session_setup::resolve_provider_label(
                &config,
                vt_cfg.as_ref(),
            );
            if label.is_empty() {
                provider_name.clone()
            } else {
                label
            }
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
                    vtcode_tui::app::EditingMode::Plan
                } else {
                    vtcode_tui::app::EditingMode::Edit
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

#[cfg(test)]
mod tests {
    use super::{
        TurnHistoryCheckpoint, archive::NextRuntimeArchiveId,
        archive::next_runtime_archive_id_request, archive::workspace_archive_label,
        build_partial_timeout_messages, build_tracked_file_freshness_note,
        build_unrelated_dirty_worktree_note, checkpoint_session_archive_start,
        effective_max_tool_calls_for_turn, latest_assistant_result_text,
        prepare_resume_bootstrap_without_archive, remove_transient_system_notes,
        resolve_effective_turn_timeout_secs, should_attempt_requesting_timeout_recovery,
    };
    use crate::agent::agents::ResumeSession;
    use crate::agent::runloop::git::normalize_workspace_path;
    use crate::agent::runloop::unified::run_loop_context::TurnPhase;
    use chrono::Utc;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use tempfile::TempDir;
    use vtcode_core::core::threads::ArchivedSessionIntent;
    use vtcode_core::exec::events::ThreadCompletionSubtype;
    use vtcode_core::hooks::SessionEndReason;
    use vtcode_core::llm::provider::MessageRole;
    use vtcode_core::utils::session_archive::{
        SessionArchive, SessionArchiveMetadata, SessionListing, SessionMessage, SessionSnapshot,
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
    fn requesting_partial_timeout_recovery_message_mentions_continuation() {
        let (timeout_message, timeout_error_message) =
            build_partial_timeout_messages(660, TurnPhase::Requesting, 25, 0, true);
        assert!(timeout_message.contains("continuing with a compacted tool-free recovery pass"));
        assert!(
            timeout_error_message.contains("Continuing with a compacted tool-free recovery pass")
        );
    }

    #[test]
    fn requesting_partial_timeout_without_recovery_mentions_retry_skip() {
        let (timeout_message, timeout_error_message) =
            build_partial_timeout_messages(660, TurnPhase::Requesting, 25, 0, false);
        assert!(timeout_message.contains("retry is skipped"));
        assert!(!timeout_message.contains("continuing with a compacted tool-free recovery pass"));
        assert!(
            !timeout_error_message.contains("Continuing with a compacted tool-free recovery pass")
        );
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
            build_partial_timeout_messages(660, TurnPhase::Requesting, 0, 0, false);
        assert!(!timeout_message.contains("continuing with a compacted tool-free recovery pass"));
        assert!(
            !timeout_error_message.contains("Continuing with a compacted tool-free recovery pass")
        );
    }

    #[test]
    fn requesting_timeout_recovery_only_runs_once() {
        assert!(should_attempt_requesting_timeout_recovery(
            TurnPhase::Requesting,
            true,
            false
        ));
        assert!(!should_attempt_requesting_timeout_recovery(
            TurnPhase::Requesting,
            true,
            true
        ));
        assert!(!should_attempt_requesting_timeout_recovery(
            TurnPhase::ExecutingTools,
            true,
            false
        ));
    }

    #[test]
    fn transient_system_note_cleanup_removes_by_content_from_latest_match() {
        let note = "Freshness note: file changed".to_string();
        let older = vtcode_core::llm::provider::Message::system(note.clone());
        let transient = vtcode_core::llm::provider::Message::system(note.clone());
        let mut history = vec![
            older,
            vtcode_core::llm::provider::Message::assistant("summary".to_string()),
            transient,
            vtcode_core::llm::provider::Message::user("preserved".to_string()),
        ];

        remove_transient_system_notes(&mut history, std::slice::from_ref(&note));

        assert_eq!(history.len(), 3);
        assert_eq!(history[0].content.as_text(), note);
        assert_eq!(history[1].content.as_text(), "summary");
        assert_eq!(history[2].content.as_text(), "preserved");
    }

    #[test]
    fn workspace_archive_label_uses_directory_name() {
        assert_eq!(workspace_archive_label(Path::new("/tmp/demo")), "demo");
    }

    #[test]
    fn tracked_file_freshness_note_uses_relative_paths_and_reread_guidance() {
        let note = build_tracked_file_freshness_note(
            Path::new("/tmp/workspace"),
            &[
                PathBuf::from("/tmp/workspace/src/main.rs"),
                PathBuf::from("/tmp/workspace/docs/project/TODO.md"),
            ],
        )
        .expect("freshness note");

        assert!(note.contains("Freshness note"));
        assert!(note.contains("- src/main.rs"));
        assert!(note.contains("- docs/project/TODO.md"));
        assert!(note.contains("Re-read these files before relying on earlier content"));
    }

    fn init_git_repo() -> TempDir {
        let temp = TempDir::new().expect("temp dir");
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(temp.path())
                .status()
                .expect("git command");
            assert!(status.success(), "git command failed: {args:?}");
        };

        run(&["init"]);
        run(&["config", "user.name", "VT Code"]);
        run(&["config", "user.email", "vtcode@example.com"]);
        temp
    }

    fn seed_dirty_repo() -> (TempDir, PathBuf) {
        let repo = init_git_repo();
        let path = repo.path().join("docs/project/TODO.md");
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, "before\n").expect("write");

        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(repo.path())
                .status()
                .expect("git command");
            assert!(status.success(), "git command failed: {args:?}");
        };

        run(&["add", "."]);
        run(&["commit", "-m", "test: seed repo"]);
        fs::write(&path, "after\n").expect("write");
        (repo, path)
    }

    #[test]
    fn unrelated_dirty_worktree_note_uses_relative_paths_and_user_owned_guidance() {
        let (repo, path) = seed_dirty_repo();

        let note = build_unrelated_dirty_worktree_note(repo.path(), &BTreeSet::new())
            .expect("note build")
            .expect("note");

        assert!(note.contains("Workspace note"));
        assert!(note.contains("docs/project/TODO.md"));
        assert!(note.contains("user-owned changes"));
        assert!(!note.contains(&path.display().to_string()));
    }

    #[test]
    fn unrelated_dirty_worktree_note_skips_agent_touched_files() {
        let (repo, path) = seed_dirty_repo();
        let mut touched_paths = BTreeSet::new();
        touched_paths.insert(normalize_workspace_path(repo.path(), &path));

        let note =
            build_unrelated_dirty_worktree_note(repo.path(), &touched_paths).expect("note build");

        assert!(note.is_none());
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
            summarize: false,
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

    #[test]
    fn resume_bootstrap_without_archive_reuses_identifier_for_in_place_resume() {
        let resume = resume_session(ArchivedSessionIntent::ResumeInPlace);
        let (bootstrap, thread_id) = prepare_resume_bootstrap_without_archive(
            &resume,
            SessionArchiveMetadata::new(
                "workspace",
                "/tmp/workspace",
                "model",
                "provider",
                "theme",
                "medium",
            ),
            None,
        );

        assert_eq!(thread_id, "session-source");
        assert_eq!(
            bootstrap
                .metadata
                .as_ref()
                .map(|meta| meta.workspace_label.as_str()),
            Some("workspace")
        );
    }

    #[test]
    fn resume_bootstrap_without_archive_prefers_reserved_identifier_for_forks() {
        let resume = resume_session(ArchivedSessionIntent::ForkNewArchive {
            custom_suffix: Some("branch".to_string()),
            summarize: false,
        });
        let (_, thread_id) = prepare_resume_bootstrap_without_archive(
            &resume,
            SessionArchiveMetadata::new(
                "workspace",
                "/tmp/workspace",
                "model",
                "provider",
                "theme",
                "medium",
            ),
            Some("reserved-session-id".to_string()),
        );

        assert_eq!(thread_id, "reserved-session-id");
    }

    #[test]
    fn resume_bootstrap_without_archive_preserves_compatible_prompt_cache_lineage() {
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
                )
                .with_prompt_cache_lineage_id("lineage-123"),
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
        let resume = ResumeSession::from_listing(&listing, ArchivedSessionIntent::ResumeInPlace);
        let (bootstrap, _) = prepare_resume_bootstrap_without_archive(
            &resume,
            SessionArchiveMetadata::new(
                "workspace",
                "/tmp/workspace",
                "model",
                "provider",
                "other-theme",
                "high",
            ),
            None,
        );

        assert_eq!(
            bootstrap
                .metadata
                .as_ref()
                .and_then(|meta| meta.prompt_cache_lineage_id.as_deref()),
            Some("lineage-123")
        );
    }

    #[test]
    fn thread_completion_status_matches_public_contract() {
        assert_eq!(
            SessionEndReason::Completed.thread_completion_status(false),
            ("completed", ThreadCompletionSubtype::Success)
        );
        assert_eq!(
            SessionEndReason::NewSession.thread_completion_status(false),
            ("new_session", ThreadCompletionSubtype::Success)
        );
        assert_eq!(
            SessionEndReason::Exit.thread_completion_status(false),
            ("exit", ThreadCompletionSubtype::Cancelled)
        );
        assert_eq!(
            SessionEndReason::Cancelled.thread_completion_status(false),
            ("cancelled", ThreadCompletionSubtype::Cancelled)
        );
        assert_eq!(
            SessionEndReason::Error.thread_completion_status(false),
            ("error", ThreadCompletionSubtype::ErrorDuringExecution)
        );
        assert_eq!(
            SessionEndReason::Completed.thread_completion_status(true),
            (
                "budget_limit_reached",
                ThreadCompletionSubtype::ErrorMaxBudgetUsd
            )
        );
    }

    #[test]
    fn latest_assistant_result_text_uses_latest_nonempty_assistant_message() {
        let messages = vec![
            vtcode_core::llm::provider::Message::user("hello".to_string()),
            vtcode_core::llm::provider::Message::assistant(" first ".to_string()),
            vtcode_core::llm::provider::Message::tool_response(
                "call-1".to_string(),
                "{\"ok\":true}".to_string(),
            ),
            vtcode_core::llm::provider::Message::assistant(" final answer ".to_string()),
        ];

        assert_eq!(
            latest_assistant_result_text(&messages),
            Some("final answer".to_string())
        );
    }

    #[tokio::test]
    async fn checkpoint_session_archive_start_writes_initial_snapshot() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let archive_path = temp_dir.path().join("session-vtcode-test-start.json");
        let metadata = SessionArchiveMetadata::new(
            "workspace",
            "/tmp/workspace",
            "model",
            "provider",
            "theme",
            "medium",
        );
        let archive = SessionArchive::resume_from_listing(
            &SessionListing {
                path: archive_path.clone(),
                snapshot: SessionSnapshot {
                    metadata: metadata.clone(),
                    started_at: Utc::now(),
                    ended_at: Utc::now(),
                    total_messages: 0,
                    distinct_tools: Vec::new(),
                    transcript: Vec::new(),
                    messages: Vec::new(),
                    progress: None,
                    error_logs: Vec::new(),
                },
            },
            metadata.clone(),
        );
        let thread_manager = vtcode_core::core::threads::ThreadManager::new();
        let thread_handle = thread_manager.start_thread_with_identifier(
            "session-vtcode-test-start",
            vtcode_core::core::threads::ThreadBootstrap::new(Some(metadata)).with_messages(vec![
                vtcode_core::llm::provider::Message::user("hello".to_string()),
            ]),
        );

        checkpoint_session_archive_start(&archive, &thread_handle)
            .await
            .expect("startup checkpoint");

        let snapshot: SessionSnapshot =
            serde_json::from_str(&std::fs::read_to_string(archive_path).expect("read archive"))
                .expect("parse archive");
        assert_eq!(snapshot.total_messages, 1);
        assert_eq!(snapshot.messages.len(), 1);
        assert!(snapshot.progress.is_some());
    }
}
