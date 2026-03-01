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
use vtcode_core::command_safety::shell_parser::parse_shell_commands_tree_sitter;
use vtcode_core::llm::provider as uni;
use vtcode_core::session::SessionId;

pub(crate) struct DirectToolContext<'a, 'b> {
    pub interaction_ctx: &'b mut InteractionLoopContext<'a>,
    pub input_status_state: &'b mut InputStatusState,
}

enum DirectToolInput {
    Execute {
        tool_name: String,
        args: serde_json::Value,
        is_bang_prefix: bool,
    },
    InvalidBang {
        command: String,
        diagnosis: String,
    },
}

pub(crate) async fn handle_direct_tool_execution(
    input: &str,
    ctx: &mut DirectToolContext<'_, '_>,
) -> Result<Option<InteractionOutcome>> {
    let Some(parsed) = parse_direct_tool_input(input) else {
        return Ok(None);
    };

    let (tool_name_str, args, is_bang_prefix) = match parsed {
        DirectToolInput::Execute {
            tool_name,
            args,
            is_bang_prefix,
        } => (tool_name, args, is_bang_prefix),
        DirectToolInput::InvalidBang { command, diagnosis } => {
            ctx.interaction_ctx.renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                "Shell mode (!): command rejected (invalid bash grammar).",
            )?;
            ctx.interaction_ctx.renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                &format!("Diagnosis: {diagnosis}"),
            )?;
            ctx.interaction_ctx.renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                &format!("Recovery: fix syntax and retry as `!{command}`, or remove `!` to ask in natural language."),
            )?;
            return Ok(Some(InteractionOutcome::DirectToolHandled));
        }
    };

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

fn parse_direct_tool_input(input: &str) -> Option<DirectToolInput> {
    // Check for bash mode with '!' prefix or explicit 'run' command
    let trimmed = input.trim_start();
    if let Some(rest) = trimmed.strip_prefix('!') {
        let bash_command = rest.trim();
        if bash_command.is_empty() {
            return None;
        }
        return match validate_bang_shell_command(bash_command) {
            Ok(()) => Some(DirectToolInput::Execute {
                tool_name: "bash".to_string(),
                args: serde_json::json!({ "command": bash_command }),
                is_bang_prefix: true,
            }),
            Err(diagnosis) => Some(DirectToolInput::InvalidBang {
                command: bash_command.to_string(),
                diagnosis,
            }),
        };
    }

    crate::agent::runloop::unified::shell::detect_explicit_run_command(input).map(
        |(tool_name, args)| DirectToolInput::Execute {
            tool_name,
            args,
            is_bang_prefix: false,
        },
    )
}

fn validate_bang_shell_command(command: &str) -> std::result::Result<(), String> {
    match parse_shell_commands_tree_sitter(command) {
        Ok(commands) if !commands.is_empty() => Ok(()),
        Ok(_) => Err("No executable shell command found.".to_string()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::{DirectToolInput, parse_direct_tool_input};

    #[test]
    fn parses_bang_prefix_with_leading_whitespace() {
        let parsed = parse_direct_tool_input("   !echo hello").expect("direct tool");
        match parsed {
            DirectToolInput::Execute {
                tool_name,
                args,
                is_bang_prefix,
            } => {
                assert_eq!(tool_name, "bash");
                assert_eq!(args["command"], "echo hello");
                assert!(is_bang_prefix);
            }
            DirectToolInput::InvalidBang { .. } => {
                panic!("expected valid !-command to parse");
            }
        }
    }

    #[test]
    fn rejects_invalid_bang_command_with_diagnosis() {
        let parsed = parse_direct_tool_input("! )(").expect("invalid command should be handled");
        match parsed {
            DirectToolInput::InvalidBang { command, diagnosis } => {
                assert_eq!(command, ")(");
                assert!(!diagnosis.trim().is_empty());
            }
            DirectToolInput::Execute { .. } => {
                panic!("expected invalid !-command to be rejected");
            }
        }
    }

    #[test]
    fn rejects_empty_bang_command() {
        assert!(parse_direct_tool_input("!   ").is_none());
    }
}
