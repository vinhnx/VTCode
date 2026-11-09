use super::{BorrowedVersionedEvent, EVENT_SCHEMA_VERSION};
use vtcode_exec_events::{
    ThreadEvent, ThreadStartedEvent, TurnCompletedEvent, TurnStartedEvent, Usage, VersionedThreadEvent,
};

/// The borrowed envelope must produce JSON byte-identical to
/// `VersionedThreadEvent::new(event.clone())`. This guards against drift if
/// either the envelope or the canonical wrapper is modified.
#[test]
fn borrowed_envelope_matches_versioned_envelope() {
    for event in [
        ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread".to_string() }),
        ThreadEvent::TurnStarted(TurnStartedEvent::default()),
        ThreadEvent::TurnCompleted(TurnCompletedEvent {
            final_response_item_id: None,
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }),
    ] {
        let canonical = serde_json::to_string(&VersionedThreadEvent::new(event.clone())).expect("canonical serialize");
        let borrowed = serde_json::to_string(&BorrowedVersionedEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            event: &event,
        })
        .expect("borrowed serialize");
        assert_eq!(canonical, borrowed, "JSON differs for {event:?}");
    }
}
