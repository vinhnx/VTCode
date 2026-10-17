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
//! Compartment topology (sandboxing-basics actor model): the broker
//! (`SandboxManager` + `vtcode sandbox-exec` launcher) owns every grant; each
//! sandboxed child is a leaf worker. Workers never forward capabilities to
//! each other — there is no `SCM_RIGHTS` passing between sandboxed processes,
//! only parent↔child pipes the broker created. File descriptors behave as
//! capabilities except for `ioctl`s, so terminal injection (`TIOCSTI`,
//! `TIOCSCTTY`) is denied in seccomp while general TTY ioctls stay available
//! for PTY sessions. Privileges only ever decrease (`PR_SET_NO_NEW_PRIVS` +
//! Landlock `restrict_self` + seccomp); namespaces are never used as the
//! sandbox mechanism.
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
//! ```rust,ignore
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

mod child_spawn;
mod debug;
mod exec_env;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod linux_seccomp;
mod manager;
mod permissions;
mod policy;

pub use child_spawn::{
    FILTERED_ENV_VARS, PRESERVED_ENV_VARS, VTCODE_SANDBOX_ACTIVE, VTCODE_SANDBOX_NETWORK_DISABLED, VTCODE_SANDBOX_TYPE,
    VTCODE_SANDBOX_WRITABLE_ROOTS, build_sanitized_env, filter_sensitive_env, setup_parent_death_signal,
    should_filter_env_var,
};
pub use debug::{
    DebugSubcommand, SandboxDebugResult, debug_sandbox, sandbox_capabilities_summary, test_network_blocked,
    test_path_writable,
};
pub use exec_env::{CommandSpec, ExecEnv, ExecExpiration, LinuxSandboxLauncher, SandboxType};
#[cfg(target_os = "linux")]
pub use linux::{apply_sandbox_restrictions, landlock_supported};
pub use manager::{SandboxManager, SandboxTransformError};
pub use permissions::{AdditionalPermissions, SandboxPermissions, ValidatedSandboxRequest};
pub use policy::{
    BLOCKED_SYSCALLS, DEFAULT_SENSITIVE_PATHS, FILTERED_SYSCALLS, NetworkAllowlistEntry, ResourceLimits,
    SECCOMP_PROFILE_VERSION, SandboxPolicy, SeccompProfile, SensitivePath, WritableRoot, default_sensitive_paths,
};
