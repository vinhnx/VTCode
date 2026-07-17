//! MCP client management built on top of the Codex MCP building blocks.
//!
//! Re-exported from `vtcode-mcp` for backward compatibility.
//! The `cli` module remains local as it depends on `crate::cli::input_hardening`.

pub mod cli;

// Re-export everything from vtcode-mcp
pub use vtcode_mcp::*;
