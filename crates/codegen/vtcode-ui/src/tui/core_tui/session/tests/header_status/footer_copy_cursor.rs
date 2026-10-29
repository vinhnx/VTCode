use super::*;

#[test]
fn footer_git_status_does_not_repeat_git_label() {
    let mut session = fresh_session();
    setup_shimmer_session(&mut session, "git: feature/login | ✓");

    let rendered = session.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let text = rendered.spans.iter().map(|span| span.content.as_ref()).collect::<String>();

    assert_eq!(text.matches("git:").count(), 0);
    assert!(text.contains("feature/login ✓"), "unexpected footer: {text}");
}
#[test]
fn footer_git_status_without_separator_does_not_repeat_git_label() {
    let mut session = fresh_session();
    setup_shimmer_session(&mut session, "git: main*");

    let rendered = session.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let text = rendered.spans.iter().map(|span| span.content.as_ref()).collect::<String>();

    assert_eq!(text, "main*");
}
#[test]
fn copy_notification_renders_in_input_status_line() {
    let mut session = fresh_session();
    session.show_copy_notification(277);

    let rendered = session
        .render_input_status_line(VIEW_WIDTH)
        .expect("input status line")
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("copied 277 chars to clipboard"), "got: {rendered}");
}
#[test]
fn copy_notification_includes_char_count() {
    assert_eq!(Session::format_copy_notification(1), "copied 1 char to clipboard");
    assert_eq!(Session::format_copy_notification(277), "copied 277 chars to clipboard");

    let mut session = fresh_session();
    session.show_copy_notification(1);
    assert_eq!(session.copy_notification_text().as_deref(), Some("copied 1 char to clipboard"));
}
#[test]
fn copy_notification_expires_after_duration() {
    let mut session = fresh_session();
    session.show_copy_notification(5);
    session.copy_notification_until = Some(Instant::now().checked_sub(Duration::from_secs(1)).unwrap());
    session.handle_tick();

    let rendered = session
        .render_input_status_line(VIEW_WIDTH)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
        .unwrap_or_default();

    assert!(!rendered.contains("chars to clipboard"));
}
#[test]
fn cursor_visible_while_scrolling() {
    let mut session = fresh_session();
    let initial = input_data(&mut session);
    assert!(initial.cursor_should_be_visible);

    session.scroll_line_down();
    let during_scroll = input_data(&mut session);
    assert!(during_scroll.cursor_should_be_visible);
    assert!(session.use_steady_cursor());

    session.scroll_cursor_steady_until = Some(Instant::now().checked_sub(Duration::from_millis(1)).unwrap());
    session.handle_tick();

    assert!(!session.use_steady_cursor());
    let after_scroll = input_data(&mut session);
    assert!(after_scroll.cursor_should_be_visible);
}
#[test]
fn cursor_steady_during_shimmer() {
    let mut session = fresh_session();
    let initial = input_data(&mut session);
    assert!(initial.cursor_should_be_visible);
    assert!(!session.use_steady_cursor());

    setup_shimmer_session(&mut session, "Running command: test");
    let during_shimmer = input_data(&mut session);
    assert!(during_shimmer.cursor_should_be_visible);
    assert!(session.use_steady_cursor());

    clear_shimmer_session(&mut session);
    assert!(!session.use_steady_cursor());
    let after_shimmer = input_data(&mut session);
    assert!(after_shimmer.cursor_should_be_visible);
}
#[test]
fn cursor_fake_during_status_shimmer() {
    let mut session = fresh_session();
    let initial = input_data(&mut session);
    assert!(initial.cursor_should_be_visible);
    assert!(!initial.use_fake_cursor);

    setup_shimmer_session(&mut session, "Loading (Esc, Ctrl+C, or /stop to stop)");
    let during_shimmer = input_data(&mut session);
    assert!(during_shimmer.cursor_should_be_visible);
    assert!(during_shimmer.use_fake_cursor);
    assert!(session.use_steady_cursor());

    clear_shimmer_session(&mut session);
    let after_shimmer = input_data(&mut session);
    assert!(after_shimmer.cursor_should_be_visible);
    assert!(!after_shimmer.use_fake_cursor);
}
