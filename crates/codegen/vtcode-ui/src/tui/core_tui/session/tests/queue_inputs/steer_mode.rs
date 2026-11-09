use super::*;
use crate::tui::core_tui::session::config::BusyEnterBehavior;

#[test]
fn default_busy_enter_behavior_is_queue() {
    assert_eq!(AppearanceConfig::default().busy_enter_behavior, BusyEnterBehavior::Queue);
}

#[test]
fn steer_mode_busy_enter_emits_steer() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.appearance.busy_enter_behavior = BusyEnterBehavior::Steer;
    set_busy_status(&mut session);
    session.set_input("refine the search".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match &event {
        Some(InlineEvent::Steer(value)) => {
            assert_eq!(value.text, "refine the search");
        }
        other => panic!("expected Steer in steer mode, got: {other:?}"),
    }
    // Steered input injects into the live turn: no optimistic local queue entry.
    assert!(session.queued_inputs.is_empty());
    assert_eq!(session.input_manager.content(), "");
}

#[test]
fn steer_mode_busy_slash_stays_queued() {
    // Slash commands are runloop actions, not model input: they must never
    // steer, even when the mode is steer.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.appearance.busy_enter_behavior = BusyEnterBehavior::Steer;
    set_busy_status(&mut session);
    session.set_input("/model gpt-4o".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match &event {
        Some(InlineEvent::QueueSubmit(value)) => {
            assert_eq!(value.text, "/model gpt-4o");
            assert!(!value.batchable, "slash commands must stay one per turn");
        }
        other => panic!("expected QueueSubmit for slash in steer mode, got: {other:?}"),
    }
    assert_eq!(session.queued_inputs, vec!["/model gpt-4o".to_string()]);
}

#[test]
fn steer_mode_bare_slash_stays_queued() {
    // A lone "/" has no command name, but it still starts with a slash:
    // both surfaces must queue it rather than steering model text.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.appearance.busy_enter_behavior = BusyEnterBehavior::Steer;
    set_busy_status(&mut session);
    session.set_input("/".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(&event, Some(InlineEvent::QueueSubmit(value)) if value.text == "/"),
        "expected QueueSubmit for bare slash in steer mode, got: {event:?}"
    );
}

#[test]
fn steer_mode_busy_attachment_stays_queued() {
    // The steering channel is text-only: attachment payloads stay queued so
    // they dispatch whole instead of bouncing back as a restored draft.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.appearance.busy_enter_behavior = BusyEnterBehavior::Steer;
    set_busy_status(&mut session);
    session.set_input("see image".to_string());
    session
        .input_manager
        .set_attachments(vec![ContentPart::image("encoded", "image/png")]);

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match &event {
        Some(InlineEvent::QueueSubmit(value)) => {
            assert_eq!(value.text, "see image");
            assert!(value.has_attachments(), "attachment must stay queued with its message");
        }
        other => panic!("expected QueueSubmit with attachment in steer mode, got: {other:?}"),
    }
    assert_eq!(session.queued_inputs, vec!["see image".to_string()]);
}

#[test]
fn steer_mode_busy_control_enter_still_queues() {
    // Ctrl+Enter keeps its explicit queue role regardless of the mode.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.appearance.busy_enter_behavior = BusyEnterBehavior::Steer;
    set_busy_status(&mut session);
    session.set_input("batch me".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    match &event {
        Some(InlineEvent::QueueSubmit(value)) => {
            assert_eq!(value.text, "batch me");
            assert!(value.batchable, "busy Ctrl+Enter plain text must coalesce");
        }
        other => panic!("expected batchable QueueSubmit for Ctrl+Enter in steer mode, got: {other:?}"),
    }
}

#[test]
fn steer_mode_busy_tab_still_queues() {
    // Tab keeps its explicit queue role regardless of the mode.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.appearance.busy_enter_behavior = BusyEnterBehavior::Steer;
    set_busy_status(&mut session);
    session.set_input("tabbed follow-up".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    match &event {
        Some(InlineEvent::QueueSubmit(value)) => {
            assert_eq!(value.text, "tabbed follow-up");
            assert!(value.batchable, "busy Tab plain text must coalesce");
        }
        other => panic!("expected batchable QueueSubmit for Tab in steer mode, got: {other:?}"),
    }
}

#[test]
fn app_steer_mode_busy_enter_emits_steer() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.appearance.busy_enter_behavior = BusyEnterBehavior::Steer;
    set_app_session_busy_status(&mut session);
    session.core.set_input("refine the search".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match &event {
        Some(AppInlineEvent::Steer(value)) => {
            assert_eq!(value.text, "refine the search");
        }
        other => panic!("expected Steer in steer mode, got: {other:?}"),
    }
    assert!(session.core.queued_inputs.is_empty());
    assert_eq!(session.core.input_manager.content(), "");
}

#[test]
fn app_steer_mode_busy_slash_never_steers() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.appearance.busy_enter_behavior = BusyEnterBehavior::Steer;
    set_app_session_busy_status(&mut session);
    session.core.set_input("/copy".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(&event, Some(AppInlineEvent::Submit(value)) if value.text == "/copy"),
        "expected immediate Submit for slash in steer mode, got: {event:?}"
    );
}

#[test]
fn app_steer_mode_bare_slash_stays_queued() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.appearance.busy_enter_behavior = BusyEnterBehavior::Steer;
    set_app_session_busy_status(&mut session);
    session.core.set_input("/".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(&event, Some(AppInlineEvent::QueueSubmit(value)) if value.text == "/"),
        "expected QueueSubmit for bare slash in steer mode, got: {event:?}"
    );
}
