mod bootstrap;
mod debug_routing;
mod ide;
mod prompt_input;
mod tracing;

pub(crate) use bootstrap::{
    build_augmented_cli_command, debug_runtime_flag_enabled, resolve_runtime_color_policy,
    resolve_startup_context,
};
pub(crate) use debug_routing::configure_debug_session_routing;
pub(crate) use ide::detect_available_ide;
pub(crate) use prompt_input::build_print_prompt;
pub(crate) use tracing::{
    initialize_default_error_tracing, initialize_tracing, initialize_tracing_from_config,
};
pub(crate) use vtcode_core::utils::session_debug::{
    build_command_debug_session_id, configure_runtime_debug_context, runtime_archive_session_id,
    runtime_debug_log_path, set_runtime_archive_session_id,
};
