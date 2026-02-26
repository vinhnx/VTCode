use anyhow::Result;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::session_archive::find_session_by_identifier;

use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::model_picker::ModelPickerProgress;
use crate::agent::runloop::prompt::refine_and_enrich_prompt;
use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::inline_events::{
    InlineEventLoopResources, InlineInterruptCoordinator, InlineLoopAction, poll_inline_loop_action,
};
use crate::agent::runloop::unified::model_selection::{
    finalize_model_selection, finalize_subagent_model_selection, finalize_team_model_selection,
};
use crate::agent::runloop::unified::state::ModelPickerTarget;
use crate::agent::runloop::unified::turn::session::{
    mcp_lifecycle, slash_command_handler, tool_dispatch,
};
use crate::hooks::lifecycle::SessionEndReason;

use super::interaction_loop::{InteractionLoopContext, InteractionOutcome, InteractionState};
use super::interaction_loop_team::{direct_message_target, handle_team_switch, poll_team_mailbox};

const REPEATED_FOLLOW_UP_DIRECTIVE: &str = "User has asked to continue repeatedly. Do not keep exploring silently. In your next assistant response, provide a concrete status update: completed work, current blocker, and the exact next action. If a recent tool error provides a replacement tool (for example read_pty_session), use it directly instead of retrying the same failing call.";
const REPEATED_FOLLOW_UP_STALLED_DIRECTIVE: &str = "Previous turn stalled or aborted and the user asked to continue repeatedly. Recover autonomously without asking for more user prompts: identify the likely root cause from recent errors, execute one adjusted strategy, and then provide either a completion summary or a final blocker review with specific next action. Do not repeat a failing tool call when the error already provides the next tool to use.";

pub(super) async fn run_interaction_loop_impl(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<InteractionOutcome> {
    const MCP_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

    loop {
        let spooled_count = ctx.tool_registry.spooled_files_count().await;
        crate::agent::runloop::unified::status_line::update_spooled_files_count(
            state.input_status_state,
            spooled_count,
        );
        let context_limit_tokens = ctx
            .provider_client
            .effective_context_size(&ctx.config.model);
        if let Some(context_used_tokens) = ctx.context_manager.current_exact_token_usage() {
            crate::agent::runloop::unified::status_line::update_context_budget(
                state.input_status_state,
                context_used_tokens,
                context_limit_tokens,
            );
        } else {
            crate::agent::runloop::unified::status_line::clear_context_budget(
                state.input_status_state,
            );
        }
        crate::agent::runloop::unified::status_line::update_team_status(
            state.input_status_state,
            ctx.session_stats,
        );

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

        if ctx.ctrl_c_state.is_exit_requested() {
            return Ok(InteractionOutcome::Exit {
                reason: SessionEndReason::Exit,
            });
        }

        let interrupts = InlineInterruptCoordinator::new(ctx.ctrl_c_state.as_ref());
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
                InlineLoopAction::PlanApproved {
                    auto_accept,
                    clear_context,
                } => {
                    ctx.renderer.line(
                        MessageStyle::Info,
                        if clear_context {
                            "Plan approved. Clearing context and auto-accepting edits..."
                        } else if auto_accept {
                            "Plan approved with auto-accept. Starting execution..."
                        } else {
                            "Plan approved. Starting execution with manual approval..."
                        },
                    )?;
                    return Ok(InteractionOutcome::PlanApproved {
                        auto_accept,
                        clear_context,
                    });
                }
                InlineLoopAction::PlanEditRequested => {
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

        if let Some(target) = direct_message_target(ctx.session_stats)
            && !input_owned.trim_start().starts_with('/')
            && let Some(team) = ctx.session_stats.team_state.as_mut()
        {
            team.send_message(&target, "lead", input_owned.clone(), None)
                .await?;
            ctx.renderer
                .line(MessageStyle::Info, &format!("Message sent to {}.", target))?;
            continue;
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
                    let Some(picker_state) = state.model_picker_state.take() else {
                        tracing::warn!(
                            "Model picker completed but state was missing; skipping completion flow"
                        );
                        continue;
                    };
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

        if ctx
            .session_stats
            .register_follow_up_prompt(input_owned.as_str())
        {
            if ctx.session_stats.turn_stalled() {
                let stall_reason = ctx
                    .session_stats
                    .turn_stall_reason()
                    .unwrap_or("Previous turn stalled without a detailed reason.")
                    .to_string();
                if let Ok(mut detector) = ctx.autonomous_executor.loop_detector().write() {
                    detector.reset();
                } else {
                    tracing::warn!(
                        "Failed to reset loop detector during stalled follow-up recovery"
                    );
                }
                ctx.conversation_history.push(uni::Message::system(
                    REPEATED_FOLLOW_UP_STALLED_DIRECTIVE.to_string(),
                ));
                ctx.session_stats.suppress_next_follow_up_prompt();
                input_owned = format!(
                    "Continue autonomously from the last stalled turn. Stall reason: {}. Keep working until you can provide a concrete conclusion and final review.",
                    stall_reason
                );
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Repeated follow-up after stalled turn detected; enforcing autonomous recovery and conclusion.",
                )?;
            } else {
                let directive = REPEATED_FOLLOW_UP_DIRECTIVE;
                ctx.conversation_history
                    .push(uni::Message::system(directive.to_string()));
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Repeated follow-up detected; forcing a concrete status/conclusion.",
                )?;
            }
        }
        let input = input_owned.as_str();
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
        return Ok(InteractionOutcome::Continue {
            input: input.to_string(),
        });
    }
}
