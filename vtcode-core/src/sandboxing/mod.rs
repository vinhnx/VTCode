//! Sandboxing module for VT Code
//!
//! This module provides sandbox policies and execution environment transformations
//! inspired by the OpenAI Codex execution model and the AI sandbox field guide.
//! It enables safe command execution with configurable isolation levels.
//!
//! ## Architecture
//!
//! The sandboxing system implements the field guide's three-question model:
//! - **Boundary**: What is shared (kernel-enforced via Seatbelt/Landlock)
//! - **Policy**: What can code touch (SandboxPolicy enum)
//! - **Lifecycle**: What survives between runs (session-scoped approvals)
//!
//! Key components:
//! - **SandboxPolicy**: Configurable isolation levels (ReadOnly, WorkspaceWrite, DangerFullAccess)
//! - **SandboxManager**: Transforms command specifications into sandboxed execution environments
//! - **SandboxPermissions**: Fine-grained permission control for individual operations
//! - **NetworkAllowlistEntry**: Domain-based network egress control
//! - **SensitivePath**: Credential location blocking
//! - **ResourceLimits**: Memory, PID, disk, and CPU limits
//!
//! ## Usage
//!
//! ```rust,no_run
//! use vtcode_core::sandboxing::{SandboxPolicy, SandboxManager, CommandSpec, ResourceLimits};
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
//! let exec_env = manager.transform(spec, &policy, std::path::Path::new("/tmp"), None)?;
//! # Ok::<(), anyhow::Error>(())
//! ```

mod exec_env;
mod manager;
mod permissions;
mod policy;

pub use exec_env::{CommandSpec, ExecEnv, ExecExpiration, SandboxType};
pub use manager::{SandboxManager, SandboxTransformError};
pub use permissions::SandboxPermissions;
pub use policy::{
    BLOCKED_SYSCALLS, DEFAULT_SENSITIVE_PATHS, FILTERED_SYSCALLS, NetworkAllowlistEntry,
    ResourceLimits, SandboxPolicy, SeccompProfile, SensitivePath, WritableRoot,
    default_sensitive_paths,
};
