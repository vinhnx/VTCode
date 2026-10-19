#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::super::*;
use super::helpers::*;
use crate::tui::core_tui::types::InlineTranscriptLine;

const SEED_LINES: usize = 40;

#[test]
fn append_lines_pins_transcript_to_bottom_without_follow_scroll() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    for index in 0..SEED_LINES {
        session.push_line(InlineMessageKind::Agent, vec![make_segment(&format!("{LABEL_PREFIX}-{index}"))]);
    }

    // Geometry is established on the first render; scroll after it so the
    // seed transcript exceeds the viewport.
    let _ = visible_transcript(&mut session);
    session.scroll_page_up();
    assert!(session.scroll_offset() > 0, "seed transcript must exceed the viewport before the batch");

    session.handle_command(InlineCommand::AppendLines {
        lines: vec![
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
    });

    assert_eq!(session.scroll_offset(), 0, "batched load must land on the last line");
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
