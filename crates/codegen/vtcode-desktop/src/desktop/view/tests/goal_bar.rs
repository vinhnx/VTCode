use super::*;
use vtcode_desktop_protocol::ThreadEventNotification;
use vtcode_exec_events::{
    ThreadEvent, VersionedThreadEvent,
    goal::{GoalSnapshot, GoalState, GoalUpdatedEvent},
};

fn snapshot(objective: &str, state: GoalState) -> GoalSnapshot {
    GoalSnapshot {
        goal_id: "goal-1".into(),
        state,
        objective: objective.into(),
        elapsed_ms: 11_000,
        tokens_used: 250,
        token_budget: None,
        pause_message: None,
    }
}

fn goal_notification(goal: Option<GoalSnapshot>) -> ThreadEventNotification {
    ThreadEventNotification {
        session_id: "session".into(),
        turn_id: None,
        event: VersionedThreadEvent::new(ThreadEvent::GoalUpdated(Box::new(GoalUpdatedEvent { goal }))),
    }
}

#[test]
fn goal_locale_keys_resolve_to_english() {
    for (key, expected) in [
        ("app.composer.goal_pursuing", "Pursuing goal"),
        ("app.composer.goal_paused", "Paused goal"),
        ("app.composer.goal_complete", "Goal complete"),
        ("app.composer.goal_blocked", "Goal blocked"),
        ("app.composer.goal_budget", "Goal over budget"),
        ("app.composer.goal_pause", "Pause goal"),
        ("app.composer.goal_resume", "Resume goal"),
        ("app.composer.goal_clear", "Clear goal"),
        ("app.composer.goal_expand", "Expand goal objective"),
        ("app.composer.goal_collapse", "Collapse goal objective"),
        ("app.composer.goal_notice_title", "Goal"),
        ("app.composer.goal_cleared", "Goal cleared"),
        ("app.utilities.goal_title", "Goal"),
        ("app.utilities.goal_empty", "No thread goal. Use /goal <objective> to set one."),
    ] {
        assert_eq!(rust_i18n::t!(key).to_string(), expected, "missing locale: {key}");
    }
}

#[gpui_kit::test]
fn goal_updates_replace_the_bar_snapshot(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.selected_session_id = Some("session".into());
        assert!(view.goal_snapshot.is_none());
        view.handle_thread_event(goal_notification(Some(snapshot("Ship it", GoalState::Active))));
        assert_eq!(view.goal_snapshot.as_ref().unwrap().objective, "Ship it");

        // A later update replaces, never merges.
        view.handle_thread_event(goal_notification(Some(snapshot("Ship it", GoalState::Paused))));
        assert_eq!(view.goal_snapshot.as_ref().unwrap().state, GoalState::Paused);

        // A clear removes the bar.
        view.handle_thread_event(goal_notification(None));
        assert!(view.goal_snapshot.is_none());
    });
}

#[gpui_kit::test]
fn goal_transitions_notice_once_per_state_change(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.selected_session_id = Some("session".into());
        let base = view.transcript.len();

        view.handle_thread_event(goal_notification(Some(snapshot("Ship it", GoalState::Active))));
        assert_eq!(view.transcript.len(), base + 1);

        // Same-state refreshes (per-turn updates) stay quiet.
        view.handle_thread_event(goal_notification(Some(snapshot("Ship it", GoalState::Active))));
        assert_eq!(view.transcript.len(), base + 1);

        view.handle_thread_event(goal_notification(Some(snapshot("Ship it", GoalState::Paused))));
        assert_eq!(view.transcript.len(), base + 2);
        let latest = view
            .transcript
            .items()
            .rev()
            .find(|item| item.title.as_ref() == "Goal")
            .unwrap();
        assert!(latest.text.contains("Paused goal: Ship it"), "{}", latest.text);

        view.handle_thread_event(goal_notification(None));
        assert_eq!(view.transcript.len(), base + 3);
    });
}

#[gpui_kit::test]
fn goal_snapshot_seeds_from_newest_restored_checkpoint(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        let timeline = vec![
            VersionedThreadEvent::new(ThreadEvent::GoalUpdated(Box::new(GoalUpdatedEvent {
                goal: Some(snapshot("Old", GoalState::Active)),
            }))),
            VersionedThreadEvent::new(ThreadEvent::GoalUpdated(Box::new(GoalUpdatedEvent {
                goal: Some(snapshot("New", GoalState::Paused)),
            }))),
        ];
        view.restore_goal_snapshot(&timeline);
        let current = view.goal_snapshot.as_ref().unwrap();
        assert_eq!(current.objective, "New");
        assert_eq!(current.state, GoalState::Paused);

        // A trailing clear restores empty, and timelines without goal
        // activity keep the empty state.
        view.restore_goal_snapshot(&[VersionedThreadEvent::new(ThreadEvent::GoalUpdated(Box::new(
            GoalUpdatedEvent { goal: None },
        )))]);
        assert!(view.goal_snapshot.is_none());
        view.restore_goal_snapshot(&[]);
        assert!(view.goal_snapshot.is_none());

        // Session switches never inherit another conversation's goal.
        view.goal_snapshot = Some(snapshot("Old", GoalState::Active));
        view.goal_expanded = true;
        view.reset_session_state();
        assert!(view.goal_snapshot.is_none());
        assert!(!view.goal_expanded);
    });
}

#[gpui_kit::test]
fn goal_execute_sends_rpc_and_applies_response(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, _window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            assert!(view.execute_goal_command("pause".into(), cx));
        });
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
            panic!("goal command must send a request");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::GOAL_EXECUTE);
        assert_eq!(params["sessionId"], "session");
        assert_eq!(params["args"], "pause");

        // A mutating response refreshes the bar without a notice or a turn.
        let response = vtcode_desktop_protocol::GoalExecuteResult {
            session_id: "session".into(),
            lines: vec!["Goal paused.".into()],
            mutated: true,
            submit: None,
            goal: Some(snapshot("Ship it", GoalState::Paused)),
        };
        view.update(cx, |view, cx| {
            let before = view.transcript.len();
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::GOAL_EXECUTE.into(),
                    result: Some(serde_json::to_value(response).unwrap()),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.goal_snapshot.as_ref().unwrap().state, GoalState::Paused);
            assert_eq!(view.transcript.len(), before, "mutating responses stay quiet");
            assert!(view.pending_submission.is_none());
        });

        // A read-only response surfaces its lines as a transcript notice.
        let response = vtcode_desktop_protocol::GoalExecuteResult {
            session_id: "session".into(),
            lines: vec!["Goal [Paused]: Ship it".into()],
            mutated: false,
            submit: None,
            goal: Some(snapshot("Ship it", GoalState::Paused)),
        };
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::GOAL_EXECUTE.into(),
                    result: Some(serde_json::to_value(response).unwrap()),
                    error: None,
                },
                cx,
            );
            assert!(
                view.transcript.items().any(|item| item.text.contains("Goal [Paused]")),
                "status output must surface as a notice"
            );
        });

        // A submit response starts a normal turn with the objective.
        let response = vtcode_desktop_protocol::GoalExecuteResult {
            session_id: "session".into(),
            lines: vec!["Goal set: Ship it".into()],
            mutated: true,
            submit: Some("Ship it".into()),
            goal: Some(snapshot("Ship it", GoalState::Active)),
        };
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::GOAL_EXECUTE.into(),
                    result: Some(serde_json::to_value(response).unwrap()),
                    error: None,
                },
                cx,
            );
        });
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
            panic!("submit response must start a turn");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
        assert_eq!(params["prompt"], "Ship it");

        // A stale response for another session changes nothing.
        let stale = vtcode_desktop_protocol::GoalExecuteResult {
            session_id: "other".into(),
            lines: vec!["Goal cleared.".into()],
            mutated: true,
            submit: None,
            goal: None,
        };
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::GOAL_EXECUTE.into(),
                    result: Some(serde_json::to_value(stale).unwrap()),
                    error: None,
                },
                cx,
            );
            assert!(view.goal_snapshot.is_some(), "stale clear must not wipe the bar");
        });
    })
    .unwrap();
}

#[test]
fn goal_state_labels_cover_every_wire_state() {
    use super::super::goal::goal_state_label;

    assert_eq!(goal_state_label(GoalState::Active), "Pursuing goal");
    assert_eq!(goal_state_label(GoalState::Paused), "Paused goal");
    assert_eq!(goal_state_label(GoalState::Complete), "Goal complete");
    assert_eq!(goal_state_label(GoalState::Blocked), "Goal blocked");
    assert_eq!(goal_state_label(GoalState::BudgetLimited), "Goal over budget");
    assert_eq!(goal_state_label(GoalState::Unknown), "Paused goal");
}

#[gpui_kit::test]
fn goal_utilities_section_renders_with_and_without_snapshot(cx: &mut TestAppContext) {
    use crate::desktop::inspector::InspectorTab;

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_tab = InspectorTab::Utilities;
            view.goal_snapshot = Some(snapshot("Ship it", GoalState::Active));
        });
        window.render_frame(cx);
        assert!(window.try_find("utilities-goal").is_some());

        view.update(cx, |view, _| {
            view.goal_snapshot = None;
        });
        window.render_frame(cx);
        assert!(window.try_find("utilities-goal").is_some(), "empty state still renders");
    })
    .unwrap();
}

#[gpui_kit::test]
fn goal_bar_buttons_drive_live_loop_across_frames(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.handle_thread_event(goal_notification(Some(snapshot("Ship it", GoalState::Active))));
        });
        window.render_frame(cx);
        assert!(window.try_find("goal-bar").is_some());
        assert!(window.try_find("goal-pause").is_some());

        // Pause click sends quiet RPC, never a turn.
        window.click("goal-pause", cx);
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
            panic!("pause must send a request");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::GOAL_EXECUTE);
        assert_eq!(params["args"], "pause");

        // Paused response flips the bar within one frame.
        let paused = vtcode_desktop_protocol::GoalExecuteResult {
            session_id: "session".into(),
            lines: vec!["Goal paused.".into()],
            mutated: true,
            submit: None,
            goal: Some(snapshot("Ship it", GoalState::Paused)),
        };
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::GOAL_EXECUTE.into(),
                    result: Some(serde_json::to_value(paused).unwrap()),
                    error: None,
                },
                cx,
            );
        });
        window.render_frame(cx);
        assert!(window.try_find("goal-pause").is_none(), "paused hides pause");
        assert!(window.try_find("goal-resume").is_some(), "paused offers resume");

        // Resume click sends quiet RPC.
        window.click("goal-resume", cx);
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
            panic!("resume must send a request");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::GOAL_EXECUTE);
        assert_eq!(params["args"], "resume");

        // Clear click sends quiet RPC; cleared snapshot hides the bar.
        window.click("goal-clear", cx);
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
            panic!("clear must send a request");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::GOAL_EXECUTE);
        assert_eq!(params["args"], "clear");
        view.update(cx, |view, _| {
            view.handle_thread_event(goal_notification(None));
        });
        window.render_frame(cx);
        assert!(window.try_find("goal-bar").is_none());

        // Expand toggles full objective and back, frame by frame.
        view.update(cx, |view, _| {
            view.handle_thread_event(goal_notification(Some(snapshot("Ship it", GoalState::Active))));
            assert!(!view.goal_expanded);
        });
        window.render_frame(cx);
        window.click("goal-expand", cx);
        view.update(cx, |view, _| assert!(view.goal_expanded));
        window.render_frame(cx);
        assert!(window.try_find("goal-bar").is_some());
        window.click("goal-expand", cx);
        view.update(cx, |view, _| assert!(!view.goal_expanded));
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn goal_bar_renders_with_snapshot_and_hides_without(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.selected_session_id = Some("session".into());
            view.goal_snapshot = Some(snapshot("Ship the release", GoalState::Active));
        });
        window.render_frame(cx);
        assert!(window.try_find("goal-bar").is_some(), "bar must render with a snapshot");
        assert!(window.try_find("goal-pause").is_some(), "active goal offers pause");
        assert!(window.try_find("goal-resume").is_none(), "active goal hides resume");

        view.update(cx, |view, _| {
            view.goal_snapshot = Some(snapshot("Ship the release", GoalState::Paused));
        });
        window.render_frame(cx);
        assert!(window.try_find("goal-resume").is_some(), "paused goal offers resume");
        assert!(window.try_find("goal-pause").is_none(), "paused goal hides pause");

        view.update(cx, |view, _| {
            view.goal_snapshot = None;
        });
        window.render_frame(cx);
        assert!(window.try_find("goal-bar").is_none(), "bar must hide without a snapshot");
    })
    .unwrap();
}
