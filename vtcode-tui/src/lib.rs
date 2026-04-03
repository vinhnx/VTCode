//! `vtcode-tui` exposes inline terminal UI APIs as a reusable crate.
//!
//! The migrated implementation source lives in `src/core_tui/`.
//! Public API is exported directly from this crate.

#[allow(dead_code)]
mod cache;
#[allow(dead_code)]
mod config;
mod options;
pub mod ui;
pub mod utils;

pub mod app;
pub mod core;
pub mod core_tui;
pub mod host;
mod session_options;

pub use config::SyntaxHighlightingConfig as TuiSyntaxHighlightingConfig;
pub use config::{KeyboardProtocolConfig, ReasoningEffortLevel, UiSurfacePreference};
pub use core_tui::{log, panic_hook};
pub use options::{FullscreenInteractionSettings, KeyboardProtocolSettings, SessionSurface};
pub use ui::theme::{ThemeSuite, available_theme_suites, theme_suite_id, theme_suite_label};

/// Commonly used TUI API items.
pub mod prelude {
    pub use crate::app::prelude::*;
    pub use crate::core::prelude::*;
}
