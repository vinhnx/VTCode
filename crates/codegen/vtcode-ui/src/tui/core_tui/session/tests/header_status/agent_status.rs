use super::*;

#[test]
fn permission_overlay_surfaces_action_required_status() {
    let mut session = fresh_session();
    session.show_overlay(OverlayRequest::List(ListOverlayRequest {
        title: "Tool Permission Required".to_string(),
        lines: vec!["Need approval".to_string()],
        footer_hint: None,
        items: vec![InlineListItem {
            title: "Approve Once".to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ToolApproval(true)),
            search_value: None,
            ..Default::default()
        }],
        selected: Some(InlineListSelection::ToolApproval(true)),
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

    assert!(rendered.contains("Approval required"));
    assert!(status_requires_shimmer("Approval required"));
}
#[test]
fn reduced_motion_keeps_background_task_status_as_static_text() {
    let mut session = fresh_session();
    session.appearance.reduce_motion_mode = true;
    session.set_background_activity_count(2);

    let rendered = session.render_input_status_line(VIEW_WIDTH).expect("background status line");
    let text = rendered.spans.iter().map(|span| span.content.as_ref()).collect::<String>();

    assert!(text.contains("Running 2 background tasks..."), "background status should remain readable: {text}");
    assert_eq!(rendered.spans[0].content.as_ref(), "Running 2 background tasks...");
}
#[test]
fn header_meta_line_renders_active_primary_agent() {
    let mut session = fresh_session();
    session.header_context.primary_agent = Some("planner".to_string());

    assert_header_contains_badge(&mut session, "Planner");
}
#[test]
fn set_primary_agent_command_updates_header_badge() {
    let mut session = fresh_session();

    session.handle_command(InlineCommand::SetPrimaryAgent { name: Some("reviewer".to_string()), color: None });
    assert_header_contains_badge(&mut session, "Reviewer");

    session.handle_command(InlineCommand::SetPrimaryAgent { name: None, color: None });
    let text = header_line_text(&mut session);
    assert!(text.contains("Build"));
    assert!(!text.contains("Reviewer"));
}
#[test]
fn bottom_status_excludes_primary_agent() {
    let mut session = fresh_session();

    assert_eq!(session.status_right_text(), None);

    session.handle_command(InlineCommand::SetInputStatus {
        left: None,
        right: Some("gpt-5.5 | 98% context left".to_string()),
    });
    session.handle_command(InlineCommand::SetPrimaryAgent { name: Some("reviewer".to_string()), color: None });

    assert_eq!(session.status_right_text(), Some("gpt-5.5 | 98% context left"));
}
#[test]
fn header_context_updates_preserve_active_primary_agent() {
    let mut session = fresh_session();
    session.handle_command(InlineCommand::SetPrimaryAgent { name: Some("planner".to_string()), color: None });

    let mut replacement = session.header_context.clone();
    replacement.primary_agent = None;
    replacement.model = "Model: replacement".to_string();
    session.handle_command(InlineCommand::SetHeaderContext { context: Box::new(replacement) });

    assert_header_contains_badge(&mut session, "Planner");
}
