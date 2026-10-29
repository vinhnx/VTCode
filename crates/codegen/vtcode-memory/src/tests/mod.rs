//! Tests for the unified session store.

pub(super) use std::fs;
pub(super) use std::sync::Arc;

pub(super) use tempfile::TempDir;
pub(super) use vtcode_exec_events::{
    AgentMessageItem, ItemCompletedEvent, ThreadCompletedEvent, ThreadCompletionSubtype, ThreadEvent, ThreadItem,
    ThreadItemDetails, ThreadStartedEvent, TurnCompletedEvent, TurnStartedEvent, Usage, VersionedThreadEvent,
};

pub(super) use crate::event_log::{DEFAULT_MAX_EVENTS, SessionEventLog};
pub(super) use crate::migration::migrate_legacy;
pub(super) use crate::query::{query_facts, recent_sessions};
pub(super) use crate::{
    EvictionSummaryHook, SessionStoreError, open, open_with_eviction_summary, retention::apply_retention, sessions_root,
};

fn sample_turn() -> Vec<ThreadEvent> {
    vec![
        ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread".to_string() }),
        ThreadEvent::TurnStarted(TurnStartedEvent::default()),
        ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }),
        ThreadEvent::ThreadCompleted(Box::new(ThreadCompletedEvent {
            thread_id: "thread".to_string(),
            completed_at: None,
            session_id: "session".to_string(),
            subtype: ThreadCompletionSubtype::Success,
            outcome_code: "completed".to_string(),
            result: None,
            stop_reason: None,
            usage: Usage::default(),
            total_cost_usd: None,
            num_turns: 1,
        })),
    ]
}

fn sample_zero_turn_completion() -> Vec<ThreadEvent> {
    vec![
        ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "empty".to_string() }),
        ThreadEvent::ThreadCompleted(Box::new(ThreadCompletedEvent {
            thread_id: "empty".to_string(),
            completed_at: None,
            session_id: "session".to_string(),
            subtype: ThreadCompletionSubtype::Cancelled,
            outcome_code: "exit".to_string(),
            result: None,
            stop_reason: None,
            usage: Usage::default(),
            total_cost_usd: None,
            num_turns: 0,
        })),
    ]
}

mod manifest_scan;
mod retention;
mod session_store;
