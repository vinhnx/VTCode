use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Maximum number of follow-up intents held by a runtime before new intents
/// park in the overflow buffer. The overflow buffer keeps rapid influx from
/// silently dropping user messages; once both buffers are full the caller
/// receives `FollowUpQueueFull` so back-pressure stays visible.
pub const MAX_QUEUED_FOLLOW_UP_INTENTS: usize = 64;
/// Overflow park capacity after the primary FIFO is full. Combined with the
/// primary cap this bounds total retained follow-ups while still absorbing
/// a burst instead of discarding it.
pub const MAX_OVERFLOW_FOLLOW_UP_INTENTS: usize = 64;
/// Maximum number of applied follow-up IDs retained for restart recovery.
pub const MAX_APPLIED_FOLLOW_UP_INTENT_IDS: usize = 64;

/// A follow-up instruction with a stable identity for recovery and
/// de-duplication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedFollowUpIntent {
    id: String,
    text: String,
}

impl QueuedFollowUpIntent {
    /// Create a new intent with a UUIDv4 identity.
    #[must_use]
    pub fn new(text: String) -> Self {
        Self { id: Uuid::new_v4().to_string(), text }
    }

    /// Reconstruct an intent recovered from durable state.
    pub fn from_parts(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self { id: id.into(), text: text.into() }
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Consume the intent and return its user-visible text.
    #[must_use]
    pub fn into_parts(self) -> (String, String) {
        (self.id, self.text)
    }
}

/// Error returned when a runtime cannot accept another follow-up intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowUpQueueFull {
    pub capacity: usize,
}

impl fmt::Display for FollowUpQueueFull {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "follow-up intent queue is full (capacity {})", self.capacity)
    }
}

impl std::error::Error for FollowUpQueueFull {}

/// Messages used to steer the agent's execution loop from an external source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SteeringMessage {
    /// Stop the agent's execution loop immediately (Steering).
    SteerStop,
    /// Pause the agent's execution loop.
    Pause,
    /// Resume the agent's execution loop.
    Resume,
    /// Inject input as a follow-up user message. The turn loop applies it to
    /// live history at the next iteration boundary (mid-turn steering) or, if
    /// the turn ends first, delivers it at the next turn boundary.
    FollowUpInput(String),
}
