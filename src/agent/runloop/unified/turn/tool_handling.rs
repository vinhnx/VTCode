#![allow(clippy::too_many_arguments)]
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::registry::labels::tool_action_label;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext;
use crate::agent::runloop::unified::turn::utils::{
    enforce_history_limits, truncate_message_content,
};

pub(crate) async fn handle_tool_execution_result(
    ctx: &mut TurnLoopContext<'_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    tool_result: &ToolExecutionStatus,
    working_history: &mut Vec<uni::Message>,
    turn_modified_files: &mut BTreeSet<PathBuf>,
    vt_cfg: Option<&VTCodeConfig>,
    traj: &TrajectoryLogger,
) -> Result<()> {
    match tool_result {
        ToolExecutionStatus::Success {
            output,
            stdout: _,
            modified_files,
            command_success,
            has_more,
        } => {
            // Add successful tool result to history

            let content_for_model = match output {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Object(obj) => serde_json::to_string(obj).unwrap_or_default(),
                _ => output.to_string(),
            };

            let limited_content = truncate_message_content(&content_for_model);
            working_history.push(uni::Message::tool_response_with_origin(
                tool_call_id,
                limited_content,
                tool_name.to_string(),
            ));
            enforce_history_limits(working_history);

            // Build a ToolPipelineOutcome to leverage centralized handling
            let pipeline_outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
                output: output.clone(),
                stdout: None,
                modified_files: modified_files.clone(),
                command_success: *command_success,
                has_more: *has_more,
            });

            // Build a small RunLoopContext to reuse the generic `handle_pipeline_output`
            let (_any_write, mod_files, last_stdout) = crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_from_turn_ctx(
                ctx,
                tool_name,
                args_val,
                &pipeline_outcome,
                vt_cfg,
                traj,
            )
            .await?;

            for f in mod_files {
                turn_modified_files.insert(f);
            }
            let _ = last_stdout;

            // Handle lifecycle hooks
            if let Some(hooks) = ctx.lifecycle_hooks {
                match hooks
                    .run_post_tool_use(tool_name, Some(args_val), output)
                    .await
                {
                    Ok(outcome) => {
                        crate::agent::runloop::unified::turn::utils::render_hook_messages(
                            ctx.renderer,
                            &outcome.messages,
                        )?;
                        for context in outcome.additional_context {
                            if !context.trim().is_empty() {
                                working_history.push(uni::Message::system(context));
                            }
                        }
                    }
                    Err(err) => {
                        ctx.renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to run post-tool hooks: {}", err),
                        )?;
                    }
                }
            }
        }
        ToolExecutionStatus::Failure { error } => {
            // Add error result to history — use sanitized, categorised message
            let (primary_msg, hint) = super::turn_helpers::format_tool_error_for_user(
                tool_name,
                &error.to_string(),
            );
            ctx.renderer.line(MessageStyle::Error, &primary_msg)?;
            if let Some(h) = &hint {
                ctx.renderer.line(MessageStyle::Info, h)?;
            }

            let error_content = serde_json::json!({"error": primary_msg});
            let limited_content = truncate_message_content(&error_content.to_string());
            working_history.push(uni::Message::tool_response_with_origin(
                tool_call_id,
                limited_content,
                tool_name.to_string(),
            ));
            enforce_history_limits(working_history);
        }
        ToolExecutionStatus::Timeout { error } => {
            // Add timeout result to history with sanitized message
            let sanitized = super::turn_helpers::sanitize_error_for_display(&error.message);
            let error_msg = format!("Tool '{}' timed out: {}", tool_name, sanitized);
            ctx.renderer.line(MessageStyle::Error, &error_msg)?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Hint: The operation exceeded its time limit. Try a smaller scope or increase the timeout.",
            )?;

            let error_content = serde_json::json!({"error": error_msg});
            let limited_content = truncate_message_content(&error_content.to_string());
            working_history.push(uni::Message::tool_response_with_origin(
                tool_call_id,
                limited_content,
                tool_name.to_string(),
            ));
            enforce_history_limits(working_history);
        }
        ToolExecutionStatus::Cancelled => {
            // Add cancellation result to history
            let display_tool = tool_action_label(tool_name, args_val);
            let error_msg = format!("Tool '{}' execution cancelled", display_tool);
            ctx.renderer.line(MessageStyle::Info, &error_msg)?;

            let error_content = serde_json::json!({"error": error_msg});
            let limited_content = truncate_message_content(&error_content.to_string());
            working_history.push(uni::Message::tool_response_with_origin(
                tool_call_id,
                limited_content,
                tool_name.to_string(),
            ));
            enforce_history_limits(working_history);
        }

    }

    // Handle MCP events using the shared MCP event recorder
    if tool_name.starts_with("mcp_") {
        super::tool_outcomes::execution_result::record_mcp_event_to_panel(
            &mut ctx.mcp_panel_state,
            tool_name,
            tool_result,
        );
    }

    Ok(())
}
