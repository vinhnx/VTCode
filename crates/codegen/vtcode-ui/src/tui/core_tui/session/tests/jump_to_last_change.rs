#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::super::*;
use super::helpers::*;
use std::time::{Duration, Instant};

fn push_agent_lines(session: &mut Session, count: usize) {
    for index in 0..count {
        session.push_line(InlineMessageKind::Agent, vec![make_segment(&format!("line {index}"))]);
    }
}

fn prepare_sized_session(line_count: usize) -> Session {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.apply_transcript_width(VIEW_WIDTH);
    session.apply_transcript_rows(6);
    push_agent_lines(&mut session, line_count);
    session.ensure_scroll_metrics();
    session
}

#[test]
fn distinct_changes_tracked_but_streaming_repeats_collapse() {
    let mut session = prepare_sized_session(0);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("a")]);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("b")]);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("c")]);
    assert_eq!(session.last_change_line_idx, Some(2));

    // Streaming chunks to the same tail line must not change the target.
    session.append_inline(InlineMessageKind::Agent, make_segment(" +more"));
    session.append_inline(InlineMessageKind::Agent, make_segment(" +even-more"));
    assert_eq!(session.last_change_line_idx, Some(2));
}

#[test]
fn gate_needs_scrolled_up_with_a_tracked_change() {
    let mut session = prepare_sized_session(10);
    // At live bottom there is nothing to jump to.
    assert!(!session.should_show_jump_to_last_change(), "at live bottom there is nothing to jump to");

    session.scroll_page_up();
    assert!(session.should_show_jump_to_last_change(), "scrolled + tracked change shows");

    // No tracked change (fresh/cleared transcript) must not show.
    session.last_change_line_idx = None;
    assert!(!session.should_show_jump_to_last_change(), "no tracked change must not show while scrolled");
}

#[test]
fn jump_pins_tail_change_to_bottom() {
    let mut session = prepare_sized_session(10);
    session.scroll_page_up();
    assert!(session.scroll_offset() > 0);
    assert!(session.should_show_jump_to_last_change());

    assert!(session.jump_to_last_change());
    assert_eq!(session.scroll_offset(), 0, "tail change pins to live bottom (offset 0)");
    assert!(!session.should_show_jump_to_last_change(), "at bottom the hint hides");
}

#[test]
fn jump_pins_earlier_change_above_bottom_asymmetric() {
    let mut session = prepare_sized_session(10);
    // Simulate an edit to an earlier line after newer lines exist.
    session.record_transcript_change(1);
    session.scroll_page_up();
    assert!(session.current_max_scroll_offset() > 0);

    assert!(session.jump_to_last_change());
    let offset_after_early = session.scroll_offset();
    assert!(offset_after_early > 0, "earlier change stays scrolled (offset {offset_after_early})");

    // Contrast: tail change jumps to zero, earlier change does not.
    session.record_transcript_change(9);
    session.scroll_page_up();
    assert!(session.jump_to_last_change());
    assert_eq!(session.scroll_offset(), 0);
}

#[test]
fn new_distinct_change_updates_target() {
    let mut session = prepare_sized_session(5);
    session.scroll_page_up();
    assert!(session.jump_to_last_change());

    session.push_line(InlineMessageKind::Agent, vec![make_segment("new tail")]);
    assert_eq!(session.last_change_line_idx, Some(5));
}

#[test]
fn clear_screen_resets_jump_state() {
    let mut session = prepare_sized_session(4);
    session.scroll_page_up();
    assert!(session.jump_to_last_change());
    session.clear_screen();
    assert_eq!(session.last_change_line_idx, None);
    assert!(!session.should_show_jump_to_last_change());
}

#[test]
fn eviction_shifts_tracked_index_and_drops_prefix() {
    let mut session = prepare_sized_session(5);
    // History is [0..4], last=4.
    session.shift_tracked_change_after_eviction(2);
    assert_eq!(session.last_change_line_idx, Some(2));

    // Evicting past the last change clears it instead of underflowing.
    session.shift_tracked_change_after_eviction(5);
    assert_eq!(session.last_change_line_idx, None);
}

#[test]
fn jump_returns_false_without_target_or_width() {
    let mut empty = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(!empty.jump_to_last_change());

    let mut no_width = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    no_width.push_line(InlineMessageKind::Agent, vec![make_segment("x")]);
    no_width.push_line(InlineMessageKind::Agent, vec![make_segment("y")]);
    // Width 0 blocks row mapping.
    assert!(!no_width.jump_to_last_change());
}

#[test]
fn default_binding_is_ctrl_end_with_label() {
    use crate::tui::core_tui::session::action::BindingStore;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let store = BindingStore::default();
    let key = KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL);
    assert_eq!(store.resolve(&key), Some(Action::JumpToLastChange));
    assert!(store.primary_key_label(Action::JumpToLastChange).is_some());
}

#[test]
fn jump_dispatch_falls_back_to_bottom_when_scrolled_without_target() {
    let mut session = prepare_sized_session(8);
    session.scroll_page_up();
    assert!(session.scroll_offset() > 0);
    // Simulate fresh tracking (e.g. after clear + rescroll with no changes).
    session.last_change_line_idx = None;

    let event = action::dispatch_rebindable_action(&mut session, Action::JumpToLastChange);
    assert!(matches!(event, Some(InlineEvent::JumpToLastChange)));
    assert_eq!(session.scroll_offset(), 0);
}

#[test]
fn jump_dispatch_falls_back_to_cursor_end_when_at_bottom_without_target() {
    let mut session = prepare_sized_session(8);
    session.set_input("hello".to_string());
    session.set_cursor(0);
    session.last_change_line_idx = None;

    let event = action::dispatch_rebindable_action(&mut session, Action::JumpToLastChange);
    assert!(event.is_none());
    assert_eq!(session.input_manager.cursor(), 5, "cursor must move to buffer end");
}

#[test]
fn jump_dispatch_at_bottom_with_tracked_change_preserves_cursor_end() {
    let mut session = prepare_sized_session(8);
    session.set_input("hello".to_string());
    session.set_cursor(0);
    // A tracked change exists, but at the live bottom there is nothing to
    // reveal, so Ctrl+End must keep the legacy cursor-end behavior instead of
    // silently doing nothing.
    assert!(session.last_change_line_idx.is_some());
    assert!(!session.should_show_jump_to_last_change());

    let event = action::dispatch_rebindable_action(&mut session, Action::JumpToLastChange);
    assert!(event.is_none());
    assert_eq!(session.input_manager.cursor(), 5, "cursor must move to buffer end");
}

#[test]
fn wheel_scroll_via_coalesced_path_shows_jump_hint() {
    let mut session = prepare_sized_session(10);
    // Mouse wheel-up routes through apply_coalesced_scroll with a negative
    // line delta; the hint must appear there too, not only on PgUp/line scrolls.
    session.apply_coalesced_scroll(-3, 0);
    assert!(session.scroll_offset() > 0, "coalesced scroll must move the view");
    assert!(session.should_show_jump_to_last_change());

    // Scrolling back to the bottom clears the hint.
    session.apply_coalesced_scroll(3, 0);
    assert_eq!(session.scroll_offset(), 0);
    assert!(!session.should_show_jump_to_last_change());
}

#[test]
fn append_while_scrolled_counts_pending_and_preserves_position() {
    let mut session = prepare_sized_session(10);
    session.scroll_page_up();
    let offset_before = session.scroll_offset();
    assert!(offset_before > 0);
    assert_eq!(session.pending_new_messages, 0, "scrolling over old content counts nothing");

    for index in 0..3 {
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![make_segment(&format!("fresh {index}"))],
        });
    }
    assert_eq!(session.pending_new_messages, 3, "one unseen row per appended line");
    assert_eq!(session.scroll_offset(), offset_before + 3, "viewport must not drift toward new content");

    let status = session.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let text = status.spans.iter().map(|span| span.content.as_ref()).collect::<String>();
    assert!(text.contains("↓ 3 new"), "pending arrivals surface in the footer, got: {text}");
}

#[test]
fn append_while_following_pins_bottom_without_pending() {
    let mut session = prepare_sized_session(10);
    for index in 0..3 {
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![make_segment(&format!("fresh {index}"))],
        });
    }
    assert_eq!(session.scroll_offset(), 0, "following pins to the live bottom");
    assert_eq!(session.pending_new_messages, 0, "visible arrivals count nothing");
    assert!(session.build_scroll_indicator().is_none(), "no indicator at the bottom edge");
}

#[test]
fn moving_toward_bottom_shrinks_pending_until_it_clears() {
    let mut session = prepare_sized_session(10);
    session.scroll_page_up();
    for index in 0..4 {
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![make_segment(&format!("fresh {index}"))],
        });
    }
    assert_eq!(session.pending_new_messages, 4);
    // Rows reveal oldest-first, so the count only shrinks once the new rows
    // themselves start entering the viewport.
    while session.scroll_offset() > 2 {
        session.scroll_line_down();
    }
    assert_eq!(session.pending_new_messages, 2);
    session.scroll_to_bottom();
    assert_eq!(session.pending_new_messages, 0);
    assert!(session.build_scroll_indicator().is_none());
}

#[test]
fn pasted_message_while_scrolled_preserves_position() {
    let mut session = prepare_sized_session(10);
    session.scroll_page_up();
    let offset_before = session.scroll_offset();
    let max_before = session.current_max_scroll_offset();
    session.handle_command(InlineCommand::AppendPastedMessage {
        kind: InlineMessageKind::User,
        text: "pasted body".to_string(),
        line_count: 1,
    });
    // User rows carry chrome, so assert the layout-independent invariant:
    // the viewport shifts by exactly the row growth, all of it unseen.
    let grown = session.current_max_scroll_offset().saturating_sub(max_before);
    assert!(grown > 0, "paste must add rows");
    assert_eq!(session.scroll_offset(), offset_before + grown, "paste must not drag a scrolled view");
    assert_eq!(session.pending_new_messages, grown);
}

#[test]
fn footer_hint_appends_jump_label_while_scrolled() {
    let mut session = prepare_sized_session(10);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("session status".to_string()),
        right: None,
    });

    let before = session.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let before_text = before.spans.iter().map(|span| span.content.as_ref()).collect::<String>();
    assert!(!before_text.contains("Jump to last change"), "at bottom the hint must not render: {before_text}");

    session.scroll_page_up();
    let after = session.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let after_text = after.spans.iter().map(|span| span.content.as_ref()).collect::<String>();
    assert!(
        after_text.contains("Jump to last change [Ctrl+End]"),
        "scrolled view must show the jump hint, got: {after_text}"
    );
}

#[test]
fn jump_arms_flash_and_resolves_its_live_row_range() {
    let mut session = prepare_sized_session(10);
    session.scroll_page_up();
    assert!(session.jump_to_last_change());
    let flash = session.transcript_flash.as_ref().expect("jump arms a flash");
    assert_eq!(flash.line_idx, 9);
    assert!(flash.expires_at > Instant::now());
    let (start, end) = session
        .transcript_flash_rows(VIEW_WIDTH)
        .expect("live flash resolves a row range");
    assert!(start < end, "landed message spans at least one row");
    // Asymmetric: an earlier change flashes its own message, not the tail.
    session.record_transcript_change(1);
    session.scroll_page_up();
    assert!(session.jump_to_last_change());
    assert_eq!(session.transcript_flash.as_ref().expect("re-jump re-arms").line_idx, 1);
    let (early_start, early_end) = session.transcript_flash_rows(VIEW_WIDTH).expect("re-armed flash resolves");
    assert!(early_start < start, "earlier message lands on earlier rows");
    assert!(early_start < early_end);
}

#[test]
fn flash_expires_on_tick_and_rests_idle_afterwards() {
    let mut idle = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(!idle.needs_animation_tick());
    idle.arm_transcript_flash(0);
    assert!(idle.needs_animation_tick(), "live flash needs ticks");
    idle.transcript_flash.as_mut().expect("arm stores a flash").expires_at = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("test clock backdates 1s");
    idle.handle_tick();
    assert!(idle.transcript_flash.is_none());
    assert!(!idle.needs_animation_tick(), "expired flash must rest");
}

#[test]
fn eviction_and_clear_screen_drop_the_flash() {
    let mut session = prepare_sized_session(10);
    session.scroll_page_up();
    assert!(session.jump_to_last_change());
    assert!(session.transcript_flash.is_some());
    session.shift_tracked_change_after_eviction(5);
    assert!(session.transcript_flash.is_none(), "eviction drops the index-keyed tint");
    // Re-arm, then clear the whole transcript.
    session.record_transcript_change(4);
    session.scroll_page_up();
    assert!(session.jump_to_last_change());
    assert!(session.transcript_flash.is_some());
    session.clear_screen();
    assert!(session.transcript_flash.is_none(), "clear drops the tint");
}
