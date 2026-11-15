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
