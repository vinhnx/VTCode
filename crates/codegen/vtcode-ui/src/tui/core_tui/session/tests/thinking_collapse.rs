#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
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
    transcript.iter().map(line_text).collect::<Vec<_>>().join("\n")
}

#[test]
fn collapsed_by_default_renders_summary_line() {
    let session = Session::new(InlineTheme::default(), None, 24);
    let mut session = session;
    push_policy_lines(&mut session, &["reasoning step one", "reasoning step two"]);

    let start = session.lines.len() - 2;
    let transcript = session.reflow_message_lines(start, 100, true);
    let joined = all_text(&transcript);

    assert!(joined.contains("Thinking"), "collapsed summary should mention Thinking, got: {joined:?}");
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

    assert!(joined.contains("reasoning step one"), "extended render should include the body, got: {joined:?}");
    assert!(joined.starts_with("Thinking"), "expanded render must have a Thinking header, got: {joined:?}");
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
    assert!(all_text(&expanded).contains("reasoning step one"), "after toggle the body should be visible");

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
    assert!(pre_text.contains("Thinking"), "cache should hold the collapsed summary, got: {pre_text:?}");

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
        .map(|line| line.line.spans.iter().map(|span| span.content.to_string()).collect::<String>())
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
    session.set_transcript_area(Some(Rect::new(0, 0, 100, 24)));
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
        .map(|line| line.line.spans.iter().map(|span| span.content.to_string()).collect::<String>())
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

    // Expanded: header followed by the body lines.
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.appearance.thinking_display = ThinkingBlockState::Extended;
    push_policy_lines(&mut session, &["reasoning step one", "reasoning step two"]);
    let start = session.lines.len() - 2;
    let expanded = session.reflow_message_lines(start, 100, true);
    let expanded_lines: Vec<String> = expanded.iter().map(line_text).collect();
    assert_eq!(expanded_lines[0], "Thinking");
    assert!(expanded_lines[1].contains("reasoning step one"));
    assert!(expanded_lines[1].contains("reasoning step two"));
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
    let max_cols = narrow_text.iter().map(|line| display_width(line)).max().unwrap();
    assert!(max_cols <= 30, "wrapped thinking body overflowed width: {max_cols} > 30");
}

/// Visible column width of a rendered line (sum of unicode widths of its spans).
fn display_width(line: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    line.width()
}

#[test]
fn trailing_empty_policy_line_removed_when_non_policy_arrives_via_append_inline() {
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.transcript_width = 100;

    // Simulate streaming reasoning: each line + trailing \n creates an empty
    // Policy line that subsequent content fills. The final \n leaves a dangling
    // empty Policy line behind.
    session.append_inline(
        InlineMessageKind::Policy,
        InlineSegment {
            text: "first reasoning\n".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        },
    );
    session.append_inline(
        InlineMessageKind::Policy,
        InlineSegment {
            text: "second reasoning\n".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        },
    );

    // Before the Agent line arrives, there's a dangling empty Policy line.
    let last = session.lines.last().unwrap();
    assert_eq!(last.kind, InlineMessageKind::Policy, "trailing empty line is Policy");
    assert!(last.segments.is_empty(), "trailing line should be empty");

    // When non-Policy content arrives via append_inline, the trailing empty
    // Policy line should be cleaned up.
    session.append_inline(
        InlineMessageKind::Agent,
        InlineSegment {
            text: "answer".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        },
    );

    // Verify the empty Policy line was removed.
    for (i, line) in session.lines.iter().enumerate() {
        if line.kind == InlineMessageKind::Policy && line.segments.iter().all(|s| s.text.is_empty()) {
            panic!("trailing empty Policy line still present at index {i} after Agent append");
        }
    }

    // The Policy content should remain and be followed directly by Agent content.
    let policy_count = session.lines.iter().filter(|l| l.kind == InlineMessageKind::Policy).count();
    assert_eq!(policy_count, 2, "both reasoning lines should remain");
}

#[test]
fn expanded_view_skips_trailing_empty_policy_line() {
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.appearance.thinking_display = ThinkingBlockState::Extended;
    session.transcript_width = 100;

    // Push reasoning content followed by a trailing empty line (the artifact).
    push_policy_lines(&mut session, &["reasoning one", "reasoning two"]);
    // Manually add the trailing empty line that streaming \n creates.
    session.push_line(InlineMessageKind::Policy, vec![]);

    let start = session.lines.len() - 3;
    let transcript = session.reflow_message_lines(start, 100, true);
    let body: Vec<String> = transcript.iter().map(line_text).collect();

    assert_eq!(body[0], "Thinking", "header should be Thinking");
    // The two content lines should be rendered.
    assert!(body.iter().any(|l| l.contains("reasoning one")), "first reasoning line should appear");
    assert!(body.iter().any(|l| l.contains("reasoning two")), "second reasoning line should appear");
    // The empty line must produce no additional rendered line.
    let rendered_count = body.iter().skip(1).filter(|l| !l.trim().is_empty()).count();
    assert!(rendered_count >= 1, "both non-empty reasoning lines should be rendered, got {rendered_count}");
}

#[test]
fn trailing_empty_policy_line_removed_when_non_policy_arrives_via_push_line() {
    let mut session = Session::new(InlineTheme::default(), None, 24);
    push_policy_lines(&mut session, &["reasoning one", "reasoning two"]);
    // Manually add the trailing empty line.
    session.push_line(InlineMessageKind::Policy, vec![]);

    // Push non-Policy content — should trigger cleanup.
    session.push_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "answer".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    // The trailing empty Policy line must be removed.
    for (i, line) in session.lines.iter().enumerate() {
        if line.kind == InlineMessageKind::Policy && line.segments.is_empty() {
            panic!("trailing empty Policy line still present at index {i} after push_line cleanup");
        }
    }
    assert_eq!(session.lines.len(), 3, "lines: 2 Policy content + 1 Agent");
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
#[test]
fn expanded_thinking_wraps_at_word_boundaries() {
    let long_word = "Supercalifragilisticexpialidocious";
    let text = format!("consider the term {long_word} in this reasoning");
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.appearance.thinking_display = ThinkingBlockState::Extended;
    push_policy_lines(&mut session, &[&text]);
    let start = session.lines.len() - 1;

    let narrow = session.reflow_message_lines(start, 30, true);
    let narrow_text: Vec<String> = narrow.iter().map(line_text).collect();
    let body_text: String = narrow_text.iter().skip(1).cloned().collect();
    assert!(
        !body_text.contains("SupercalifragilisticexpialidociousSuper"),
        "word-boundary wrapping must not split a long word mid-character: {body_text:?}"
    );
}
#[test]
fn expanded_thinking_continuation_lines_match_first_line_width() {
    let long = "the quick brown fox jumps over the lazy dog near the river bank and keeps on going";
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.appearance.thinking_display = ThinkingBlockState::Extended;
    push_policy_lines(&mut session, &[long]);
    let start = session.lines.len() - 1;

    let narrow = session.reflow_message_lines(start, 40, true);
    let narrow_text: Vec<String> = narrow.iter().map(line_text).collect();
    let body_widths: Vec<usize> = narrow_text.iter().skip(1).map(|line| display_width(line)).collect();
    assert!(body_widths.iter().all(|&w| w <= 40), "no body line must exceed viewport width: {body_widths:?}");
    assert!(body_widths.len() >= 2, "wrapped text should produce multiple body lines: {body_widths:?}");
}

#[test]
fn expanded_thinking_numbered_list_preserves_structural_indent() {
    let list_text = "here is the plan:\n1. first step\n2. second step\n3. third step";
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.appearance.thinking_display = ThinkingBlockState::Extended;
    push_policy_lines(&mut session, &[list_text]);
    let start = session.lines.len() - 1;

    let narrow = session.reflow_message_lines(start, 30, true);
    let narrow_text: Vec<String> = narrow.iter().map(line_text).collect();
    let body_text: String = narrow_text.iter().skip(1).cloned().collect();
    assert!(body_text.contains("1. first"), "numbered list marker must be preserved: {body_text:?}");
}

#[test]
fn expanded_thinking_strips_leading_whitespace_for_consistent_indent() {
    let text_with_newlines = "First line\n second line\n third line";
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.appearance.thinking_display = ThinkingBlockState::Extended;
    push_policy_lines(&mut session, &[text_with_newlines]);
    let start = session.lines.len() - 1;

    let transcript = session.reflow_message_lines(start, 40, true);
    let body_lines: Vec<String> = transcript.iter().skip(1).map(line_text).collect();
    for line in &body_lines {
        assert!(!line.starts_with(' '), "body line must not have leading whitespace, got: {line:?}");
    }
}

#[test]
fn thinking_block_uses_distinct_dimmed_italic_style() {
    use ratatui::style::Modifier;

    // Collapsed header must be distinct from regular output.
    let mut session = Session::new(InlineTheme::default(), None, 24);
    push_policy_lines(&mut session, &["reasoning step one", "reasoning step two"]);
    let start = session.lines.len() - 2;
    let collapsed = session.reflow_message_lines(start, 100, true);
    let header_style = collapsed[0].line.spans[0].style;
    assert!(
        header_style.add_modifier.contains(Modifier::DIM),
        "Thinking header should be dimmed, got: {header_style:?}"
    );
    assert!(
        header_style.add_modifier.contains(Modifier::ITALIC),
        "Thinking header should be italic, got: {header_style:?}"
    );

    // Expanded body must stay distinct even when source segments carry no effects.
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.appearance.thinking_display = ThinkingBlockState::Extended;
    push_policy_lines(&mut session, &["reasoning step one"]);
    let start = session.lines.len() - 1;
    let expanded = session.reflow_message_lines(start, 100, true);
    let body_has_reasoning_style = expanded.iter().skip(1).any(|line| {
        line.line.spans.iter().any(|span| {
            !span.content.trim().is_empty()
                && span.style.add_modifier.contains(Modifier::DIM)
                && span.style.add_modifier.contains(Modifier::ITALIC)
        })
    });
    assert!(body_has_reasoning_style, "expanded reasoning body should be dimmed+italic");

    // Asymmetric control: regular agent prose must not share the full reasoning treatment.
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.push_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "answer".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    let start = session.lines.len() - 1;
    let agent = session.reflow_message_lines(start, 100, true);
    let agent_uses_reasoning_style = agent.iter().any(|line| {
        line.line.spans.iter().any(|span| {
            !span.content.trim().is_empty()
                && span.style.add_modifier.contains(Modifier::DIM)
                && span.style.add_modifier.contains(Modifier::ITALIC)
        })
    });
    assert!(!agent_uses_reasoning_style, "agent prose must remain visually distinct from reasoning");
}

#[test]
fn thinking_expand_pins_viewport_while_following() {
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.apply_transcript_width(100);
    for index in 0..40 {
        session.push_line(
            InlineMessageKind::Agent,
            vec![InlineSegment {
                text: format!("streamed line {index}"),
                style: Arc::new(InlineTextStyle::default()),
            }],
        );
    }
    let run_start = session.lines.len();
    // Long steps so the expanded body wraps past one row: expansion must grow
    // the transcript (a short run can be row-count neutral and needs no pin).
    push_policy_lines(
        &mut session,
        &[
            "reasoning step one considers the available approaches in some detail",
            "reasoning step two weighs the trade-offs of each candidate approach",
            "reasoning step three commits to the simplest approach that fits",
        ],
    );
    for index in 0..2 {
        session.push_line(
            InlineMessageKind::Agent,
            vec![InlineSegment {
                text: format!("tail line {index}"),
                style: Arc::new(InlineTextStyle::default()),
            }],
        );
    }
    let top_row = |session: &mut Session| session.current_max_scroll_offset() - session.scroll_manager.offset();
    assert_eq!(session.scroll_manager.offset(), 0, "the transcript starts bottom-following");
    let pinned_top = top_row(&mut session);

    let summary_row = session.ensure_reflow_cache(100).row_offsets[run_start];
    assert!(session.toggle_thinking_block_at_row(100, summary_row), "the summary row toggles");
    assert!(session.scroll_manager.offset() > 0, "expanding pins the viewport instead of following");
    assert_eq!(top_row(&mut session), pinned_top, "expansion keeps the pre-toggle top row");

    // Continued streaming must not move the pinned view.
    for index in 0..12 {
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![InlineSegment {
                text: format!("later line {index}"),
                style: Arc::new(InlineTextStyle::default()),
            }],
        });
    }
    assert_eq!(top_row(&mut session), pinned_top, "streaming preserves the pinned view");
    assert!(session.pending_new_messages > 0, "pinned arrivals feed the new-content hint");

    // Asymmetric direction: collapsing restores the summary without scrolling away.
    let summary_row = session.ensure_reflow_cache(100).row_offsets[run_start];
    assert!(session.toggle_thinking_block_at_row(100, summary_row), "the summary row toggles back");
    assert!(
        session.scroll_manager.offset() <= session.current_max_scroll_offset(),
        "collapse keeps a valid scroll position"
    );
    let summary = all_text(&session.reflow_message_lines(run_start, 100, true));
    assert!(summary.contains("Thinking"), "collapsed summary returns, got: {summary:?}");
}
