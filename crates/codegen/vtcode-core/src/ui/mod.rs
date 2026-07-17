//! User interface utilities and shared UI components
//!
//! This module contains shared UI functionality including loading indicators,
//! markdown rendering, and terminal utilities.

/// Unified diff rendering with ANSI styling and suppression logic.
pub mod diff_renderer;
/// Git color configuration parsing.
pub mod git_config;
/// Markdown-to-ANSI rendering for chat output.
pub mod markdown;
/// Fuzzy search utilities.
pub mod search;
/// Slash command discovery and suggestion.
pub mod slash;
/// Streaming text buffer for incremental output.
pub mod stream_buffer;
/// Styled text helpers.
pub mod styled;
/// Syntax highlighting integration.
pub mod syntax_highlight;
/// Table formatting utilities.
pub mod table_formatter;
/// Terminal capability detection.
pub mod terminal;
/// Built-in theme definitions and active style accessors.
pub mod theme;
/// Theme configuration file parsing (`.vtcode/theme.toml`).
pub mod theme_config;
/// Theme manager for loading and applying custom themes.
pub mod theme_manager;
/// TUI module re-exports.
pub mod tui;
/// Compatibility layer between core config types and TUI types.
pub mod tui_compat;
/// Global TUI mode flag.
pub mod tui_mode;
/// User confirmation dialogs.
pub mod user_confirmation;

pub use git_config::GitColorConfig;
pub use markdown::*;
pub use search::*;
pub use slash::*;
pub use styled::*;
pub use terminal::*;
pub use theme::*;
pub use theme_config::ThemeConfig;
pub use theme_manager::ThemeManager;
pub use tui::*;
pub use tui_compat::*;
pub use tui_mode::*;
pub use vtcode_ui::tui::ui::FileColorizer;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_markdown() {
        let markdown_text = r#"
# Welcome to VT Code

This is a **bold** statement and this is *italic*.

## Features

- Advanced code analysis
- Multi-language support
- Real-time collaboration
"#;

        // This should not panic
        render_markdown(markdown_text);
    }
}
