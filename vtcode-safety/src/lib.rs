//! Command safety detection, execution policies, and sandboxing for VT Code.
//!
//! This crate provides the safety subsystem extracted from `vtcode-core`:
//!
//! - **command_safety**: Granular command safety evaluation based on subcommands and options
//! - **exec_policy**: Execution authorization policies and approval requirements
//! - **sandboxing**: Sandbox policies and execution environment transformations

pub mod audit_log;
pub mod command_safety;
pub mod exec_policy;
pub mod mcp_sandbox;
pub mod sandboxing;
