//! Shared design constants: ellipses, layout breakpoints, spacing tokens.
//!
//! These constants are the single source of truth for UI values that were
//! previously scattered across multiple crates.

/// Unicode ellipsis character as a string (`\u{2026}`).
pub const ELLIPSIS: &str = "\u{2026}";

/// Unicode ellipsis character.
pub const ELLIPSIS_CHAR: char = '\u{2026}';

/// Three-dot ASCII fallback ellipsis.
pub const ELLIPSIS_ASCII: &str = "...";

/// Alias used by preview contexts. Delegates to [`ELLIPSIS`].
pub const INLINE_PREVIEW_ELLIPSIS: &str = ELLIPSIS;

// ── Layout breakpoints ──────────────────────────────────────────────────────

/// Maximum column width for compact layout mode.
pub const COMPACT_MAX_COLS: u16 = 80;

/// Maximum row height for compact layout mode.
pub const COMPACT_MAX_ROWS: u16 = 20;

/// Minimum column width for wide layout mode.
pub const WIDE_MIN_COLS: u16 = 120;

/// Minimum row height for wide layout mode.
pub const WIDE_MIN_ROWS: u16 = 24;

// ── Spacing tokens ──────────────────────────────────────────────────────────

/// Tight spacing (1 cell).
pub const SPACING_TIGHT: u16 = 1;

/// Normal spacing (2 cells).
pub const SPACING_NORMAL: u16 = 2;

/// Loose spacing (4 cells).
pub const SPACING_LOOSE: u16 = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipsis_is_unicode() {
        assert_eq!(ELLIPSIS, "\u{2026}");
        assert_eq!(ELLIPSIS.chars().count(), 1);
        assert_eq!(ELLIPSIS.len(), 3); // UTF-8: 3 bytes
    }

    #[test]
    fn ellipsis_char_matches_string() {
        assert_eq!(ELLIPSIS_CHAR, '\u{2026}');
        assert_eq!(ELLIPSIS_CHAR.to_string(), ELLIPSIS);
    }

    #[test]
    fn ellipsis_ascii_is_three_dots() {
        assert_eq!(ELLIPSIS_ASCII, "...");
        assert_eq!(ELLIPSIS_ASCII.len(), 3);
    }

    #[test]
    fn preview_ellipsis_delegates() {
        assert_eq!(INLINE_PREVIEW_ELLIPSIS, ELLIPSIS);
    }

    #[test]
    fn layout_breakpoints_are_ordered() {
        assert!(COMPACT_MAX_COLS < WIDE_MIN_COLS);
        assert!(COMPACT_MAX_ROWS < WIDE_MIN_ROWS);
    }

    #[test]
    fn spacing_tokens_are_ordered() {
        assert!(SPACING_TIGHT < SPACING_NORMAL);
        assert!(SPACING_NORMAL < SPACING_LOOSE);
    }
}
