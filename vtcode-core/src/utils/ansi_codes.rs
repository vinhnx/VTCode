/// ANSI escape sequence constants and utilities
///
/// This module provides constants and helper functions for working with ANSI escape sequences.
/// For complete reference, see `docs/reference/ansi-escape-sequences.md`
///
/// # Performance
///
/// - All constants are `&'static str` for zero-cost abstraction
/// - Helper functions are marked `#[inline]` for optimal performance
/// - Zero-allocation variants available for hot paths
///
/// # Examples
///
/// ```
/// use vtcode_core::utils::ansi_codes::*;
///
/// // Basic usage
/// let error = colored("Error", FG_RED);
/// let success = colored("Success", FG_GREEN);
///
/// // Semantic colors
/// use vtcode_core::utils::ansi_codes::semantic::*;
/// let warning = colored("Warning", WARNING);
///
/// // Display width (for alignment)
/// let width = display_width("\x1b[31mHello\x1b[0m"); // Returns 5
/// ```

/// Escape character (ESC = 0x1B = 27)
pub const ESC: &str = "\x1b";

/// Control Sequence Introducer (CSI = ESC[)
pub const CSI: &str = "\x1b[";

/// Operating System Command (OSC = ESC])
pub const OSC: &str = "\x1b]";

/// Device Control String (DCS = ESC P)
pub const DCS: &str = "\x1bP";

/// String Terminator (ST = ESC \)
pub const ST: &str = "\x1b\\";

/// Bell character (BEL = 0x07)
pub const BEL: &str = "\x07";

// === Reset ===
/// Reset all attributes (ESC[0m)
pub const RESET: &str = "\x1b[0m";

// === Text Styles ===
/// Bold text (ESC[1m)
pub const BOLD: &str = "\x1b[1m";

/// Dim/faint text (ESC[2m)
pub const DIM: &str = "\x1b[2m";

/// Italic text (ESC[3m)
pub const ITALIC: &str = "\x1b[3m";

/// Underlined text (ESC[4m)
pub const UNDERLINE: &str = "\x1b[4m";

/// Blinking text (ESC[5m)
pub const BLINK: &str = "\x1b[5m";

/// Reverse/inverse colors (ESC[7m)
pub const REVERSE: &str = "\x1b[7m";

/// Hidden/invisible text (ESC[8m)
pub const HIDDEN: &str = "\x1b[8m";

/// Strikethrough text (ESC[9m)
pub const STRIKETHROUGH: &str = "\x1b[9m";

/// Reset bold/dim (ESC[22m)
pub const RESET_BOLD_DIM: &str = "\x1b[22m";

/// Reset italic (ESC[23m)
pub const RESET_ITALIC: &str = "\x1b[23m";

/// Reset underline (ESC[24m)
pub const RESET_UNDERLINE: &str = "\x1b[24m";

/// Reset blink (ESC[25m)
pub const RESET_BLINK: &str = "\x1b[25m";

/// Reset reverse (ESC[27m)
pub const RESET_REVERSE: &str = "\x1b[27m";

/// Reset hidden (ESC[28m)
pub const RESET_HIDDEN: &str = "\x1b[28m";

/// Reset strikethrough (ESC[29m)
pub const RESET_STRIKETHROUGH: &str = "\x1b[29m";

// === Foreground Colors (30-37) ===
/// Black foreground (ESC[30m)
pub const FG_BLACK: &str = "\x1b[30m";

/// Red foreground (ESC[31m)
pub const FG_RED: &str = "\x1b[31m";

/// Green foreground (ESC[32m)
pub const FG_GREEN: &str = "\x1b[32m";

/// Yellow foreground (ESC[33m)
pub const FG_YELLOW: &str = "\x1b[33m";

/// Blue foreground (ESC[34m)
pub const FG_BLUE: &str = "\x1b[34m";

/// Magenta foreground (ESC[35m)
pub const FG_MAGENTA: &str = "\x1b[35m";

/// Cyan foreground (ESC[36m)
pub const FG_CYAN: &str = "\x1b[36m";

/// White foreground (ESC[37m)
pub const FG_WHITE: &str = "\x1b[37m";

/// Default foreground (ESC[39m)
pub const FG_DEFAULT: &str = "\x1b[39m";

// === Background Colors (40-47) ===
/// Black background (ESC[40m)
pub const BG_BLACK: &str = "\x1b[40m";

/// Red background (ESC[41m)
pub const BG_RED: &str = "\x1b[41m";

/// Green background (ESC[42m)
pub const BG_GREEN: &str = "\x1b[42m";

/// Yellow background (ESC[43m)
pub const BG_YELLOW: &str = "\x1b[43m";

/// Blue background (ESC[44m)
pub const BG_BLUE: &str = "\x1b[44m";

/// Magenta background (ESC[45m)
pub const BG_MAGENTA: &str = "\x1b[45m";

/// Cyan background (ESC[46m)
pub const BG_CYAN: &str = "\x1b[46m";

/// White background (ESC[47m)
pub const BG_WHITE: &str = "\x1b[47m";

/// Default background (ESC[49m)
pub const BG_DEFAULT: &str = "\x1b[49m";

// === Bright Foreground Colors (90-97) ===
/// Bright black foreground (ESC[90m)
pub const FG_BRIGHT_BLACK: &str = "\x1b[90m";

/// Bright red foreground (ESC[91m)
pub const FG_BRIGHT_RED: &str = "\x1b[91m";

/// Bright green foreground (ESC[92m)
pub const FG_BRIGHT_GREEN: &str = "\x1b[92m";

/// Bright yellow foreground (ESC[93m)
pub const FG_BRIGHT_YELLOW: &str = "\x1b[93m";

/// Bright blue foreground (ESC[94m)
pub const FG_BRIGHT_BLUE: &str = "\x1b[94m";

/// Bright magenta foreground (ESC[95m)
pub const FG_BRIGHT_MAGENTA: &str = "\x1b[95m";

/// Bright cyan foreground (ESC[96m)
pub const FG_BRIGHT_CYAN: &str = "\x1b[96m";

/// Bright white foreground (ESC[97m)
pub const FG_BRIGHT_WHITE: &str = "\x1b[97m";

// === Bright Background Colors (100-107) ===
/// Bright black background (ESC[100m)
pub const BG_BRIGHT_BLACK: &str = "\x1b[100m";

/// Bright red background (ESC[101m)
pub const BG_BRIGHT_RED: &str = "\x1b[101m";

/// Bright green background (ESC[102m)
pub const BG_BRIGHT_GREEN: &str = "\x1b[102m";

/// Bright yellow background (ESC[103m)
pub const BG_BRIGHT_YELLOW: &str = "\x1b[103m";

/// Bright blue background (ESC[104m)
pub const BG_BRIGHT_BLUE: &str = "\x1b[104m";

/// Bright magenta background (ESC[105m)
pub const BG_BRIGHT_MAGENTA: &str = "\x1b[105m";

/// Bright cyan background (ESC[106m)
pub const BG_BRIGHT_CYAN: &str = "\x1b[106m";

/// Bright white background (ESC[107m)
pub const BG_BRIGHT_WHITE: &str = "\x1b[107m";

// === Cursor Control ===
/// Move cursor to home position (0,0) - ESC[H
pub const CURSOR_HOME: &str = "\x1b[H";

/// Hide cursor (ESC[?25l)
pub const CURSOR_HIDE: &str = "\x1b[?25l";

/// Show cursor (ESC[?25h)
pub const CURSOR_SHOW: &str = "\x1b[?25h";

/// Save cursor position (DEC) - ESC 7
pub const CURSOR_SAVE_DEC: &str = "\x1b7";

/// Restore cursor position (DEC) - ESC 8
pub const CURSOR_RESTORE_DEC: &str = "\x1b8";

/// Save cursor position (SCO) - ESC[s
pub const CURSOR_SAVE_SCO: &str = "\x1b[s";

/// Restore cursor position (SCO) - ESC[u
pub const CURSOR_RESTORE_SCO: &str = "\x1b[u";

// === Erase Functions ===
/// Clear entire screen (ESC[2J)
pub const CLEAR_SCREEN: &str = "\x1b[2J";

/// Clear from cursor to end of screen (ESC[0J or ESC[J)
pub const CLEAR_TO_END_OF_SCREEN: &str = "\x1b[0J";

/// Clear from cursor to beginning of screen (ESC[1J)
pub const CLEAR_TO_START_OF_SCREEN: &str = "\x1b[1J";

/// Clear saved lines (ESC[3J)
pub const CLEAR_SAVED_LINES: &str = "\x1b[3J";

/// Clear entire line (ESC[2K)
pub const CLEAR_LINE: &str = "\x1b[2K";

/// Clear from cursor to end of line (ESC[0K or ESC[K)
pub const CLEAR_TO_END_OF_LINE: &str = "\x1b[0K";

/// Clear from cursor to start of line (ESC[1K)
pub const CLEAR_TO_START_OF_LINE: &str = "\x1b[1K";

// === Screen Modes ===
/// Enable alternative buffer (ESC[?1049h)
pub const ALT_BUFFER_ENABLE: &str = "\x1b[?1049h";

/// Disable alternative buffer (ESC[?1049l)
pub const ALT_BUFFER_DISABLE: &str = "\x1b[?1049l";

/// Save screen (ESC[?47h)
pub const SCREEN_SAVE: &str = "\x1b[?47h";

/// Restore screen (ESC[?47l)
pub const SCREEN_RESTORE: &str = "\x1b[?47l";

/// Enable line wrapping (ESC[=7h)
pub const LINE_WRAP_ENABLE: &str = "\x1b[=7h";

/// Disable line wrapping (ESC[=7l)
pub const LINE_WRAP_DISABLE: &str = "\x1b[=7l";

// === Helper Functions ===

/// Create a cursor movement sequence to move up N lines
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::cursor_up;
/// assert_eq!(cursor_up(5), "\x1b[5A");
/// ```
#[inline]
pub fn cursor_up(n: u16) -> String {
    format!("\x1b[{}A", n)
}

/// Create a cursor movement sequence to move down N lines
#[inline]
pub fn cursor_down(n: u16) -> String {
    format!("\x1b[{}B", n)
}

/// Create a cursor movement sequence to move right N columns
#[inline]
pub fn cursor_right(n: u16) -> String {
    format!("\x1b[{}C", n)
}

/// Create a cursor movement sequence to move left N columns
#[inline]
pub fn cursor_left(n: u16) -> String {
    format!("\x1b[{}D", n)
}

/// Create a cursor movement sequence to move to specific row and column
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::cursor_to;
/// assert_eq!(cursor_to(10, 20), "\x1b[10;20H");
/// ```
#[inline]
pub fn cursor_to(row: u16, col: u16) -> String {
    format!("\x1b[{};{}H", row, col)
}

/// Create a 256-color foreground sequence
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::fg_256;
/// assert_eq!(fg_256(196), "\x1b[38;5;196m"); // Bright red
/// ```
#[inline]
pub fn fg_256(color_id: u8) -> String {
    format!("\x1b[38;5;{}m", color_id)
}

/// Create a 256-color background sequence
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::bg_256;
/// assert_eq!(bg_256(21), "\x1b[48;5;21m"); // Blue
/// ```
#[inline]
pub fn bg_256(color_id: u8) -> String {
    format!("\x1b[48;5;{}m", color_id)
}

/// Create an RGB foreground color sequence
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::fg_rgb;
/// assert_eq!(fg_rgb(255, 128, 0), "\x1b[38;2;255;128;0m"); // Orange
/// ```
#[inline]
pub fn fg_rgb(r: u8, g: u8, b: u8) -> String {
    format!("\x1b[38;2;{};{};{}m", r, g, b)
}

/// Create an RGB background color sequence
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::bg_rgb;
/// assert_eq!(bg_rgb(0, 128, 255), "\x1b[48;2;0;128;255m"); // Light blue
/// ```
#[inline]
pub fn bg_rgb(r: u8, g: u8, b: u8) -> String {
    format!("\x1b[48;2;{};{};{}m", r, g, b)
}

/// Create a styled text with foreground color and reset
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::colored;
/// assert_eq!(colored("Error", "\x1b[31m"), "\x1b[31mError\x1b[0m");
/// ```
#[inline]
pub fn colored(text: &str, color: &str) -> String {
    format!("{}{}{}", color, text, RESET)
}

/// Create bold text
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::bold;
/// assert_eq!(bold("Important"), "\x1b[1mImportant\x1b[22m");
/// ```
#[inline]
pub fn bold(text: &str) -> String {
    format!("{}{}{}", BOLD, text, RESET_BOLD_DIM)
}

/// Create italic text
#[inline]
pub fn italic(text: &str) -> String {
    format!("{}{}{}", ITALIC, text, RESET_ITALIC)
}

/// Create underlined text
#[inline]
pub fn underline(text: &str) -> String {
    format!("{}{}{}", UNDERLINE, text, RESET_UNDERLINE)
}

/// Create dimmed text
#[inline]
pub fn dim(text: &str) -> String {
    format!("{}{}{}", DIM, text, RESET_BOLD_DIM)
}

/// Combine multiple styles
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::{combine_styles, BOLD, FG_RED};
/// let styled = combine_styles("Error", &[BOLD, FG_RED]);
/// assert!(styled.contains("Error"));
/// ```
#[inline]
pub fn combine_styles(text: &str, styles: &[&str]) -> String {
    let mut result = String::with_capacity(text.len() + styles.len() * 10);
    for style in styles {
        result.push_str(style);
    }
    result.push_str(text);
    result.push_str(RESET);
    result
}

// === Phase 1 Improvements ===

/// Semantic color constants for common use cases
pub mod semantic {
    use super::*;

    /// Error/danger color (bright red)
    pub const ERROR: &str = FG_BRIGHT_RED;

    /// Success color (bright green)
    pub const SUCCESS: &str = FG_BRIGHT_GREEN;

    /// Warning color (bright yellow)
    pub const WARNING: &str = FG_BRIGHT_YELLOW;

    /// Info color (bright cyan)
    pub const INFO: &str = FG_BRIGHT_CYAN;

    /// Muted/secondary text (dim)
    pub const MUTED: &str = DIM;

    /// Emphasis (bold)
    pub const EMPHASIS: &str = BOLD;

    /// Debug/trace color (gray)
    pub const DEBUG: &str = FG_BRIGHT_BLACK;
}

/// Check if text contains any ANSI escape sequences
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::contains_ansi;
/// assert!(contains_ansi("\x1b[31mRed\x1b[0m"));
/// assert!(!contains_ansi("Plain text"));
/// ```
#[inline]
pub fn contains_ansi(text: &str) -> bool {
    text.contains(ESC)
}

/// Check if text starts with an ANSI escape sequence
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::starts_with_ansi;
/// assert!(starts_with_ansi("\x1b[31mRed"));
/// assert!(!starts_with_ansi("Plain\x1b[31m"));
/// ```
#[inline]
pub fn starts_with_ansi(text: &str) -> bool {
    text.starts_with(ESC)
}

/// Check if text ends with an ANSI escape sequence
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::ends_with_ansi;
/// assert!(ends_with_ansi("Text\x1b[0m"));
/// assert!(!ends_with_ansi("\x1b[31mText"));
/// ```
#[inline]
pub fn ends_with_ansi(text: &str) -> bool {
    text.ends_with('m') && text.contains(ESC)
}

/// Get the display width of text (length without ANSI codes)
///
/// This is useful for aligning text that contains ANSI sequences.
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::display_width;
/// assert_eq!(display_width("\x1b[31mHello\x1b[0m"), 5);
/// assert_eq!(display_width("Hello"), 5);
/// ```
#[inline]
pub fn display_width(text: &str) -> usize {
    crate::utils::ansi_parser::strip_ansi(text).len()
}

/// Pad text to a specific display width, preserving ANSI codes
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::pad_to_width;
/// let text = "\x1b[31mHi\x1b[0m";
/// let padded = pad_to_width(text, 5, ' ');
/// assert_eq!(display_width(&padded), 5);
/// ```
pub fn pad_to_width(text: &str, width: usize, pad_char: char) -> String {
    let current_width = display_width(text);
    if current_width >= width {
        text.to_string()
    } else {
        let padding = pad_char.to_string().repeat(width - current_width);
        format!("{}{}", text, padding)
    }
}

/// Truncate text to a maximum display width, preserving ANSI codes
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::truncate_to_width;
/// let text = "\x1b[31mHello World\x1b[0m";
/// let truncated = truncate_to_width(text, 5, "...");
/// assert!(display_width(&truncated) <= 8); // 5 + "..."
/// ```
pub fn truncate_to_width(text: &str, max_width: usize, ellipsis: &str) -> String {
    let stripped = crate::utils::ansi_parser::strip_ansi(text);
    if stripped.len() <= max_width {
        return text.to_string();
    }

    // Simple truncation - preserve ANSI codes at start
    let truncate_at = max_width.saturating_sub(ellipsis.len());
    let truncated_plain: String = stripped.chars().take(truncate_at).collect();
    
    // Try to preserve leading ANSI codes
    if starts_with_ansi(text) {
        // Find where the actual text starts
        let mut ansi_prefix = String::new();
        for ch in text.chars() {
            ansi_prefix.push(ch);
            if ch == '\x1b' {
                // Start of escape sequence
                continue;
            }
            if ch.is_alphabetic() && ansi_prefix.contains('\x1b') {
                // End of escape sequence
                break;
            }
        }
        format!("{}{}{}{}", ansi_prefix, truncated_plain, ellipsis, RESET)
    } else {
        format!("{}{}", truncated_plain, ellipsis)
    }
}

/// Write ANSI-styled text directly to a writer (zero allocation)
///
/// This is more efficient than creating a String when writing to stdout/stderr.
///
/// # Example
/// ```no_run
/// use vtcode_core::utils::ansi_codes::{write_styled, FG_RED};
/// use std::io::stdout;
///
/// let mut out = stdout();
/// write_styled(&mut out, "Error", FG_RED).unwrap();
/// ```
#[inline]
pub fn write_styled<W: std::io::Write>(
    writer: &mut W,
    text: &str,
    style: &str,
) -> std::io::Result<()> {
    writer.write_all(style.as_bytes())?;
    writer.write_all(text.as_bytes())?;
    writer.write_all(RESET.as_bytes())?;
    Ok(())
}

/// Format ANSI-styled text into an existing string buffer
///
/// This avoids allocating a new String when you already have a buffer.
///
/// # Example
/// ```
/// use vtcode_core::utils::ansi_codes::{format_styled_into, FG_GREEN};
///
/// let mut buffer = String::with_capacity(100);
/// format_styled_into(&mut buffer, "Success", FG_GREEN);
/// assert!(buffer.contains("Success"));
/// ```
#[inline]
pub fn format_styled_into(buffer: &mut String, text: &str, style: &str) {
    buffer.push_str(style);
    buffer.push_str(text);
    buffer.push_str(RESET);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_movement() {
        assert_eq!(cursor_up(5), "\x1b[5A");
        assert_eq!(cursor_down(10), "\x1b[10B");
        assert_eq!(cursor_right(3), "\x1b[3C");
        assert_eq!(cursor_left(7), "\x1b[7D");
    }

    #[test]
    fn test_cursor_position() {
        assert_eq!(cursor_to(10, 20), "\x1b[10;20H");
        assert_eq!(cursor_to(1, 1), "\x1b[1;1H");
    }

    #[test]
    fn test_256_colors() {
        assert_eq!(fg_256(196), "\x1b[38;5;196m");
        assert_eq!(bg_256(21), "\x1b[48;5;21m");
    }

    #[test]
    fn test_rgb_colors() {
        assert_eq!(fg_rgb(255, 128, 0), "\x1b[38;2;255;128;0m");
        assert_eq!(bg_rgb(0, 128, 255), "\x1b[48;2;0;128;255m");
    }

    #[test]
    fn test_colored_text() {
        assert_eq!(colored("Error", FG_RED), "\x1b[31mError\x1b[0m");
        assert_eq!(colored("Success", FG_GREEN), "\x1b[32mSuccess\x1b[0m");
    }

    #[test]
    fn test_styled_text() {
        assert_eq!(bold("Bold"), "\x1b[1mBold\x1b[22m");
        assert_eq!(italic("Italic"), "\x1b[3mItalic\x1b[23m");
        assert_eq!(underline("Underline"), "\x1b[4mUnderline\x1b[24m");
        assert_eq!(dim("Dim"), "\x1b[2mDim\x1b[22m");
    }

    #[test]
    fn test_combine_styles() {
        let styled = combine_styles("Text", &[BOLD, FG_RED]);
        assert!(styled.contains("Text"));
        assert!(styled.contains("\x1b[1m"));
        assert!(styled.contains("\x1b[31m"));
        assert!(styled.ends_with("\x1b[0m"));
    }

    #[test]
    fn test_constants() {
        assert_eq!(ESC, "\x1b");
        assert_eq!(CSI, "\x1b[");
        assert_eq!(RESET, "\x1b[0m");
        assert_eq!(CURSOR_HIDE, "\x1b[?25l");
        assert_eq!(CURSOR_SHOW, "\x1b[?25h");
        assert_eq!(CLEAR_SCREEN, "\x1b[2J");
        assert_eq!(CLEAR_LINE, "\x1b[2K");
    }

    #[test]
    fn test_semantic_colors() {
        assert_eq!(semantic::ERROR, FG_BRIGHT_RED);
        assert_eq!(semantic::SUCCESS, FG_BRIGHT_GREEN);
        assert_eq!(semantic::WARNING, FG_BRIGHT_YELLOW);
        assert_eq!(semantic::INFO, FG_BRIGHT_CYAN);
    }

    #[test]
    fn test_contains_ansi() {
        assert!(contains_ansi("\x1b[31mRed\x1b[0m"));
        assert!(contains_ansi("Text\x1b[0m"));
        assert!(!contains_ansi("Plain text"));
    }

    #[test]
    fn test_starts_with_ansi() {
        assert!(starts_with_ansi("\x1b[31mRed"));
        assert!(!starts_with_ansi("Plain\x1b[31m"));
    }

    #[test]
    fn test_ends_with_ansi() {
        assert!(ends_with_ansi("Text\x1b[0m"));
        assert!(!ends_with_ansi("\x1b[31mText"));
    }

    #[test]
    fn test_display_width() {
        assert_eq!(display_width("\x1b[31mHello\x1b[0m"), 5);
        assert_eq!(display_width("Hello"), 5);
        assert_eq!(display_width("\x1b[1;32mbold green\x1b[0m"), 10);
    }

    #[test]
    fn test_pad_to_width() {
        let text = "\x1b[31mHi\x1b[0m";
        let padded = pad_to_width(text, 5, ' ');
        assert_eq!(display_width(&padded), 5);
        assert!(padded.starts_with("\x1b[31m"));
    }

    #[test]
    fn test_truncate_to_width() {
        let text = "\x1b[31mHello World\x1b[0m";
        let truncated = truncate_to_width(text, 5, "...");
        let width = display_width(&truncated);
        assert!(width <= 8, "Width {} should be <= 8", width);
    }

    #[test]
    fn test_format_styled_into() {
        let mut buffer = String::new();
        format_styled_into(&mut buffer, "Test", FG_RED);
        assert_eq!(buffer, "\x1b[31mTest\x1b[0m");
    }

    #[test]
    fn test_ansi_roundtrip() {
        let original = "Hello, World!";
        let styled = colored(original, FG_RED);
        let stripped = crate::utils::ansi_parser::strip_ansi(&styled);
        assert_eq!(stripped, original);
    }
}
