use super::*;
use crate::desktop::view::InspectorTab;
use crate::desktop::view::goal::goal_live_elapsed_label;
use std::time::{Duration, Instant};
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
        assert_eq!(view.transcript.get(base).unwrap().notice_count(), 1);

        // Same-state refreshes (per-turn updates) stay quiet.
        view.handle_thread_event(goal_notification(Some(snapshot("Ship it", GoalState::Active))));
        assert_eq!(view.transcript.get(base).unwrap().notice_count(), 1);

        view.handle_thread_event(goal_notification(Some(snapshot("Ship it", GoalState::Paused))));
        assert_eq!(view.transcript.len(), base + 1, "consecutive notices share one callout");
        let head = view.transcript.get(base).unwrap();
        assert_eq!(head.notice_count(), 2);
        assert!(
            head.notice_stack
                .iter()
                .any(|entry| entry.text.contains("Paused goal: Ship it")),
            "the newest goal state is folded into the callout"
        );

        view.handle_thread_event(goal_notification(None));
        assert_eq!(view.transcript.get(base).unwrap().notice_count(), 3);
    });
}

#[gpui_kit::test]
fn repeated_goal_snapshot_keeps_the_original_live_elapsed_stamp(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.selected_session_id = Some("session".into());
        view.handle_thread_event(goal_notification(Some(snapshot("Ship it", GoalState::Active))));
        assert!(view.goal_snapshot_at.is_some(), "first update stamps arrival");
        // Move the stamp into the past so a spurious re-stamp is unambiguous.
        let arrived = Instant::now()
            .checked_sub(Duration::from_secs(30))
            .expect("30s fits in an Instant");
        view.goal_snapshot_at = Some(arrived);

        // A byte-identical snapshot at the handler boundary (the live producer
        // normally advances `elapsed_ms` each turn) must not restart the label.
        view.handle_thread_event(goal_notification(Some(snapshot("Ship it", GoalState::Active))));
        assert_eq!(view.goal_snapshot_at, Some(arrived), "an unchanged snapshot must not restart the live timer");

        // A real change re-stamps, and repeating that value keeps the new stamp.
        view.handle_thread_event(goal_notification(Some(snapshot("Ship it", GoalState::Paused))));
        let changed = view.goal_snapshot_at.expect("a changed snapshot stamps arrival");
        assert!(changed > arrived);
        view.handle_thread_event(goal_notification(Some(snapshot("Ship it", GoalState::Paused))));
        assert_eq!(view.goal_snapshot_at, Some(changed));

        // Clearing drops the stamp; repeating the clear keeps it absent.
        view.handle_thread_event(goal_notification(None));
        assert!(view.goal_snapshot_at.is_none());
        view.handle_thread_event(goal_notification(None));
        assert!(view.goal_snapshot_at.is_none());
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
        assert!(
            window.try_find("utilities-goal-state-icon").is_some(),
            "goal identity mark must render with the snapshot"
        );

        view.update(cx, |view, _| {
            view.goal_snapshot = None;
        });
        window.render_frame(cx);
        assert!(window.try_find("utilities-goal").is_some(), "empty state still renders");
        assert!(window.try_find("utilities-goal-state-icon").is_none(), "no snapshot means no mark");
    })
    .unwrap();
}

#[gpui_kit::test]
fn inspector_goal_pin_shows_on_every_tab_and_opens_utilities(cx: &mut TestAppContext) {
    use crate::desktop::inspector::InspectorTab;

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_tab = InspectorTab::Tasks;
            view.goal_snapshot = Some(snapshot("Ship it", GoalState::Active));
        });
        window.render_frame(cx);
        assert!(window.try_find("inspector-goal-pin").is_some(), "pin shows above the Tasks rail");
        assert!(window.try_find("inspector-goal-pin-icon").is_some(), "pin carries the state mark");

        window.click("inspector-goal-pin", cx);
        view.update(cx, |view, _| {
            assert_eq!(view.inspector_tab, InspectorTab::Utilities, "pin reveals full status in Utilities");
            view.goal_snapshot = None;
        });
        window.render_frame(cx);
        assert!(window.try_find("inspector-goal-pin").is_none(), "no goal means no pin");
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
        assert!(window.try_find("goal-state-icon").is_some(), "active bar carries the mark");
        assert!(window.try_find("goal-pause").is_some(), "active goal offers pause");
        assert!(window.try_find("goal-resume").is_none(), "active goal hides resume");

        view.update(cx, |view, _| {
            view.goal_snapshot = Some(snapshot("Ship the release", GoalState::Paused));
        });
        window.render_frame(cx);
        assert!(window.try_find("goal-state-icon").is_some(), "paused bar keeps the mark");
        assert!(window.try_find("goal-resume").is_some(), "paused goal offers resume");
        assert!(window.try_find("goal-pause").is_none(), "paused goal hides pause");

        view.update(cx, |view, _| {
            view.goal_snapshot = None;
        });
        window.render_frame(cx);
        assert!(window.try_find("goal-bar").is_none(), "bar must hide without a snapshot");
        assert!(window.try_find("goal-state-icon").is_none(), "no bar means no mark");
    })
    .unwrap();
}

#[gpui_kit::test]
fn goal_ticker_task_is_retained_once_started(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        assert!(view.goal_tick_task.is_none());
        view.start_goal_ticker(cx);
        assert!(view.goal_tick_task.is_some(), "dropping the task would freeze the bar");
        view.start_goal_ticker(cx);
        assert!(view.goal_tick_task.is_some(), "restart must stay idempotent");
    });
}

#[gpui_kit::test]
fn goal_snapshot_setter_stamps_arrival_and_clears_it(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        assert!(view.goal_snapshot_at.is_none());
        view.set_goal_snapshot(Some(snapshot("Ship it", GoalState::Active)));
        assert!(view.goal_snapshot_at.is_some(), "set must stamp arrival for the live timer");
        view.set_goal_snapshot(None);
        assert!(view.goal_snapshot_at.is_none(), "clear must drop the stamp");
    });
}

#[test]
fn goal_live_label_ticks_while_active_and_freezes_otherwise() {
    let active = GoalSnapshot {
        elapsed_ms: 11_000,
        ..snapshot("Ship it", GoalState::Active)
    };
    let minute_ago = Instant::now().checked_sub(Duration::from_secs(61)).unwrap();
    assert_eq!(
        goal_live_elapsed_label(&active, Some(minute_ago)),
        "1m 12s",
        "active label must add wall-clock since arrival"
    );
    let boundary = GoalSnapshot {
        elapsed_ms: 59_900,
        ..snapshot("Ship it", GoalState::Active)
    };
    let half_second_ago = Instant::now().checked_sub(Duration::from_millis(500)).unwrap();
    assert_eq!(
        goal_live_elapsed_label(&boundary, Some(half_second_ago)),
        "1m 0s",
        "active label must cross the minute boundary without a new snapshot"
    );
    assert_eq!(goal_live_elapsed_label(&active, None), "11s", "missing stamp must fall back to the snapshot");

    let hour_ago = Instant::now().checked_sub(Duration::from_secs(3600)).unwrap();
    for state in [
        GoalState::Paused,
        GoalState::Unknown,
        GoalState::Complete,
        GoalState::Blocked,
        GoalState::BudgetLimited,
    ] {
        let settled = GoalSnapshot { elapsed_ms: 11_000, ..snapshot("Ship it", state) };
        assert_eq!(
            goal_live_elapsed_label(&settled, Some(hour_ago)),
            "11s",
            "{state:?} must freeze even an hour after arrival"
        );
    }
}

#[gpui_kit::test]
fn goal_objective_renders_as_markdown_in_expanded_surfaces(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let objective = "**Review:** the code\n\n1. **Analyze:** options\n2. Implement";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.selected_session_id = Some("session".into());
            view.goal_snapshot = Some(snapshot(objective, GoalState::Active));
        });
        window.render_frame(cx);
        assert!(
            !window.find("goal-bar-objective").label().unwrap().contains("**"),
            "compact bar keeps plain preview"
        );

        window.click("goal-expand", cx);
        window.render_frame(cx);
        let bar = window.find("goal-bar-objective");
        let label = bar.label().unwrap();
        assert!(!label.contains("**"), "expanded bar strips markdown markers: {label:?}");
        assert!(label.contains("Review"), "expanded bar keeps objective text: {label:?}");

        view.update(cx, |view, cx| view.open_inspector_tab(InspectorTab::Utilities, cx));
        window.render_frame(cx);
        let panel = window.find("utilities-goal-markdown");
        let label = panel.label().unwrap();
        assert!(!label.contains("**"), "utilities panel strips markdown markers: {label:?}");
        assert!(label.contains("Analyze"), "utilities panel keeps list text: {label:?}");
    })
    .unwrap();
}

#[gpui_kit::test]
fn goal_notice_renders_the_objective_as_markdown(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.note_goal_transition(Some(&snapshot("**Bold** objective", GoalState::Active))));
        window.render_frame(cx);
        let id = view.read(cx).transcript.items().last().expect("goal notice").id.clone();
        let body = window.find(format!("notice-details-body-{id}"));
        let label = body.label().unwrap();
        assert!(!label.contains("**"), "expanded goal notice strips markdown markers: {label:?}");
        assert!(label.contains("Bold"), "expanded goal notice keeps objective text: {label:?}");
    })
    .unwrap();
}

#[gpui_kit::test]
fn folded_goal_notices_keep_markdown_treatment(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.note_goal_transition(Some(&snapshot("**First** objective", GoalState::Active)));
            view.note_goal_transition(Some(&snapshot("**Second** objective", GoalState::Blocked)));
        });
        window.render_frame(cx);
        let notices: Vec<_> = view
            .read(cx)
            .transcript
            .items()
            .filter(|item| item.is_notice())
            .cloned()
            .collect();
        assert_eq!(notices.len(), 1, "consecutive goal notices still fold into one callout");
        assert_eq!(notices[0].notice_count(), 2);
        let id = notices[0].id.clone();
        window.click(format!("notice-stack-toggle-{id}"), cx);
        window.render_frame(cx);
        for entry in &notices[0].notice_stack {
            let body = window.find(format!("notice-stack-entry-{}", entry.id));
            let label = body.label().unwrap();
            assert!(!label.contains("**"), "folded goal entry strips markdown markers: {label:?}");
        }
    })
    .unwrap();
}
