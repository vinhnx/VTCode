use anstream::println as styled_println;
use anstyle::{AnsiColor, Color, Effects, Reset, Style};

/// Style presets for consistent UI theming
pub struct Styles;

impl Styles {
    /// Error message style (red)
    pub fn error() -> Style {
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red)))
    }

    /// Warning message style (red)
    pub fn warning() -> Style {
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red)))
    }

    /// Success message style (green)
    pub fn success() -> Style {
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)))
    }

    /// Info message style (cyan)
    pub fn info() -> Style {
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan)))
    }

    /// Debug message style (cyan)
    pub fn debug() -> Style {
        Style::new()
            .fg_color(Some(Color::Ansi(AnsiColor::Cyan)))
            .dimmed()
    }

    /// Bold text style
    pub fn bold() -> Style {
        Style::new().effects(Effects::BOLD)
    }

    /// Bold error style
    pub fn bold_error() -> Style {
        Self::error().bold()
    }

    /// Bold success style
    pub fn bold_success() -> Style {
        Self::success().bold()
    }

    /// Bold warning style
    pub fn bold_warning() -> Style {
        Self::warning().bold()
    }

    /// Header style (bold)
    pub fn header() -> Style {
        Style::new().effects(Effects::BOLD)
    }

    /// Code style (magenta)
    pub fn code() -> Style {
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Magenta)))
    }

    /// Render style to ANSI string
    pub fn render(style: &Style) -> String {
        style.to_string()
    }

    /// Render reset ANSI string
    pub fn render_reset() -> String {
        Reset.to_string()
    }
}

/// Print a styled error message
pub fn error(message: &str) {
    styled_println!(
        "{}{}{}",
        Styles::render(&Styles::error()),
        message,
        Styles::render_reset()
    );
}

/// Print a styled warning message
pub fn warning(message: &str) {
    styled_println!(
        "{}{}{}",
        Styles::render(&Styles::warning()),
        message,
        Styles::render_reset()
    );
}

/// Print a styled success message
pub fn success(message: &str) {
    styled_println!(
        "{}{}{}",
        Styles::render(&Styles::success()),
        message,
        Styles::render_reset()
    );
}

/// Print a styled info message
pub fn info(message: &str) {
    styled_println!(
        "{}{}{}",
        Styles::render(&Styles::info()),
        message,
        Styles::render_reset()
    );
}

/// Print a styled debug message
pub fn debug(message: &str) {
    styled_println!(
        "{}{}{}",
        Styles::render(&Styles::debug()),
        message,
        Styles::render_reset()
    );
}

/// Print a styled bold message
pub fn bold(message: &str) {
    styled_println!(
        "{}{}{}",
        Styles::render(&Styles::bold()),
        message,
        Styles::render_reset()
    );
}

/// Print a styled message with custom style
pub fn styled(style: &Style, message: &str) {
    styled_println!(
        "{}{}{}",
        Styles::render(style),
        message,
        Styles::render_reset()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_styles() {
        // These should not panic
        error("Test error");
        warning("Test warning");
        success("Test success");
        info("Test info");
        debug("Test debug");
        bold("Test bold");
        styled(&Styles::header(), "Test custom");
    }
}
