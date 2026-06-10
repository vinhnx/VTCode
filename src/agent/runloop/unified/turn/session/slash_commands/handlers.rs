pub(crate) use super::{SlashCommandContext, SlashCommandControl};

#[path = "activation.rs"]
mod activation;
#[path = "agents.rs"]
mod agents;
#[path = "apps.rs"]
mod apps;
#[path = "compact.rs"]
mod compact;
#[path = "config_toml.rs"]
mod config_toml;
#[path = "control.rs"]
mod control;
#[path = "diagnostics.rs"]
mod diagnostics;
#[path = "effort.rs"]
mod effort;
#[path = "interactive.rs"]
mod interactive;
#[path = "local_server.rs"]
mod local_server;
#[path = "mcp.rs"]
mod mcp;
#[path = "oauth.rs"]
mod oauth;
#[path = "planning.rs"]
mod planning;
#[path = "rewind.rs"]
mod rewind;
#[path = "schedule.rs"]
mod schedule;
#[path = "share_log.rs"]
mod share_log;
#[path = "skills.rs"]
mod skills;
#[path = "ui.rs"]
mod ui;
#[path = "update.rs"]
mod update;
#[path = "workspace.rs"]
mod workspace;

pub(crate) use control::scheduler_enabled;

pub(super) use agents::{handle_manage_agents, handle_manage_subprocesses};
pub(crate) use apps::run_with_event_loop_suspended;
pub(super) use apps::{
    handle_launch_editor, handle_launch_git, handle_new_session, handle_open_docs,
    handle_open_donate_links,
};
pub(super) use compact::handle_compact_conversation;
pub(super) use control::{
    handle_clear_conversation, handle_clear_screen, handle_copy_latest_assistant_reply,
    handle_exit, handle_manage_loop, handle_notify, handle_show_hooks, handle_show_permissions,
    handle_show_settings, handle_show_settings_at_path, handle_stop_agent,
    show_settings_at_path_from_context,
};
pub(super) use diagnostics::{
    handle_run_doctor, handle_show_memory, handle_show_memory_config, handle_show_status,
    handle_start_doctor_interactive, handle_start_terminal_setup,
};
pub(super) use effort::handle_set_effort;
pub(super) use interactive::{
    handle_show_jobs_panel, handle_toggle_tasks_panel, handle_trigger_prompt_suggestions,
};
pub(super) use local_server::handle_manage_local_server;
pub(super) use mcp::handle_manage_mcp;
pub(super) use oauth::{
    handle_oauth_login, handle_oauth_logout, handle_refresh_oauth, handle_show_auth_status,
    handle_start_oauth_provider_picker,
};
pub(super) use planning::handle_toggle_planning_workflow;
pub(super) use rewind::{handle_open_rewind_picker, handle_rewind_latest, handle_rewind_to_turn};
pub(super) use schedule::handle_manage_schedule;
pub(super) use share_log::handle_share_log;
pub(super) use skills::handle_manage_skills;
pub(super) use ui::{
    handle_start_file_browser, handle_start_history_picker, handle_start_model_selection,
    handle_start_session_palette, handle_start_statusline_setup, handle_start_terminal_title_setup,
    handle_start_theme_palette, handle_theme_changed, handle_toggle_ide_context,
};
pub(super) use update::handle_update;
pub(super) use workspace::handle_initialize_workspace;
