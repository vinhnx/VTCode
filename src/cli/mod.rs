//! CLI command implementations with modular architecture

// Feature-gated tool-capable chat; fallback to minimal REPL
pub mod acp;
pub mod analyze;
pub mod ask;
pub mod auto;
pub mod benchmark;
pub mod chat_tools;
pub mod compress_context;
pub mod config;
pub mod create_project;
pub mod init;
pub mod init_project;
pub mod man;
pub mod mcp;
pub mod performance;
pub mod revert;
pub mod snapshots;
pub mod trajectory;

// Re-export command handlers for backward compatibility
pub use acp::handle_acp_command;
pub use analyze::handle_analyze_command;
pub use ask::handle_ask_command as handle_ask_single_command;
pub use auto::handle_auto_task_command;
pub use benchmark::{BenchmarkCommandOptions, handle_benchmark_command};
// Use the modular runloop by default
pub use chat_tools::handle_chat_command;
pub use compress_context::handle_compress_context_command;
pub use config::handle_config_command;
pub use create_project::handle_create_project_command;
pub use init::handle_init_command;
pub use init_project::handle_init_project_command;
pub use man::handle_man_command;
pub use mcp::handle_mcp_command;
pub use performance::handle_performance_command;
pub use revert::handle_revert_command;
pub use snapshots::{handle_cleanup_snapshots_command, handle_snapshots_command};
pub use trajectory::handle_trajectory_command as handle_trajectory_logs_command;

use std::path::Path;

/// Export the current workspace directory through the expected environment variable
pub fn set_workspace_env(path: &Path) {
    // SAFETY: Setting a process-local environment variable is safe; the OS copies the value.
    unsafe {
        std::env::set_var("WORKSPACE_DIR", path);
    }
}
