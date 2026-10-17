mod editor;
mod hook_approval;
mod init;
mod session_mode;
mod shell;
mod signal;
mod skill_setup;
mod types;
mod ui;

#[cfg(test)]
pub(crate) use editor::EditorOpenRequest;
#[cfg(test)]
pub(crate) use editor::bounded_editor_open_requests;
pub(crate) use editor::{EditorOpenDispatcher, EditorOpenRequestSender, spawn_editor_open_coordinator};
pub(crate) use init::active_deferred_tool_policy;
pub(crate) use init::configured_anthropic_config;
pub(crate) use init::create_provider_client;
pub(crate) use init::refresh_tool_snapshot;
pub(crate) use init::resolve_provider_label;
pub(crate) use init::session_mcp_config;
pub(crate) use init::{complete_session_registry, hydrate_session_runtime, initialize_session_critical};
pub(crate) use session_mode::active_primary_agent_from_specs_for_mode;
pub(crate) use shell::initialize_session_shell;
pub(crate) use signal::{mark_exit_postamble_armed, spawn_signal_handler};
pub(crate) use types::SessionState;
#[cfg(test)]
pub(crate) use ui::summarize_thread_event_preview;
pub(crate) use ui::{
    SessionUiLaunchOptions, apply_post_hydration_ui, initialize_session_ui, refresh_local_agents,
    run_session_start_hooks,
};
pub(crate) use ui::{build_structured_resume_lines, render_resume_lines};
