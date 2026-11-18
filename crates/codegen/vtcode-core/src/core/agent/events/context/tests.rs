use super::*;
use serde_json::json;

fn track_launch(tracker: &mut ExecutionContextTracker, item: &str, session: &str) {
    let args = json!({"cmd":"cargo check --locked"});
    let mut started = super::super::tool_started_event(item.into(), "exec_command", Some(&args), Some(item));
    tracker.annotate(&mut started);
    let pending = tracker
        .exec_session_output_event(item, "exec_command", &args, &json!({"session_id":session}))
        .unwrap();
    let ThreadEvent::ItemCompleted(completed) = pending else {
        panic!("canonical output expected")
    };
    let ThreadItemDetails::ToolOutput(output) = completed.item.details else {
        panic!("tool output expected")
    };
    assert_eq!(output.call_id, item);
    assert_eq!(output.status, crate::exec::events::ToolCallStatus::InProgress);
}

fn background(session: &str, task: &str, exit_code: Option<i32>) -> ThreadEvent {
    ThreadEvent::ItemCompleted(crate::exec::events::ItemCompletedEvent { item: crate::exec::events::ThreadItem {
        id: "background".into(), context: None,
        details: ThreadItemDetails::Harness(Box::new(serde_json::from_value(json!({
            "event":crate::exec::events::HarnessEventKind::BackgroundSubprocessCompleted,
            "task_id":task,"session_id":session,"exec_session_id":session,"exit_code":exit_code,"message":"observed exit",
        })).unwrap())),
    }})
}

#[test]
fn native_delegation_status_is_captured_only_for_its_recorded_parent() {
    let mut tracker = ExecutionContextTracker::default();
    let current = tracker.begin("parent", "turn", "Delegate", InputOrigin::User);
    let output = json!({
        "id":"agent-1", "session_id":"child", "parent_thread_id":"parent", "agent_name":"reviewer", "display_label":"Review", "description":"Check storage", "source":"builtin", "status":"running", "background":true, "depth":1, "created_at":"2026-10-03T00:00:00Z", "updated_at":"2026-10-03T00:00:01Z",
    });
    let event = tracker.delegation_status_event("call", "agent", &output).unwrap();
    let ThreadEvent::ItemCompleted(completed) = event else {
        panic!("status event expected")
    };
    let context = completed.item.context.unwrap();
    assert_eq!(context.task_id, current.task_id);
    assert_eq!(context.actor_id, "child");
    assert_eq!(context.parent_actor_id.as_deref(), Some("parent"));
    let ThreadItemDetails::Harness(status) = completed.item.details else {
        panic!("harness status expected")
    };
    assert_eq!(status.status.as_deref(), Some("running"));
    assert!(tracker.delegation_status_event("call", "exec_command", &output).is_none());
    assert!(
        tracker
            .delegation_status_event("call", "agent", &json!({"session_id":"child"}))
            .is_none()
    );
    let mut unrelated = output.clone();
    unrelated["parent_thread_id"] = json!("unrelated");
    assert!(tracker.delegation_status_event("call", "agent", &unrelated).is_none());
    assert!(
        tracker
            .delegation_status_event("wait", "agent", &json!({"completed":true,"entry":output}))
            .is_some()
    );
}

#[test]
fn session_message_sends_emit_correlated_harness_status() {
    let mut tracker = ExecutionContextTracker::default();
    let current = tracker.begin("sess-a", "turn", "Ask peer", InputOrigin::User);
    let output = json!({"message_id": "msg-1", "to_session": "sess-b"});

    for tool in ["ask_session", "answer_session"] {
        let event = tracker.session_message_event("call", tool, &output).unwrap();
        let ThreadEvent::ItemCompleted(completed) = event else {
            panic!("status event expected for {tool}")
        };
        assert_eq!(completed.item.id, "call:session-message");
        let context = completed.item.context.unwrap();
        assert_eq!(context.task_id, current.task_id);
        assert_eq!(context.actor_id, "sess-a");
        let ThreadItemDetails::Harness(status) = completed.item.details else {
            panic!("harness status expected for {tool}")
        };
        assert_eq!(status.event, crate::exec::events::HarnessEventKind::SessionMessageSent);
        assert_eq!(status.session_id.as_deref(), Some("sess-b"));
        assert_eq!(status.correlation_id.as_deref(), Some("msg-1"));
    }

    // Discovery and unrelated tools produce no status event.
    assert!(tracker.session_message_event("call", "list_sessions", &output).is_none());
    assert!(tracker.session_message_event("call", "exec_command", &output).is_none());
    // Malformed outputs (missing routing fields) are ignored, not panicked on.
    assert!(
        tracker
            .session_message_event("call", "ask_session", &json!({"to_session": "sess-b"}))
            .is_none()
    );
    assert!(
        tracker
            .session_message_event("call", "ask_session", &json!({"message_id": "msg-1"}))
            .is_none()
    );
}

#[test]
fn tracker_mutations_publish_typed_checklist_snapshots() {
    use crate::exec::events::tracker::{TrackerItemState, TrackerItemStatus};

    let tracker = ExecutionContextTracker::default();
    let output = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "completed": 1,
            "total": 3,
            "items": [
                {"index": 1, "description": "Write code", "status": "completed"},
                {"index_path": "2.1", "description": "Review docs – expand later", "status": "in_progress"},
                {"description": "Ship it", "status": "blocked"},
            ],
        },
    });
    let event = tracker.tracker_status_event("task_tracker", &output).unwrap();
    let ThreadEvent::TrackerUpdated(snapshot) = event else {
        panic!("tracker snapshot expected")
    };
    assert_eq!(snapshot.title, "Release");
    assert_eq!((snapshot.completed, snapshot.total), (1, 3));
    assert_eq!(
        snapshot.items,
        vec![
            TrackerItemState {
                index_path: "1".to_string(),
                description: "Write code".to_string(),
                status: TrackerItemStatus::Completed,
            },
            TrackerItemState {
                index_path: "2.1".to_string(),
                description: "Review docs".to_string(),
                status: TrackerItemStatus::InProgress,
            },
            TrackerItemState {
                index_path: "3".to_string(),
                description: "Ship it".to_string(),
                status: TrackerItemStatus::Blocked,
            },
        ]
    );
}

#[test]
fn tracker_snapshots_prefer_explicit_counts_and_skip_malformed_rows() {
    use crate::exec::events::tracker::TrackerItemStatus;

    let tracker = ExecutionContextTracker::default();
    // Explicit counts win over derived ones.
    let explicit = json!({"checklist": {"title": "T", "completed": 5, "total": 9, "items": [
        {"index": 1, "description": "Only row", "status": "completed"},
    ]}});
    let event = tracker.tracker_status_event("task_tracker", &explicit).unwrap();
    let ThreadEvent::TrackerUpdated(snapshot) = event else {
        panic!("tracker snapshot expected")
    };
    assert_eq!((snapshot.completed, snapshot.total), (5, 9));
    assert_eq!(snapshot.items.len(), 1);

    // Unknown statuses and blank descriptions drop; a missing status
    // defaults to pending; `text` backs `description`.
    let derived = json!({"checklist": {"items": [
        {"index": 1, "description": "Done", "status": "completed"},
        {"index": 2, "description": "Mystery", "status": "archived"},
        {"index": 3, "description": "   "},
        {"index": 4, "text": "Fallback", "status": "pending"},
        {"index": 5, "description": "Defaulted"},
    ]}});
    let event = tracker.tracker_status_event("task_tracker", &derived).unwrap();
    let ThreadEvent::TrackerUpdated(snapshot) = event else {
        panic!("tracker snapshot expected")
    };
    assert_eq!(
        snapshot
            .items
            .iter()
            .map(|item| (item.index_path.as_str(), item.description.as_str(), item.status))
            .collect::<Vec<_>>(),
        vec![
            ("1", "Done", TrackerItemStatus::Completed),
            ("4", "Fallback", TrackerItemStatus::Pending),
            ("5", "Defaulted", TrackerItemStatus::Pending),
        ]
    );
    assert_eq!((snapshot.completed, snapshot.total), (1, 3));

    // Reversed inputs keep input order with equal counts (asymmetric pair).
    let forward = json!({"checklist": {"items": [
        {"index": 1, "description": "Alpha", "status": "completed"},
        {"index": 2, "description": "Beta", "status": "pending"},
    ]}});
    let backward = json!({"checklist": {"items": [
        {"index": 2, "description": "Beta", "status": "pending"},
        {"index": 1, "description": "Alpha", "status": "completed"},
    ]}});
    let forward_items = match tracker.tracker_status_event("task_tracker", &forward) {
        Some(ThreadEvent::TrackerUpdated(snapshot)) => snapshot.items,
        _ => panic!("tracker snapshot expected"),
    };
    let backward_items = match tracker.tracker_status_event("task_tracker", &backward) {
        Some(ThreadEvent::TrackerUpdated(snapshot)) => snapshot.items,
        _ => panic!("tracker snapshot expected"),
    };
    assert_eq!(forward_items.len(), 2);
    assert_eq!(backward_items.len(), 2);
    assert_eq!(forward_items[0].description, "Alpha");
    assert_eq!(backward_items[0].description, "Beta");

    // Unrelated tools, missing checklists, and count-less empty
    // checklists produce no event.
    assert!(tracker.tracker_status_event("exec_command", &derived).is_none());
    assert!(tracker.tracker_status_event("task_tracker", &json!({"status": "ok"})).is_none());
    assert!(
        tracker
            .tracker_status_event("task_tracker", &json!({"checklist": {"title": "Empty", "items": []}}))
            .is_none()
    );
    // Explicit counts keep an empty checklist visible.
    let empty = tracker
        .tracker_status_event(
            "task_tracker",
            &json!({"checklist": {"title": "Empty", "completed": 0, "total": 0, "items": []}}),
        )
        .unwrap();
    let ThreadEvent::TrackerUpdated(snapshot) = empty else {
        panic!("tracker snapshot expected")
    };
    assert!(snapshot.items.is_empty());
    assert_eq!((snapshot.completed, snapshot.total), (0, 0));
}

#[test]
fn exec_session_mapping_is_bounded_and_isolates_actors_and_unrelated_sessions() {
    let mut tracker = ExecutionContextTracker::default();
    let original = tracker.begin("root", "turn-1", "Verify", InputOrigin::User);
    for index in 0..=MAX_TRACKED_EXEC_LAUNCHES {
        track_launch(&mut tracker, &format!("launch-{index}"), &format!("session-{index}"));
    }
    assert_eq!(tracker.exec_launches.len(), MAX_TRACKED_EXEC_LAUNCHES);
    assert!(
        tracker
            .exec_session_output_event(
                "poll",
                "write_stdin",
                &json!({"session_id":"session-0","chars":""}),
                &json!({"exit_code":0})
            )
            .is_none()
    );
    assert!(
        tracker
            .exec_session_output_event(
                "poll",
                "write_stdin",
                &json!({"session_id":"unknown","chars":""}),
                &json!({"exit_code":0})
            )
            .is_none()
    );
    tracker.begin("other-actor", "turn-2", "Unrelated actor", InputOrigin::User);
    assert!(
        tracker
            .exec_session_output_event(
                "poll",
                "write_stdin",
                &json!({"session_id":"session-1","chars":""}),
                &json!({"exit_code":0})
            )
            .is_none()
    );
    tracker.begin("root", "turn-3", "Another task", InputOrigin::User);
    let completed = tracker
        .exec_session_output_event(
            "poll",
            "write_stdin",
            &json!({"session_id":"session-1","chars":""}),
            &json!({"exit_code":0}),
        )
        .unwrap();
    let ThreadEvent::ItemCompleted(completed) = completed else {
        panic!("completion expected")
    };
    let context = completed.item.context.unwrap();
    assert_eq!(context.task_id, original.task_id);
    assert_eq!(context.turn_id, "turn-1");
    assert_eq!(context.actor_id, "root");
    assert_eq!(tracker.exec_launches.len(), MAX_TRACKED_EXEC_LAUNCHES - 1);
}

#[test]
fn raw_background_exit_correlates_to_the_original_verifier_only() {
    let mut tracker = ExecutionContextTracker::default();
    let original = tracker.begin("root", "turn", "Verify", InputOrigin::User);
    track_launch(&mut tracker, "original-call", "session");
    assert!(
        tracker
            .background_exec_output_event(&mut background("other", "exec:other", Some(0)))
            .is_none()
    );
    assert!(
        tracker
            .background_exec_output_event(&mut background("session", "managed-task", Some(0)))
            .is_none()
    );
    let pending = tracker
        .background_exec_output_event(&mut background("session", "exec:session", None))
        .unwrap();
    let ThreadEvent::ItemCompleted(pending) = pending else {
        panic!("pending output expected")
    };
    let ThreadItemDetails::ToolOutput(output) = pending.item.details else {
        panic!("tool output expected")
    };
    assert_eq!(output.status, crate::exec::events::ToolCallStatus::InProgress);
    tracker.begin("root", "new-turn", "Another task", InputOrigin::User);
    let mut raw = background("session", "exec:session", Some(7));
    tracker.annotate(&mut raw);
    let terminal = tracker.background_exec_output_event(&mut raw).unwrap();
    tracker.annotate(&mut raw);
    let ThreadEvent::ItemCompleted(raw) = raw else {
        panic!("raw completion expected")
    };
    assert_eq!(raw.item.context.unwrap().task_id, original.task_id);
    assert_eq!(tracker.current.as_ref().unwrap().turn_id, "new-turn");
    let ThreadEvent::ItemCompleted(terminal) = terminal else {
        panic!("terminal output expected")
    };
    assert_eq!(terminal.item.context.unwrap().task_id, original.task_id);
    let ThreadItemDetails::ToolOutput(output) = terminal.item.details else {
        panic!("tool output expected")
    };
    assert_eq!(output.call_id, "original-call");
    assert_eq!(output.status, crate::exec::events::ToolCallStatus::Failed);
    assert_eq!(output.exit_code, Some(7));
    assert!(
        tracker
            .background_exec_output_event(&mut background("session", "exec:session", Some(0)))
            .is_none()
    );
}

#[test]
fn execution_task_identity_survives_corrections_and_handoffs() {
    let mut tracker = ExecutionContextTracker::default();
    let first = tracker.begin("root", "turn-1", "Fix login", InputOrigin::User);
    for (turn, origin) in [
        ("turn-2", InputOrigin::Correction),
        ("turn-3", InputOrigin::PlanApproval),
        ("turn-4", InputOrigin::Continuation),
        ("turn-5", InputOrigin::Retry),
    ] {
        let current = tracker.begin("root", turn, "changed input", origin);
        assert_eq!(current.task_id, first.task_id);
        assert_eq!(current.goal.as_deref(), Some("Fix login"));
        assert_eq!(current.turn_id, turn);
    }
    let next = tracker.begin("root", "turn-6", "Add logging", InputOrigin::User);
    assert_ne!(next.task_id, first.task_id);
    assert_eq!(next.goal.as_deref(), Some("Add logging"));
}

#[test]
fn public_decision_bounds_are_validated_in_characters_and_fail_closed() {
    assert!(validate_decision_input(json!({"summary":"é".repeat(240),"rationale":"r".repeat(1000),"alternatives":["a","b","c"],"evidence_ids":vec!["id";8]})).is_ok());
    for invalid in [
        json!({"summary":"a".repeat(241),"rationale":"why"}),
        json!({"summary":"choice","rationale":"a".repeat(1001)}),
        json!({"summary":"choice","rationale":"why","alternatives":["a","b","c","d"]}),
        json!({"summary":"choice","rationale":"why","evidence_ids":[""]}),
        json!({"summary":"choice","rationale":"why","private_reasoning":"hidden"}),
        json!({"summary":" ","rationale":"why"}),
    ] {
        assert!(validate_decision_input(invalid).is_err());
    }
}

#[test]
fn public_decision_accepts_valid_common_output_metadata() {
    let args = json!({"summary":"Reuse parser","rationale":"Preserve validation","max_output_tokens":100});
    let decision = validate_decision_input(args.clone()).unwrap();
    assert_eq!(decision.summary, "Reuse parser");
    let event = super::super::tool_invocation_completed_event(
        "decision-call".into(),
        "record_decision",
        Some(&args),
        None,
        crate::exec::events::ToolCallStatus::Completed,
        crate::exec::events::ToolOutcome::Success,
    );
    assert!(decision_completed_event(&event).is_some());
    for invalid in [json!(0), json!(50_001), json!("100"), json!(1.5)] {
        let mut args = args.clone();
        args["max_output_tokens"] = invalid;
        assert!(validate_decision_input(args).is_err());
    }
}
