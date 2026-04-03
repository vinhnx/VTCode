//! Shared UI protocol types used across VT Code crates.
//!
//! These types form the data model shared between `vtcode-core` (the agent
//! library) and `vtcode-tui` (the terminal surface). Extracting them here
//! lets headless builds compile without duplicating every enum and struct.
//!
//! The channel protocol types (`InlineCommand`, `InlineHandle`,
//! `InlineSession`) remain in the crate that owns them because the app-layer
//! and core-layer protocols diverge.

mod markdown;
mod selection;
mod style;
mod types;

pub use markdown::*;
pub use selection::*;
pub use style::*;
pub use types::*;
