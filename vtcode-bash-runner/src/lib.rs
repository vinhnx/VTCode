//! Cross-platform command runner modeled after VT Code's original bash
//! wrapper. The crate exposes a trait-based executor so downstream
//! applications can swap the underlying process strategy (system shell,
//! pure-Rust emulation, or dry-run logging) while reusing the higher-level
//! helpers for workspace-safe filesystem manipulation.
//!
//! ## Modules
//!
//! - [`executor`] - Command execution strategies (process, dry-run, pure-rust)
//! - [`runner`] - High-level `BashRunner` for workspace-safe operations
//! - [`background`] - Background task management
//! - [`pipe`] - Async pipe-based process spawning with unified handles
//! - [`process`] - Process handle types for PTY and pipe backends
//! - [`process_group`] - Process group management for reliable cleanup
//! - [`stream`] - Stream utilities for reading output

pub mod background;
pub mod executor;
pub mod pipe;
pub mod policy;
pub mod process;
pub mod process_group;
pub mod runner;
pub mod stream;

// Background task management
pub use background::{BackgroundCommandManager, BackgroundTaskHandle, BackgroundTaskStatus};

// Executor variants
#[cfg(feature = "dry-run")]
pub use executor::DryRunCommandExecutor;
#[cfg(feature = "exec-events")]
pub use executor::EventfulExecutor;
#[cfg(feature = "pure-rust")]
pub use executor::PureRustCommandExecutor;
pub use executor::{
    CommandCategory, CommandExecutor, CommandInvocation, CommandOutput, CommandStatus,
    ProcessCommandExecutor, ShellKind,
};

// Policy types
pub use policy::{AllowAllPolicy, CommandPolicy, WorkspaceGuardPolicy};

// Runner
pub use runner::BashRunner;

// Stream utilities
pub use stream::{ReadLineResult, read_line_with_limit};

// Pipe-based process spawning (codex-rs compatible)
pub use pipe::{
    PipeSpawnOptions, PipeStdinMode, spawn_process as spawn_pipe_process,
    spawn_process_no_stdin as spawn_pipe_process_no_stdin,
    spawn_process_with_options as spawn_pipe_process_with_options,
};

// Process handle types (unified interface for PTY and pipe)
pub use process::{
    ChildTerminator, ExecCommandSession, ProcessHandle, PtyHandles, SpawnedProcess, SpawnedPty,
    collect_output_until_exit,
};

// Process group utilities
pub use process_group::{
    DEFAULT_GRACEFUL_TIMEOUT_MS, GracefulTerminationResult, KillSignal, detach_from_tty,
    graceful_kill_process_group, graceful_kill_process_group_default, kill_child_process_group,
    kill_child_process_group_with_signal, kill_process_group, kill_process_group_by_pid,
    kill_process_group_by_pid_with_signal, kill_process_group_with_signal, set_parent_death_signal,
    set_process_group,
};

#[cfg(windows)]
pub use process_group::kill_process;
