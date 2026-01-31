//! Message handling helpers for tool outcomes.

use anyhow::Result;
use vtcode_core::llm::provider as uni;

use vtcode_core::utils::ansi::MessageStyle;


use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};
use crate::agent::runloop::unified::turn::guards::handle_turn_balancer;

use super::helpers::{
    push_assistant_message, reasoning_duplicates_content,
};

pub(crate) fn handle_assistant_response(
    ctx: &mut TurnProcessingContext<'_>,
    assistant_text: String,
    reasoning: Option<String>,
    response_streamed: bool,
) -> Result<()> {

    if !response_streamed {
        if !assistant_text.trim().is_empty() {
            ctx.renderer.line(MessageStyle::Response, &assistant_text)?;
        }
        if let Some(reasoning_text) = reasoning.as_ref()
            && !reasoning_text.trim().is_empty()
        {
            let duplicates_content = !assistant_text.trim().is_empty()
                && reasoning_duplicates_content(reasoning_text, &assistant_text);
            if !reasoning_text.trim().is_empty() && !duplicates_content {
                let cleaned_for_display =
                    vtcode_core::llm::providers::clean_reasoning_text(reasoning_text);
                ctx.renderer
                    .line(MessageStyle::Reasoning, &cleaned_for_display)?;
            }
        }
    }

    if !assistant_text.trim().is_empty() {
        let msg = uni::Message::assistant(assistant_text.clone());
        let msg_with_reasoning = if let Some(reasoning_text) = reasoning {
            if reasoning_duplicates_content(&reasoning_text, &assistant_text) {
                msg
            } else {
                msg.with_reasoning(Some(reasoning_text))
            }
        } else {
            msg
        };
        push_assistant_message(ctx.working_history, msg_with_reasoning);
    } else if let Some(reasoning_text) = reasoning {
        push_assistant_message(
            ctx.working_history,
            uni::Message::assistant(String::new()).with_reasoning(Some(reasoning_text)),
        );
    }

    Ok(())
}

pub(crate) struct HandleTextResponseParams<'a> {
    pub ctx: &'a mut TurnProcessingContext<'a>,
    pub repeated_tool_attempts: &'a mut std::collections::HashMap<String, usize>,
    pub turn_modified_files: &'a mut std::collections::BTreeSet<std::path::PathBuf>,
    pub traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
    pub text: String,
    pub reasoning: Option<String>,
    pub response_streamed: bool,
    pub step_count: usize,
    pub session_end_reason: &'a mut crate::hooks::lifecycle::SessionEndReason,
    pub max_tool_loops: usize,
    pub tool_repeat_limit: usize,
}

pub(crate) async fn handle_text_response<'a>(
    mut params: HandleTextResponseParams<'a>,
) -> Result<TurnHandlerOutcome> {
    if !params.response_streamed {
        if !params.text.trim().is_empty() {
            params
                .t_ctx.ctx
                .renderer
                .line(MessageStyle::Response, &params.text)?;
        }
        if let Some(reasoning_text) = params.reasoning.as_ref()
            && !reasoning_text.trim().is_empty()
        {
            let duplicates_content = !params.text.trim().is_empty()
                && reasoning_duplicates_content(reasoning_text, &params.text);
            if !reasoning_text.trim().is_empty() && !duplicates_content {
                let cleaned_for_display =
                    vtcode_core::llm::providers::clean_reasoning_text(reasoning_text);
                params
                    .t_ctx.ctx
                    .renderer
                    .line(MessageStyle::Reasoning, &cleaned_for_display)?;
            }
        }
    }

    if let Some((tool_name, args)) =
        crate::agent::runloop::text_tools::detect_textual_tool_call(&params.text)
    {
        let args_json = serde_json::json!(&args);
        let tool_call_str = format!("call_textual_{}", params.t_ctx.ctx.working_history.len());

        let call_tool_name = tool_name;
        let call_args_val = args_json;

        use crate::agent::runloop::unified::tool_summary::{
            describe_tool_action, humanize_tool_name,
        };
        let (headline, _) = describe_tool_action(&call_tool_name, &call_args_val);
        let notice = if headline.is_empty() {
            format!("Detected {} request", humanize_tool_name(&call_tool_name))
        } else {
            format!("Detected {headline}")
        };
        params.t_ctx.ctx.renderer.line(MessageStyle::Info, &notice)?;

        use super::handlers::handle_single_tool_call;
        let tool_name_owned = call_tool_name.to_string();
        let outcome_result = handle_single_tool_call(
            params.t_ctx,
            tool_call_str,
            &tool_name_owned,
            call_args_val,
        )
        .await?;

        if let Some(outcome) = outcome_result {
            return Ok(outcome);
        }

        Ok(handle_turn_balancer(
            params.t_ctx.ctx,
            params.step_count,
            params.t_ctx.repeated_tool_attempts,
            params.max_tool_loops,
            params.tool_repeat_limit,
        )
        .await)
    } else {
        let msg = uni::Message::assistant(params.text.clone());
        let msg_with_reasoning = if let Some(reasoning_text) = params.reasoning {
            if reasoning_duplicates_content(&reasoning_text, &params.text) {
                msg
            } else {
                msg.with_reasoning(Some(reasoning_text))
            }
        } else {
            msg
        };

        if !params.text.is_empty() || msg_with_reasoning.reasoning.is_some() {
            push_assistant_message(params.t_ctx.ctx.working_history, msg_with_reasoning);
        }

        Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed))
    }
}
