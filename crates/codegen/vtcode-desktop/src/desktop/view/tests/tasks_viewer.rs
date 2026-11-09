use super::super::inspector_panels::tracker_current_index;
use super::*;
use crate::desktop::inspector::InspectorTab;
use crate::desktop::transcript::ToolCategory;
use vtcode_desktop_protocol::ThreadEventNotification;
use vtcode_exec_events::{
    ThreadEvent, VersionedThreadEvent,
    tracker::{TrackerItemState, TrackerItemStatus, TrackerSnapshot},
};

fn item(index_path: &str, description: &str, status: TrackerItemStatus) -> TrackerItemState {
    TrackerItemState {
        index_path: index_path.into(),
        description: description.into(),
        status,
    }
}

fn snapshot(title: &str, completed: usize, total: usize, items: Vec<TrackerItemState>) -> TrackerSnapshot {
    TrackerSnapshot { title: title.into(), completed, total, items }
}

fn tracker_notification(snapshot: TrackerSnapshot) -> ThreadEventNotification {
    ThreadEventNotification {
        session_id: "session".into(),
        turn_id: None,
        event: VersionedThreadEvent::new(ThreadEvent::TrackerUpdated(Box::new(snapshot))),
    }
}

#[gpui_kit::test]
fn tracker_updates_replace_the_tasks_snapshot(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.selected_session_id = Some("session".into());
        assert!(view.tracker_snapshot.is_none());
        view.handle_thread_event(tracker_notification(snapshot(
            "Release",
            1,
            2,
            vec![
                item("1", "Write code", TrackerItemStatus::Completed),
                item("2", "Ship it", TrackerItemStatus::Pending),
            ],
        )));
        let current = view.tracker_snapshot.as_ref().unwrap();
        assert_eq!(current.title, "Release");
        assert_eq!((current.completed, current.total), (1, 2));

        // A later update replaces, never merges: stale rows disappear.
        view.handle_thread_event(tracker_notification(snapshot(
            "Release",
            2,
            2,
            vec![
                item("1", "Write code", TrackerItemStatus::Completed),
                item("2", "Ship it", TrackerItemStatus::Completed),
            ],
        )));
        let current = view.tracker_snapshot.as_ref().unwrap();
        assert_eq!((current.completed, current.total), (2, 2));
        assert!(current.items.iter().all(|item| item.status == TrackerItemStatus::Completed));
    });
}

#[gpui_kit::test]
fn tracker_snapshot_seeds_from_newest_restored_checkpoint(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        let timeline = vec![
            VersionedThreadEvent::new(ThreadEvent::TrackerUpdated(Box::new(snapshot(
                "Old",
                0,
                2,
                vec![item("1", "Stale", TrackerItemStatus::Pending)],
            )))),
            VersionedThreadEvent::new(ThreadEvent::TrackerUpdated(Box::new(snapshot(
                "New",
                1,
                2,
                vec![
                    item("1", "Done", TrackerItemStatus::Completed),
                    item("2", "Todo", TrackerItemStatus::Pending),
                ],
            )))),
        ];
        view.restore_tracker_snapshot(&timeline);
        let current = view.tracker_snapshot.as_ref().unwrap();
        assert_eq!(current.title, "New");
        assert_eq!((current.completed, current.total), (1, 2));

        // Timelines without tracker activity keep the empty state.
        view.tracker_snapshot = None;
        view.restore_tracker_snapshot(&[]);
        assert!(view.tracker_snapshot.is_none());

        // Session switches never inherit another conversation's checklist.
        view.tracker_snapshot = Some(snapshot("Old", 0, 0, Vec::new()));
        view.reset_session_state();
        assert!(view.tracker_snapshot.is_none());
    });
}

#[gpui_kit::test]
fn disconnect_clears_the_tasks_snapshot(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    for event in [
        BackendEvent::LaunchFailed("service failed".into()),
        BackendEvent::Exited { status: Some(1), authentication_required: false },
    ] {
        view.update(cx, |view, cx| {
            view.tracker_snapshot = Some(snapshot("Release", 1, 2, Vec::new()));
            view.handle_backend_event(event, cx);
            assert!(view.tracker_snapshot.is_none());
        });
    }
}

#[gpui_kit::test]
fn tasks_tab_renders_header_progress_and_rows(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.tracker_snapshot = Some(snapshot(
                "Release",
                1,
                3,
                vec![
                    item("1", "Write code", TrackerItemStatus::Completed),
                    item("2.1", "Review docs", TrackerItemStatus::InProgress),
                    item("3", "Ship it", TrackerItemStatus::Blocked),
                ],
            ));
            view.inspector_tab = InspectorTab::Tasks;
        });
        window.render_frame(cx);
        assert!(window.try_find("tasks-list").is_some());
        assert!(window.try_find("tasks-progress").is_some());
        assert!(window.try_find("task-row-1").is_some());
        assert!(window.try_find("task-row-2.1").is_some());
        assert!(window.try_find("task-row-3").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn tasks_tab_hides_without_a_snapshot_and_empties_on_empty_items(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        // No snapshot: the tab stays out of the rail.
        view.update(cx, |view, _| {
            view.tracker_snapshot = None;
            assert!(!view.visible_inspector_tabs().contains(&InspectorTab::Tasks));
            // A stranded selection resets when the snapshot clears.
            view.tracker_snapshot = Some(snapshot("Release", 0, 0, Vec::new()));
            view.inspector_tab = InspectorTab::Tasks;
            view.reset_session_state();
            assert_eq!(view.inspector_tab, InspectorTab::Changes);
        });

        // An empty checklist keeps the tab with the empty state.
        view.update(cx, |view, _| {
            view.tracker_snapshot = Some(snapshot("Release", 0, 0, Vec::new()));
            view.inspector_tab = InspectorTab::Tasks;
        });
        window.render_frame(cx);
        assert!(window.try_find("tasks-list").is_some());
        assert!(window.try_find("tasks-progress").is_none());
        assert!(window.try_find("tasks-rows").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn tracker_rows_render_the_inline_task_block(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.selected_session_id = Some("session".into());
            view.tracker_snapshot = Some(snapshot(
                "Release",
                1,
                2,
                vec![
                    item("1", "Write code", TrackerItemStatus::Completed),
                    item("2", "Ship it", TrackerItemStatus::InProgress),
                ],
            ));
            let mut tracker_row =
                TranscriptItem::new("tool-tracker-1".into(), "tool".into(), "task_tracker".into(), "completed".into());
            tracker_row.activity_category = Some(ToolCategory::Tracker);
            view.transcript.push(tracker_row);
            // Commentary breaks grouping so each tool row stands alone.
            view.transcript.push(TranscriptItem::new(
                "gap-one".into(),
                "assistant".into(),
                "VT Code".into(),
                "Noted.".into(),
            ));
            let plain_row =
                TranscriptItem::new("tool-exec-1".into(), "tool".into(), "exec_command".into(), "completed".into());
            view.transcript.push(plain_row);
        });
        window.render_frame(cx);
        assert!(window.try_find("tracker-inline-tool-tracker-1").is_some());
        assert!(window.try_find("tracker-inline-tool-exec-1").is_none());

        // Without a snapshot the block stays out of every tracker row.
        view.update(cx, |view, _| {
            view.tracker_snapshot = None;
        });
        window.render_frame(cx);
        assert!(window.try_find("tracker-inline-tool-tracker-1").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn expanded_tracker_rows_bound_the_inline_tree_with_overflow(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.selected_session_id = Some("session".into());
            let items = (1..=31)
                .map(|index| {
                    item(
                        &index.to_string(),
                        &format!("Step {index}"),
                        if index == 1 {
                            TrackerItemStatus::Completed
                        } else {
                            TrackerItemStatus::Pending
                        },
                    )
                })
                .collect::<Vec<_>>();
            view.tracker_snapshot = Some(snapshot("Big", 1, 31, items));
            let mut tracker_row = TranscriptItem::new(
                "tool-tracker-big".into(),
                "tool".into(),
                "task_tracker".into(),
                "completed".into(),
            );
            tracker_row.activity_category = Some(ToolCategory::Tracker);
            tracker_row.expanded = true;
            view.transcript.push(tracker_row);
        });
        window.render_frame(cx);
        // Rows 0..30 render; the 31st folds into the overflow line.
        assert!(window.try_find("tracker-inline-tool-tracker-big-row-0").is_some());
        assert!(window.try_find("tracker-inline-tool-tracker-big-row-29").is_some());
        assert!(window.try_find("tracker-inline-tool-tracker-big-row-30").is_none());
        assert!(window.try_find("tracker-inline-tool-tracker-big-overflow").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn tracker_current_pick_prefers_leaves_and_live_statuses(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        let _ = view;
        // In-progress leaf beats pending and blocked rows at any depth.
        let items = vec![
            item("1", "Parent", TrackerItemStatus::Pending),
            item("1.1", "Leaf", TrackerItemStatus::InProgress),
            item("2", "Blocked", TrackerItemStatus::Blocked),
        ];
        assert_eq!(tracker_current_index(&items), Some(1));
        // Asymmetric twin: pending leads only without in-progress rows.
        let items = vec![
            item("1", "Leaf", TrackerItemStatus::Pending),
            item("2", "Blocked", TrackerItemStatus::Blocked),
        ];
        assert_eq!(tracker_current_index(&items), Some(0));
        // Parents lose to leaves within the same status.
        let items = vec![
            item("1", "Parent", TrackerItemStatus::InProgress),
            item("1.1", "Leaf", TrackerItemStatus::InProgress),
        ];
        assert_eq!(tracker_current_index(&items), Some(1));
        // Completed-only and empty checklists have no current task.
        let items = vec![item("1", "Done", TrackerItemStatus::Completed)];
        assert_eq!(tracker_current_index(&items), None);
        assert_eq!(tracker_current_index(&[]), None);
    });
}
