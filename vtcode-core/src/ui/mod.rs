//! User interface utilities and shared UI components
//!
//! This module contains shared UI functionality including loading indicators,
//! markdown rendering, and terminal utilities.

pub mod diff_renderer;
pub mod iocraft;
pub mod markdown;
pub mod spinner;
pub mod styled;
pub mod terminal;
pub mod theme;
pub mod user_confirmation;

pub use markdown::*;
pub use spinner::*;
pub use styled::*;
pub use terminal::*;
pub use theme::*;

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
