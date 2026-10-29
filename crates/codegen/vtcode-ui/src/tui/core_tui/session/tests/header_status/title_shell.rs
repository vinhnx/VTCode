use super::*;

#[test]
fn non_permission_overlay_surfaces_generic_action_required_status() {
    let mut session = fresh_session();
    session.show_overlay(OverlayRequest::List(ListOverlayRequest {
        title: "Open External Link".to_string(),
        lines: vec!["example.com".to_string()],
        footer_hint: None,
        items: vec![InlineListItem {
            title: "Open".to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction("open".to_string())),
            search_value: None,
            ..Default::default()
        }],
        selected: None,
        search: None,
        hotkeys: Vec::new(),
        status: None,
    }));

    let rendered = session
        .render_input_status_line(VIEW_WIDTH)
        .expect("input status line")
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("Action required"));
    assert!(status_requires_shimmer("Action required"));
}
#[test]
fn status_requires_shimmer_for_input_required_states() {
    assert!(status_requires_shimmer("Approval required"));
    assert!(status_requires_shimmer("Input required"));
    assert!(status_requires_shimmer("Waiting for approval"));
    assert!(status_requires_shimmer("Action required"));
}
#[test]
fn status_requires_shimmer_preserves_case_and_spinner_matching() {
    assert!(status_requires_shimmer("  RUNNING TOOL: cargo check  "));
    assert!(status_requires_shimmer("WAITING FOR INPUT"));
    assert!(status_requires_shimmer("- Compiling workspace"));
    assert!(!status_requires_shimmer("Completed successfully"));
}
#[test]
fn unchanged_terminal_title_commands_do_not_request_redraw() {
    let mut session = fresh_session();
    assert!(session.take_redraw(), "fresh sessions start dirty");

    let command = InlineCommand::SetTerminalTitleThreadLabel { label: Some("main".to_string()) };
    session.handle_command(command);
    assert!(session.take_redraw(), "changed title state should request a redraw");

    session.handle_command(InlineCommand::SetTerminalTitleThreadLabel { label: Some("main".to_string()) });
    assert!(!session.take_redraw(), "unchanged title state should not request a redraw");
}
#[test]
fn bang_prefix_input_shows_shell_mode_status_hint() {
    let mut session = fresh_session();
    session.set_input("!echo hello".to_string());

    let spans = session
        .build_input_status_widget_data(VIEW_WIDTH)
        .expect("expected shell mode status hint");
    let rendered: String = spans.iter().map(|span| span.content.clone().into_owned()).collect();

    assert!(rendered.contains("Shell mode (!):"));
}
#[test]
fn bang_prefix_input_enables_shell_mode_border_title() {
    let mut session = fresh_session();
    session.set_input("   !ls -la".to_string());

    assert_eq!(session.shell_mode_border_title(), Some(" ! Shell mode "));
}
#[test]
fn bang_prefix_input_uses_zero_padding_for_visible_input_area() {
    let mut session = fresh_session();
    session.set_input("!echo hello".to_string());

    assert_eq!(session.input_block_padding(), ratatui::widgets::Padding::new(0, 0, 0, 0));
}
#[test]
fn non_bang_input_has_no_shell_mode_border_title() {
    let mut session = fresh_session();
    session.set_input("run ls -la".to_string());

    assert_eq!(session.shell_mode_border_title(), None);
}
#[test]
fn non_bang_input_uses_default_padding() {
    let mut session = fresh_session();
    session.set_input("run ls -la".to_string());

    assert_eq!(
        session.input_block_padding(),
        ratatui::widgets::Padding::new(
            ui::INLINE_INPUT_PADDING_HORIZONTAL,
            ui::INLINE_INPUT_PADDING_HORIZONTAL,
            ui::INLINE_INPUT_PADDING_VERTICAL,
            ui::INLINE_INPUT_PADDING_VERTICAL,
        )
    );
}
