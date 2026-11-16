use super::*;

fn edit_call(id: &str, status: vtcode_exec_events::ToolCallStatus) -> vtcode_exec_events::ThreadItem {
    use vtcode_exec_events::*;
    ThreadItem {
        id: id.into(),
        context: None,
        details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
            tool_name: "edit_file".into(),
            arguments: Some(json!({"path": "src/main.rs"})),
            tool_call_id: None,
            status,
            outcome: None,
        })),
    }
}

fn file_change(id: &str, additions: Option<u64>, deletions: Option<u64>) -> vtcode_exec_events::ThreadItem {
    use vtcode_exec_events::*;
    ThreadItem {
        id: id.into(),
        context: None,
        details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
            diff_incomplete: None,
            changes: vec![FileUpdateChange {
                path: "src/main.rs".into(),
                kind: PatchChangeKind::Update,
                terminators_normalized: None,
            }],
            status: PatchApplyStatus::Completed,
            unified_diff: None,
            additions,
            deletions,
        })),
    }
}

fn row_title(view: &DesktopView, id: &str) -> String {
    view.transcript
        .items()
        .find(|row| row.id == id)
        .map(|row| row.title.to_string())
        .unwrap_or_default()
}

fn notify(event: vtcode_exec_events::ThreadEvent) -> vtcode_desktop_protocol::ThreadEventNotification {
    use vtcode_exec_events::VersionedThreadEvent;
    vtcode_desktop_protocol::ThreadEventNotification {
        session_id: "session".into(),
        turn_id: Some("turn".into()),
        event: VersionedThreadEvent::new(event),
    }
}

#[gpui_kit::test]
fn edited_filename_opens_its_change(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.handle_thread_event(notify(ThreadEvent::ItemStarted(ItemStartedEvent {
            item: edit_call("edit-1", ToolCallStatus::InProgress),
        })));
        let mut change = file_change("edit-1:changes", Some(1), Some(1));
        if let ThreadItemDetails::FileChange(change) = &mut change.details {
            change.unified_diff = Some("--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n".into());
        }
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent { item: change })));
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("tool-file-edit-1").role(), Some(gpui_kit::Role::Button));
        assert_eq!(window.find("tool-chevron-edit-1").role(), Some(gpui_kit::Role::Button));
        let disclosure = window.find("tool-edit-1").bounds();
        let filename = window.find("tool-file-edit-1").bounds();
        assert!(
            filename.origin.x >= disclosure.origin.x + disclosure.size.width,
            "filename must not overlap disclosure"
        );
        assert_eq!(filename.origin.y, disclosure.origin.y, "both controls share the same row");
        window.click("tool-edit-1", cx);
        assert!(
            view.read(cx)
                .transcript
                .items()
                .find(|row| row.id == "edit-1")
                .unwrap()
                .expanded
        );
        window.click("tool-chevron-edit-1", cx);
        assert!(
            !view
                .read(cx)
                .transcript
                .items()
                .find(|row| row.id == "edit-1")
                .unwrap()
                .expanded,
            "chevron toggles like every other row"
        );
        window.click("tool-chevron-edit-1", cx);
        assert!(
            view.read(cx)
                .transcript
                .items()
                .find(|row| row.id == "edit-1")
                .unwrap()
                .expanded
        );
        window.render_frame(cx);
        focus_element_by_tab(window, "tool-file-edit-1", cx);
        window.press("enter", cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("src/main.rs"));
        assert_eq!(view.read(cx).inspector_tab, super::super::InspectorTab::Changes);
    })
    .unwrap();
}

#[gpui_kit::test]
fn edit_link_survives_change_arriving_before_invocation(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        let mut change = file_change("edit-1:changes", Some(1), Some(1));
        if let ThreadItemDetails::FileChange(change) = &mut change.details {
            change.unified_diff = Some("--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n".into());
        }
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent { item: change })));
        view.handle_thread_event(notify(ThreadEvent::ItemStarted(ItemStartedEvent {
            item: edit_call("edit-1", ToolCallStatus::InProgress),
        })));
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("tool-file-edit-1", cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("src/main.rs"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn restored_edit_activity_opens_its_change(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        let mut change = file_change("edit-1:changes", Some(1), Some(1));
        if let ThreadItemDetails::FileChange(change) = &mut change.details {
            change.unified_diff = Some("--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n".into());
        }
        view.project_restored_review(&[change]);
        view.project_runtime_item(&edit_call("edit-1", ToolCallStatus::Completed), true);
        view.open_inspector_tab(super::super::InspectorTab::Activity, cx);
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("activity-change-edit-1").role(), Some(gpui_kit::Role::Button));
        window.click("activity-change-edit-1", cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("src/main.rs"));
        assert_eq!(view.read(cx).inspector_tab, super::super::InspectorTab::Changes);
    })
    .unwrap();
}

#[gpui_kit::test]
fn restored_edit_rejects_a_change_from_another_turn(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (_window, view) = open_desktop(cx);
    let context = |turn: &str| {
        Box::new(ItemContext {
            task_id: "task".into(),
            turn_id: turn.into(),
            actor_id: "actor".into(),
            parent_actor_id: None,
            timestamp: "2026-11-05T00:00:00Z".into(),
            activity: None,
        })
    };
    view.update(cx, |view, _| {
        let mut change = file_change("edit-1:changes", Some(1), Some(1));
        change.context = Some(context("other-turn"));
        if let ThreadItemDetails::FileChange(change) = &mut change.details {
            change.unified_diff = Some("--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n".into());
        }
        view.project_restored_review(&[change]);
        let mut call = edit_call("edit-1", ToolCallStatus::Completed);
        call.context = Some(context("current-turn"));
        view.project_runtime_item(&call, true);
        assert!(
            view.restored_activity
                .iter()
                .find(|row| row.id == "edit-1")
                .unwrap()
                .edit_path
                .is_none()
        );
    });
}

#[gpui_kit::test]
fn ambiguous_change_does_not_offer_a_misleading_filename_action(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.handle_thread_event(notify(ThreadEvent::ItemStarted(ItemStartedEvent {
            item: edit_call("edit-1", ToolCallStatus::InProgress),
        })));
        let mut item = file_change("edit-1:changes", Some(1), Some(1));
        if let ThreadItemDetails::FileChange(change) = &mut item.details {
            change.changes.push(FileUpdateChange {
                path: "other/main.rs".into(),
                kind: PatchChangeKind::Update,
                terminators_normalized: None,
            });
            change.unified_diff = Some("--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n".into());
        }
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent { item })));
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("tool-file-edit-1").is_none());
        assert_eq!(window.find("tool-edit-1").role(), Some(gpui_kit::Role::Button));
        view.update(cx, |view, _| {
            let mut item = file_change("edit-1:changes", Some(1), Some(1));
            if let ThreadItemDetails::FileChange(change) = &mut item.details {
                change.changes.push(FileUpdateChange {
                    path: "other/secondary.rs".into(),
                    kind: PatchChangeKind::Update,
                    terminators_normalized: None,
                });
                change.unified_diff = Some("--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n".into());
            }
            view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent { item })));
        });
        window.render_frame(cx);
        window.click("tool-file-edit-1", cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("src/main.rs"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn failed_change_does_not_offer_filename_action(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.handle_thread_event(notify(ThreadEvent::ItemStarted(ItemStartedEvent {
            item: edit_call("edit-1", ToolCallStatus::InProgress),
        })));
        let mut item = file_change("edit-1:changes", Some(1), Some(1));
        if let ThreadItemDetails::FileChange(change) = &mut item.details {
            change.status = PatchApplyStatus::Failed;
            change.unified_diff = Some("--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n".into());
        }
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent { item })));
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("tool-file-edit-1").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn edit_rows_show_per_call_stats_from_changes_sibling(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.handle_thread_event(notify(ThreadEvent::ItemStarted(ItemStartedEvent {
            item: edit_call("edit-1", ToolCallStatus::InProgress),
        })));
        assert_eq!(row_title(view, "edit-1"), "Edited main.rs");
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: file_change("edit-1:changes", Some(22), Some(0)),
        })));
        assert_eq!(row_title(view, "edit-1"), "Edited main.rs +22 \u{2212}0");
        // The join does not disturb the diff card itself.
        let card = view.transcript.items().find(|row| row.id == "edit-1:changes").unwrap();
        assert!(card.diff.is_some());
    });
    let _ = window;
}

#[gpui_kit::test]
fn edit_stats_survive_reprojection_and_replays_exactly_once(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.handle_thread_event(notify(ThreadEvent::ItemStarted(ItemStartedEvent {
            item: edit_call("edit-1", ToolCallStatus::InProgress),
        })));
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: file_change("edit-1:changes", Some(3), Some(1)),
        })));
        // Completion re-projects the invocation row from the gist; the stamp
        // carries over instead of appending a second suffix.
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: edit_call("edit-1", ToolCallStatus::Completed),
        })));
        assert_eq!(row_title(view, "edit-1"), "Edited main.rs +3 \u{2212}1");
        // Duplicate FileChange events are safe too.
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: file_change("edit-1:changes", Some(3), Some(1)),
        })));
        assert_eq!(row_title(view, "edit-1"), "Edited main.rs +3 \u{2212}1");
    });
    let _ = window;
}

#[gpui_kit::test]
fn edit_stats_handle_absent_zero_and_orphan_counts(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.handle_thread_event(notify(ThreadEvent::ItemStarted(ItemStartedEvent {
            item: edit_call("edit-1", ToolCallStatus::InProgress),
        })));
        view.handle_thread_event(notify(ThreadEvent::ItemStarted(ItemStartedEvent {
            item: edit_call("edit-2", ToolCallStatus::InProgress),
        })));
        // Missing counts leave the plain gist; zero counts still render.
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: file_change("edit-1:changes", None, None),
        })));
        assert_eq!(row_title(view, "edit-1"), "Edited main.rs");
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: file_change("edit-2:changes", Some(0), Some(0)),
        })));
        assert_eq!(row_title(view, "edit-2"), "Edited main.rs +0 \u{2212}0");
        // Orphan changes (no sibling row) only land the diff card.
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: file_change("ghost:changes", Some(9), Some(9)),
        })));
        assert_eq!(row_title(view, "edit-1"), "Edited main.rs");
        assert!(view.transcript.items().any(|row| row.id == "ghost:changes"));
    });
    let _ = window;
}
