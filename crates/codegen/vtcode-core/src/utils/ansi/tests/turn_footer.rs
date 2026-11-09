//! Per-turn wall-clock footer: deferral inside a turn, immediate outside it.
//!
//! `run_turn_loop` arms deferral with `begin_turn_message_timestamp` and stamps
//! once with `finish_turn_message_timestamp`, so a turn carrying several agent
//! messages shows exactly one clock. Anything rendered outside a turn (the
//! session-exit reprint, pre-turn prompt replies) has no boundary to flush at
//! and must stamp immediately.

use super::{collect_kinded_append_lines, is_timestamp_footer};
use crate::ui::tui::{InlineHandle, InlineMessageKind};
use crate::utils::ansi::{AnsiRenderer, MessageStyle};

/// Renderer with an inline sink, deferral armed as `run_turn_loop` does.
fn turn_renderer() -> (AnsiRenderer, tokio::sync::mpsc::UnboundedReceiver<crate::ui::InlineCommand>) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.begin_turn_message_timestamp();
    (renderer, receiver)
}

/// Renderer with an inline sink and no open turn.
fn idle_renderer() -> (AnsiRenderer, tokio::sync::mpsc::UnboundedReceiver<crate::ui::InlineCommand>) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    (AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default()), receiver)
}

#[test]
fn response_stamps_timestamp_footer_at_turn_end() {
    let (mut renderer, mut receiver) = turn_renderer();
    assert!(renderer.show_message_timestamps(), "timestamps default on");

    renderer.line(MessageStyle::Response, "hello").unwrap();

    let body_lines = collect_kinded_append_lines(&mut receiver);
    assert_eq!(body_lines.len(), 1, "body only before turn end: {body_lines:?}");
    assert_eq!(body_lines[0].0, InlineMessageKind::Agent);
    assert_eq!(body_lines[0].1, "  hello");

    renderer.finish_turn_message_timestamp().unwrap();

    let footer_lines = collect_kinded_append_lines(&mut receiver);
    assert_eq!(footer_lines.len(), 1, "exactly one footer at turn end: {footer_lines:?}");
    assert_eq!(footer_lines[0].0, InlineMessageKind::Agent, "footer must glue to its message block");
    assert!(is_timestamp_footer(&footer_lines[0].1), "footer shape: {:?}", footer_lines[0].1);
}

#[test]
fn intermediate_responses_do_not_stamp_until_turn_end() {
    let (mut renderer, mut receiver) = turn_renderer();

    renderer.line(MessageStyle::Response, "first intermediate").unwrap();
    renderer.line(MessageStyle::ToolOutput, "tool output").unwrap();
    renderer.line(MessageStyle::Response, "second intermediate").unwrap();

    let body_lines = collect_kinded_append_lines(&mut receiver);
    assert!(
        !body_lines.iter().any(|(_, text)| is_timestamp_footer(text)),
        "no per-response clock: {body_lines:?}"
    );

    renderer.finish_turn_message_timestamp().unwrap();
    let footer_lines = collect_kinded_append_lines(&mut receiver);
    assert_eq!(footer_lines.len(), 1, "exactly one footer for the whole turn: {footer_lines:?}");
    assert!(is_timestamp_footer(&footer_lines[0].1), "footer shape: {:?}", footer_lines[0].1);
}

/// Regression: the session-exit reprint renders the final answer on a renderer
/// that never reaches a turn boundary. Deferral must not swallow its clock.
#[test]
fn out_of_turn_response_stamps_immediately() {
    let (mut renderer, mut receiver) = idle_renderer();

    renderer.line(MessageStyle::Response, "reprinted answer").unwrap();

    let lines = collect_kinded_append_lines(&mut receiver);
    assert_eq!(lines.len(), 2, "body plus its clock: {lines:?}");
    assert_eq!(lines[0].1, "  reprinted answer");
    assert!(is_timestamp_footer(&lines[1].1), "closing the block: {:?}", lines[1].1);
}

#[test]
fn out_of_turn_streamed_response_stamps_immediately() {
    let (mut renderer, mut receiver) = idle_renderer();

    renderer.stream_markdown_response("streamed answer", 0).unwrap();
    renderer.finish_streamed_response().unwrap();

    let lines = collect_kinded_append_lines(&mut receiver);
    assert!(
        lines.iter().any(|(_, text)| is_timestamp_footer(text)),
        "sinkless-boundary stream must stamp on finish: {lines:?}"
    );
}

/// Regression: the session reuses one renderer. A finished turn must disarm
/// deferral, otherwise every later response waits for a boundary that is gone.
#[test]
fn finish_turn_disarms_deferral_for_later_output() {
    let (mut renderer, mut receiver) = turn_renderer();

    renderer.line(MessageStyle::Response, "in-turn body").unwrap();
    renderer.finish_turn_message_timestamp().unwrap();
    let _ = collect_kinded_append_lines(&mut receiver);

    renderer.line(MessageStyle::Response, "later out-of-turn body").unwrap();

    let lines = collect_kinded_append_lines(&mut receiver);
    assert!(
        lines.iter().any(|(_, text)| is_timestamp_footer(text)),
        "post-turn output must stamp without another boundary: {lines:?}"
    );
}

#[test]
fn response_footer_suppressed_when_disabled() {
    let (mut renderer, mut receiver) = turn_renderer();
    renderer.set_show_message_timestamps(false);

    renderer.line(MessageStyle::Response, "hello").unwrap();
    renderer.finish_turn_message_timestamp().unwrap();

    let lines = collect_kinded_append_lines(&mut receiver);
    assert_eq!(lines.len(), 1, "body only: {lines:?}");
    assert!(!is_timestamp_footer(&lines[0].1));
}

#[test]
fn response_footer_skipped_for_empty_response() {
    let (mut renderer, mut receiver) = turn_renderer();

    renderer.line(MessageStyle::Response, "   ").unwrap();
    renderer.finish_turn_message_timestamp().unwrap();

    let lines = collect_kinded_append_lines(&mut receiver);
    assert!(!lines.iter().any(|(_, text)| is_timestamp_footer(text)), "no footer on empty body: {lines:?}");
}

#[test]
fn response_footer_skipped_during_transcript_batch() {
    use crate::ui::InlineCommand;

    let (mut renderer, mut receiver) = turn_renderer();

    renderer.begin_transcript_batch();
    renderer.line(MessageStyle::Response, "replayed answer").unwrap();
    renderer.end_transcript_batch();
    renderer.finish_turn_message_timestamp().unwrap();

    let mut batched = Vec::new();
    while let Ok(command) = receiver.try_recv() {
        match command {
            InlineCommand::AppendLines { lines } => batched.extend(lines),
            _ => panic!("replay must batch every line in one AppendLines command"),
        }
    }
    let texts = batched
        .iter()
        .map(|line| line.segments.iter().map(|segment| segment.text.as_str()).collect::<String>())
        .collect::<Vec<_>>();
    assert!(texts.iter().any(|text| text.contains("replayed answer")), "body missing: {texts:?}");
    assert!(
        !texts.iter().any(|text| is_timestamp_footer(text)),
        "replayed history must not gain fresh timestamps: {texts:?}"
    );
}

#[test]
fn response_footer_never_recorded_in_transcript_log() {
    use crate::utils::transcript;

    let (mut renderer, mut receiver) = turn_renderer();

    // No global clear and no presence assert: other tests share the log, and
    // no fixture body is footer-shaped, so absence is race-free either way.
    renderer.line(MessageStyle::Response, "uniquely stamped body").unwrap();
    renderer.finish_turn_message_timestamp().unwrap();
    let _ = collect_kinded_append_lines(&mut receiver);

    let snapshot = transcript::snapshot();
    assert!(
        !snapshot.iter().any(|line| is_timestamp_footer(line)),
        "chrome must not leak into tail-matching and hooks"
    );
}

#[test]
fn non_response_styles_carry_no_footer() {
    let (mut renderer, mut receiver) = turn_renderer();

    renderer.line(MessageStyle::Info, "a plain note").unwrap();
    renderer.finish_turn_message_timestamp().unwrap();

    let lines = collect_kinded_append_lines(&mut receiver);
    assert!(!lines.is_empty(), "body must render");
    assert!(!lines.iter().any(|(_, text)| is_timestamp_footer(text)), "info must not stamp: {lines:?}");
}

#[test]
fn streaming_defers_footer_until_finish() {
    use crate::ui::InlineCommand;

    let (mut renderer, mut receiver) = turn_renderer();

    renderer.stream_markdown_response("streamed answer", 0).unwrap();
    let mut saw_append = false;
    while let Ok(command) = receiver.try_recv() {
        match command {
            InlineCommand::ReplaceLast { .. } => {}
            InlineCommand::AppendLine { .. } => saw_append = true,
            _ => {}
        }
    }
    assert!(!saw_append, "per-delta streaming must not stamp");

    renderer.finish_streamed_response().unwrap();
    renderer.finish_turn_message_timestamp().unwrap();
    let lines = collect_kinded_append_lines(&mut receiver);
    assert_eq!(lines.len(), 1, "exactly one footer at turn end: {lines:?}");
    assert_eq!(lines[0].0, InlineMessageKind::Agent);
    assert!(is_timestamp_footer(&lines[0].1), "footer shape: {:?}", lines[0].1);
}

#[test]
fn finish_streamed_response_without_sink_is_noop() {
    let mut renderer = AnsiRenderer::stdout();
    renderer.begin_turn_message_timestamp();
    renderer.finish_streamed_response().expect("sinkless finish must succeed");
    renderer
        .finish_turn_message_timestamp()
        .expect("sinkless turn finish must succeed");
}

#[test]
fn finish_turn_message_timestamp_without_pending_is_noop() {
    let (mut renderer, mut receiver) = turn_renderer();

    renderer.finish_turn_message_timestamp().unwrap();

    assert!(collect_kinded_append_lines(&mut receiver).is_empty(), "no body means no clock");
}

#[test]
fn begin_turn_clears_stale_pending_footer() {
    let (mut renderer, mut receiver) = turn_renderer();

    renderer.line(MessageStyle::Response, "aborted turn body").unwrap();
    renderer.begin_turn_message_timestamp();
    renderer.finish_turn_message_timestamp().unwrap();

    let lines = collect_kinded_append_lines(&mut receiver);
    assert!(
        !lines.iter().any(|(_, text)| is_timestamp_footer(text)),
        "a new turn must not inherit an aborted turn's footer: {lines:?}"
    );
}

#[test]
fn direct_response_skips_transcript_log_for_footer() {
    use crate::utils::transcript;

    // Direct mode writes real stdout (captured by the test runner); the log
    // assertion below proves the footer guard ran on this path too. Absence
    // only: the log is shared with parallel tests.
    let mut renderer = AnsiRenderer::stdout();
    renderer.begin_turn_message_timestamp();
    renderer.line(MessageStyle::Response, "direct uniquely stamped body").unwrap();
    renderer.finish_turn_message_timestamp().unwrap();

    let snapshot = transcript::snapshot();
    assert!(!snapshot.iter().any(|line| is_timestamp_footer(line)), "direct footer must not leak into the log");
}
