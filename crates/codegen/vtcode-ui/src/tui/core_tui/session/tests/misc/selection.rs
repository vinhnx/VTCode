use super::*;

#[test]
fn overlay_retains_input_ownership_across_activity_transitions() {
    use crate::tui::core_tui::session::input_manager::InputOwner;
    for initial in [
        ActivityState::Idle,
        ActivityState::StartingBuild,
        ActivityState::Building,
    ] {
        for next in [
            ActivityState::Idle,
            ActivityState::StartingBuild,
            ActivityState::Building,
            ActivityState::Blocked,
        ] {
            let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
            session.handle_command(InlineCommand::SetActivityState(initial));
            session.handle_command(InlineCommand::ShowOverlay {
                request: Box::new(OverlayRequest::Modal(crate::tui::core_tui::types::ModalOverlayRequest {
                    title: "Ownership".to_string(),
                    lines: Vec::new(),
                    secure_prompt: None,
                    is_help_modal: false,
                })),
            });
            session.handle_command(InlineCommand::SetActivityState(next));
            assert_eq!(session.input_owner(), InputOwner::Overlay);
            assert!(!session.input_enabled());
            session.close_overlay();
            assert_eq!(session.input_enabled(), !next.is_busy(), "{initial:?} -> {next:?}");
        }
    }
}
#[test]
fn clear_mouse_selection_reports_and_drops_a_visible_selection() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    assert!(!session.clear_mouse_selection(), "an empty selection must report no change");

    session.mouse_selection.set_selection((2, 3), (9, 3));
    assert!(session.mouse_selection.has_selection);

    assert!(session.clear_mouse_selection(), "a visible selection must report a change");
    assert!(!session.mouse_selection.has_selection);
    assert!(!session.mouse_selection.is_selecting);
    assert!(!session.clear_mouse_selection(), "clearing twice is a no-op");
}
#[test]
fn escape_dismisses_selection_before_arming_rewind() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.mouse_selection.set_selection((1, 1), (6, 1));

    let event = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(event.is_none(), "dismissing a selection consumes the key, got {event:?}");
    assert!(!session.mouse_selection.has_selection, "Esc must clear the highlight");
    assert_eq!(session.last_escape_press, None, "dismissal must not arm the rewind double-press");
}
#[test]
fn escape_still_interrupts_running_activity_with_a_selection() {
    // Asymmetric guard: the selection branch must not shadow interrupt
    // precedence while a turn is running.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.mouse_selection.set_selection((1, 1), (6, 1));
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: read_file".to_string()),
        right: None,
    });
    assert!(session.is_running_activity(), "fixture must look like an active turn");

    let event = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(
        matches!(event, Some(InlineEvent::Interrupt)),
        "running activity keeps interrupt precedence, got {event:?}"
    );
    assert!(session.mouse_selection.has_selection, "interrupt must not silently consume the selection");
}
#[test]
fn set_fullscreen_interaction_command_updates_all_fields() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(session.fullscreen.interaction.copy_on_select, "copy_on_select defaults on");
    assert_eq!(session.fullscreen.interaction.scroll_speed, 3);

    session.handle_command(InlineCommand::SetFullscreenInteraction {
        interaction: FullscreenInteractionSettings {
            mouse_capture: false,
            copy_on_select: false,
            scroll_speed: 7,
        },
    });

    assert!(!session.fullscreen.interaction.copy_on_select);
    assert!(!session.fullscreen.interaction.mouse_capture);
    assert_eq!(session.fullscreen.interaction.scroll_speed, 7);
}
#[test]
fn app_session_forwards_fullscreen_interaction_to_core() {
    use crate::tui::core_tui::app::types::InlineCommand as AppInlineCommand;

    let mut session = app_session_with_input("", 0);
    assert!(session.core.fullscreen.interaction.copy_on_select);

    session.handle_command(AppInlineCommand::SetFullscreenInteraction {
        interaction: FullscreenInteractionSettings {
            mouse_capture: true,
            copy_on_select: false,
            scroll_speed: 3,
        },
    });

    assert!(
        !session.core.fullscreen.interaction.copy_on_select,
        "app-layer command must reach the core session's fullscreen interaction"
    );
}
