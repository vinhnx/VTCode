//! `vtcode-tui` exposes inline terminal UI APIs as a reusable crate.
//!
//! The migrated implementation source lives in `src/core_tui/`.
//! Public API is exported directly from this crate.

#[allow(dead_code)]
mod cache;
#[allow(dead_code)]
mod config;
pub mod ui;
pub mod utils;

pub mod core_tui;
pub mod host;
mod session_options;

pub use config::SyntaxHighlightingConfig as TuiSyntaxHighlightingConfig;
pub use config::{KeyboardProtocolConfig, ReasoningEffortLevel, UiSurfacePreference};
pub use core_tui::session::config::AppearanceConfig as SessionAppearanceConfig;
pub use core_tui::*;
pub use session_options::{
    KeyboardProtocolSettings, SessionOptions, SessionSurface, spawn_session_with_host,
    spawn_session_with_options,
};
pub use ui::theme::{ThemeSuite, available_theme_suites, theme_suite_id, theme_suite_label};

/// Commonly used TUI API items.
pub mod prelude {
    pub use crate::{
        EditingMode, InlineCommand, InlineEvent, InlineHandle, InlineMessageKind, InlineSegment,
        InlineSession, InlineTextStyle, InlineTheme, KeyboardProtocolSettings,
        PlanConfirmationResult, PlanContent, PlanPhase, PlanStep, SecurePromptConfig,
        SessionAppearanceConfig, SessionOptions, SessionSurface, SlashCommandItem, TrustMode,
        WizardModalMode, WizardStep, available_theme_suites, convert_style, spawn_session,
        spawn_session_with_host, spawn_session_with_options, spawn_session_with_prompts,
        spawn_session_with_prompts_and_options, theme_from_styles, theme_suite_id,
        theme_suite_label,
    };
}
