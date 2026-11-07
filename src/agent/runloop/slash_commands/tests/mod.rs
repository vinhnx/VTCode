pub(super) use super::builtins::{
    CheckupCommand, DoctorCommand, parse_checkup_args, parse_doctor_args, parse_update_args,
};
pub(super) use super::{
    AgentManagerAction, CompactConversationCommand, SecretCommandAction, SessionLogExportFormat, SlashCommandOutcome,
    SubprocessManagerAction, handle_slash_command,
};
pub(super) use vtcode_core::compaction::ManualCompactionOptions;
pub(super) use vtcode_core::config::types::ReasoningEffortLevel;
pub(super) use vtcode_core::skills::command_skill_specs;
pub(super) use vtcode_core::utils::ansi::AnsiRenderer;
pub(super) use vtcode_ui::tui::app::InlineHandle;

fn renderer_for_tests() -> AnsiRenderer {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(tx), Default::default())
}

// -- /local command tests --

use super::LocalServerAction;

mod config_agents;
mod parse_session;
mod plan_webmcp_compact;
mod registry_local;
mod secret_vim;
mod templates_review;
mod terminal_icon;
