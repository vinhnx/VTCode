use super::super::turn_undo::TurnUndoState;
use super::*;
use crate::desktop::transcript::{DiffFile, DiffFileKind, TurnFile};

fn undo_fixture_row() -> TranscriptItem {
    let mut row = TranscriptItem::new("row-undo".into(), "assistant".into(), "Agent".into(), "done".into());
    row.turn_id = Some("turn-a".into());
    row.turn_files = vec![TurnFile {
        file: DiffFile { path: "src/a.rs".into(), kind: DiffFileKind::Add },
        counts: Some((2, 1)),
    }];
    row.turn_file_totals = Some((2, 1));
    row
}

fn undo_response(session_id: &str, turn_id: &str, undone: bool, reason: &str) -> BackendEvent {
    BackendEvent::Response {
        method: vtcode_desktop_protocol::method::TURN_UNDO.to_string(),
        result: Some(json!({
            "sessionId": session_id,
            "turnId": turn_id,
            "undone": undone,
            "files": [],
            "reason": reason,
        })),
        error: None,
    }
}

fn arm_undo_backend(view: &Entity<DesktopView>, cx: &mut gpui_kit::App) -> tokio::sync::mpsc::Receiver<BackendCommand> {
    let (commands, requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.selected_session_id = Some("session-a".into());
    });
    requests
}

#[gpui_kit::test]
fn undo_button_sends_turn_undo_and_marks_the_turn_pending(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let mut requests = arm_undo_backend(&view, cx);
        // load_history resets transcript state but keeps the selected session.
        view.update(cx, |view, _| view.load_history(vec![undo_fixture_row()]));
        window.render_frame(cx);
        window.click("turn-undo-turn-a", cx);
        view.update(cx, |view, _| {
            assert_eq!(view.turn_undos.get("turn-a"), Some(&TurnUndoState::Pending));
        });
        let BackendCommand::Request { method, params } = requests.try_recv().expect("undo request") else {
            panic!("expected undo request");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_UNDO);
        assert_eq!(params["sessionId"], "session-a");
        assert_eq!(params["turnId"], "turn-a");
        assert!(requests.try_recv().is_err(), "one click sends one request");
    })
    .unwrap();
}

#[gpui_kit::test]
fn undo_success_flips_the_card_to_undone(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        let _requests = arm_undo_backend(&view, cx);
        view.update(cx, |view, cx| {
            assert!(view.request_turn_undo("turn-a"));
            view.handle_backend_event(undo_response("session-a", "turn-a", true, ""), cx);
            assert_eq!(view.turn_undos.get("turn-a"), Some(&TurnUndoState::Undone));
            assert_eq!(view.status, "Turn file changes undone.");
            assert!(view.error.is_none());
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn undo_refusal_reopens_the_button(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        let _requests = arm_undo_backend(&view, cx);
        view.update(cx, |view, cx| {
            assert!(view.request_turn_undo("turn-a"));
            view.handle_backend_event(undo_response("session-a", "turn-a", false, "file changed since turn"), cx);
            assert!(!view.turn_undos.contains_key("turn-a"), "a refused turn renders its button again");
            assert_eq!(view.status, "Service request failed");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn undo_ignores_stale_and_foreign_responses(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        let _requests = arm_undo_backend(&view, cx);
        view.update(cx, |view, cx| {
            assert!(view.request_turn_undo("turn-a"));
            view.status = "steady".into();
            view.handle_backend_event(undo_response("session-a", "turn-b", true, ""), cx);
            view.handle_backend_event(undo_response("session-b", "turn-a", true, ""), cx);
            assert_eq!(
                view.turn_undos.get("turn-a"),
                Some(&TurnUndoState::Pending),
                "stale answers never touch the live request"
            );
            assert!(view.error.is_none());
            assert_eq!(view.status, "steady");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn undo_error_response_reopens_pending_turns(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        let _requests = arm_undo_backend(&view, cx);
        view.update(cx, |view, cx| {
            assert!(view.request_turn_undo("turn-a"));
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_UNDO.to_string(),
                    result: None,
                    error: Some("service exploded".into()),
                },
                cx,
            );
            assert!(view.turn_undos.is_empty(), "transport errors carry no turn id, so all pending reopen");
            assert_eq!(view.error.as_deref(), Some("service exploded"));
            assert_eq!(view.status, "Service request failed");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn undo_request_guards_duplicates_sessions_and_replays(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        let (commands, mut requests) = tokio::sync::mpsc::channel(4);
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            assert!(!view.request_turn_undo("turn-a"), "no session selected sends nothing");
            view.selected_session_id = Some("session-a".into());
            assert!(view.request_turn_undo("turn-a"));
            assert!(!view.request_turn_undo("turn-a"), "a pending turn sends nothing more");
        });
        assert!(requests.try_recv().is_ok());
        assert!(requests.try_recv().is_err(), "guarded requests never reach the queue");
        view.update(cx, |view, cx| {
            view.handle_backend_event(undo_response("session-a", "turn-a", true, ""), cx);
            assert!(!view.request_turn_undo("turn-a"), "an undone turn stays final");
        });
        assert!(requests.try_recv().is_err());
    })
    .unwrap();
}

#[gpui_kit::test]
fn undo_invalid_response_reopens_pending_and_reports(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        let _requests = arm_undo_backend(&view, cx);
        view.update(cx, |view, cx| {
            assert!(view.request_turn_undo("turn-a"));
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_UNDO.to_string(),
                    result: Some(json!({"turnId": "turn-a"})),
                    error: None,
                },
                cx,
            );
            assert!(view.turn_undos.is_empty(), "an unparseable answer ends the flight");
            assert_eq!(view.error.as_deref(), Some("Could not undo turn file changes."));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn undo_state_clears_on_session_reset(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        let _previous = view.turn_undos.insert("turn-a".into(), TurnUndoState::Pending);
        let _previous = view.turn_undos.insert("turn-b".into(), TurnUndoState::Undone);
        view.reset_session_state();
        assert!(view.turn_undos.is_empty());
    });
}
