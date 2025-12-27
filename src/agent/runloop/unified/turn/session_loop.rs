use anyhow::Result;
use ratatui::crossterm::terminal::disable_raw_mode;
use std::collections::VecDeque;
use std::io::Write;

use std::time::Instant;

use vtcode_core::config::constants::defaults;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::session_archive::{SessionMessage, SessionProgressArgs};

use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::model_picker::ModelPickerState;

use super::context::TurnLoopResult as RunLoopTurnLoopResult;
use super::finalization::finalize_session;
use super::turn_loop::TurnLoopOutcome;

use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::session_setup::{
    SessionState, initialize_session, initialize_session_ui, spawn_signal_handler,
};
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::workspace_links::LinkedDirectory;
use crate::hooks::lifecycle::{SessionEndReason, SessionStartTrigger};

const RECENT_MESSAGE_LIMIT: usize = 16;

#[allow(dead_code)]
enum TurnLoopResult {
    Completed,
    Aborted,
    Cancelled,
    Blocked { reason: Option<String> },
}

#[allow(dead_code)]
const SELF_REVIEW_MIN_LENGTH: usize = 240;

pub(crate) async fn run_single_agent_loop_unified(
    config: &CoreAgentConfig,
    mut vt_cfg: Option<VTCodeConfig>,
    skip_confirmations: bool,
    full_auto: bool,
    resume: Option<ResumeSession>,
) -> Result<()> {
    // Create a guard that ensures terminal is restored even on early return
    // This is important because the TUI task may not shutdown cleanly
    let _terminal_cleanup_guard = TerminalCleanupGuard::new();

    // Note: The global panic hook in vtcode-core handles terminal restoration on panic
    let mut config = config.clone();
    let mut resume_state = resume;

    loop {
        // Take the pending resume request (if any) for this session iteration.
        // New resume requests issued mid-session will populate `resume_state` again.
        let resume_request = resume_state.take();
        let resume_ref = resume_request.as_ref();

        let _session_trigger = if resume_ref.is_some() {
            SessionStartTrigger::Resume
        } else {
            SessionStartTrigger::Startup
        };

        let mut session_state =
            initialize_session(&config, vt_cfg.as_ref(), full_auto, resume_ref).await?;

        let ui_setup = initialize_session_ui(
            &config,
            vt_cfg.as_ref(),
            &mut session_state,
            resume_ref,
            full_auto,
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

        let SessionState {
            session_bootstrap,
            mut provider_client,
            mut tool_registry,
            tools,
            cached_tools,
            mut conversation_history,
            decision_ledger,
            pruning_ledger,
            trajectory: traj,
            async_mcp_manager,
            mut mcp_panel_state,
            tool_result_cache,
            tool_permission_cache,
            approval_recorder,
            trim_config,
            loaded_skills,
            custom_prompts,
            safety_validator,
            ..
        } = session_state;

        let _signal_handler = spawn_signal_handler(
            ctrl_c_state.clone(),
            ctrl_c_notify.clone(),
            async_mcp_manager.clone(),
        );

        let mut session_stats = SessionStats::default();
        let mut linked_directories: Vec<LinkedDirectory> = Vec::new();
        let mut model_picker_state: Option<ModelPickerState> = None;
        let mut palette_state: Option<ActivePalette> = None;
        let mut last_forced_redraw = Instant::now();
        let mut input_status_state = InputStatusState::default();
        let mut queued_inputs: VecDeque<String> = VecDeque::new();
        let mut ctrl_c_notice_displayed = false;
        let mut mcp_catalog_initialized = tool_registry.mcp_client().is_some();
        let mut last_known_mcp_tools: Vec<String> = Vec::new();
        let mut last_mcp_refresh = std::time::Instant::now();

        loop {
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
                conversation_history: &mut conversation_history,
                decision_ledger: &decision_ledger,
                pruning_ledger: &pruning_ledger,
                context_manager: &mut context_manager,
                session_stats: &mut session_stats,
                mcp_panel_state: &mut mcp_panel_state,
                linked_directories: &mut linked_directories,
                lifecycle_hooks: lifecycle_hooks.as_ref(),
                full_auto,
                approval_recorder: &approval_recorder,
                tool_permission_cache: &tool_permission_cache,
                loaded_skills: &loaded_skills,
                custom_prompts: &custom_prompts,
                trim_config: &trim_config,
                default_placeholder: &mut default_placeholder,
                follow_up_placeholder: &mut follow_up_placeholder,
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

            let interaction_outcome = crate::agent::runloop::unified::turn::session::interaction_loop::run_interaction_loop(
                &mut interaction_ctx,
                &mut interaction_state,
            ).await?;

            use crate::agent::runloop::unified::turn::session::interaction_loop::InteractionOutcome;

            let input = match interaction_outcome {
                InteractionOutcome::Exit { reason } => {
                    session_end_reason = reason;
                    break;
                }
                InteractionOutcome::Resume { resume_session } => {
                    resume_state = Some(resume_session);
                    session_end_reason = SessionEndReason::Completed; // Will be ignored by loop restart logic but sets state
                    break; // Restart loop
                }
                InteractionOutcome::Continue { input } => input,
            };
            // Removed: Tool response pruning
            // Removed: Context window enforcement to respect token limits

            let working_history = conversation_history.clone();
            let _max_tool_loops = vt_cfg
                .as_ref()
                .map(|cfg| cfg.tools.max_tool_loops)
                .filter(|&value| value > 0)
                .unwrap_or(defaults::DEFAULT_MAX_TOOL_LOOPS);

            // Unused turn-level locals removed after refactor
            let _tool_repeat_limit = vt_cfg
                .as_ref()
                .map(|cfg| cfg.tools.max_repeated_tool_calls)
                .filter(|&value| value > 0)
                .unwrap_or(defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS);
            // repeated tool attempts now managed in the turn loop; omitted here

            // Initialize loop detection
            let _loop_detection_enabled = vt_cfg
                .as_ref()
                .map(|cfg| !cfg.model.skip_loop_detection)
                .unwrap_or(true);
            let _loop_detection_threshold = vt_cfg
                .as_ref()
                .map(|cfg| cfg.model.loop_detection_threshold)
                .unwrap_or(3);
            let _loop_detection_interactive = vt_cfg
                .as_ref()
                .map(|cfg| cfg.model.loop_detection_interactive)
                .unwrap_or(true);
            // loop detection instance not used in the session loop path
            let mut _loop_detection_disabled_for_session = false;

            // New unified turn loop: use TurnLoopContext and run_turn_loop
            let turn_loop_ctx = crate::agent::runloop::unified::turn::TurnLoopContext {
                renderer: &mut renderer,
                handle: &handle,
                session: &mut session,
                session_stats: &mut session_stats,
                mcp_panel_state: &mut mcp_panel_state,
                tool_result_cache: &tool_result_cache,
                approval_recorder: &approval_recorder,
                decision_ledger: &decision_ledger,
                pruning_ledger: &pruning_ledger,
                tool_registry: &mut tool_registry,
                tools: &tools,
                cached_tools: &cached_tools,
                ctrl_c_state: &ctrl_c_state,
                ctrl_c_notify: &ctrl_c_notify,
                context_manager: &mut context_manager,
                last_forced_redraw: &mut last_forced_redraw,
                input_status_state: &mut input_status_state,
                lifecycle_hooks: lifecycle_hooks.as_ref(),
                default_placeholder: &default_placeholder,
                tool_permission_cache: &tool_permission_cache,
                safety_validator: &safety_validator,
            };
            let outcome = match crate::agent::runloop::unified::turn::run_turn_loop(
                &input,
                working_history.clone(),
                turn_loop_ctx,
                &config,
                vt_cfg.as_ref(),
                &mut provider_client,
                &traj,
                skip_confirmations,
                full_auto,
                &mut session_end_reason,
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(err) => {
                    // Handle errors gracefully - display to user but continue the session
                    tracing::error!("Turn execution error: {}", err);
                    // Clear the spinner from input status area
                    handle.set_input_status(None, None);
                    // Clear any pending output before showing error
                    let _ = renderer.line_if_not_empty(MessageStyle::Output);
                    // Display error without panicking even if renderer fails
                    let _ = renderer.line(MessageStyle::Error, &format!("Error: {}", err));
                    TurnLoopOutcome {
                        result: RunLoopTurnLoopResult::Aborted,
                        working_history,
                        turn_modified_files: std::collections::BTreeSet::new(),
                    }
                }
            };
            // Apply canonical side-effects for the turn outcome (history, checkpoints, session end reason)
            // Apply canonical side-effects for the turn outcome (history, checkpoints, session end reason)
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

                if let Err(err) = archive.persist_progress(SessionProgressArgs {
                    total_messages: conversation_history.len(),
                    distinct_tools: distinct_tools.clone(),
                    recent_messages,
                    turn_number: progress_turn,
                    token_usage: None,
                    max_context_tokens: Some(trim_config.max_tokens),
                    loaded_skills: Some(skill_names),
                }) {
                    tracing::warn!("Failed to persist session progress: {}", err);
                }
            }
            let _turn_result = outcome.result;

            // Check for session exit and continue to next iteration otherwise.
            if matches!(session_end_reason, SessionEndReason::Exit) {
                break;
            }

            continue;
        }

        // Capture loaded skills before finalizing session
        if let Some(archive) = session_archive.as_mut() {
            let skill_names: Vec<String> = loaded_skills.read().await.keys().cloned().collect();
            archive.set_loaded_skills(skill_names);
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
            Some(&pruning_ledger),
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

        // If the session ended with NewSession, restart the loop with fresh config
        // If a new resume request was queued (e.g., via /sessions), start it now.
        if resume_state.is_some() {
            continue;
        }

        if matches!(session_end_reason, SessionEndReason::NewSession) {
            // Reload config to pick up any changes
            vt_cfg =
                vtcode_core::config::loader::ConfigManager::load_from_workspace(&config.workspace)
                    .ok()
                    .map(|manager| manager.config().clone());
            resume_state = None;
            continue;
        }

        break;
    }

    Ok(())
}

/// Guard that ensures terminal is restored to a clean state when dropped
/// This handles cases where the TUI doesn't shutdown cleanly or the session
/// exits early (e.g., due to Ctrl+C or other signals)
struct TerminalCleanupGuard;

impl TerminalCleanupGuard {
    fn new() -> Self {
        Self
    }
}

impl Drop for TerminalCleanupGuard {
    fn drop(&mut self) {
        // Minimal terminal cleanup as last resort
        // The TUI's run_inline_tui should handle full cleanup, this is just a safety net
        // We deliberately avoid sending escape sequences to prevent conflicts with TUI cleanup

        // Attempt to disable raw mode if still enabled
        let _ = disable_raw_mode();

        // Ensure stdout is flushed
        let mut stdout = std::io::stdout();
        let _ = stdout.flush();

        // Wait for terminal to finish processing any pending operations
        // This prevents incomplete writes from corrupting the terminal
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
