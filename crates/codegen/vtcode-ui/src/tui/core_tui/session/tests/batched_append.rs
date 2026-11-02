#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::super::*;
use super::helpers::*;
use crate::tui::core_tui::types::InlineTranscriptLine;

const SEED_LINES: usize = 40;

fn append_lines(session: &mut Session, lines: Vec<InlineTranscriptLine>) {
    session.handle_command(InlineCommand::AppendLines { lines });
}

#[test]
fn append_lines_appends_every_line_and_stays_at_bottom() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    for index in 0..SEED_LINES {
        session.push_line(InlineMessageKind::Agent, vec![make_segment(&format!("{LABEL_PREFIX}-{index}"))]);
    }

    // Establish transcript geometry so the offset and viewport are meaningful.
    let _ = visible_transcript(&mut session);
    assert_eq!(session.scroll_offset(), 0, "a fresh transcript is pinned to the bottom");

    append_lines(
        &mut session,
        vec![
            InlineTranscriptLine {
                kind: InlineMessageKind::User,
                segments: vec![make_segment("batched-user")],
            },
            InlineTranscriptLine {
                kind: InlineMessageKind::Info,
                segments: Vec::new(),
            },
            InlineTranscriptLine {
                kind: InlineMessageKind::Agent,
                segments: vec![make_segment("batched-agent")],
            },
        ],
    );

    assert_eq!(session.scroll_offset(), 0, "a pinned transcript follows the new lines");
    assert!(!session.user_scrolled);

    let transcript = visible_transcript(&mut session);
    let last = transcript
        .iter()
        .rev()
        .find(|line| !line.is_empty())
        .map(String::as_str)
        .expect("transcript must render content");
    assert_eq!(last, "batched-agent");
    assert!(transcript.iter().any(|line| line == "batched-user"));
}

#[test]
fn append_lines_preserves_a_scrolled_view() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    for index in 0..SEED_LINES {
        session.push_line(InlineMessageKind::Agent, vec![make_segment(&format!("{LABEL_PREFIX}-{index}"))]);
    }

    // Geometry is established on the first render; scroll after it so the
    // seed transcript exceeds the viewport.
    let _ = visible_transcript(&mut session);
    session.scroll_page_up();
    let before = visible_transcript(&mut session);
    let before_offset = session.scroll_offset();
    assert!(before_offset > 0, "seed transcript must exceed the viewport before the batch");
    assert!(session.user_scrolled);

    append_lines(
        &mut session,
        vec![InlineTranscriptLine {
            kind: InlineMessageKind::Agent,
            segments: vec![make_segment("batched-agent")],
        }],
    );

    // Batched appends follow the same rule as streamed singles: a scrolled-up
    // view keeps its position instead of jumping to the newest line.
    assert_eq!(session.scroll_offset(), before_offset + 1);
    assert!(session.user_scrolled, "a scrolled view stays user-owned");

    let after = visible_transcript(&mut session);
    assert_eq!(after.len(), before.len());
    let anchor_before = before.iter().find(|line| !line.is_empty());
    let anchor_after = after.iter().find(|line| !line.is_empty());
    assert_eq!(anchor_before, anchor_after, "the visible window follows content, not the end");
}

#[test]
fn streaming_state_detects_agent_lines_inside_a_batch() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(!session.is_streaming_final_answer);

    append_lines(
        &mut session,
        vec![
            InlineTranscriptLine {
                kind: InlineMessageKind::Info,
                segments: vec![make_segment("seed")],
            },
            InlineTranscriptLine {
                kind: InlineMessageKind::Agent,
                segments: vec![make_segment("answer")],
            },
        ],
    );

    assert!(session.is_streaming_final_answer, "an Agent line inside a batch starts streaming state");
}
