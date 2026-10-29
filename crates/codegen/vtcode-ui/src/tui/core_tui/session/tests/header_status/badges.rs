use super::*;

#[test]
fn header_badge_reports_finished_history_statically() {
    let mut session = fresh_session();
    session.set_background_finished_count(2);

    assert_eq!(session.background_header_badge_text().as_deref(), Some("✓ 2 done"));
    assert!(session.header_meta_line().to_string().contains("✓ 2 done"));

    let idle = session
        .render_input_status_line(VIEW_WIDTH)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
        .unwrap_or_default();
    assert!(idle.contains("2 agents finished"), "{idle}");
}
#[test]
fn question_mark_opens_help_overlay_when_input_is_empty() {
    let mut session = fresh_session();

    let result = session.process_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));

    assert!(result.is_none());
    let modal = session.modal_state().expect("help modal should open");
    assert_eq!(modal.title, "Keyboard Shortcuts");
    assert!(modal.is_help_modal, "should be a help modal");
}
#[test]
fn question_mark_inserts_character_when_input_has_content() {
    let mut session = session_with_input("why", 3);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "why?");
    assert_eq!(session.cursor(), 4);
    assert!(session.modal_state().is_none());
}
#[test]
fn header_shows_safe_badge_for_tools_policy_trust() {
    let mut session = fresh_session();
    session.header_context.workspace_trust = format!("{}tools policy", ui::HEADER_TRUST_PREFIX);
    session.input_manager.set_content("test".to_string());
    session.input_manager.set_cursor(session.input_manager.content().len());

    assert_header_contains_badge(&mut session, "Safe");
}
#[test]
fn header_shows_full_auto_trust_badge_for_full_auto_trust() {
    let mut session = fresh_session();
    session.header_context.workspace_trust = format!("{}full auto", ui::HEADER_TRUST_PREFIX);
    session.input_manager.set_content("test".to_string());
    session.input_manager.set_cursor(session.input_manager.content().len());

    assert_header_contains_badge(&mut session, "Full-auto");
}
#[test]
fn header_omits_auto_permission_badge() {
    let mut session = fresh_session();
    session.header_context.workspace_trust = format!("{}tools policy", ui::HEADER_TRUST_PREFIX);
    session.input_manager.set_content("test".to_string());
    session.input_manager.set_cursor(session.input_manager.content().len());

    let text = header_line_text(&mut session);
    assert!(text.contains("Safe"));
    assert!(!text.contains("Auto"));
}
#[test]
fn header_title_line_shows_model_context_window() {
    let mut session = fresh_session();
    session.header_context.provider = format!("{}Anthropic", ui::HEADER_PROVIDER_PREFIX);
    session.header_context.model = format!("{}claude-sonnet-4-6", ui::HEADER_MODEL_PREFIX);
    session.header_context.context_window_size = Some(1_000_000);

    let summary = line_text(&session.header_title_line());
    assert!(summary.contains("claude-sonnet-4-6 (1M)"));

    session.header_context.model = format!("{}claude-haiku-4-5", ui::HEADER_MODEL_PREFIX);
    session.header_context.context_window_size = Some(200_000);

    let summary = line_text(&session.header_title_line());
    assert!(summary.contains("claude-haiku-4-5 (200K)"));
}
#[test]
fn header_title_line_shows_service_tier_when_configured() {
    let mut session = fresh_session();
    session.header_context.provider = format!("{}openai", ui::HEADER_PROVIDER_PREFIX);
    session.header_context.model = format!("{}gpt-6-astra", ui::HEADER_MODEL_PREFIX);
    session.header_context.service_tier = Some(format!("{}ultrafast", ui::HEADER_SERVICE_TIER_PREFIX));

    let summary = line_text(&session.header_title_line());
    assert!(summary.contains("ULTRAFAST"), "tier should appear in title line, got: {summary}");

    session.header_context.service_tier = Some(format!("{}priority", ui::HEADER_SERVICE_TIER_PREFIX));
    let summary = line_text(&session.header_title_line());
    assert!(summary.contains("Priority"), "tier label should follow configuration, got: {summary}");
}
