#[path = "slash_commands/builtins.rs"]
mod builtins;
#[path = "slash_commands/dispatch.rs"]
mod dispatch;
#[path = "slash_commands/flow.rs"]
mod flow;
#[path = "slash_commands/management.rs"]
mod management;
#[path = "slash_commands/models.rs"]
mod models;
#[path = "slash_commands/parsing.rs"]
mod parsing;
#[path = "slash_commands/rendering.rs"]
mod rendering;

use dispatch::normalize_command_key;
pub(crate) use dispatch::{execute_command_skill_by_name, handle_slash_command};
pub(crate) use models::{
    AgentDefinitionScope, AgentManagerAction, CompactConversationCommand, LocalServerAction,
    McpCommandAction, OAuthProviderAction, ScheduleCommandAction, SessionLogExportFormat,
    SessionPaletteMode, SlashCommandOutcome, StatuslineTargetMode, SubprocessManagerAction,
    ThemePaletteMode,
};

#[cfg(test)]
#[path = "slash_commands/tests.rs"]
mod tests;
