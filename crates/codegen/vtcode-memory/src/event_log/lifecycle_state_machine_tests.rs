use super::{LifecycleKind, LogState};
use vtcode_exec_events::{
    ThreadCompletedEvent, ThreadCompletionSubtype, ThreadEvent, ThreadStartedEvent, TurnCompletedEvent,
    TurnFailedEvent, TurnStartedEvent, Usage,
};

fn fresh_state() -> LogState {
    LogState::new("test-session")
}

#[test]
fn lifecycle_kind_from_event_covers_all_variants() {
    assert_eq!(
        LifecycleKind::from_event(&ThreadEvent::TurnStarted(TurnStartedEvent::default())),
        LifecycleKind::TurnStarted
    );
    assert_eq!(
        LifecycleKind::from_event(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
            final_response_item_id: None,
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new()
        })),
        LifecycleKind::TurnCompleted
    );
    assert_eq!(
        LifecycleKind::from_event(&ThreadEvent::TurnFailed(TurnFailedEvent {
            completed_at: None,
            message: "err".to_string(),
            usage: None,
        })),
        LifecycleKind::TurnFailed
    );
    assert_eq!(
        LifecycleKind::from_event(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "x".to_string() })),
        LifecycleKind::ThreadStarted
    );
    assert_eq!(
        LifecycleKind::from_event(&ThreadEvent::ThreadCompleted(Box::new(ThreadCompletedEvent {
            completed_at: None,
            thread_id: "x".to_string(),
            session_id: "x".to_string(),
            subtype: ThreadCompletionSubtype::Success,
            outcome_code: "completed".to_string(),
            result: None,
            stop_reason: None,
            usage: Usage::default(),
            total_cost_usd: None,
            num_turns: 1,
        }))),
        LifecycleKind::ThreadCompleted
    );
    assert_eq!(LifecycleKind::from_kind("thread.started"), LifecycleKind::ThreadStarted);
    assert_eq!(LifecycleKind::from_kind("thread.completed"), LifecycleKind::ThreadCompleted);
}

#[test]
fn lifecycle_kind_from_str_matches_event_discriminator() {
    assert_eq!(LifecycleKind::from_kind("turn.started"), LifecycleKind::TurnStarted);
    assert_eq!(LifecycleKind::from_kind("turn.completed"), LifecycleKind::TurnCompleted);
    assert_eq!(LifecycleKind::from_kind("turn.failed"), LifecycleKind::TurnFailed);
    assert_eq!(LifecycleKind::from_kind("tool.called"), LifecycleKind::Other);
    assert_eq!(LifecycleKind::from_kind("thread.started"), LifecycleKind::ThreadStarted);
    assert_eq!(LifecycleKind::from_kind("thread.completed"), LifecycleKind::ThreadCompleted);
}

#[test]
fn turn_started_pushes_index_entry_and_sets_in_turn() {
    let mut st = fresh_state();
    st.manifest.status = "completed".to_string();
    let is_boundary = st.apply_lifecycle_event(LifecycleKind::TurnStarted, 0, 100);
    assert!(!is_boundary, "TurnStarted is not a turn boundary");
    assert!(st.in_turn);
    assert_eq!(st.manifest.status, "active");
    assert_eq!(st.index.entries.len(), 1);
    let entry = &st.index.entries[0];
    assert_eq!(entry.turn_number, 1);
    assert_eq!(entry.start_offset, 0);
    assert_eq!(entry.end_offset, 100);
    assert_eq!(entry.event_count, 1);
}

#[test]
fn intermediate_events_extend_current_turn() {
    let mut st = fresh_state();
    st.apply_lifecycle_event(LifecycleKind::TurnStarted, 0, 100);
    // Simulate two intermediate events.
    let is_b1 = st.apply_lifecycle_event(LifecycleKind::Other, 100, 200);
    let is_b2 = st.apply_lifecycle_event(LifecycleKind::Other, 200, 300);
    assert!(!is_b1 && !is_b2);
    assert!(st.in_turn);
    assert_eq!(st.index.entries.len(), 1);
    let entry = &st.index.entries[0];
    assert_eq!(entry.end_offset, 300);
    assert_eq!(entry.event_count, 3);
}

#[test]
fn turn_completed_closes_turn_and_returns_boundary() {
    let mut st = fresh_state();
    st.apply_lifecycle_event(LifecycleKind::TurnStarted, 0, 100);
    st.apply_lifecycle_event(LifecycleKind::Other, 100, 200);
    let is_boundary = st.apply_lifecycle_event(LifecycleKind::TurnCompleted, 200, 300);
    assert!(is_boundary);
    assert!(!st.in_turn);
    assert_eq!(st.manifest.turn_count, 1);
    assert_eq!(st.manifest.status, "active");
    st.apply_lifecycle_event(LifecycleKind::ThreadCompleted, 300, 400);
    assert_eq!(st.manifest.status, "completed");
    let entry = &st.index.entries[0];
    assert_eq!(entry.end_offset, 300);
    assert_eq!(entry.event_count, 3);
}

#[test]
fn turn_failed_closes_turn_without_terminal_thread_status() {
    let mut st = fresh_state();
    st.apply_lifecycle_event(LifecycleKind::TurnStarted, 0, 100);
    let is_boundary = st.apply_lifecycle_event(LifecycleKind::TurnFailed, 100, 200);
    assert!(is_boundary);
    assert!(!st.in_turn);
    assert_eq!(st.manifest.turn_count, 1);
    assert_eq!(st.manifest.status, "active");
    st.apply_lifecycle_event(LifecycleKind::ThreadCompleted, 200, 300);
    assert_eq!(st.manifest.status, "completed");
}

#[test]
fn turn_completed_without_turn_started_is_idempotent() {
    let mut st = fresh_state();
    // Receiving TurnCompleted without a preceding TurnStarted should not
    // panic or corrupt the index; terminal status remains active until the
    // thread lifecycle itself completes.
    let is_boundary = st.apply_lifecycle_event(LifecycleKind::TurnCompleted, 0, 100);
    assert!(is_boundary);
    assert!(!st.in_turn);
    assert_eq!(st.manifest.turn_count, 0, "no turn was started");
    assert_eq!(st.manifest.status, "active");
    st.apply_lifecycle_event(LifecycleKind::ThreadCompleted, 100, 200);
    assert_eq!(st.manifest.status, "completed");
    assert!(st.index.entries.is_empty());
}

#[test]
fn multiple_turns_get_incrementing_ordinals() {
    let mut st = fresh_state();
    for n in 1..=3 {
        st.apply_lifecycle_event(LifecycleKind::TurnStarted, n * 100, n * 100 + 50);
        st.apply_lifecycle_event(LifecycleKind::TurnCompleted, n * 100 + 50, n * 100 + 100);
    }
    assert_eq!(st.index.entries.len(), 3);
    for (i, entry) in st.index.entries.iter().enumerate() {
        assert_eq!(entry.turn_number, (i + 1) as u64);
    }
    assert_eq!(st.manifest.turn_count, 3);
}
