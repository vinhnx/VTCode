use super::*;

#[test]
fn input_compact_preview_for_image_path() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let image_path = "/tmp/Screenshot 2026-02-06 at 3.39.48 PM.png";

    session.insert_paste_text(image_path);

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Image:"));
    assert!(rendered.contains("Screenshot 2026-02-06"));
}
#[test]
fn input_compact_preview_for_quoted_image_path() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let image_path = "\"/tmp/Screenshot 2026-02-06 at 3.39.48 PM.png\"";

    session.insert_paste_text(image_path);

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Image:"));
    assert!(rendered.contains("Screenshot 2026-02-06"));
}
#[test]
fn control_g_launches_editor() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let event = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    assert!(matches!(result, Some(InlineEvent::LaunchEditor { draft }) if draft.is_empty()));
}
#[test]
fn control_g_launches_editor_with_draft() {
    let text = "hello world";
    let mut session = session_with_input(text, 0);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));

    assert!(matches!(result, Some(InlineEvent::LaunchEditor { draft }) if draft == text));
}
#[test]
fn control_g_launches_editor_from_plan_confirmation_modal() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.input_status_right = Some("model | 25% context".to_string());
    let plan = app_types::PlanContent::from_markdown(
        "Test Plan".to_string(),
        "## Plan of Work\n- Step 1",
        Some(".vtcode/plans/test-plan.md".to_string()),
    );
    show_plan_confirmation_overlay(&mut session, plan);

    let event = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    assert!(matches!(
        result,
        Some(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Hotkey(
            OverlayHotkeyAction::LaunchEditor
        ))))
    ));
    assert!(session.modal_state().is_none());
}
#[test]
fn plan_confirmation_modal_matches_four_way_gate_copy() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let plan = app_types::PlanContent::from_markdown(
        "Test Plan".to_string(),
        "## Implementation Plan\n1. Step",
        Some(".vtcode/plans/test-plan.md".to_string()),
    );
    show_plan_confirmation_overlay(&mut session, plan);

    let modal = session.modal_state().expect("plan confirmation modal should be present");
    assert_eq!(modal.title, "Ready to code?");
    assert_eq!(
        modal.lines.first().map(String::as_str),
        Some("A plan is ready to execute. Would you like to proceed?")
    );

    let list = modal.list.as_ref().expect("plan confirmation should include list options");
    assert_eq!(list.items.len(), 3);

    assert_eq!(list.items[0].title, "Yes, auto-accept edits");
    assert_eq!(list.items[0].subtitle.as_deref(), Some("Execute with auto-approval."));
    assert_eq!(list.items[0].badge.as_deref(), Some("Recommended"));

    assert_eq!(list.items[1].title, "Yes, manually approve edits");
    assert_eq!(list.items[1].subtitle.as_deref(), Some("Keep context and confirm each edit before applying."));

    assert_eq!(list.items[2].title, "Type feedback to revise the plan");
    assert_eq!(list.items[2].subtitle.as_deref(), Some("Return to planning workflow and refine the plan."));
}
#[test]
fn arrow_keys_never_launch_editor() {
    let text = "hello world";
    let mut session = session_with_input(text, 0);

    // Test Right arrow with all possible modifier combinations
    for modifiers in [
        KeyModifiers::empty(),
        KeyModifiers::CONTROL,
        KeyModifiers::SHIFT,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
        KeyModifiers::META,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        KeyModifiers::CONTROL | KeyModifiers::SUPER,
    ] {
        let event = KeyEvent::new(KeyCode::Right, modifiers);
        let result = session.process_key(event);
        assert!(
            !matches!(result, Some(InlineEvent::LaunchEditor { .. })),
            "Right arrow with modifiers {modifiers:?} should not launch editor"
        );
    }

    // Test other arrow keys for safety
    for key_code in [KeyCode::Left, KeyCode::Up, KeyCode::Down] {
        let event = KeyEvent::new(key_code, KeyModifiers::SUPER);
        let result = session.process_key(event);
        assert!(
            !matches!(result, Some(InlineEvent::LaunchEditor { .. })),
            "{key_code:?} with SUPER should not launch editor"
        );
    }
}
