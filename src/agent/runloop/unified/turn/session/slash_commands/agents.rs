use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::slash_commands::AgentCommandAction;

use super::{SlashCommandContext, SlashCommandControl};

pub async fn handle_manage_agents(
    ctx: SlashCommandContext<'_>,
    action: AgentCommandAction,
) -> Result<SlashCommandControl> {
    match action {
        AgentCommandAction::List => {
            ctx.renderer
                .line(MessageStyle::Info, "Built-in Subagents")?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  explore         - Fast read-only codebase search (haiku)",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  plan            - Research specialist for planning mode (sonnet)",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  general         - Multi-step tasks with full capabilities (sonnet)",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  code-reviewer   - Code quality and security review",
            )?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  debugger        - Error investigation and fixes",
            )?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            Ok(SlashCommandControl::Continue)
        }
        AgentCommandAction::Create
        | AgentCommandAction::Edit(_)
        | AgentCommandAction::Delete(_) => {
            ctx.renderer.line(
                MessageStyle::Error,
                "Custom subagents are not supported in this version. Use built-in agents instead.",
            )?;
            Ok(SlashCommandControl::Continue)
        }
        AgentCommandAction::Help => {
            ctx.renderer
                .line(MessageStyle::Info, "Subagent Management")?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(MessageStyle::Output, "Usage: /agents [list]")?;
            ctx.renderer.line(MessageStyle::Output, "")?;
            ctx.renderer.line(
                MessageStyle::Output,
                "  /agents              List all available built-in subagents",
            )?;
            Ok(SlashCommandControl::Continue)
        }
    }
}
