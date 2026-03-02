use anyhow::Result;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::SlashCommandOutcome;

pub(super) fn handle_agents_command(
    _args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    renderer.line(
        MessageStyle::Error,
        "The subagents system has been removed. '/agents' is no longer available.",
    )?;
    Ok(SlashCommandOutcome::Handled)
}

pub(super) fn handle_team_command(
    _args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    renderer.line(
        MessageStyle::Error,
        "Agent teams have been removed. '/team' is no longer available.",
    )?;
    Ok(SlashCommandOutcome::Handled)
}
