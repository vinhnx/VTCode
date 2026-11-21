//! Authoritative terminal display-cell width ("variable column width") math.
//!
//! A terminal cell is not one byte, nor even one character: combining marks
//! occupy no cell, East-Asian wide glyphs occupy two, and East-Asian-ambiguous
//! glyphs occupy one or two depending on terminal configuration. Every
//! wrap/truncate/pad/hit-test decision must use the same verdict or frames,
//! links and tool output drift out of alignment.
//!
//! On a terminal configured to render East-Asian-ambiguous glyphs (UAX #11)
//! as two cells (`ui.ambiguous_width = "wide"`), the CJK width table is used
//! so the math matches what the terminal actually shows.
//!
//! The halfwidth voiced sound marks `U+FF9E` (dakuten) and `U+FF9F`
//! (handakuten) are the one correction `unicode-width` tables have disagreed
//! on: older tables report them as zero width even though a terminal draws
//! them in one cell, so wrap and truncation math would otherwise credit one
//! cell for a two-cell glyph. Only the *missing* width is credited, so newer
//! tables that already count the mark are not double-counted.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::terminal_capabilities::ambiguous_width_wide;

/// Terminal display width of `text` in cells.
#[must_use]
pub fn display_width(text: &str) -> usize {
    let missing_mark_width: usize = text.chars().map(missing_sound_mark_width).sum();
    let width = if ambiguous_width_wide() {
        UnicodeWidthStr::width_cjk(text)
    } else {
        UnicodeWidthStr::width(text)
    };
    width.saturating_add(missing_mark_width)
}

/// Width older `unicode-width` tables leave out for a halfwidth sound mark.
fn missing_sound_mark_width(ch: char) -> usize {
    if matches!(ch, '\u{FF9E}' | '\u{FF9F}') {
        1usize.saturating_sub(UnicodeWidthChar::width(ch).unwrap_or(0))
    } else {
        0
    }
}

/// Terminal display width of one character under the resolved
/// ambiguous-width verdict, for char-by-char column mapping.
#[must_use]
pub fn char_display_width(ch: char) -> usize {
    char_display_width_or(ch, 0)
}

/// Terminal display width of one character, with `fallback` for characters
/// the width tables do not classify (control characters), matching call
/// sites that treat unknown widths as one cell.
#[must_use]
pub fn char_display_width_or(ch: char, fallback: usize) -> usize {
    let width = if ambiguous_width_wide() {
        UnicodeWidthChar::width_cjk(ch).unwrap_or(fallback)
    } else {
        UnicodeWidthChar::width(ch).unwrap_or(fallback)
    };
    width.saturating_add(missing_sound_mark_width(ch))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_protocol::AmbiguousWidthMode;

    #[test]
    fn counts_halfwidth_sound_marks_without_double_counting() {
        assert_eq!(display_width(""), 0);
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width("日本"), 4);
        assert_eq!(display_width("ｶﾞ"), 2);
        assert_eq!(display_width("ﾊﾟ"), 2);
        assert_eq!(display_width("\u{FF9E}"), 1);
        assert_eq!(display_width("aｶﾞ"), 3);
        assert_eq!(char_display_width('\u{FF9E}'), 1);
        assert_eq!(char_display_width('\u{FF9F}'), 1);
    }

    #[test]
    fn counts_ambiguous_glyphs_narrow_by_default() {
        assert_eq!(display_width("├──"), 3);
        assert_eq!(display_width("│ α"), 3);
        assert_eq!(char_display_width('│'), 1);
        assert_eq!(char_display_width('α'), 1);
    }

    #[test]
    fn counts_ambiguous_glyphs_wide_when_configured() {
        // nextest runs each test in its own process, so the one-time verdict
        // override is isolated here. Ideographs are unambiguous in both
        // tables; only the ambiguous set changes.
        crate::terminal_capabilities::set_ambiguous_width(AmbiguousWidthMode::Wide);
        assert_eq!(display_width("├──"), 6);
        assert_eq!(display_width("│ α"), 4);
        assert_eq!(char_display_width('│'), 2);
        assert_eq!(char_display_width('α'), 1); // cjk table keeps Greek narrow
        assert_eq!(display_width("日本"), 4);
        assert_eq!(display_width("ｶﾞ"), 2);
    }

    #[test]
    fn char_width_fallback_applies_to_unclassified_characters() {
        assert_eq!(char_display_width_or('\n', 1), 1);
        assert_eq!(char_display_width_or('日', 1), 2);
    }
}
