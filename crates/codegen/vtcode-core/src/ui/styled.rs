//! Styled terminal output helpers.
//!
//! `Styles` is re-exported from `vtcode-commons::styling`.
//! Print convenience functions remain here because they depend on `anstream`.

use anstream::println as styled_println;
use anstyle::Style;

pub use vtcode_commons::styling::Styles;

/// Print a styled error message
pub fn error(message: &str) {
    styled_println!("{}{}{}", Styles::render(&Styles::error()), message, Styles::render_reset());
}

/// Print a styled warning message
pub fn warning(message: &str) {
    styled_println!("{}{}{}", Styles::render(&Styles::warning()), message, Styles::render_reset());
}

/// Print a styled success message
pub fn success(message: &str) {
    styled_println!("{}{}{}", Styles::render(&Styles::success()), message, Styles::render_reset());
}

/// Print a styled info message
pub fn info(message: &str) {
    styled_println!("{}{}{}", Styles::render(&Styles::info()), message, Styles::render_reset());
}

/// Print a styled debug message
pub fn debug(message: &str) {
    styled_println!("{}{}{}", Styles::render(&Styles::debug()), message, Styles::render_reset());
}

/// Print a styled bold message
pub fn bold(message: &str) {
    styled_println!("{}{}{}", Styles::render(&Styles::bold()), message, Styles::render_reset());
}

/// Print a styled message with custom style
pub fn styled(style: &Style, message: &str) {
    styled_println!("{}{}{}", Styles::render(style), message, Styles::render_reset());
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
