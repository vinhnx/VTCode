//! Shared interface definitions bridging the CLI binary and reusable
//! vtcode-core abstractions.
//!
//! The traits exposed here let the binary depend on narrow contracts for
//! driving sessions, interacting with the inline UI session, and servicing ACP
//! transports without directly depending on concrete implementations.

pub mod acp;
pub mod session;
pub mod ui;

pub use acp::{AcpClientAdapter, AcpLaunchParams};
pub use session::{SessionMode, SessionRuntime, SessionRuntimeParams};
pub use ui::UiSession;
