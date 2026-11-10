//! Compatibility facade for shared theme APIs.

#[cfg(feature = "tui")]
pub use vtcode_ui::theme::*;

#[cfg(not(feature = "tui"))]
mod headless_theme {
    use anstyle::{AnsiColor, Color, Style};

    /// Styles resolved from the active palette.
    ///
    /// Keep field-compatible with `vtcode_ui::theme::ThemeStyles`. Headless
    /// builds have no theme runtime, so [`active_styles`] returns neutral
    /// (unstyled) values.
    #[derive(Clone, Debug)]
    pub struct ThemeStyles {
        pub info: Style,
        pub error: Style,
        pub warning: Style,
        pub output: Style,
        pub response: Style,
        pub reasoning: Style,
        pub tool: Style,
        pub tool_detail: Style,
        pub tool_output: Style,
        pub pty_output: Style,
        pub status: Style,
        pub mcp: Style,
        pub user: Style,
        pub primary: Style,
        pub secondary: Style,
        pub background: Color,
        pub foreground: Color,
    }

    /// Active styles (headless: neutral defaults, no theme runtime).
    pub fn active_styles() -> ThemeStyles {
        ThemeStyles {
            info: Style::default(),
            error: Style::default(),
            warning: Style::default(),
            output: Style::default(),
            response: Style::default(),
            reasoning: Style::default(),
            tool: Style::default(),
            tool_detail: Style::default(),
            tool_output: Style::default(),
            pty_output: Style::default(),
            status: Style::default(),
            mcp: Style::default(),
            user: Style::default(),
            primary: Style::default(),
            secondary: Style::default(),
            background: Color::Ansi(AnsiColor::Black),
            foreground: Color::Ansi(AnsiColor::White),
        }
    }

    /// Active theme id (headless: always the default theme).
    pub fn active_theme_id() -> String {
        vtcode_config::constants::defaults::DEFAULT_THEME.to_string()
    }

    /// Whether a theme id is light (headless: always dark).
    pub fn is_light_theme(_theme_id: &str) -> bool {
        false
    }

    /// Recommended syntax theme for a UI theme (headless: the default ramp,
    /// matching the fallback arm of the TUI implementation).
    pub fn get_syntax_theme_for_ui_theme(_ui_theme: &str) -> &'static str {
        "base16-ocean.dark"
    }
}

#[cfg(not(feature = "tui"))]
pub use headless_theme::*;
