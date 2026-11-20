//! Tests for the sibling `mod.rs` (repo pattern: `foo.rs` and `foo/mod.rs` both
//! load `foo/tests.rs`). Kept in a separate file to respect the 1000-line ceiling.

use super::*;
use ratatui::style::Color;
use std::time::{Duration, Instant};

#[test]
fn extract_text_clamps_area_to_buffer_bounds() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
    buf[(0, 0)].set_symbol("A");
    buf[(1, 0)].set_symbol("B");
    buf[(0, 1)].set_symbol("C");
    buf[(1, 1)].set_symbol("D");

    let mut selection = MouseSelectionState::new();
    selection.start_selection(0, 0);
    selection.finish_selection(5, 5);

    let text = selection.extract_text(&buf, Rect::new(0, 0, 10, 10));
    assert_eq!(text, "AB\nCD");
}

/// Build a buffer whose rows are the given strings, left-aligned.
fn buffer_with_rows(rows: &[&str], width: u16) -> Buffer {
    let height = u16::try_from(rows.len()).expect("row count fits in u16");
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    for (row_index, row) in rows.iter().enumerate() {
        for (col, ch) in row.chars().enumerate() {
            let col = u16::try_from(col).expect("column fits in u16");
            if col >= width {
                break;
            }
            buf[(col, u16::try_from(row_index).expect("row fits in u16"))].set_symbol(&ch.to_string());
        }
    }
    buf
}

#[test]
fn overlay_copy_drops_the_list_selection_cursor() {
    // A modal list renders a two-cell gutter: the cursor on the focused
    // row, blanks on the others.
    let buf = buffer_with_rows(&["┃ Option one", "  Option two"], 20);

    let mut selection = MouseSelectionState::new();
    selection.start_overlay_selection(0, 0);
    selection.finish_selection(14, 1);

    let text = selection.extract_text(&buf, buf.area);
    assert_eq!(text, "Option one\nOption two", "the gutter is chrome, the item text is content");
}

#[test]
fn overlay_copy_drops_the_gutter_when_the_selection_starts_inside_it() {
    // Dragging from the marker itself must not leave the padding behind.
    let buf = buffer_with_rows(&["┃ Option one"], 20);

    let mut selection = MouseSelectionState::new();
    selection.start_overlay_selection(0, 0);
    selection.finish_selection(13, 0);

    let text = selection.extract_text(&buf, buf.area);
    assert_eq!(text, "Option one");
}

#[test]
fn transcript_copy_strips_framing_pipe_prefix() {
    // Block framing `│` is chrome: the copy keeps the content and the
    // block indent, not the pipe.
    let buf = buffer_with_rows(&["  │ nested output"], 20);

    let mut selection = MouseSelectionState::new();
    selection.start_selection(0, 0);
    selection.finish_selection(17, 0);

    let text = selection.extract_text(&buf, buf.area);
    assert_eq!(text, "  nested output");
}

#[test]
fn transcript_copy_strips_tree_markers_and_wrapped_header_continuations() {
    // `reflow` tree rows start at the row edge; wrapped command headers
    // continue with the exact `  │ ` prefix. Both prefixes are chrome.
    let buf = buffer_with_rows(&["└ File types:", "├ 12 files", "  │ --coverage"], 20);

    let mut selection = MouseSelectionState::new();
    selection.start_selection(0, 0);
    selection.finish_selection(14, 2);

    let text = selection.extract_text(&buf, buf.area);
    assert_eq!(text, "File types:\n12 files\n  --coverage");
}

#[test]
fn transcript_copy_keeps_program_output_starting_with_the_same_glyphs() {
    // PTY program output is indented four spaces — beyond the two-space
    // chrome forms — and `tree` output has no space after the glyph, so
    // neither loses its glyphs to the chrome stripper.
    let buf = buffer_with_rows(&["    │   branch", "    ├── src"], 20);

    let mut selection = MouseSelectionState::new();
    selection.start_selection(0, 0);
    selection.finish_selection(12, 1);

    let text = selection.extract_text(&buf, buf.area);
    assert_eq!(text, "    │   branch\n    ├── src");
}

#[test]
fn transcript_copy_drops_markdown_table_rule_rows() {
    // The header/body separator is a pure box-drawing rule: decoration in
    // a copy, so the row disappears while data rows survive untouched.
    let buf = buffer_with_rows(&["name        age", "──────  ───", "alice       30"], 20);

    let mut selection = MouseSelectionState::new();
    selection.start_selection(0, 0);
    selection.finish_selection(19, 2);

    let text = selection.extract_text(&buf, buf.area);
    assert_eq!(text, "name        age\nalice       30");
}

#[test]
fn transcript_copy_keeps_box_drawing_diagram_rows() {
    // Language-less code blocks and prose can hold box-drawing diagrams. The
    // rule-drop only claims the two glyphs transcript chrome actually draws
    // rules with (`─`/`━`); rows that mix other box glyphs are content, and
    // a row-edge `│` stays (fenced trees and ASCII tables start with it).
    let buf = buffer_with_rows(&["┌──┬──┐", "│  │  │", "├──┼──┤"], 20);

    let mut selection = MouseSelectionState::new();
    selection.start_selection(0, 0);
    selection.finish_selection(8, 2);

    let text = selection.extract_text(&buf, buf.area);
    assert_eq!(text, "┌──┬──┐\n│  │  │\n├──┼──┤");
}

#[test]
fn transcript_copy_keeps_row_edge_pipe_for_quotes_and_fenced_content() {
    // `│ ` at the row edge is a blockquote bar or fenced content, not the
    // tool frame; only the two-space `  │ ` form is unambiguous chrome.
    let buf = buffer_with_rows(&["│ quoted prose", "│   pasted/tree"], 20);

    let mut selection = MouseSelectionState::new();
    selection.start_selection(0, 0);
    selection.finish_selection(16, 1);

    let text = selection.extract_text(&buf, buf.area);
    assert_eq!(text, "│ quoted prose\n│   pasted/tree");
}

#[test]
fn transcript_copy_keeps_a_selection_cursor_that_is_part_of_the_content() {
    // A transcript row may legitimately contain the cursor glyph; the
    // transcript surface never treats it as chrome.
    let buf = buffer_with_rows(&["┃ literal text"], 20);

    let mut selection = MouseSelectionState::new();
    selection.start_selection(0, 0);
    selection.finish_selection(14, 0);

    let text = selection.extract_text(&buf, buf.area);
    assert_eq!(text, "┃ literal text");
}

#[test]
fn apply_highlight_clamps_area_to_buffer_bounds() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
    buf[(0, 0)].set_fg(Color::Red);
    buf[(0, 0)].set_bg(Color::Blue);

    let mut selection = MouseSelectionState::new();
    selection.start_selection(0, 0);
    selection.finish_selection(5, 5);

    selection.apply_highlight(&mut buf, Rect::new(0, 0, 10, 10));

    assert_eq!(buf[(0, 0)].fg, Color::Blue);
    assert_eq!(buf[(0, 0)].bg, Color::Red);
}

#[test]
fn word_selection_range_selects_clicked_word() {
    assert_eq!(word_selection_range("hello world", 1), Some((0, 5)));
    assert_eq!(word_selection_range("hello world", 7), Some((6, 11)));
}

#[test]
fn word_selection_range_returns_none_for_whitespace() {
    assert_eq!(word_selection_range("hello world", 5), None);
}

#[test]
fn adjust_for_scroll_shifts_rows() {
    let mut sel = MouseSelectionState::new();
    sel.set_selection((2, 5), (10, 8));

    sel.adjust_for_scroll(3);
    assert_eq!(sel.start, (2, 8));
    assert_eq!(sel.end, (10, 11));
    assert!(sel.has_selection);
}

#[test]
fn adjust_for_scroll_negative() {
    let mut sel = MouseSelectionState::new();
    sel.set_selection((0, 10), (5, 15));

    sel.adjust_for_scroll(-4);
    assert_eq!(sel.start, (0, 6));
    assert_eq!(sel.end, (5, 11));
}

#[test]
fn adjust_for_scroll_clears_when_offscreen() {
    let mut sel = MouseSelectionState::new();
    sel.set_selection((0, 2), (5, 4));

    sel.adjust_for_scroll(-10);
    assert!(!sel.has_selection);
    assert!(!sel.is_selecting);
}

#[test]
fn adjust_for_scroll_noop_without_selection() {
    let mut sel = MouseSelectionState::new();
    sel.adjust_for_scroll(5);
    assert!(!sel.has_selection);
}

#[test]
fn register_click_detects_double_clicks_at_same_position() {
    let mut selection = MouseSelectionState::new();
    let now = Instant::now();

    assert!(!selection.register_click(3, 7, now));
    assert!(selection.register_click(3, 7, now + Duration::from_millis(250)));
    assert!(!selection.register_click(4, 7, now + Duration::from_millis(250)));
}

#[test]
fn spawn_clipboard_command_requires_success_exit_status() {
    let failing = {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg("exit 1");
        cmd
    };
    assert!(!spawn_clipboard_command(failing, "hello"));

    let succeeding = {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg("cat > /dev/null");
        cmd
    };
    assert!(spawn_clipboard_command(succeeding, "hello"));
}

#[test]
fn spawn_clipboard_command_treats_hanging_helper_as_success() {
    // A helper that holds stdin open and never exits must not block the
    // UI thread forever: the bounded wait accepts the already-written
    // input and returns without hanging.
    let hanging = {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg("cat > /dev/null & sleep 10");
        cmd
    };
    let start = Instant::now();
    assert!(spawn_clipboard_command(hanging, "hello"));
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "bounded wait must return promptly, took {:?}",
        start.elapsed()
    );
}

#[test]
fn spawn_clipboard_command_large_write_to_non_reading_helper_returns_promptly() {
    // A helper that never reads stdin would block a synchronous write_all
    // forever once the 64KB pipe buffer fills. The detached writer must not
    // stall the caller: the bounded poll returns without joining it.
    let non_reading = {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg("sleep 10");
        cmd
    };
    let big: String = "x".repeat(128 * 1024);
    let start = Instant::now();
    let result = spawn_clipboard_command(non_reading, &big);
    assert!(start.elapsed() < Duration::from_secs(2), "must not block on the writer, took {:?}", start.elapsed());
    // No write failure was observed within the budget, so the input is
    // treated as accepted even though the helper never reads.
    assert!(result);
}

#[test]
fn build_osc52_payload_emits_base64_clipboard_sequence() {
    let payload = build_osc52_payload("hi", false).expect("osc52 payload");
    assert!(payload.starts_with("\x1b]52;c;"), "unexpected payload: {payload:?}");
    assert!(payload.ends_with("\x1b\\"));
    assert!(payload.contains("aGk="));
}

#[test]
fn build_osc52_payload_wraps_in_tmux_passthrough() {
    let plain = build_osc52_payload("hi", false).expect("plain payload");
    let wrapped = build_osc52_payload("hi", true).expect("wrapped payload");

    // DCS introducer + doubled-ESC payload + ST terminator.
    assert_eq!(wrapped, format!("\x1bPtmux;\x1b{}\x1b\\", plain.replace('\x1b', "\x1b\x1b")));
    assert!(wrapped.starts_with("\x1bPtmux;\x1b\x1b\x1b]52;c;"));
    assert!(wrapped.ends_with("\x1b\x1b\\\x1b\\"));
}

#[test]
fn wrap_tmux_passthrough_doubles_every_escape() {
    assert_eq!(wrap_tmux_passthrough("\x1ba\x1bb"), "\x1bPtmux;\x1b\x1b\x1ba\x1b\x1bb\x1b\\");
}
