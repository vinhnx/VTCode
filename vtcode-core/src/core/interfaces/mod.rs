//! Shared interface definitions bridging the CLI binary and reusable
//! vtcode-core abstractions.
//!
//! The traits exposed here let the binary depend on narrow contracts for
//! driving turns, interacting with the inline UI session, and servicing ACP
//! transports without directly depending on concrete implementations.

pub mod acp;
pub mod turn;
pub mod ui;

pub use acp::{AcpClientAdapter, AcpLaunchParams};
pub use turn::{TurnDriver, TurnDriverParams};
pub use ui::UiSession;
