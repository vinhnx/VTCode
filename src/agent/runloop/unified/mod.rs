pub mod async_mcp_manager;

pub mod context_manager;

mod diagnostics;
mod display;
mod driver;
mod incremental_system_prompt;
mod inline_events;
mod intent_extractor;

mod mcp_support;
mod mcp_tool_manager;
mod model_selection;
pub(crate) mod palettes;
pub(crate) mod plan_blocks;
mod plan_confirmation;
mod plan_mode_state;
mod progress;
mod prompts;
mod request_user_input;
pub(crate) mod run_loop_context;
pub mod session_setup;
mod shell;
pub mod state;
mod status_line;
mod status_line_command;
pub(crate) mod team_state;
mod team_tmux;
pub(crate) mod tool_catalog;
mod tool_ledger;
mod tool_output_handler;
mod tool_output_handler_unified;
mod tool_output_helpers;
mod tool_pipeline;
mod tool_routing;
mod tool_summary;
mod tool_summary_helpers;
#[cfg(test)]
mod tool_summary_tests;
pub mod turn;
mod ui_interaction;
mod ui_interaction_stream;
mod ui_interaction_stream_helpers;
#[cfg(test)]
mod ui_interaction_tests;
mod wizard_modal;
mod workspace_links;

// Reasoning utilities (centralized)
pub(crate) mod reasoning;

// Optimization and safety modules
pub mod tool_call_safety;

// Golden path integration for unified tool execution
pub(crate) mod golden_path;

pub(crate) use driver::UnifiedTurnDriver;
pub(crate) use intent_extractor::extract_action_from_messages;
