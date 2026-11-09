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
