use anyhow::Result;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;

use vtcode_core::agent_teams::TeamRole;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::find_session_by_identifier;

use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::model_picker::{ModelPickerProgress, ModelPickerState};
use crate::agent::runloop::prompt::refine_and_enrich_prompt;
use vtcode_core::core::agent::steering::SteeringMessage;

use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::inline_events::{
    InlineEventLoopResources, InlineInterruptCoordinator, InlineLoopAction, TeamSwitchDirection,
    poll_inline_loop_action,
};

use crate::agent::runloop::unified::model_selection::{
    finalize_model_selection, finalize_subagent_model_selection, finalize_team_model_selection,
};
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::state::{CtrlCState, ModelPickerTarget, SessionStats};
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::welcome::SessionBootstrap;

use crate::agent::runloop::unified::turn::session::{
    mcp_lifecycle, slash_command_handler, tool_dispatch,
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
    pub tool_catalog: &'a Arc<ToolCatalogState>,
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
    pub tool_result_cache: &'a Arc<tokio::sync::RwLock<vtcode_core::tools::ToolResultCache>>,
    pub traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
    pub harness_emitter:
        Option<&'a crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
    pub safety_validator: &'a Arc<
        tokio::sync::RwLock<
            crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator,
        >,
    >,
    pub circuit_breaker: &'a Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
    pub tool_health_tracker: &'a Arc<vtcode_core::tools::health::ToolHealthTracker>,
    pub rate_limiter: &'a Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
    pub telemetry: &'a Arc<vtcode_core::core::telemetry::TelemetryManager>,
    pub autonomous_executor: &'a Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
    pub error_recovery:
        &'a Arc<std::sync::RwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>>,
    pub last_forced_redraw: &'a mut std::time::Instant,
    pub harness_config: vtcode_config::core::agent::AgentHarnessConfig,
    pub steering_receiver: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
}

impl<'a> InteractionLoopContext<'a> {
    pub fn as_turn_processing_context<'b>(
        &'b mut self,
        harness_state: &'b mut crate::agent::runloop::unified::run_loop_context::HarnessTurnState,
        auto_exit_plan_mode_attempted: &'b mut bool,
        input_status_state: &'b mut crate::agent::runloop::unified::status_line::InputStatusState,
    ) -> crate::agent::runloop::unified::turn::context::TurnProcessingContext<'b> {
        crate::agent::runloop::unified::turn::context::TurnProcessingContext {
            renderer: self.renderer,
            handle: self.handle,
            session_stats: self.session_stats,
            auto_exit_plan_mode_attempted,
            mcp_panel_state: self.mcp_panel_state,
            tool_result_cache: self.tool_result_cache,
            approval_recorder: self.approval_recorder,
            decision_ledger: self.decision_ledger,
            working_history: self.conversation_history,
            tool_registry: self.tool_registry,
            tools: self.tools,
            tool_catalog: self.tool_catalog,
            ctrl_c_state: self.ctrl_c_state,
            ctrl_c_notify: self.ctrl_c_notify,
            vt_cfg: self.vt_cfg.as_ref(),
            context_manager: self.context_manager,
            last_forced_redraw: self.last_forced_redraw,
            input_status_state,
            session: self.session,
            lifecycle_hooks: self.lifecycle_hooks,
            default_placeholder: self.default_placeholder,
            tool_permission_cache: self.tool_permission_cache,
            safety_validator: self.safety_validator,
            provider_client: self.provider_client,
            config: self.config,
            traj: self.traj,
            full_auto: self.full_auto,
            circuit_breaker: self.circuit_breaker,
            tool_health_tracker: self.tool_health_tracker,
            rate_limiter: self.rate_limiter,
            telemetry: self.telemetry,
            autonomous_executor: self.autonomous_executor,
            error_recovery: self.error_recovery,
            harness_state,
            harness_emitter: self.harness_emitter,
            steering_receiver: self.steering_receiver,
        }
    }
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
    Continue {
        input: String,
    },
    Exit {
        reason: SessionEndReason,
    },
    Resume {
        resume_session: Box<ResumeSession>,
    },
    /// Plan approved by user (Claude Code style HITL) - transition from Plan to Edit mode
    PlanApproved {
        /// If true, auto-accept file edits without prompting
        auto_accept: bool,
    },
}

pub(crate) async fn run_interaction_loop(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<InteractionOutcome> {
    const MCP_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

    loop {
        // Update spooled files count for status line (dynamic context indicator)
        let spooled_count = ctx.tool_registry.spooled_files_count().await;
        crate::agent::runloop::unified::status_line::update_spooled_files_count(
            state.input_status_state,
            spooled_count,
        );
        crate::agent::runloop::unified::status_line::update_team_status(
            state.input_status_state,
            ctx.session_stats,
        );

        // Refresh status line
        if let Err(error) =
            crate::agent::runloop::unified::status_line::update_input_status_if_changed(
                ctx.handle,
                &ctx.config.workspace,
                &ctx.config.model,
                ctx.config.reasoning_effort.as_str(),
                ctx.vt_cfg.as_ref().map(|cfg| &cfg.ui.status_line),
                state.input_status_state,
            )
            .await
        {
            tracing::warn!("Failed to refresh status line: {}", error);
        }

        if let Err(error) = poll_team_mailbox(ctx).await {
            tracing::warn!("Failed to read team mailbox: {}", error);
        }

        // Context efficiency metrics tracking has been removed along with context trim functionality

        if ctx.ctrl_c_state.is_exit_requested() {
            return Ok(InteractionOutcome::Exit {
                reason: SessionEndReason::Exit,
            });
        }

        let interrupts = InlineInterruptCoordinator::new(ctx.ctrl_c_state.as_ref());

        // Handle MCP
        // Handle MCP
        if let Some(mcp_manager) = ctx.async_mcp_manager {
            mcp_lifecycle::handle_mcp_updates(
                mcp_manager,
                ctx.tool_registry,
                ctx.tools,
                ctx.tool_catalog,
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
            team_active: ctx.session_stats.team_context.is_some(),
        };

        let mut input_owned =
            match poll_inline_loop_action(ctx.session, ctx.ctrl_c_notify, resources).await? {
                InlineLoopAction::Continue => continue,
                InlineLoopAction::Submit(text) => text,
                InlineLoopAction::ToggleDelegateMode => {
                    let enabled = ctx.session_stats.toggle_delegate_mode();
                    ctx.renderer.line(
                        MessageStyle::Info,
                        if enabled {
                            "Delegate mode enabled (coordination only)."
                        } else {
                            "Delegate mode disabled."
                        },
                    )?;
                    continue;
                }
                InlineLoopAction::SwitchTeammate(direction) => {
                    handle_team_switch(ctx, direction).await?;
                    continue;
                }
                InlineLoopAction::Exit(reason) => {
                    return Ok(InteractionOutcome::Exit { reason });
                }
                InlineLoopAction::PlanApproved { auto_accept } => {
                    // User approved the plan - transition from Plan to Edit mode
                    ctx.renderer.line(
                        MessageStyle::Info,
                        if auto_accept {
                            "Plan approved with auto-accept. Starting execution..."
                        } else {
                            "Plan approved. Starting execution with manual approval..."
                        },
                    )?;
                    // The editing mode transition and auto-accept state should be
                    // handled by the caller based on this outcome
                    return Ok(InteractionOutcome::PlanApproved { auto_accept });
                }
                InlineLoopAction::PlanEditRequested => {
                    // User wants to return to plan editing
                    ctx.renderer.line(
                        MessageStyle::Info,
                        "Returning to plan mode. Continue refining your plan.",
                    )?;
                    continue;
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
                InlineLoopAction::DiffApproved | InlineLoopAction::DiffRejected => {
                    continue;
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

        if let Some(cfg) = ctx.vt_cfg.as_ref()
            && let Err(err) =
                crate::agent::runloop::unified::turn::workspace::apply_workspace_config_to_registry(
                    ctx.tool_registry,
                    cfg,
                )
        {
            tracing::warn!("Failed to apply workspace configuration to tools: {}", err);
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

        match slash_command_handler::handle_input_commands(input_owned.as_str(), ctx, state).await?
        {
            slash_command_handler::CommandProcessingResult::Outcome(outcome) => return Ok(outcome),
            slash_command_handler::CommandProcessingResult::ContinueLoop => continue,
            slash_command_handler::CommandProcessingResult::UpdateInput(new_input) => {
                input_owned = new_input;
            }
            slash_command_handler::CommandProcessingResult::NotHandled => {}
        }

        if let Some(target) = direct_message_target(ctx.session_stats) {
            if !input_owned.trim_start().starts_with('/') {
                if let Some(team) = ctx.session_stats.team_state.as_mut() {
                    team.send_message(&target, "lead", input_owned.clone(), None)
                        .await?;
                    ctx.renderer
                        .line(MessageStyle::Info, &format!("Message sent to {}.", target))?;
                    continue;
                }
            }
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
                    let target = ctx.session_stats.model_picker_target;
                    ctx.session_stats.model_picker_target = ModelPickerTarget::Main;
                    match target {
                        ModelPickerTarget::Main => {
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
                        }
                        ModelPickerTarget::SubagentDefault => {
                            if let Err(err) = finalize_subagent_model_selection(
                                ctx.renderer,
                                selection,
                                ctx.vt_cfg,
                                &ctx.config.workspace,
                            )
                            .await
                            {
                                ctx.renderer.line(
                                    MessageStyle::Error,
                                    &format!("Failed to set subagent model: {}", err),
                                )?;
                            }
                        }
                        ModelPickerTarget::TeamDefault => {
                            if let Err(err) = finalize_team_model_selection(
                                ctx.renderer,
                                selection,
                                ctx.vt_cfg,
                                &ctx.config.workspace,
                            )
                            .await
                            {
                                ctx.renderer.line(
                                    MessageStyle::Error,
                                    &format!("Failed to set team model: {}", err),
                                )?;
                            }
                        }
                    }
                    continue;
                }
            }
        }

        let input = input_owned.as_str();

        // Check for direct tool execution (bash / run)
        {
            let mut direct_tool_ctx = tool_dispatch::DirectToolContext {
                interaction_ctx: ctx,
                input_status_state: state.input_status_state,
            };

            if let Some(outcome) =
                tool_dispatch::handle_direct_tool_execution(input, &mut direct_tool_ctx).await?
            {
                return Ok(outcome);
            }
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

        display_user_message(ctx.renderer, input)?;

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

fn direct_message_target(session_stats: &SessionStats) -> Option<String> {
    let context = session_stats.team_context.as_ref()?;
    if context.role != TeamRole::Lead {
        return None;
    }
    session_stats
        .team_state
        .as_ref()
        .and_then(|team| team.active_teammate())
        .map(|name| name.to_string())
}

async fn handle_team_switch(
    ctx: &mut InteractionLoopContext<'_>,
    direction: TeamSwitchDirection,
) -> Result<()> {
    let role = ctx.session_stats.team_context.as_ref().map(|ctx| ctx.role);
    if matches!(role, Some(TeamRole::Teammate)) {
        ctx.renderer.line(
            MessageStyle::Info,
            "Active teammate selection is only available to the lead.",
        )?;
        return Ok(());
    }

    let Some(team) = ctx.session_stats.team_state.as_mut() else {
        ctx.renderer
            .line(MessageStyle::Info, "No active team. Use /team start.")?;
        return Ok(());
    };

    let mut options = Vec::new();
    options.push(None);
    for name in team.teammate_names() {
        options.push(Some(name));
    }

    if options.len() <= 1 {
        ctx.renderer
            .line(MessageStyle::Info, "No teammates to select.")?;
        return Ok(());
    }

    let current = team.active_teammate().map(|name| name.to_string());
    let current_idx = options
        .iter()
        .position(|entry| entry.as_deref() == current.as_deref())
        .unwrap_or(0);

    let next_idx = match direction {
        TeamSwitchDirection::Next => (current_idx + 1) % options.len(),
        TeamSwitchDirection::Previous => {
            if current_idx == 0 {
                options.len() - 1
            } else {
                current_idx - 1
            }
        }
    };

    let next = options[next_idx].clone();
    team.set_active_teammate(next.clone()).await?;
    let label = next.as_deref().unwrap_or("lead");
    ctx.renderer
        .line(MessageStyle::Info, &format!("Active teammate: {}.", label))?;

    Ok(())
}

async fn poll_team_mailbox(ctx: &mut InteractionLoopContext<'_>) -> Result<()> {
    let team_context = match ctx.session_stats.team_context.as_ref() {
        Some(context) => context.clone(),
        None => return Ok(()),
    };

    if ctx.session_stats.team_state.is_none() {
        let storage =
            vtcode_core::agent_teams::TeamStorage::from_config(ctx.vt_cfg.as_ref()).await?;
        match crate::agent::runloop::unified::team_state::TeamState::load(
            storage,
            &team_context.team_name,
        )
        .await
        {
            Ok(mut team) => {
                // Restore persisted mailbox offset so we don't re-read old messages
                let r = match team_context.role {
                    TeamRole::Lead => "lead",
                    TeamRole::Teammate => {
                        team_context.teammate_name.as_deref().unwrap_or("teammate")
                    }
                };
                let _ = team.load_persisted_offset(r).await;
                ctx.session_stats.team_state = Some(team);
            }
            Err(err) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to load team '{}': {}", team_context.team_name, err),
                )?;
                ctx.session_stats.team_context = None;
                return Ok(());
            }
        }
    }

    let recipient = match team_context.role {
        TeamRole::Lead => "lead".to_string(),
        TeamRole::Teammate => team_context
            .teammate_name
            .clone()
            .unwrap_or_else(|| "teammate".to_string()),
    };

    let Some(team) = ctx.session_stats.team_state.as_mut() else {
        return Ok(());
    };

    // Reload tasks so we see changes from tmux teammates.
    team.reload_tasks().await?;

    let messages = team.read_mailbox(&recipient).await?;
    for message in &messages {
        if let Some(proto) = &message.protocol {
            let injected = handle_team_protocol(ctx, &message.sender, proto)?;
            if let Some(text) = injected {
                ctx.conversation_history.push(uni::Message::system(text));
            }
            continue;
        }

        let text = message.content.as_deref().unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }

        let mut header = format!("Team message from {}", message.sender);
        if let Some(task_id) = message.task_id {
            header.push_str(&format!(" (task #{})", task_id));
        }
        ctx.renderer.line(MessageStyle::Info, &header)?;
        ctx.renderer.line(MessageStyle::Output, text)?;

        let injected = if let Some(task_id) = message.task_id {
            format!(
                "[Team message from {} re task #{}]\n{}",
                message.sender, task_id, text
            )
        } else {
            format!("[Team message from {}]\n{}", message.sender, text)
        };
        ctx.conversation_history.push(uni::Message::user(injected));
    }

    Ok(())
}

/// Handle protocol messages, returning an optional string to inject into
/// conversation history so the model can act on lifecycle events.
fn handle_team_protocol(
    ctx: &mut InteractionLoopContext<'_>,
    sender: &str,
    proto: &vtcode_core::agent_teams::TeamProtocolMessage,
) -> Result<Option<String>> {
    use vtcode_core::agent_teams::TeamProtocolType;
    let inject = match proto.r#type {
        TeamProtocolType::IdleNotification => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Teammate '{}' is now idle.", sender),
            )?;
            Some(format!(
                "[vtcode:team_protocol] Teammate '{}' is idle and available for new tasks.",
                sender
            ))
        }
        TeamProtocolType::ShutdownRequest => {
            ctx.renderer.line(
                MessageStyle::Warning,
                &format!("Teammate '{}' requested shutdown.", sender),
            )?;
            Some(format!(
                "[vtcode:team_protocol] Teammate '{}' requested shutdown.",
                sender
            ))
        }
        TeamProtocolType::ShutdownApproved => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Shutdown approved for '{}'.", sender),
            )?;
            Some(format!(
                "[vtcode:team_protocol] Shutdown approved for '{}'.",
                sender
            ))
        }
        TeamProtocolType::TaskUpdate => {
            let detail = proto
                .details
                .as_ref()
                .map(|d| d.to_string())
                .unwrap_or_default();
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Task update from '{}': {}", sender, detail),
            )?;
            None // Task updates are visible via task reload; avoid context bloat.
        }
    };
    Ok(inject)
}
