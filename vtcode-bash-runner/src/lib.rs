//! Cross-platform command runner modeled after VTCode's original bash
//! wrapper. The crate exposes a trait-based executor so downstream
//! applications can swap the underlying process strategy (system shell,
//! pure-Rust emulation, or dry-run logging) while reusing the higher-level
//! helpers for workspace-safe filesystem manipulation.

pub mod executor;
pub mod policy;
pub mod runner;

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
pub use policy::{AllowAllPolicy, CommandPolicy, WorkspaceGuardPolicy};
pub use runner::BashRunner;
