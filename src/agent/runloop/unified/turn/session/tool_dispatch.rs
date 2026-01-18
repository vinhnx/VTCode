
use anyhow::Result;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::turn::session::interaction_loop::InteractionOutcome;
use crate::agent::runloop::tool_output::render_tool_output;

pub(crate) struct DirectToolContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub conversation_history: &'a mut Vec<uni::Message>,
    pub handle: &'a InlineHandle,
    pub input_status_state: &'a mut InputStatusState,
    pub tool_registry: &'a mut vtcode_core::tools::registry::ToolRegistry,
    pub vt_cfg: &'a Option<VTCodeConfig>,
    pub follow_up_placeholder: &'a Option<String>,
}

pub(crate) async fn handle_direct_tool_execution(
    input: &str,
    ctx: &mut DirectToolContext<'_>,
) -> Result<Option<InteractionOutcome>> {
    // Check for bash mode with '!' prefix
    if input.starts_with('!') {
        let bash_command = input.trim_start_matches('!').trim();
        if !bash_command.is_empty() {
            display_user_message(ctx.renderer, input)?;
            ctx.conversation_history
                .push(uni::Message::user(input.to_string()));

            // Show spinner while command is running
            let spinner =
                crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner::new(
                    ctx.handle,
                    ctx.input_status_state.left.clone(),
                    ctx.input_status_state.right.clone(),
                    format!("Running: {}", bash_command),
                );

            // Execute bash command directly
            let tool_call_id = format!("bash_{}", ctx.conversation_history.len());

            // Find the bash tool in the registry
            let args = serde_json::json!({"command": bash_command});
            let bash_result = ctx.tool_registry.execute_tool_ref("bash", &args).await;

            // Stop spinner before rendering output
            spinner.finish();

            let command_succeeded = match bash_result {
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
                    result
                        .get("exit_code")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0)
                        == 0
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
                    false
                }
            };

            ctx.handle.clear_input();
            ctx.handle
                .set_placeholder(ctx.follow_up_placeholder.clone());

            // Return to trigger LLM follow-up with context about what happened
            let follow_up_prompt = if command_succeeded {
                format!(
                    "I ran `{}`. Please briefly acknowledge the result and suggest what to do next.",
                    bash_command
                )
            } else {
                format!(
                    "I ran `{}` but it failed. Please analyze the error and suggest how to fix it.",
                    bash_command
                )
            };
            return Ok(Some(InteractionOutcome::Continue {
                input: follow_up_prompt,
            }));
        }
    }

    // Check for explicit "run <command>" pattern
    if let Some((tool_name, tool_args)) =
        crate::agent::runloop::unified::shell::detect_explicit_run_command(input)
    {
        display_user_message(ctx.renderer, input)?;
        ctx.conversation_history
            .push(uni::Message::user(input.to_string()));

        // Show spinner while command is running
        let command_preview = tool_args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("command");
        let spinner = crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner::new(
            ctx.handle,
            ctx.input_status_state.left.clone(),
            ctx.input_status_state.right.clone(),
            format!("Running: {}", command_preview),
        );

        let tool_call_id = format!("explicit_run_{}", ctx.conversation_history.len());
        let bash_result = ctx
            .tool_registry
            .execute_tool_ref(&tool_name, &tool_args)
            .await;

        // Stop spinner before rendering output
        spinner.finish();

        let command_succeeded = match bash_result {
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
                result
                    .get("exit_code")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0)
                    == 0
            }
            Err(err) => {
                ctx.renderer
                    .line(MessageStyle::Error, &format!("Command failed: {}", err))?;
                ctx.conversation_history.push(uni::Message::tool_response(
                    tool_call_id.clone(),
                    format!("{{\"error\": \"{}\"}}", err),
                ));
                false
            }
        };

        ctx.handle.clear_input();
        ctx.handle
            .set_placeholder(ctx.follow_up_placeholder.clone());

        // Return to trigger LLM follow-up with context about what happened
        let follow_up_prompt = if command_succeeded {
            format!(
                "I ran `{}`. Please briefly acknowledge the result and suggest what to do next.",
                command_preview
            )
        } else {
            format!(
                "I ran `{}` but it failed. Please analyze the error and suggest how to fix it.",
                command_preview
            )
        };
        return Ok(Some(InteractionOutcome::Continue {
            input: follow_up_prompt,
        }));
    }

    Ok(None)
}
