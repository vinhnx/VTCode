use anyhow::Result;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::find_session_by_identifier;

use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::model_picker::{ModelPickerProgress, ModelPickerState};
use crate::agent::runloop::prompt::refine_and_enrich_prompt;
use crate::agent::runloop::slash_commands::handle_slash_command;
use crate::agent::runloop::tool_output::render_tool_output;
use crate::agent::runloop::unified::async_mcp_manager::{AsyncMcpManager, McpInitStatus};
use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::inline_events::{
    InlineEventLoopResources, InlineInterruptCoordinator, InlineLoopAction, poll_inline_loop_action,
};
use crate::agent::runloop::unified::mcp_tool_manager::McpToolManager;
use crate::agent::runloop::unified::model_selection::finalize_model_selection;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::welcome::SessionBootstrap;

use crate::agent::runloop::unified::turn::session::slash_commands;
use crate::agent::runloop::unified::turn::session::slash_commands::{
    SlashCommandContext, SlashCommandControl,
};
use crate::hooks::lifecycle::SessionEndReason;

#[allow(clippy::too_many_arguments)]
pub(crate) struct InteractionLoopContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub session: &'a mut vtcode_core::ui::tui::InlineSession,
    pub handle: &'a InlineHandle,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub config: &'a mut AgentConfig,
    pub vt_cfg: &'a mut Option<VTCodeConfig>,
    pub provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub session_bootstrap: &'a SessionBootstrap,
    pub async_mcp_manager: &'a Option<Arc<AsyncMcpManager>>,
    pub tool_registry: &'a mut vtcode_core::tools::registry::ToolRegistry,
    pub tools: &'a Arc<tokio::sync::RwLock<Vec<uni::ToolDefinition>>>,
    pub conversation_history: &'a mut Vec<uni::Message>,
    pub decision_ledger:
        &'a Arc<tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
    pub context_manager: &'a mut crate::agent::runloop::unified::context_manager::ContextManager,
    pub session_stats: &'a mut SessionStats,
    pub mcp_panel_state: &'a mut crate::agent::runloop::mcp_events::McpPanelState,
    pub linked_directories:
        &'a mut Vec<crate::agent::runloop::unified::workspace_links::LinkedDirectory>,
    pub lifecycle_hooks: Option<&'a crate::hooks::lifecycle::LifecycleHookEngine>,
    pub full_auto: bool,
    pub approval_recorder: &'a Arc<vtcode_core::tools::ApprovalRecorder>,
    pub tool_permission_cache: &'a Arc<tokio::sync::RwLock<vtcode_core::acp::ToolPermissionCache>>,
    pub loaded_skills:
        &'a Arc<tokio::sync::RwLock<std::collections::HashMap<String, vtcode_core::skills::Skill>>>,
    pub custom_prompts: &'a vtcode_core::prompts::CustomPromptRegistry,
    pub custom_slash_commands: &'a vtcode_core::prompts::CustomSlashCommandRegistry,
    pub default_placeholder: &'a mut Option<String>,
    pub follow_up_placeholder: &'a mut Option<String>,
    pub checkpoint_manager: Option<&'a vtcode_core::core::agent::snapshots::SnapshotManager>,
}

pub(crate) struct InteractionState<'a> {
    pub input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
    pub queued_inputs: &'a mut VecDeque<String>,
    pub model_picker_state: &'a mut Option<ModelPickerState>,
    pub palette_state: &'a mut Option<ActivePalette>,
    pub last_known_mcp_tools: &'a mut Vec<String>,
    pub mcp_catalog_initialized: &'a mut bool,
    pub last_mcp_refresh: &'a mut Instant,
    pub ctrl_c_notice_displayed: &'a mut bool,
}

pub(crate) enum InteractionOutcome {
    Continue { input: String },
    Exit { reason: SessionEndReason },
    Resume { resume_session: Box<ResumeSession> },
}

pub(crate) async fn run_interaction_loop(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<InteractionOutcome> {
    const MCP_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

    loop {
        // Refresh status line
        let vim_mode_enabled = ctx
            .vt_cfg
            .as_ref()
            .map(|cfg| cfg.agent.vim_mode_enabled)
            .unwrap_or(false);
        let vim_mode_normal = false; // No vim state in interaction loop

        if let Err(error) =
            crate::agent::runloop::unified::status_line::update_input_status_if_changed(
                ctx.handle,
                &ctx.config.workspace,
                &ctx.config.model,
                ctx.config.reasoning_effort.as_str(),
                ctx.vt_cfg.as_ref().map(|cfg| &cfg.ui.status_line),
                vim_mode_enabled,
                vim_mode_normal,
                state.input_status_state,
            )
            .await
        {
            tracing::warn!("Failed to refresh status line: {}", error);
        }

        // Context efficiency metrics tracking has been removed along with context trim functionality

        if ctx.ctrl_c_state.is_exit_requested() {
            return Ok(InteractionOutcome::Exit {
                reason: SessionEndReason::Exit,
            });
        }

        let interrupts = InlineInterruptCoordinator::new(ctx.ctrl_c_state.as_ref());

        // Handle MCP
        if let Some(mcp_manager) = ctx.async_mcp_manager {
            handle_mcp_updates(
                mcp_manager,
                ctx.tool_registry,
                ctx.tools,
                ctx.renderer,
                state.mcp_catalog_initialized,
                state.last_mcp_refresh,
                state.last_known_mcp_tools,
                MCP_REFRESH_INTERVAL,
            )
            .await?;
        }

        let resources = InlineEventLoopResources {
            renderer: ctx.renderer,
            handle: ctx.handle,
            interrupts,
            ctrl_c_notice_displayed: state.ctrl_c_notice_displayed,
            default_placeholder: ctx.default_placeholder,
            queued_inputs: state.queued_inputs,
            model_picker_state: state.model_picker_state,
            palette_state: state.palette_state,
            config: ctx.config,
            vt_cfg: ctx.vt_cfg,
            provider_client: ctx.provider_client,
            session_bootstrap: ctx.session_bootstrap,
            full_auto: ctx.full_auto,
        };

        let mut input_owned =
            match poll_inline_loop_action(ctx.session, ctx.ctrl_c_notify, resources).await? {
                InlineLoopAction::Continue => continue,
                InlineLoopAction::Submit(text) => text,
                InlineLoopAction::Exit(reason) => {
                    return Ok(InteractionOutcome::Exit { reason });
                }
                InlineLoopAction::ResumeSession(session_id) => {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        &format!("Loading session: {}", session_id),
                    )?;

                    match find_session_by_identifier(&session_id).await {
                        Ok(Some(listing)) => {
                            let history_iter = if !listing.snapshot.messages.is_empty() {
                                listing.snapshot.messages.iter()
                            } else if let Some(progress) = &listing.snapshot.progress {
                                progress.recent_messages.iter()
                            } else {
                                [].iter()
                            };
                            let history = history_iter.map(uni::Message::from).collect();

                            let resume = ResumeSession {
                                identifier: listing.identifier(),
                                snapshot: listing.snapshot.clone(),
                                history,
                                path: listing.path.clone(),
                                is_fork: false,
                            };

                            ctx.renderer.line(
                                MessageStyle::Info,
                                &format!("Restarting with session: {}", session_id),
                            )?;
                            return Ok(InteractionOutcome::Resume {
                                resume_session: Box::new(resume),
                            });
                        }
                        Ok(None) => {
                            ctx.renderer.line(
                                MessageStyle::Error,
                                &format!("Session not found: {}", session_id),
                            )?;
                            continue;
                        }
                        Err(err) => {
                            ctx.renderer.line(
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

        if let Err(err) = crate::agent::runloop::unified::turn::workspace::refresh_vt_config(
            &ctx.config.workspace,
            ctx.config,
            ctx.vt_cfg,
        )
        .await
        {
            tracing::warn!("Failed to refresh workspace configuration: {}", err);
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to reload configuration: {}", err),
            )?;
        }

        // Check for MCP errors
        if let Some(mcp_manager) = ctx.async_mcp_manager {
            let mcp_status = mcp_manager.get_status().await;
            if mcp_status.is_error()
                && let Some(error_msg) = mcp_status.get_error_message()
            {
                ctx.renderer
                    .line(MessageStyle::Error, &format!("MCP Error: {}", error_msg))?;
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Use /mcp to check status or update your vtcode.toml configuration.",
                )?;
            }
        }

        if let Some(next_placeholder) = ctx.follow_up_placeholder.take() {
            ctx.handle.set_placeholder(Some(next_placeholder.clone()));
            *ctx.default_placeholder = Some(next_placeholder);
        }

        match input_owned.as_str() {
            "" => continue,
            "exit" | "quit" => {
                ctx.renderer.line(MessageStyle::Info, "✓")?;
                return Ok(InteractionOutcome::Exit {
                    reason: SessionEndReason::Exit,
                });
            }
            "help" => {
                ctx.renderer
                    .line(MessageStyle::Info, "Commands: exit, help")?;
                continue;
            }
            input if input.starts_with('/') => {
                if let Some(command_input) = input.strip_prefix('/') {
                    let outcome = handle_slash_command(
                        command_input,
                        ctx.renderer,
                        ctx.custom_prompts,
                        Some(ctx.custom_slash_commands),
                    )
                    .await?;

                    let command_result = slash_commands::handle_outcome(
                        outcome,
                        SlashCommandContext {
                            renderer: ctx.renderer,
                            handle: ctx.handle,
                            session: ctx.session,
                            config: ctx.config,
                            vt_cfg: ctx.vt_cfg,
                            provider_client: ctx.provider_client,
                            session_bootstrap: ctx.session_bootstrap,
                            model_picker_state: state.model_picker_state,
                            palette_state: state.palette_state,
                            tool_registry: ctx.tool_registry,
                            conversation_history: ctx.conversation_history,
                            decision_ledger: ctx.decision_ledger,
                            context_manager: ctx.context_manager,
                            session_stats: ctx.session_stats,
                            tools: ctx.tools,
                            async_mcp_manager: ctx.async_mcp_manager.as_ref(),
                            mcp_panel_state: ctx.mcp_panel_state,
                            linked_directories: ctx.linked_directories,
                            ctrl_c_state: ctx.ctrl_c_state,
                            ctrl_c_notify: ctx.ctrl_c_notify,
                            default_placeholder: ctx.default_placeholder,
                            lifecycle_hooks: ctx.lifecycle_hooks,
                            full_auto: ctx.full_auto,
                            approval_recorder: Some(ctx.approval_recorder),
                            tool_permission_cache: ctx.tool_permission_cache,
                            loaded_skills: ctx.loaded_skills,
                            checkpoint_manager: ctx.checkpoint_manager,
                        },
                    )
                    .await?;

                    match command_result {
                        SlashCommandControl::SubmitPrompt(prompt) => {
                            input_owned = prompt;
                        }
                        SlashCommandControl::Continue => continue,
                        SlashCommandControl::BreakWithReason(reason) => {
                            return Ok(InteractionOutcome::Exit { reason });
                        }
                        SlashCommandControl::BreakWithoutReason => {
                            return Ok(InteractionOutcome::Exit {
                                reason: SessionEndReason::Exit,
                            });
                        }
                    }
                }
            }
            _ => {}
        }

        if let Some(hooks) = ctx.lifecycle_hooks {
            match hooks.run_user_prompt_submit(input_owned.as_str()).await {
                Ok(outcome) => {
                    crate::agent::runloop::unified::turn::utils::render_hook_messages(
                        ctx.renderer,
                        &outcome.messages,
                    )?;
                    if !outcome.allow_prompt {
                        ctx.handle.clear_input();
                        continue;
                    }
                    for context in outcome.additional_context {
                        if !context.trim().is_empty() {
                            ctx.conversation_history.push(uni::Message::system(context));
                        }
                    }
                }
                Err(err) => {
                    ctx.renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to run prompt hooks: {}", err),
                    )?;
                }
            }
        }

        if let Some(picker) = state.model_picker_state.as_mut() {
            let progress = picker.handle_input(ctx.renderer, input_owned.as_str())?;
            match progress {
                ModelPickerProgress::InProgress => continue,
                ModelPickerProgress::NeedsRefresh => {
                    picker.refresh_dynamic_models(ctx.renderer).await?;
                    continue;
                }
                ModelPickerProgress::Cancelled => {
                    *state.model_picker_state = None;
                    continue;
                }
                ModelPickerProgress::Completed(selection) => {
                    let picker_state = state.model_picker_state.take().unwrap();
                    if let Err(err) = finalize_model_selection(
                        ctx.renderer,
                        &picker_state,
                        selection,
                        ctx.config,
                        ctx.vt_cfg,
                        ctx.provider_client,
                        ctx.session_bootstrap,
                        ctx.handle,
                        ctx.full_auto,
                    )
                    .await
                    {
                        ctx.renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to apply model selection: {}", err),
                        )?;
                    }
                    continue;
                }
            }
        }

        let input = input_owned.as_str();

        // Check for bash mode with '!' prefix
        if input.starts_with('!') {
            let bash_command = input.trim_start_matches('!').trim();
            if !bash_command.is_empty() {
                display_user_message(ctx.renderer, input)?;
                ctx.conversation_history
                    .push(uni::Message::user(input.to_string()));

                // Execute bash command directly
                let tool_call_id = format!("bash_{}", ctx.conversation_history.len());

                // Find the bash tool in the registry
                let args = serde_json::json!({"command": bash_command});
                let bash_result = ctx.tool_registry.execute_tool_ref("bash", &args).await;

                match bash_result {
                    Ok(result) => {
                        render_tool_output(
                            ctx.renderer,
                            Some("bash"),
                            &result,
                            ctx.vt_cfg.as_ref(),
                        )
                        .await?;

                        let result_str = serde_json::to_string(&result).unwrap_or_default();
                        ctx.conversation_history.push(uni::Message::tool_response(
                            tool_call_id.clone(),
                            result_str,
                        ));
                    }
                    Err(err) => {
                        ctx.renderer.line(
                            MessageStyle::Error,
                            &format!("Bash command failed: {}", err),
                        )?;
                        ctx.conversation_history.push(uni::Message::tool_response(
                            tool_call_id.clone(),
                            format!("{{\"error\": \"{}\"}}", err),
                        ));
                    }
                }

                ctx.handle.clear_input();
                ctx.handle.set_placeholder(ctx.default_placeholder.clone());
                continue;
            }
        }

        // Check for explicit "run <command>" pattern
        if let Some((tool_name, tool_args)) =
            crate::agent::runloop::unified::shell::detect_explicit_run_command(input)
        {
            display_user_message(ctx.renderer, input)?;
            ctx.conversation_history
                .push(uni::Message::user(input.to_string()));

            let tool_call_id = format!("explicit_run_{}", ctx.conversation_history.len());
            match ctx
                .tool_registry
                .execute_tool_ref(&tool_name, &tool_args)
                .await
            {
                Ok(result) => {
                    render_tool_output(
                        ctx.renderer,
                        Some(&tool_name),
                        &result,
                        ctx.vt_cfg.as_ref(),
                    )
                    .await?;

                    let result_str = serde_json::to_string(&result).unwrap_or_default();
                    ctx.conversation_history.push(uni::Message::tool_response(
                        tool_call_id.clone(),
                        result_str,
                    ));
                }
                Err(err) => {
                    ctx.renderer
                        .line(MessageStyle::Error, &format!("Command failed: {}", err))?;
                    ctx.conversation_history.push(uni::Message::tool_response(
                        tool_call_id.clone(),
                        format!("{{\"error\": \"{}\"}}", err),
                    ));
                }
            }

            ctx.handle.clear_input();
            ctx.handle.set_placeholder(ctx.default_placeholder.clone());
            continue;
        }

        let processed_content =
            match vtcode_core::utils::at_pattern::parse_at_patterns(input, &ctx.config.workspace)
                .await
            {
                Ok(content) => content,
                Err(e) => {
                    tracing::warn!("Failed to parse @ patterns: {}", e);
                    uni::MessageContent::text(input.to_string())
                }
            };

        let refined_content = match &processed_content {
            uni::MessageContent::Text(text) => {
                let refined_text =
                    refine_and_enrich_prompt(text, ctx.config, ctx.vt_cfg.as_ref()).await;
                uni::MessageContent::text(refined_text)
            }
            uni::MessageContent::Parts(parts) => {
                let mut refined_parts = Vec::new();
                for part in parts {
                    match part {
                        uni::ContentPart::Text { text } => {
                            let refined_text =
                                refine_and_enrich_prompt(text, ctx.config, ctx.vt_cfg.as_ref())
                                    .await;
                            refined_parts.push(uni::ContentPart::text(refined_text));
                        }
                        _ => refined_parts.push(part.clone()),
                    }
                }
                uni::MessageContent::parts(refined_parts)
            }
        };

        match &refined_content {
            uni::MessageContent::Text(text) => display_user_message(ctx.renderer, text)?,
            uni::MessageContent::Parts(parts) => {
                let mut display_parts = Vec::new();
                for part in parts {
                    if let uni::ContentPart::Text { text } = part {
                        display_parts.push(text.as_str());
                    }
                }
                let display_text = display_parts.join(" ");
                display_user_message(ctx.renderer, &display_text)?;
            }
        }

        let user_message = match refined_content {
            uni::MessageContent::Text(text) => uni::Message::user(text),
            uni::MessageContent::Parts(parts) => uni::Message::user_with_parts(parts),
        };

        ctx.conversation_history.push(user_message);

        // Input processed successfully, return Continue outcome for session loop to invoke run_turn_loop
        return Ok(InteractionOutcome::Continue {
            input: input.to_string(),
        });
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_mcp_updates(
    mcp_manager: &AsyncMcpManager,
    tool_registry: &mut vtcode_core::tools::registry::ToolRegistry,
    tools: &Arc<tokio::sync::RwLock<Vec<uni::ToolDefinition>>>,
    renderer: &mut AnsiRenderer,
    mcp_catalog_initialized: &mut bool,
    last_mcp_refresh: &mut Instant,
    last_known_mcp_tools: &mut Vec<String>,
    refresh_interval: std::time::Duration,
) -> Result<()> {
    if !*mcp_catalog_initialized {
        match mcp_manager.get_status().await {
            McpInitStatus::Ready { client } => {
                tool_registry.set_mcp_client(Arc::clone(&client));
                match tool_registry.refresh_mcp_tools().await {
                    Ok(()) => {
                        let mut registered_tools = 0usize;
                        match tool_registry.list_mcp_tools().await {
                            Ok(mcp_tools) => {
                                let new_definitions =
                                    crate::agent::runloop::unified::session_setup::build_mcp_tool_definitions(
                                        &mcp_tools,
                                    );
                                registered_tools = new_definitions.len();
                                let _updated_snapshot = {
                                    let mut guard = tools.write().await;
                                    guard.retain(|tool| {
                                        !tool.function.as_ref().unwrap().name.starts_with("mcp_")
                                    });
                                    guard.extend(new_definitions);
                                    guard.clone()
                                };
                                McpToolManager::enumerate_mcp_tools_after_initial_setup(
                                    tool_registry,
                                    tools,
                                    mcp_tools,
                                    last_known_mcp_tools,
                                )
                                .await?;
                            }
                            Err(err) => {
                                tracing::warn!(
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
                        tracing::warn!("Failed to refresh MCP tools after initialization: {err}");
                        renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to index MCP tools: {}", err),
                        )?;
                        renderer.line_if_not_empty(MessageStyle::Output)?;
                    }
                }
                *mcp_catalog_initialized = true;
            }
            McpInitStatus::Error { message } => {
                renderer.line(MessageStyle::Error, &format!("MCP Error: {}", message))?;
                renderer.line_if_not_empty(MessageStyle::Output)?;
                *mcp_catalog_initialized = true;
            }
            McpInitStatus::Initializing { .. } | McpInitStatus::Disabled => {}
        }
    }

    if *mcp_catalog_initialized && last_mcp_refresh.elapsed() >= refresh_interval {
        *last_mcp_refresh = std::time::Instant::now();

        if let Ok(known_tools) = tool_registry.list_mcp_tools().await {
            let current_tool_keys: Vec<String> = known_tools
                .iter()
                .map(|t| format!("{}-{}", t.provider, t.name))
                .collect();

            if current_tool_keys != *last_known_mcp_tools {
                match tool_registry.refresh_mcp_tools().await {
                    Ok(()) => match tool_registry.list_mcp_tools().await {
                        Ok(new_mcp_tools) => {
                            let new_definitions =
                                crate::agent::runloop::unified::session_setup::build_mcp_tool_definitions(
                                    &new_mcp_tools,
                                );
                            let _updated_snapshot = {
                                let mut guard = tools.write().await;
                                guard.retain(|tool| {
                                    !tool.function.as_ref().unwrap().name.starts_with("mcp_")
                                });
                                guard.extend(new_definitions);
                                guard.clone()
                            };
                            McpToolManager::enumerate_mcp_tools_after_refresh(
                                tool_registry,
                                tools,
                                last_known_mcp_tools,
                            )
                            .await?;
                        }
                        Err(err) => {
                            tracing::warn!("Failed to enumerate MCP tools after refresh: {err}");
                        }
                    },
                    Err(err) => {
                        tracing::warn!("Failed to refresh MCP tools during dynamic update: {err}");
                    }
                }
            }
        }
    }

    Ok(())
}
