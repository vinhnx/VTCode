//! Sandboxing module for VT Code
//!
//! This module provides sandbox policies and execution environment transformations
//! inspired by the OpenAI Codex execution model. It enables safe command execution
//! with configurable isolation levels.
//!
//! ## Architecture
//!
//! The sandboxing system consists of:
//! - **SandboxPolicy**: Configurable isolation levels (ReadOnly, WorkspaceWrite, DangerFullAccess)
//! - **SandboxManager**: Transforms command specifications into sandboxed execution environments
//! - **SandboxPermissions**: Fine-grained permission control for individual operations
//!
//! ## Usage
//!
//! ```rust,no_run
//! use vtcode_core::sandboxing::{SandboxPolicy, SandboxManager, CommandSpec};
//!
//! let policy = SandboxPolicy::read_only();
//! let manager = SandboxManager::new();
//! let spec = CommandSpec {
//!     program: "cat".to_string(),
//!     args: vec!["file.txt".to_string()],
//!     ..Default::default()
//! };
//!
//! // Transform to sandboxed environment
//! let exec_env = manager.transform(spec, &policy)?;
//! ```

mod exec_env;
mod manager;
mod permissions;
mod policy;

pub use exec_env::{CommandSpec, ExecEnv, ExecExpiration, SandboxType};
pub use manager::{SandboxManager, SandboxTransformError};
pub use permissions::SandboxPermissions;
pub use policy::{SandboxPolicy, WritableRoot};
