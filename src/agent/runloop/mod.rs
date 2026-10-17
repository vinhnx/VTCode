mod git;
mod mcp_elicitation;
mod mcp_events;
mod model_picker;
mod plugin_commands;
mod plugin_commands_parser;
mod prompt;
mod skills_commands;
mod skills_commands_parser;
mod slash_commands;
mod telemetry;
mod text_tools;
pub(crate) mod tool_output;
mod ui;
pub(crate) mod ui_list;
pub(crate) mod unified;
mod welcome;
#[cfg(test)]
mod welcome_tests;

pub(crate) use crate::agent::agents::ResumeSession;
pub(crate) use plugin_commands::{
    PluginCommandAction, PluginCommandOutcome, handle_plugin_command, parse_plugin_command,
};
pub(crate) use skills_commands::{SkillCommandAction, SkillCommandOutcome, handle_skill_command, parse_skill_command};
