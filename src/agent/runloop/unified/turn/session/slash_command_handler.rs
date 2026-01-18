
use anyhow::Result;

use crate::agent::runloop::slash_commands::handle_slash_command as process_slash_command;
use crate::agent::runloop::unified::turn::session::interaction_loop::{InteractionLoopContext, InteractionState, InteractionOutcome};
use crate::agent::runloop::unified::turn::session::slash_commands::{
    self, SlashCommandContext, SlashCommandControl,
};
use crate::hooks::lifecycle::SessionEndReason;
use vtcode_core::utils::ansi::MessageStyle;

pub enum CommandProcessingResult {
    Outcome(InteractionOutcome),
    ContinueLoop,
    NotHandled,
    UpdateInput(String),
}

pub(crate) async fn handle_input_commands(
    input: &str,
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<CommandProcessingResult> {
    match input {
        "" => return Ok(CommandProcessingResult::ContinueLoop),
        "exit" | "quit" => {
            ctx.renderer.line(MessageStyle::Info, "✓")?;
            return Ok(CommandProcessingResult::Outcome(InteractionOutcome::Exit {
                reason: SessionEndReason::Exit,
            }));
        }
        "help" => {
            ctx.renderer
                .line(MessageStyle::Info, "Commands: exit, help")?;
            return Ok(CommandProcessingResult::ContinueLoop); 
        }
        input if input.starts_with('/') => {
            if let Some(command_input) = input.strip_prefix('/') {
                let outcome = process_slash_command(
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
                         return Ok(CommandProcessingResult::UpdateInput(prompt));
                    }
                    SlashCommandControl::Continue => return Ok(CommandProcessingResult::ContinueLoop),
                    SlashCommandControl::BreakWithReason(reason) => {
                        return Ok(CommandProcessingResult::Outcome(InteractionOutcome::Exit { reason }));
                    }
                    SlashCommandControl::BreakWithoutReason => {
                        return Ok(CommandProcessingResult::Outcome(InteractionOutcome::Exit {
                            reason: SessionEndReason::Exit,
                        }));
                    }
                }
            }
        }
        _ => {}
    }
    
    Ok(CommandProcessingResult::NotHandled)
}
