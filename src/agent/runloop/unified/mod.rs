pub(crate) mod async_mcp_manager;
pub(crate) mod auto_permission;

mod config_section_headings;
pub(crate) mod context_manager;

mod diagnostics;
mod display;
pub(crate) mod external_editor;
pub(crate) mod external_url_guard;
pub(super) mod fast_mode;
mod incremental_system_prompt;
mod inline_events;
mod intent_extractor;
pub(crate) mod interactive_features;
pub(crate) mod overlay_prompt;
pub(crate) mod url_guard;

mod mcp_support;
mod mcp_tool_manager;
mod model_selection;
mod model_switch_compaction;
pub(crate) mod palettes;
pub(crate) mod plan_blocks;
pub(crate) mod planning_workflow;
mod planning_workflow_state;
mod postamble;
mod progress;
mod prompts;
mod request_user_input;
pub(crate) mod run_loop_context;
mod session_runtime;
mod session_settings;
pub(crate) mod session_setup;
pub(crate) mod settings_interactive;
mod shell;
pub(crate) mod state;
mod status_line;
mod status_line_command;
pub(crate) mod stop_requests;
pub(crate) mod stream_sanitization;
pub(crate) mod tool_catalog;
pub(crate) mod tool_output_handler;
mod tool_pipeline;
mod tool_reads;
mod tool_routing;
mod tool_summary;
mod tool_summary_helpers;
#[cfg(test)]
mod tool_summary_tests;
pub(crate) mod turn;
mod ui_interaction;
mod ui_interaction_stream;
mod ui_interaction_stream_helpers;
#[cfg(test)]
mod ui_interaction_tests;
mod wait_feedback;
pub(crate) mod webmcp;
pub(crate) mod wizard_modal;
mod workspace_links;

// Reasoning utilities (centralized)
pub(crate) mod reasoning;

// Optimization and safety modules
pub(crate) mod tool_call_safety;

pub(crate) use intent_extractor::extract_action_from_messages;
pub(crate) use session_runtime::UnifiedSessionRuntime;
/// Restated so the tool-output renderer can share the exec-session predicate
/// without exposing the whole pipeline module.
pub(crate) use tool_pipeline::is_exec_session_tool_name;
