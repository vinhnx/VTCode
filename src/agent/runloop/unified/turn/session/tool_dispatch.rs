use anyhow::Result;
use std::collections::BTreeSet;

use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult};
use crate::agent::runloop::unified::turn::session::interaction_loop::{
    InteractionLoopContext, InteractionOutcome,
};
use crate::agent::runloop::unified::turn::tool_outcomes::handlers::{
    ToolOutcomeContext, handle_single_tool_call,
};
use vtcode_core::llm::provider as uni;
use vtcode_core::session::SessionId;

pub(crate) struct DirectToolContext<'a, 'b> {
    pub interaction_ctx: &'b mut InteractionLoopContext<'a>,
    pub input_status_state: &'b mut InputStatusState,
}

pub(crate) async fn handle_direct_tool_execution(
    input: &str,
    ctx: &mut DirectToolContext<'_, '_>,
) -> Result<Option<InteractionOutcome>> {
    let Some((tool_name_str, args)) = parse_direct_tool_input(input) else {
        return Ok(None);
    };
    let is_bang_prefix = input.trim_start().starts_with('!');

    // Construct HarnessTurnState (simplified for direct execution)
    let mut harness_state = HarnessTurnState::new(
        TurnRunId(SessionId::new().0),
        TurnId(SessionId::new().0),
        ctx.interaction_ctx.harness_config.max_tool_calls_per_turn,
        ctx.interaction_ctx.harness_config.max_tool_wall_clock_secs,
        ctx.interaction_ctx.harness_config.max_tool_retries,
    );

    let mut auto_exit_plan_mode_attempted = false;

    // Construct TurnProcessingContext to leverage unified execution handlers
    let mut tp_ctx = ctx.interaction_ctx.as_turn_processing_context(
        &mut harness_state,
        &mut auto_exit_plan_mode_attempted,
        ctx.input_status_state,
    );

    let mut repeated_tool_attempts =
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let mut t_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    // 1. Display user message and push to history
    if is_bang_prefix {
        t_ctx.ctx.renderer.line(
            vtcode_core::utils::ansi::MessageStyle::Info,
            "Shell mode (!): executing command directly.",
        )?;
    }
    display_user_message(t_ctx.ctx.renderer, input)?;
    t_ctx
        .ctx
        .working_history
        .push(uni::Message::user(input.to_string()));

    // 2. Inject assistant message with tool call to keep history valid for LLM
    let tool_call_id = format!(
        "direct_{}_{}",
        tool_name_str,
        t_ctx.ctx.working_history.len()
    );
    let tool_call = uni::ToolCall::function(
        tool_call_id.clone(),
        tool_name_str.clone(),
        serde_json::to_string(&args).unwrap_or_default(),
    );
    t_ctx
        .ctx
        .working_history
        .push(uni::Message::assistant_with_tools(
            String::new(),
            vec![tool_call],
        ));

    // 3. Execute through unified pipeline to ensure safety, metrics, and consistent output
    let outcome = handle_single_tool_call(&mut t_ctx, tool_call_id, &tool_name_str, args).await?;

    // 4. Cleanup UI and return outcome
    t_ctx.ctx.handle.clear_input();
    if let Some(placeholder) = t_ctx.ctx.default_placeholder {
        t_ctx
            .ctx
            .handle
            .set_placeholder(Some(placeholder.to_string()));
    }

    if let Some(TurnHandlerOutcome::Break(TurnLoopResult::Exit)) = outcome {
        return Ok(Some(InteractionOutcome::Exit {
            reason: crate::hooks::lifecycle::SessionEndReason::Exit,
        }));
    }

    // Direct tool paths already executed and rendered output; skip creating an
    // immediate LLM turn for this interaction loop iteration.
    Ok(Some(InteractionOutcome::DirectToolHandled))
}

fn parse_direct_tool_input(input: &str) -> Option<(String, serde_json::Value)> {
    // Check for bash mode with '!' prefix or explicit 'run' command
    let trimmed = input.trim_start();
    if let Some(rest) = trimmed.strip_prefix('!') {
        let bash_command = rest.trim();
        if bash_command.is_empty() {
            return None;
        }
        return Some((
            "bash".to_string(),
            serde_json::json!({ "command": bash_command }),
        ));
    }

    crate::agent::runloop::unified::shell::detect_explicit_run_command(input)
}

#[cfg(test)]
mod tests {
    use super::parse_direct_tool_input;

    #[test]
    fn parses_bang_prefix_with_leading_whitespace() {
        let (tool, args) = parse_direct_tool_input("   !echo hello").expect("direct tool");
        assert_eq!(tool, "bash");
        assert_eq!(args["command"], "echo hello");
    }

    #[test]
    fn rejects_empty_bang_command() {
        assert!(parse_direct_tool_input("!   ").is_none());
    }
}
