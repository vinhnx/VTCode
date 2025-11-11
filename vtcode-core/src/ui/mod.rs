//! User interface utilities and shared UI components
//!
//! This module contains shared UI functionality including loading indicators,
//! markdown rendering, and terminal utilities.

pub mod diff_renderer;
pub mod file_colorizer;
pub mod git_config;
pub mod markdown;
pub mod search;
pub mod slash;
pub mod styled;
pub mod terminal;
pub mod theme;
pub mod theme_config;
pub mod theme_manager;
pub mod tui;
pub mod user_confirmation;

pub use file_colorizer::FileColorizer;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_markdown() {
        let markdown_text = r#"
# Welcome to VTCode

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
