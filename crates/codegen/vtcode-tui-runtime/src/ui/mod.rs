//! User interface utilities and shared UI components.
//!
//! This module owns terminal rendering, theming, and the inline-session
//! protocol. Terminal-agnostic helpers live in `vtcode-commons::search` /
//! `vtcode_commons::terminal_detection` and the slash catalog in
//! `vtcode-skills`, so headless consumers do not pull the TUI stack.

/// Unified diff rendering with ANSI styling and bounded excerpts.
pub mod diff_renderer;
/// Git color configuration parsing.
pub mod git_config;
/// Markdown-to-ANSI rendering for chat output.
pub mod markdown;
/// Styled text helpers.
pub mod styled;
/// Syntax highlighting integration.
pub mod syntax_highlight;
/// Terminal capability detection.
pub mod terminal;
/// Built-in theme definitions and active style accessors.
pub mod theme;
/// Theme configuration file parsing (`.vtcode/theme.toml`).
pub mod theme_config;
/// Theme manager for loading and applying custom themes.
pub mod theme_manager;
/// TUI protocol types and session interface.
pub mod tui;
/// Compatibility layer between core config types and TUI types.
pub mod tui_compat;
/// Global TUI mode flag.
pub mod tui_mode;
/// User confirmation dialogs.
pub mod user_confirmation;

pub use git_config::GitColorConfig;
pub use markdown::*;
pub use styled::*;
pub use terminal::*;
pub use theme::*;
pub use theme_config::ThemeConfig;
pub use theme_manager::ThemeManager;
pub use tui::*;
pub use tui_compat::*;
pub use tui_mode::*;
pub use vtcode_ui::tui::ui::FileColorizer;
