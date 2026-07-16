#![allow(missing_docs)]
use super::super::*;
use crate::tui::prelude::InlineSegment;
use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::sync::Arc;
use vtcode_commons::ui_protocol::ThinkingBlockState;

fn make_policy_line(text: &str) -> InlineSegment {
    InlineSegment {
        text: text.to_string(),
        style: Arc::new(InlineTextStyle::default()),
    }
}

fn push_policy_lines(session: &mut Session, texts: &[&str]) {
    for text in texts {
        session.push_line(InlineMessageKind::Policy, vec![make_policy_line(text)]);
    }
}

fn line_text(rendered: &TranscriptLine) -> String {
    rendered
        .line
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>()
}

fn all_text(transcript: &[TranscriptLine]) -> String {
    transcript
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn collapsed_by_default_renders_summary_line() {
    let session = Session::new(InlineTheme::default(), None, 24);
    let mut session = session;
    push_policy_lines(&mut session, &["reasoning step one", "reasoning step two"]);

    let start = session.lines.len() - 2;
    let transcript = session.reflow_message_lines(start, 100, true);
    let joined = all_text(&transcript);

    assert!(
        joined.contains("Thinking"),
        "collapsed summary should mention Thinking, got: {joined:?}"
    );
    assert!(
        !joined.contains("reasoning step one"),
        "collapsed render must not include the body, got: {joined:?}"
    );
}

#[test]
fn extended_config_renders_full_body() {
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.appearance.thinking_display = ThinkingBlockState::Extended;
    push_policy_lines(&mut session, &["reasoning step one", "reasoning step two"]);

    let start = session.lines.len() - 2;
    let transcript = session.reflow_message_lines(start, 100, true);
    let joined = all_text(&transcript);

    assert!(
        joined.contains("reasoning step one"),
        "extended render should include the body, got: {joined:?}"
    );
    assert!(
        joined.starts_with("Thinking"),
        "expanded render must have a Thinking header, got: {joined:?}"
    );
}

#[test]
fn toggle_flips_collapse_state() {
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.transcript_width = 100;
    push_policy_lines(&mut session, &["reasoning step one", "reasoning step two"]);
    let start = session.lines.len() - 2;

    // Default is collapsed.
    let collapsed = session.reflow_message_lines(start, 100, true);
    assert!(all_text(&collapsed).contains("Thinking"));

    // Locate the summary row via the reflow cache.
    let summary_row = {
        let cache = session.ensure_reflow_cache(100);
        cache.row_offsets[start]
    };

    let toggled = session.toggle_thinking_block_at_row(100, summary_row);
    assert!(toggled, "toggle should report a toggled block");

    // Now expanded.
    let expanded = session.reflow_message_lines(start, 100, true);
    assert!(
        all_text(&expanded).contains("reasoning step one"),
        "after toggle the body should be visible"
    );

    // Toggle back to collapsed.
    let toggled_again = session.toggle_thinking_block_at_row(100, summary_row);
    assert!(toggled_again);
    let collapsed_again = session.reflow_message_lines(start, 100, true);
    assert!(all_text(&collapsed_again).contains("Thinking"));
}

#[test]
fn toggle_updates_reflow_cache() {
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.transcript_width = 100;
    push_policy_lines(&mut session, &["reasoning step one", "reasoning step two"]);
    let start = session.lines.len() - 2;

    // Prime the reflow cache (collapsed by default).
    let pre = session.ensure_reflow_cache(100);
    let pre_text = all_text(&pre.messages[start].lines);
    assert!(
        pre_text.contains("Thinking"),
        "cache should hold the collapsed summary, got: {pre_text:?}"
    );

    // Resolve the summary row and toggle.
    let summary_row = pre.row_offsets[start];
    assert!(session.toggle_thinking_block_at_row(100, summary_row));

    // The cache must reflect the expanded body after the toggle.
    let post = session.ensure_reflow_cache(100);
    let post_text = all_text(&post.messages[start].lines);
    assert!(
        post_text.contains("reasoning step one"),
        "cache should reflect expanded body after toggle, got: {post_text:?}"
    );

    // The visible-window cache (keyed only by offset/width/height) must also be
    // invalidated, otherwise the post-toggle render keeps the stale lines.
    let window = session.collect_transcript_window_cached(100, 0, 200);
    let window_text = window
        .iter()
        .map(|line| {
            line.line
                .spans
                .iter()
                .map(|span| span.content.to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        window_text.contains("reasoning step one"),
        "visible window should reflect expanded body after toggle, got: {window_text:?}"
    );
}

#[test]
fn click_on_summary_expands_via_event_handler() {
    use crossterm::event::MouseButton::Left;
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.transcript_area = Some(Rect::new(0, 0, 100, 24));
    session.transcript_width = 100;
    session.transcript_rows = 24;
    push_policy_lines(&mut session, &["reasoning step one", "reasoning step two"]);

    // Prime caches (collapsed by default). Thinking is the only block, so its
    // summary sits at global row 0.
    let pre = session.ensure_reflow_cache(100);
    let summary_row = pre.row_offsets[0];

    let mouse = MouseEvent {
        kind: MouseEventKind::Down(Left),
        column: 1,
        row: summary_row as u16,
        modifiers: KeyModifiers::empty(),
    };
    let handled = session.handle_transcript_click(mouse);
    assert!(handled, "click on the summary should be handled");

    let window = session.collect_transcript_window_cached(100, 0, 200);
    let window_text = window
        .iter()
        .map(|line| {
            line.line
                .spans
                .iter()
                .map(|span| span.content.to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        window_text.contains("reasoning step one"),
        "clicking the summary should expand the block, got: {window_text:?}"
    );
}

#[test]
fn collapsed_thinking_separated_from_agent_message() {
    let mut session = Session::new(InlineTheme::default(), None, 24);
    push_policy_lines(&mut session, &["a", "b", "c"]);
    let start = session.lines.len() - 3;

    // Append an agent response immediately after the reasoning run.
    session.push_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "answer".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    let transcript = session.reflow_message_lines(start, 100, true);
    let lines: Vec<String> = transcript.iter().map(line_text).collect();

    assert_eq!(lines[0], "Thinking");
    assert!(
        lines[1].trim().is_empty(),
        "expected a blank line between the thinking block and the agent message, got: {:?}",
        lines[1]
    );
}

#[test]
fn agent_message_has_trailing_blank_line() {
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.push_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "answer".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    let start = session.lines.len() - 1;
    // A different-kind line follows (the next turn).
    session.push_line(
        InlineMessageKind::User,
        vec![InlineSegment {
            text: "next".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    let transcript = session.reflow_message_lines(start, 100, true);
    let lines: Vec<String> = transcript.iter().map(line_text).collect();
    assert!(
        lines.last().unwrap().trim().is_empty(),
        "agent message should be followed by a blank line, got: {lines:?}"
    );
}

#[test]
fn thinking_block_layout_snapshot() {
    // Collapsed: a single arrow-prefixed summary line, no body.
    let mut session = Session::new(InlineTheme::default(), None, 24);
    push_policy_lines(&mut session, &["reasoning step one", "reasoning step two"]);
    let start = session.lines.len() - 2;
    let collapsed = session.reflow_message_lines(start, 100, true);
    let collapsed_lines: Vec<String> = collapsed.iter().map(line_text).collect();
    assert_eq!(collapsed_lines[0], "Thinking");

    // Expanded: arrow header followed by the dimmed, indented body lines.
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.appearance.thinking_display = ThinkingBlockState::Extended;
    push_policy_lines(&mut session, &["reasoning step one", "reasoning step two"]);
    let start = session.lines.len() - 2;
    let expanded = session.reflow_message_lines(start, 100, true);
    let expanded_lines: Vec<String> = expanded.iter().map(line_text).collect();
    assert_eq!(expanded_lines[0], "Thinking");
    assert_eq!(expanded_lines[1], "  reasoning step one");
    assert_eq!(expanded_lines[2], "  reasoning step two");
}

#[test]
fn expanded_thinking_wraps_within_width() {
    let long = "the quick brown fox jumps over the lazy dog near the river bank";
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.appearance.thinking_display = ThinkingBlockState::Extended;
    push_policy_lines(&mut session, &[long]);
    let start = session.lines.len() - 1;

    let narrow = session.reflow_message_lines(start, 30, true);
    let narrow_text: Vec<String> = narrow.iter().map(line_text).collect();
    // Body must be indented and wrapped onto multiple lines (no overflow past width).
    assert!(narrow_text[1].starts_with("  "));
    let max_cols = narrow_text
        .iter()
        .map(|line| display_width(line))
        .max()
        .unwrap();
    assert!(
        max_cols <= 30,
        "wrapped thinking body overflowed width: {max_cols} > 30"
    );
}

/// Visible column width of a rendered line (sum of unicode widths of its spans).
fn display_width(line: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    line.width()
}

#[test]
fn reasoning_stream_expands_run_len() {
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.transcript_width = 100;
    push_policy_lines(&mut session, &["first reasoning line"]);
    let start = session.lines.len() - 1;

    let pre = session.ensure_reflow_cache(100);
    let pre_text = all_text(&pre.messages[start].lines);
    assert!(pre_text.contains("Thinking"));

    // Stream a second reasoning line without clicking.
    push_policy_lines(&mut session, &["second reasoning line"]);

    let post = session.ensure_reflow_cache(100);
    let post_text = all_text(&post.messages[start].lines);
    assert!(post_text.contains("Thinking"));
}
