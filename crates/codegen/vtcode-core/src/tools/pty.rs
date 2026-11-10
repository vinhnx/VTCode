//! PTY session management.
//!
//! The PTY implementation only needs `portable-pty`, so it compiles
//! identically in TUI and headless builds with no feature gating.

mod command_utils;
mod formatting;
mod manager;
mod manager_utils;
mod preview;
mod screen_backend;
mod scrollback;
mod session;
mod types;

pub use command_utils::{is_cargo_command, is_cargo_command_string, is_development_toolchain_command};
pub use manager::PtyManager;
pub use portable_pty::PtySize;
pub use preview::PtyPreviewRenderer;
pub use types::{PtyCommandRequest, PtyCommandResult, PtyOutputCallback};

/// Bounded-memory metadata for a PTY session's complete output spool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PtyOutputStats {
    pub total_bytes: u64,
    pub truncated: bool,
    pub spool_path: String,
    pub spool_available: bool,
    pub spool_complete: bool,
    pub spool_integrity: Option<crate::tools::output_spooler::SpoolIntegrity>,
}

/// Termination behavior used when closing a PTY session.
///
/// `Graceful` writes `exit\n` and gives the child a SIGTERM grace window so
/// in-session closes let it clean up. `Immediate` goes straight to a process-
/// group SIGKILL: user-initiated program exit must not park the terminal on a
/// 500 ms grace window per live session when the OS reaps any remainder at
/// process exit anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyCloseMode {
    Graceful,
    Immediate,
}
