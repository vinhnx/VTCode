use super::*;

#[test]
fn alt_s_remains_subprocesses_entrypoint() {
    let mut session = app_session_with_input("", 0);

    let event = session.process_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT));

    assert!(matches!(
        event,
        Some(app_types::InlineEvent::Submit(value)) if value == "/subprocesses"
    ));
}
#[test]
fn tab_cycles_primary_agent_when_composer_is_empty() {
    // Plain Tab now enqueues like Ctrl+Enter (empty idle -> ProcessLatestQueued);
    // agent cycling lives on Shift+Tab (BackTab).
    let mut session = app_session_with_input("", 0);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(event, Some(app_types::InlineEvent::ProcessLatestQueued)));

    let cycle = session.process_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));

    assert!(matches!(cycle, Some(app_types::InlineEvent::CyclePrimaryAgentPrevious)));
}
#[test]
fn tab_character_cycles_primary_agent_when_composer_is_empty() {
    // Char('\t') without Shift enqueues; Char('\t')+Shift cycles.
    let mut session = app_session_with_input("", 0);

    let event = session.process_key(KeyEvent::new(KeyCode::Char('\t'), KeyModifiers::NONE));

    assert!(matches!(event, Some(app_types::InlineEvent::ProcessLatestQueued)));

    let cycle = session.process_key(KeyEvent::new(KeyCode::Char('\t'), KeyModifiers::SHIFT));

    assert!(matches!(cycle, Some(app_types::InlineEvent::CyclePrimaryAgent)));
}
#[test]
fn core_tab_cycles_primary_agent_when_composer_is_empty() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(event, Some(InlineEvent::ProcessLatestQueued)));

    let cycle = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));

    assert!(matches!(cycle, Some(InlineEvent::CyclePrimaryAgent)));
}
#[test]
fn tab_does_not_cycle_primary_agent_while_running() {
    // Plain Tab enqueues (empty busy -> None), Shift+Tab cycling stays locked.
    let mut session = app_session_with_input("", 0);
    load_primary_agent_palette(&mut session);
    set_app_session_busy_status(&mut session);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "");
}
#[test]
fn tab_does_not_cycle_primary_agent_while_running_with_draft() {
    // Busy Tab with draft queues like Ctrl+Enter instead of cycling.
    let mut session = app_session_with_input("Review this", "Review this".len());
    load_primary_agent_palette(&mut session);
    set_app_session_busy_status(&mut session);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(
        event,
        Some(app_types::InlineEvent::QueueSubmit(value)) if value.text == "Review this"
    ));
    assert_eq!(session.core.input_manager.content(), "");
}
#[test]
fn shift_tab_does_not_cycle_primary_agent_while_running() {
    let mut session = app_session_with_input("", 0);
    load_primary_agent_palette(&mut session);
    set_app_session_busy_status(&mut session);

    let event = session.process_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));

    assert!(event.is_none());
}
#[test]
fn tab_cycles_primary_agent_back_to_default_after_last_agent() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetPrimaryAgent { name: Some("beta".to_string()), color: None });

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));

    assert!(matches!(event, Some(app_types::InlineEvent::CyclePrimaryAgent)));
}
#[test]
fn tab_accepts_inline_prompt_suggestion_before_primary_agent_cycle() {
    let mut session = app_session_with_input("Review the current", "Review the current".len());
    load_primary_agent_palette(&mut session);
    session
        .core
        .set_inline_prompt_suggestion("Review the current diff".to_string(), true);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "Review the current diff");
    assert!(session.core.inline_prompt_suggestion.suggestion.is_none());
}
#[test]
fn tab_cycles_primary_agent_with_draft() {
    // Plain Tab submits the draft like Ctrl+Enter; Shift+Tab cycles.
    let mut session = app_session_with_input("Review this", "Review this".len());
    load_primary_agent_palette(&mut session);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(
        event,
        Some(app_types::InlineEvent::Submit(value)) if value == "Review this"
    ));
    assert_eq!(session.core.input_manager.content(), "");

    let mut session = app_session_with_input("Review this", "Review this".len());
    load_primary_agent_palette(&mut session);
    let cycle = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));

    assert!(matches!(cycle, Some(app_types::InlineEvent::CyclePrimaryAgent)));
    assert_eq!(session.core.input_manager.content(), "Review this");
}
#[test]
fn tab_cycles_primary_agent_when_queued_input_exists() {
    // Plain Tab with empty draft tries the queue path; Shift+Tab cycles.
    let mut session = app_session_with_input("", 0);
    load_primary_agent_palette(&mut session);
    set_app_session_queued_inputs(&mut session, vec!["queued follow-up".to_string()]);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(event, Some(app_types::InlineEvent::ProcessLatestQueued)));
    assert_eq!(session.core.queued_inputs, vec!["queued follow-up"]);

    let cycle = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));

    assert!(matches!(cycle, Some(app_types::InlineEvent::CyclePrimaryAgent)));
}
#[test]
fn shift_tab_cycles_previous_primary_agent() {
    let mut session = app_session_with_input("", 0);
    load_primary_agent_palette(&mut session);

    let event = session.process_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));

    assert!(matches!(event, Some(app_types::InlineEvent::CyclePrimaryAgentPrevious)));
}
#[test]
fn tab_does_not_cycle_primary_agent_in_building_recovery_or_blocked_states() {
    for state in [ActivityState::Building, ActivityState::Recovery, ActivityState::Blocked] {
        // Plain Tab never cycles (it enqueues); Shift+Tab cycling stays locked.
        let mut session = app_session_with_input("", 0);
        load_primary_agent_palette(&mut session);
        session.handle_command(app_types::InlineCommand::SetActivityState(state));

        let tab = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(
            !matches!(
                tab,
                Some(app_types::InlineEvent::CyclePrimaryAgent | app_types::InlineEvent::CyclePrimaryAgentPrevious)
            ),
            "plain Tab must not cycle in {state:?}, got {tab:?}"
        );

        let shift_tab = session.process_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert!(shift_tab.is_none(), "mode switching must stay locked in {state:?}");
    }
}
