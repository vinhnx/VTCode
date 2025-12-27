use crate::agent::runloop::unified::turn::session::slash_commands::{
    SlashCommandContext, SlashCommandControl,
};
use anyhow::Result;
use ratatui::crossterm::terminal::disable_raw_mode;
use std::collections::VecDeque;
use std::io::Write;
use std::sync::Arc;
use std::time::Instant;

use crate::agent::runloop::unified::turn::session::slash_commands;
use vtcode_core::llm::provider::{self as uni};

use tracing::warn;
use vtcode_core::config::constants::defaults;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::at_pattern::parse_at_patterns;
use vtcode_core::utils::session_archive::{SessionMessage, SessionProgressArgs};

use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::model_picker::{ModelPickerProgress, ModelPickerState};
use crate::agent::runloop::prompt::refine_and_enrich_prompt;
use crate::agent::runloop::slash_commands::handle_slash_command;
use crate::agent::runloop::unified::mcp_tool_manager::McpToolManager;

use super::context::TurnLoopResult as RunLoopTurnLoopResult;
use super::finalization::finalize_session;
use super::turn_loop::TurnLoopOutcome;
use super::utils::render_hook_messages;
use super::workspace::refresh_vt_config;
use crate::agent::runloop::unified::async_mcp_manager::McpInitStatus;

use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::inline_events::{
    InlineEventLoopResources, InlineInterruptCoordinator, InlineLoopAction, poll_inline_loop_action,
};
// loop_detection not used in session loop refactor
use crate::agent::runloop::unified::model_selection::finalize_model_selection;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::session_setup::{
    SessionState, build_mcp_tool_definitions, initialize_session, initialize_session_ui,
    spawn_signal_handler,
};
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::status_line::{
    InputStatusState, update_context_efficiency, update_input_status_if_changed,
};
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
            token_budget,
            token_counter,
            tool_result_cache,
            tool_permission_cache,
            approval_recorder,
            trim_config,
            loaded_skills,
            custom_prompts,
            token_budget_enabled,
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
        const MCP_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

        loop {
            if let Err(error) = update_input_status_if_changed(
                &handle,
                &config.workspace,
                &config.model,
                config.reasoning_effort.as_str(),
                vt_cfg.as_ref().map(|cfg| &cfg.ui.status_line),
                &mut input_status_state,
            )
            .await
            {
                warn!(
                    workspace = %config.workspace.display(),
                    error = ?error,
                    "Failed to refresh status line"
                );
            }

            // Update context efficiency metrics in status line
            if let Some(efficiency) = context_manager.last_efficiency() {
                update_context_efficiency(
                    &mut input_status_state,
                    efficiency.context_utilization_percent,
                    efficiency.total_tokens,
                    efficiency.semantic_value_per_token,
                );
            }

            if ctrl_c_state.is_exit_requested() {
                session_end_reason = SessionEndReason::Exit;
                break;
            }

            let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());

            if let Some(mcp_manager) = &async_mcp_manager {
                // Handle initial MCP client setup
                if !mcp_catalog_initialized {
                    match mcp_manager.get_status().await {
                        McpInitStatus::Ready { client } => {
                            tool_registry.set_mcp_client(Arc::clone(&client));
                            match tool_registry.refresh_mcp_tools().await {
                                Ok(()) => {
                                    let mut registered_tools = 0usize;
                                    match tool_registry.list_mcp_tools().await {
                                        Ok(mcp_tools) => {
                                            let new_definitions =
                                                build_mcp_tool_definitions(&mcp_tools);
                                            registered_tools = new_definitions.len();
                                            let _updated_snapshot = {
                                                let mut guard = tools.write().await;
                                                guard.retain(|tool| {
                                                    !tool
                                                        .function
                                                        .as_ref()
                                                        .unwrap()
                                                        .name
                                                        .starts_with("mcp_")
                                                });
                                                guard.extend(new_definitions);
                                                guard.clone()
                                            };

                                            // Enumerate MCP tools after initial setup (silently)
                                            McpToolManager::enumerate_mcp_tools_after_initial_setup(
                                                &mut tool_registry,
                                                &tools,
                                                mcp_tools,
                                                &mut last_known_mcp_tools,
                                            ).await?;
                                        }
                                        Err(err) => {
                                            warn!(
                                                "Failed to enumerate MCP tools after refresh: {err}"
                                            );
                                        }
                                    }

                                    renderer.line(
                                            MessageStyle::Info,
                                            &format!(
                                                "MCP tools ready ({} registered). Use /mcp tools to inspect the catalog.",
                                                registered_tools
                                            ),
                                        )?;
                                    renderer.line_if_not_empty(MessageStyle::Output)?;
                                }
                                Err(err) => {
                                    warn!(
                                        "Failed to refresh MCP tools after initialization: {err}"
                                    );
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Failed to index MCP tools: {}", err),
                                    )?;
                                    renderer.line_if_not_empty(MessageStyle::Output)?;
                                }
                            }
                            mcp_catalog_initialized = true;
                        }
                        McpInitStatus::Error { message } => {
                            renderer
                                .line(MessageStyle::Error, &format!("MCP Error: {}", message))?;
                            renderer.line_if_not_empty(MessageStyle::Output)?;
                            mcp_catalog_initialized = true;
                        }
                        McpInitStatus::Initializing { .. } | McpInitStatus::Disabled => {}
                    }
                }

                // Dynamic MCP tool refresh - check for new/updated tools after initialization
                if mcp_catalog_initialized && last_mcp_refresh.elapsed() >= MCP_REFRESH_INTERVAL {
                    last_mcp_refresh = std::time::Instant::now();

                    if let Ok(known_tools) = tool_registry.list_mcp_tools().await {
                        let current_tool_keys: Vec<String> = known_tools
                            .iter()
                            .map(|t| format!("{}-{}", t.provider, t.name))
                            .collect();

                        // Check if there are new or changed tools
                        if current_tool_keys != last_known_mcp_tools {
                            match tool_registry.refresh_mcp_tools().await {
                                Ok(()) => {
                                    match tool_registry.list_mcp_tools().await {
                                        Ok(new_mcp_tools) => {
                                            let new_definitions =
                                                build_mcp_tool_definitions(&new_mcp_tools);
                                            let _updated_snapshot = {
                                                let mut guard = tools.write().await;
                                                guard.retain(|tool| {
                                                    !tool
                                                        .function
                                                        .as_ref()
                                                        .unwrap()
                                                        .name
                                                        .starts_with("mcp_")
                                                });
                                                guard.extend(new_definitions);
                                                guard.clone()
                                            };

                                            // Enumerate MCP tools after refresh (silently)
                                            McpToolManager::enumerate_mcp_tools_after_refresh(
                                                &mut tool_registry,
                                                &tools,
                                                &mut last_known_mcp_tools,
                                            )
                                            .await?;
                                        }
                                        Err(err) => {
                                            warn!(
                                                "Failed to enumerate MCP tools after refresh: {err}"
                                            );
                                        }
                                    }
                                }
                                Err(err) => {
                                    warn!(
                                        "Failed to refresh MCP tools during dynamic update: {err}"
                                    );
                                }
                            }
                        }
                    }
                }
            }

            let resources = InlineEventLoopResources {
                renderer: &mut renderer,
                handle: &handle,
                interrupts,
                ctrl_c_notice_displayed: &mut ctrl_c_notice_displayed,
                default_placeholder: &default_placeholder,
                queued_inputs: &mut queued_inputs,
                model_picker_state: &mut model_picker_state,
                palette_state: &mut palette_state,
                config: &mut config,
                vt_cfg: &mut vt_cfg,
                provider_client: &mut provider_client,
                session_bootstrap: &session_bootstrap,
                full_auto,
            };

            let mut input_owned =
                match poll_inline_loop_action(&mut session, &ctrl_c_notify, resources).await? {
                    InlineLoopAction::Continue => continue,
                    InlineLoopAction::Submit(text) => text,
                    InlineLoopAction::Exit(reason) => {
                        session_end_reason = reason;
                        break;
                    }
                    InlineLoopAction::ResumeSession(session_id) => {
                        // Load and resume the selected session
                        use vtcode_core::llm::provider::Message;
                        use vtcode_core::utils::session_archive::find_session_by_identifier;

                        renderer.line(
                            MessageStyle::Info,
                            &format!("Loading session: {}", session_id),
                        )?;

                        match find_session_by_identifier(&session_id).await {
                            Ok(Some(listing)) => {
                                // Prefer full archived messages; fall back to progress snapshot.
                                let history_iter = if !listing.snapshot.messages.is_empty() {
                                    listing.snapshot.messages.iter()
                                } else if let Some(progress) = &listing.snapshot.progress {
                                    progress.recent_messages.iter()
                                } else {
                                    [].iter()
                                };
                                let history = history_iter.map(Message::from).collect();

                                #[allow(unused_assignments)]
                                {
                                    resume_state = Some(ResumeSession {
                                        identifier: listing.identifier(),
                                        snapshot: listing.snapshot.clone(),
                                        history,
                                        path: listing.path.clone(),
                                        is_fork: false,
                                    });
                                }

                                renderer.line(
                                    MessageStyle::Info,
                                    &format!("Restarting with session: {}", session_id),
                                )?;
                                session_end_reason = SessionEndReason::Completed;
                                break; // Exit current loop to restart with resumed session
                            }
                            Ok(None) => {
                                renderer.line(
                                    MessageStyle::Error,
                                    &format!("Session not found: {}", session_id),
                                )?;
                                continue;
                            }
                            Err(err) => {
                                renderer.line(
                                    MessageStyle::Error,
                                    &format!("Failed to load session: {}", err),
                                )?;
                                continue;
                            }
                        }
                    }
                };

            if input_owned.is_empty() {
                continue;
            }

            if let Err(err) = refresh_vt_config(&config.workspace, &config, &mut vt_cfg).await {
                warn!(
                    workspace = %config.workspace.display(),
                    error = ?err,
                    "Failed to refresh workspace configuration"
                );
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to reload configuration: {err}"),
                )?;
            }

            // Check for MCP status changes and report errors
            if let Some(mcp_manager) = &async_mcp_manager {
                let mcp_status = mcp_manager.get_status().await;
                if mcp_status.is_error()
                    && let Some(error_msg) = mcp_status.get_error_message()
                {
                    renderer.line(MessageStyle::Error, &format!("MCP Error: {}", error_msg))?;
                    renderer.line(
                        MessageStyle::Info,
                        "Use /mcp to check status or update your vtcode.toml configuration.",
                    )?;
                }
            }

            if let Some(next_placeholder) = follow_up_placeholder.take() {
                handle.set_placeholder(Some(next_placeholder.clone()));
                default_placeholder = Some(next_placeholder);
            }

            match input_owned.as_str() {
                "" => continue,
                "exit" | "quit" => {
                    renderer.line(MessageStyle::Info, "✓")?;
                    session_end_reason = SessionEndReason::Exit;
                    break;
                }
                "help" => {
                    renderer.line(MessageStyle::Info, "Commands: exit, help")?;
                    continue;
                }
                input if input.starts_with('/') => {
                    // Handle slash commands
                    if let Some(command_input) = input.strip_prefix('/') {
                        let outcome =
                            handle_slash_command(command_input, &mut renderer, &custom_prompts)
                                .await?;
                        let command_result = slash_commands::handle_outcome(
                            outcome,
                            SlashCommandContext {
                                renderer: &mut renderer,
                                handle: &handle,
                                session: &mut session,
                                config: &mut config,
                                vt_cfg: &mut vt_cfg,
                                provider_client: &mut provider_client,
                                session_bootstrap: &session_bootstrap,
                                model_picker_state: &mut model_picker_state,
                                palette_state: &mut palette_state,
                                tool_registry: &mut tool_registry,
                                conversation_history: &mut conversation_history,
                                decision_ledger: &decision_ledger,
                                pruning_ledger: &pruning_ledger,
                                context_manager: &mut context_manager,
                                session_stats: &mut session_stats,
                                tools: &tools,
                                token_budget_enabled,
                                trim_config: &trim_config,
                                async_mcp_manager: async_mcp_manager.as_ref(),
                                mcp_panel_state: &mut mcp_panel_state,
                                linked_directories: &mut linked_directories,
                                ctrl_c_state: &ctrl_c_state,
                                ctrl_c_notify: &ctrl_c_notify,
                                default_placeholder: &default_placeholder,
                                lifecycle_hooks: lifecycle_hooks.as_ref(),
                                full_auto,
                                approval_recorder: Some(&approval_recorder),
                                tool_permission_cache: &tool_permission_cache,
                                loaded_skills: &loaded_skills,
                            },
                        )
                        .await?;
                        match command_result {
                            SlashCommandControl::SubmitPrompt(prompt) => {
                                input_owned = prompt;
                            }
                            SlashCommandControl::Continue => continue,
                            SlashCommandControl::BreakWithReason(reason) => {
                                session_end_reason = reason;
                                break;
                            }
                            SlashCommandControl::BreakWithoutReason => break,
                        }
                    }
                }
                _ => {}
            }

            if let Some(hooks) = &lifecycle_hooks {
                match hooks.run_user_prompt_submit(input_owned.as_str()).await {
                    Ok(outcome) => {
                        render_hook_messages(&mut renderer, &outcome.messages)?;
                        if !outcome.allow_prompt {
                            handle.clear_input();
                            continue;
                        }
                        for context in outcome.additional_context {
                            if !context.trim().is_empty() {
                                conversation_history.push(uni::Message::system(context));
                            }
                        }
                    }
                    Err(err) => {
                        renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to run prompt hooks: {}", err),
                        )?;
                    }
                }
            }

            if let Some(picker) = model_picker_state.as_mut() {
                let progress = picker.handle_input(&mut renderer, input_owned.as_str())?;
                match progress {
                    ModelPickerProgress::InProgress => continue,
                    ModelPickerProgress::NeedsRefresh => {
                        picker.refresh_dynamic_models(&mut renderer).await?;
                        continue;
                    }
                    ModelPickerProgress::Cancelled => {
                        model_picker_state = None;
                        continue;
                    }
                    ModelPickerProgress::Completed(selection) => {
                        let picker_state = model_picker_state.take().unwrap();
                        if let Err(err) = finalize_model_selection(
                            &mut renderer,
                            &picker_state,
                            selection,
                            &mut config,
                            &mut vt_cfg,
                            &mut provider_client,
                            &session_bootstrap,
                            &handle,
                            full_auto,
                        )
                        .await
                        {
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Failed to apply model selection: {}", err),
                            )?;
                        }
                        continue;
                    }
                }
            }

            let input = input_owned.as_str();

            // Check for explicit "run <command>" pattern BEFORE processing
            // This bypasses LLM interpretation and executes the command directly
            if let Some((tool_name, tool_args)) =
                crate::agent::runloop::unified::shell::detect_explicit_run_command(input)
            {
                // Display the user message
                display_user_message(&mut renderer, input)?;

                // Add user message to history
                conversation_history.push(uni::Message::user(input.to_string()));

                // Execute the tool directly via tool registry
                let tool_call_id = format!("explicit_run_{}", conversation_history.len());
                match tool_registry.execute_tool_ref(&tool_name, &tool_args).await {
                    Ok(result) => {
                        // Render the command output using the standard tool output renderer
                        crate::agent::runloop::tool_output::render_tool_output(
                            &mut renderer,
                            Some(&tool_name),
                            &result,
                            vt_cfg.as_ref(),
                            None,
                        )
                        .await?;

                        // Add tool response to history
                        let result_str = serde_json::to_string(&result).unwrap_or_default();
                        conversation_history.push(uni::Message::tool_response(
                            tool_call_id.clone(),
                            result_str,
                        ));
                    }
                    Err(err) => {
                        renderer.line(MessageStyle::Error, &format!("Command failed: {}", err))?;
                        conversation_history.push(uni::Message::tool_response(
                            tool_call_id.clone(),
                            format!("{{\"error\": \"{}\"}}", err),
                        ));
                    }
                }

                // Clear input and continue to next iteration
                handle.clear_input();
                handle.set_placeholder(default_placeholder.clone());
                continue;
            }

            // Process @ patterns to embed images as base64 content
            let processed_content = match parse_at_patterns(input, &config.workspace).await {
                Ok(content) => content,
                Err(e) => {
                    // Log the error but continue with original input as text
                    tracing::warn!("Failed to parse @ patterns: {}", e);
                    uni::MessageContent::text(input.to_string())
                }
            };

            // Apply prompt refinement and vibe coding enrichment if enabled
            let refined_content = match &processed_content {
                uni::MessageContent::Text(text) => {
                    let refined_text =
                        refine_and_enrich_prompt(text, &config, vt_cfg.as_ref()).await;
                    uni::MessageContent::text(refined_text)
                }
                uni::MessageContent::Parts(parts) => {
                    let mut refined_parts = Vec::new();
                    for part in parts {
                        match part {
                            uni::ContentPart::Text { text } => {
                                let refined_text =
                                    refine_and_enrich_prompt(text, &config, vt_cfg.as_ref()).await;
                                refined_parts.push(uni::ContentPart::text(refined_text));
                            }
                            _ => refined_parts.push(part.clone()),
                        }
                    }
                    uni::MessageContent::parts(refined_parts)
                }
            };

            // Display the user message with inline border decoration
            match &refined_content {
                uni::MessageContent::Text(text) => display_user_message(&mut renderer, text)?,
                uni::MessageContent::Parts(parts) => {
                    // For multi-part content, display the text parts concatenated
                    let mut display_parts = Vec::new();
                    for part in parts {
                        if let uni::ContentPart::Text { text } = part {
                            display_parts.push(text.as_str());
                        }
                    }
                    let display_text = display_parts.join(" ");
                    display_user_message(&mut renderer, &display_text)?;
                }
            }

            // Spinner is displayed via the input status in the inline handle
            // No need to show a separate message here

            // Create user message with processed content using the appropriate constructor
            let user_message = match refined_content {
                uni::MessageContent::Text(text) => uni::Message::user(text),
                uni::MessageContent::Parts(parts) => uni::Message::user_with_parts(parts),
            };

            conversation_history.push(user_message);
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
                token_budget: &token_budget,
                token_counter: &token_counter,
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
                input,
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
                let budget_usage = token_budget.get_stats().await;

                let skill_names: Vec<String> = loaded_skills.read().await.keys().cloned().collect();

                if let Err(err) = archive.persist_progress(SessionProgressArgs {
                    total_messages: conversation_history.len(),
                    distinct_tools: distinct_tools.clone(),
                    recent_messages,
                    turn_number: progress_turn,
                    token_usage: Some(budget_usage),
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

            // If we are in full-auto mode and the plan is still in progress,
            // we should auto-continue by pushing "continue" to queued_inputs.
            if full_auto {
                let plan = tool_registry.current_plan();
                if plan.summary.status == vtcode_core::tools::PlanCompletionState::InProgress {
                    queued_inputs.push_back("continue".to_string());
                }
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
