//! Tests for the sibling `mod.rs` (repo pattern: `foo.rs` and `foo/mod.rs` both
//! load `foo/tests.rs`). Kept in a separate file to respect the 1000-line ceiling.

use super::*;

#[test]
fn trailing_wrap_whitespace_keeps_unicode_and_source_style() {
    let style = Style::default().fg(Color::Red);
    let mut spans = vec![
        Span::styled("日本\u{2003}".to_string(), style),
        Span::styled(" \t".to_string(), Style::default().fg(Color::Blue)),
    ];
    trim_trailing_wrap_whitespace(&mut spans);
    assert_eq!(spans, vec![Span::styled("日本".to_string(), style)]);
    trim_trailing_wrap_whitespace(&mut spans);
    assert_eq!(spans, vec![Span::styled("日本".to_string(), style)]);

    let mut whitespace = vec![Span::raw("\u{2003}\t".to_string())];
    trim_trailing_wrap_whitespace(&mut whitespace);
    assert!(whitespace.is_empty());
    trim_trailing_wrap_whitespace(&mut whitespace);
    assert!(whitespace.is_empty());
}

#[test]
fn wrap_ascii_fits_single_row() {
    let rows = wrap_ascii_word_boundaries("hello world", Style::default(), 20);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].spans[0].content.as_ref(), "hello world");
}

#[test]
fn wrap_line_returns_fitting_ascii_unchanged() {
    let style = Style::default().fg(Color::Green);
    for (text, width) in [("hello world", 20), ("hello world", 11)] {
        let rows = wrap_line(Line::from(Span::styled(text.to_owned(), style)), width);
        assert_eq!(rows, vec![Line::from(Span::styled(text.to_owned(), style))]);
    }
}

#[test]
fn wrap_line_wraps_overflowing_ascii_at_word_boundary() {
    let rows = wrap_line(Line::from("hello brave new world"), 10);
    let texts: Vec<&str> = rows.iter().map(|row| row.spans[0].content.as_ref()).collect();
    assert_eq!(texts, vec!["hello", "brave new", "world"]);
}

#[test]
fn wrap_ascii_breaks_on_spaces() {
    let rows = wrap_ascii_word_boundaries("hello brave new world", Style::default(), 10);
    let texts: Vec<&str> = rows.iter().map(|r| r.spans[0].content.as_ref()).collect();
    assert_eq!(texts, vec!["hello", "brave new", "world"]);
}

#[test]
fn wrap_ascii_hard_breaks_long_word() {
    let rows = wrap_ascii_word_boundaries("abcdefghijklmnop", Style::default(), 5);
    let texts: Vec<&str> = rows.iter().map(|r| r.spans[0].content.as_ref()).collect();
    assert_eq!(texts, vec!["abcde", "fghij", "klmno", "p"]);
}

#[test]
fn test_strip_ansi_codes() {
    assert_eq!(strip_ansi_codes("\x1b[31mRed text\x1b[0m"), "Red text");
    assert_eq!(strip_ansi_codes("No codes here"), "No codes here");
    assert_eq!(strip_ansi_codes("\x1b[1;32mBold green\x1b[0m"), "Bold green");
}

#[test]
fn test_simplify_tool_display() {
    assert_eq!(simplify_tool_display("file path/to/file"), "accessing path/to/file");
    assert_eq!(simplify_tool_display("path: path/to/file"), "file: path/to/file");
    assert_eq!(simplify_tool_display("grep pattern"), "searching for pattern");
}

#[test]
fn test_justify_plain_text() {
    let result = justify_plain_text("Hello world", 15);
    assert!(result.is_some());
    assert_eq!(result.unwrap().len(), 15);

    assert_eq!(justify_plain_text("Short", 10), None);
    assert_eq!(justify_plain_text("Very long text string", 10), None);
}

#[test]
fn test_detect_todo_state_pending() {
    assert_eq!(detect_todo_state("- [ ] Task"), TodoState::Pending);
    assert_eq!(detect_todo_state("* [ ] Task"), TodoState::Pending);
    assert_eq!(detect_todo_state("+ [ ] Task"), TodoState::Pending);
    assert_eq!(detect_todo_state("[ ] Task"), TodoState::Pending);
    assert_eq!(detect_todo_state("  - [ ] Indented task"), TodoState::Pending);
}

#[test]
fn test_detect_todo_state_completed() {
    assert_eq!(detect_todo_state("- [x] Done"), TodoState::Completed);
    assert_eq!(detect_todo_state("- [X] Done"), TodoState::Completed);
    assert_eq!(detect_todo_state("* [x] Done"), TodoState::Completed);
    assert_eq!(detect_todo_state("[x] Done"), TodoState::Completed);
    assert_eq!(detect_todo_state("  - [x] Indented done"), TodoState::Completed);
    assert_eq!(detect_todo_state("~~Strikethrough text~~"), TodoState::Completed);
}

#[test]
fn test_detect_todo_state_none() {
    assert_eq!(detect_todo_state("Regular text"), TodoState::None);
    assert_eq!(detect_todo_state("- Regular list item"), TodoState::None);
    assert_eq!(detect_todo_state("* Bullet point"), TodoState::None);
    assert_eq!(detect_todo_state("1. Numbered item"), TodoState::None);
}

#[test]
fn test_is_list_item() {
    assert!(is_list_item("- Item"));
    assert!(is_list_item("* Item"));
    assert!(is_list_item("+ Item"));
    assert!(is_list_item("• Item"));
    assert!(is_list_item("1. Item"));
    assert!(is_list_item("a) Item"));
    assert!(is_list_item("  - Indented"));
    assert!(!is_list_item("Regular text"));
    assert!(!is_list_item(""));
}

#[test]
fn test_coalesce_adjacent_spans_merges_same_style() {
    use ratatui::style::Style;
    use ratatui::text::Span;

    let spans = vec![
        Span::styled("a", Style::default()),
        Span::styled("b", Style::default()),
        Span::styled("c", Style::new().bold()),
        Span::styled("d", Style::new().bold()),
        Span::styled("e", Style::default()),
    ];
    let merged = coalesce_adjacent_spans(spans);
    assert_eq!(merged.len(), 3);
    assert_eq!(merged[0].content.as_ref(), "ab");
    assert_eq!(merged[1].content.as_ref(), "cd");
    assert_eq!(merged[2].content.as_ref(), "e");
}

#[test]
fn test_pty_wrapped_continuation_prefix() {
    assert_eq!(pty_wrapped_continuation_prefix("  ", "  └ cargo check"), "      ");
    assert_eq!(pty_wrapped_continuation_prefix("  ", "\u{1b}[32m• Ran cargo check -p vtcode\u{1b}[0m",), "        ");
    assert_eq!(pty_wrapped_continuation_prefix("  ", "• Ran cargo check -p vtcode"), "        ");
    assert_eq!(pty_wrapped_continuation_prefix("  ", "plain output"), "  ");
}

#[test]
fn test_display_width_counts_halfwidth_sound_marks() {
    assert_eq!(display_width(""), 0);
    assert_eq!(display_width("hello"), 5);
    assert_eq!(display_width("日本"), 4);
    // Halfwidth katakana with dakuten/handakuten: the sound mark reports
    // width 0 to unicode-width but occupies one terminal cell.
    assert_eq!(display_width("ｶﾞ"), 2);
    assert_eq!(display_width("ﾊﾟ"), 2);
    assert_eq!(display_width("\u{FF9E}"), 1);
    assert_eq!(display_width("aｶﾞ"), 3);
}

#[test]
fn display_width_counts_ambiguous_glyphs_narrow_by_default() {
    // Box-drawing glyphs are UAX #11 East-Asian-ambiguous; the default
    // verdict is one cell.
    assert_eq!(display_width("├──"), 3);
    assert_eq!(display_width("│ α"), 3);
    assert_eq!(char_display_width('│'), 1);
    assert_eq!(char_display_width('α'), 1);
}

#[test]
fn display_width_counts_ambiguous_glyphs_wide_when_configured() {
    // nextest runs each test in its own process, so the one-time verdict
    // override is isolated here; ideographs are unambiguous in both
    // tables, only the ambiguous set changes. unicode-width's `cjk`
    // table widens the box-drawing range (the chrome we care about) but
    // keeps some other UAX #11 ambiguous chars (e.g. Greek) narrow.
    vtcode_commons::terminal_capabilities::set_ambiguous_width(vtcode_commons::ui_protocol::AmbiguousWidthMode::Wide);
    assert_eq!(display_width("├──"), 6);
    assert_eq!(display_width("│ α"), 4);
    assert_eq!(char_display_width('│'), 2);
    assert_eq!(char_display_width('α'), 1);
    // Ideographs stay two cells; the FF9E correction is unaffected
    // (halfwidth sound marks are not in the ambiguous set).
    assert_eq!(display_width("日本"), 4);
    assert_eq!(display_width("ｶﾞ"), 2);
}

#[test]
fn test_wrap_line_splits_halfwidth_katakana_with_dakuten() {
    let wrapped = wrap_line(Line::from("aｶﾞ"), 2);
    let rendered: String = wrapped
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.clone()))
        .collect();
    assert_eq!(rendered, "aｶﾞ", "wrapping must preserve all characters");
    assert_eq!(wrapped.len(), 2, "dakuten occupies a cell, so width-3 content must wrap at width 2");
}

#[test]
fn test_wrap_continuation_prefix_inherits_diff_bg() {
    use ratatui::style::{Color, Style};
    use ratatui::text::Span;

    let bg = Style::default().bg(Color::Rgb(20, 58, 45));
    let line = Line::from(vec![Span::styled("- long diff body that must wrap onto a second row", bg)]);
    let wrapped = wrap_line_with_hanging_prefix(line, 20, "  ");
    assert!(wrapped.len() > 1, "narrow width must wrap");
    assert_eq!(
        wrapped[1].spans[0].style.bg,
        Some(Color::Rgb(20, 58, 45)),
        "wrapped diff rows must not start with an unpainted strip"
    );
}

#[test]
fn test_wrap_continuation_prefix_stays_plain_without_bg() {
    use ratatui::text::Span;

    let line = Line::from(vec![Span::raw("plain prose that must wrap onto a second row here")]);
    let wrapped = wrap_line_with_hanging_prefix(line, 20, "  ");
    assert!(wrapped.len() > 1, "narrow width must wrap");
    assert_eq!(wrapped[1].spans[0].style.bg, None);
}

#[test]
fn test_blockquote_continuation_preserves_bar() {
    assert_eq!(structural_continuation_prefix("│ quote"), "│ ");
    assert_eq!(structural_continuation_prefix("│ │ nested"), "│ │ ");
    assert_eq!(structural_continuation_prefix("│ • item"), "│   ");
    assert_eq!(structural_continuation_prefix("  │ indented"), "  │ ");
    assert_eq!(structural_continuation_prefix("• item"), "  ");
    assert_eq!(structural_continuation_prefix("plain"), String::new());
}

#[test]
fn test_wrap_line_preserves_blockquote_bar() {
    let line = Line::from("│ alpha beta gamma delta epsilon zeta eta theta");
    let wrapped = wrap_line(line, 12);
    assert!(wrapped.len() > 1, "narrow width must wrap, got {wrapped:?}");
    let rendered: Vec<String> = wrapped
        .iter()
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
        .collect();
    assert!(rendered[0].starts_with("│ "), "first row keeps bar, got {:?}", rendered[0]);
    for (idx, text) in rendered.iter().enumerate().skip(1) {
        assert!(
            text.starts_with("│ "),
            "wrapped blockquote continuation {idx} must keep bar, got {text:?} in {rendered:?}"
        );
    }
}
