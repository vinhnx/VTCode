use super::*;
use crate::desktop::inspector;

fn wait_call(
    id: &str,
    session: &str,
    timeout_secs: u64,
    status: vtcode_exec_events::ToolCallStatus,
) -> vtcode_exec_events::ThreadItem {
    use vtcode_exec_events::*;
    ThreadItem {
        id: id.into(),
        context: None,
        details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
            tool_name: "write_stdin".into(),
            arguments: Some(json!({
                "session_id": session,
                "action": "wait",
                "wait_timeout_seconds": timeout_secs,
            })),
            tool_call_id: None,
            status,
            outcome: None,
        })),
    }
}

fn wait_output(id: &str, call_id: &str, body: &str) -> vtcode_exec_events::ThreadItem {
    use vtcode_exec_events::*;
    ThreadItem {
        id: id.into(),
        context: None,
        details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
            call_id: call_id.into(),
            tool_call_id: None,
            spool_path: None,
            output: body.into(),
            is_delta: None,
            exit_code: None,
            status: ToolCallStatus::Completed,
            app_previews: Vec::new(),
        })),
    }
}

fn wait_call_in_turn(
    id: &str,
    session: &str,
    timeout_secs: u64,
    status: vtcode_exec_events::ToolCallStatus,
    turn_id: &str,
) -> vtcode_exec_events::ThreadItem {
    use vtcode_exec_events::*;
    let mut item = wait_call(id, session, timeout_secs, status);
    item.context = Some(Box::new(ItemContext {
        task_id: "task".into(),
        turn_id: turn_id.into(),
        actor_id: "actor".into(),
        parent_actor_id: None,
        timestamp: "2026-11-18T03:00:00+00:00".into(),
        activity: None,
    }));
    item
}

fn read_call(id: &str) -> vtcode_exec_events::ThreadItem {
    use vtcode_exec_events::*;
    ThreadItem {
        id: id.into(),
        context: None,
        details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
            tool_name: "read_file".into(),
            arguments: Some(json!({"path": "src/main.rs"})),
            tool_call_id: None,
            status: ToolCallStatus::Completed,
            outcome: None,
        })),
    }
}

fn notify(event: vtcode_exec_events::ThreadEvent) -> vtcode_desktop_protocol::ThreadEventNotification {
    use vtcode_exec_events::VersionedThreadEvent;
    vtcode_desktop_protocol::ThreadEventNotification {
        session_id: "session".into(),
        turn_id: Some("turn".into()),
        event: VersionedThreadEvent::new(event),
    }
}

fn complete_wait(view: &mut DesktopView, id: &str, session: &str, timeout_secs: u64) {
    use vtcode_exec_events::*;
    view.handle_thread_event(notify(ThreadEvent::ItemStarted(ItemStartedEvent {
        item: wait_call(id, session, timeout_secs, ToolCallStatus::InProgress),
    })));
    view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: wait_call(id, session, timeout_secs, ToolCallStatus::Completed),
    })));
}

fn tool_rows(view: &DesktopView) -> Vec<(String, String, String)> {
    view.transcript
        .items()
        .filter(|row| row.is_tool())
        .map(|row| (row.id.clone(), row.title.to_string(), row.text.to_string()))
        .collect()
}

#[gpui_kit::test]
fn consecutive_session_waits_coalesce_into_one_row(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        complete_wait(view, "wait-1", "run-1", 1);
        complete_wait(view, "wait-2", "run-1", 2);
        complete_wait(view, "wait-3", "run-1", 4);
        let rows = tool_rows(view);
        assert_eq!(rows.len(), 1, "three consecutive waits for one session render as one row");
        assert_eq!(rows[0].0, "wait-1", "the first wait keeps the surviving row identity");
        assert_eq!(rows[0].1, "Wait for command session ×3");
        assert!(
            rows[0].2.contains("Session run-1 · wait up to 4s"),
            "the row carries the latest wait deadline: {:?}",
            rows[0].2
        );
        assert!(rows[0].2.starts_with("completed"), "the row settles with the latest status: {:?}", rows[0].2);
    });
}

#[gpui_kit::test]
fn coalesced_wait_output_lands_on_the_surviving_row(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        complete_wait(view, "wait-1", "run-1", 1);
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: wait_output("wait-1:output", "wait-1", "previous capture"),
        })));
        complete_wait(view, "wait-2", "run-1", 8);
        let mut delta = wait_output("wait-2:output", "wait-2", "first chunk");
        if let ThreadItemDetails::ToolOutput(output) = &mut delta.details {
            output.is_delta = Some(true);
        }
        view.handle_thread_event(notify(ThreadEvent::ItemUpdated(ItemUpdatedEvent { item: delta })));
        assert_eq!(view.transcript.get(0).unwrap().output.as_deref(), Some("first chunk"));
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: wait_output("wait-2:output", "wait-2", "Checking vtcode-mcp v0.183.1"),
        })));
        let selected = inspector::output_for(&view.transcript, &[], Some("wait-1")).expect("surviving output");
        assert_eq!(selected.id, "wait-2:output");
        assert_eq!(selected.row_id, "wait-1");
        let rows: Vec<_> = view.transcript.items().filter(|row| row.is_tool()).collect();
        assert_eq!(rows.len(), 1, "the absorbed wait leaves no standalone output row");
        assert_eq!(
            rows[0].output.as_deref(),
            Some("Checking vtcode-mcp v0.183.1"),
            "the second wait's capture merges into the surviving row"
        );
    });
}

#[gpui_kit::test]
fn restored_session_waits_coalesce_into_one_row(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.project_runtime_item(&wait_call("wait-1", "run-1", 1, ToolCallStatus::Completed), true);
        view.project_runtime_item(&wait_output("wait-1:output", "wait-1", "previous capture"), true);
        view.project_runtime_item(&wait_call("wait-2", "run-1", 8, ToolCallStatus::Completed), true);
        view.project_runtime_item(&wait_output("wait-2:output", "wait-2", "Checking vtcode-mcp"), true);
        assert_eq!(view.restored_activity.len(), 1, "restored waits share one activity row like live waits");
        assert_eq!(view.restored_activity[0].title.as_str(), "Wait for command session ×2");
        assert_eq!(
            view.restored_activity[0].output.as_deref(),
            Some("Checking vtcode-mcp"),
            "the absorbed wait's capture merges into the surviving restored row"
        );
    });
}

#[gpui_kit::test]
fn followup_captures_merge_without_stranding_or_wiping(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    // Metadata-only heartbeat between two real captures, as a polling build
    // produces: only the envelope, no human text.
    let heartbeat = "Structured output:\n{\"backend\": \"pipe\", \"lifecycle_state\": \"running\"}";
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        complete_wait(view, "wait-1", "run-1", 1);
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: wait_output("wait-1:output", "wait-1", "compiling foo"),
        })));
        complete_wait(view, "wait-2", "run-1", 2);
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: wait_output("wait-2:output", "wait-2", heartbeat),
        })));
        let rows: Vec<_> = view.transcript.items().filter(|row| row.is_tool()).collect();
        assert_eq!(rows.len(), 1, "the blank heartbeat strands no row");
        assert_eq!(
            rows[0].output.as_deref(),
            Some("compiling foo"),
            "a blank follow-up capture never wipes the survivor's output"
        );
        complete_wait(view, "wait-3", "run-1", 4);
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: wait_output("wait-3:output", "wait-3", "compiling bar"),
        })));
        let rows: Vec<_> = view.transcript.items().filter(|row| row.is_tool()).collect();
        assert_eq!(rows.len(), 1, "the third capture merges instead of stranding");
        assert_eq!(rows[0].title.as_str(), "Wait for command session ×3");
        assert_eq!(rows[0].output.as_deref(), Some("compiling bar"), "the latest non-blank capture wins");
    });
}

#[gpui_kit::test]
fn restored_absorbed_reprojection_keeps_the_run_count(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.project_runtime_item(&wait_call("wait-1", "run-1", 1, ToolCallStatus::Completed), true);
        view.project_runtime_item(&wait_call("wait-2", "run-1", 8, ToolCallStatus::InProgress), true);
        // The completed snapshot of the absorbed call replays under its own
        // id: the survivor refreshes, the run does not grow.
        view.project_runtime_item(&wait_call("wait-2", "run-1", 8, ToolCallStatus::Completed), true);
        assert_eq!(view.restored_activity.len(), 1);
        assert_eq!(view.restored_activity[0].title.as_str(), "Wait for command session ×2");
    });
}

#[gpui_kit::test]
fn cross_turn_poll_starts_a_new_wait_row(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        for (id, turn) in [("wait-1", "turn-A"), ("wait-2", "turn-A"), ("wait-3", "turn-B")] {
            view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: wait_call_in_turn(id, "run-1", 1, ToolCallStatus::Completed, turn),
            })));
        }
        let rows = tool_rows(view);
        assert_eq!(rows.len(), 2, "same-turn polls merge, the next turn splits: {rows:?}");
        assert_eq!(rows[0].1, "Wait for command session ×2");
        assert_eq!(rows[1].1, "Wait for command session");
    });
}

#[gpui_kit::test]
fn different_session_starts_a_new_wait_row(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        complete_wait(view, "wait-1", "run-1", 1);
        complete_wait(view, "wait-2", "run-2", 1);
        complete_wait(view, "wait-3", "run-1", 1);
        let rows = tool_rows(view);
        assert_eq!(rows.len(), 3, "non-consecutive sessions never merge: {rows:?}");
        assert_eq!(rows[0].1, "Wait for command session");
        assert_eq!(rows[1].1, "Wait for command session");
        assert_eq!(rows[2].1, "Wait for command session");
    });
}

#[gpui_kit::test]
fn interleaved_tool_breaks_the_wait_run(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        complete_wait(view, "wait-1", "run-1", 1);
        view.handle_thread_event(notify(ThreadEvent::ItemCompleted(ItemCompletedEvent { item: read_call("read-1") })));
        complete_wait(view, "wait-2", "run-1", 2);
        let rows = tool_rows(view);
        assert_eq!(rows.len(), 3, "an intervening tool ends coalescing: {rows:?}");
        assert_eq!(rows[0].1, "Wait for command session");
        assert_eq!(rows[2].1, "Wait for command session");
    });
}

#[gpui_kit::test]
fn adjacent_waits_in_different_turns_keep_distinct_rows(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        for (id, turn) in [("wait-a", "turn-a"), ("wait-b", "turn-b")] {
            let mut item = wait_call(id, "same-session", 3, ToolCallStatus::Completed);
            item.context = Some(Box::new(ItemContext {
                task_id: "task".into(),
                turn_id: turn.into(),
                actor_id: "agent".into(),
                parent_actor_id: None,
                timestamp: String::new(),
                activity: None,
            }));
            view.update_runtime_item_for_turn(&item, Some(turn));
            view.project_runtime_item(&item, true);
        }
        assert_eq!(tool_rows(view).len(), 2);
        assert_eq!(view.restored_activity.len(), 2);
    });
}
