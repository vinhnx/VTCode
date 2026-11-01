//! Turn submission, acceptance, cancellation, and queue DTOs.

use serde::{Deserialize, Serialize};

/// A turn was rejected before delivery because a startup interaction is pending.
pub const STARTUP_INTERACTION_PENDING_ERROR: i32 = -32001;

/// Fallback turn id for interactions raised with no active turn (startup
/// decisions). Produced by the service, matched by the desktop client.
pub const SESSION_START_TURN_ID: &str = "session-start";

/// Maximum queued-message previews carried by one queue-status notification.
/// Mirrors the CLI sidebar, which renders the first few queued inputs.
pub const MAX_QUEUE_STATUS_PREVIEWS: usize = 5;

/// Maximum characters per queued-message preview before truncation.
/// Mirrors the CLI sidebar preview width.
pub const MAX_QUEUE_PREVIEW_CHARS: usize = 50;

/// Build the display preview for one queued message: the full text when it
/// fits, otherwise the first [`MAX_QUEUE_PREVIEW_CHARS`] characters plus an
/// ellipsis. Truncation is char-boundary safe; newlines are preserved for the
/// client to render.
#[must_use]
pub fn queue_preview_label(text: &str) -> String {
    let mut preview = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= MAX_QUEUE_PREVIEW_CHARS {
            preview.push('\u{2026}');
            return preview;
        }
        preview.push(ch);
    }
    preview
}

/// Parameters for submitting a prompt to the selected session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSubmitParams {
    /// Session that owns the new turn.
    pub session_id: String,
    /// User prompt.
    pub prompt: String,
    /// Inline text attachments read by the client, applied as content parts.
    #[serde(default)]
    pub attachments: Vec<TurnAttachment>,
    /// Whether a busy-time submission may merge with adjacent queued text-only
    /// submissions into one turn. Mirrors the CLI Ctrl+Enter batchable flag;
    /// submissions carrying attachments always dispatch alone.
    #[serde(default)]
    pub batchable: bool,
}

/// One client-read text attachment for a submitted turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnAttachment {
    /// Workspace-relative path used for labeling.
    pub path: String,
    /// Bounded file content.
    pub text: String,
}

/// Accepted turn response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnAccepted {
    /// Session identifier.
    pub session_id: String,
    /// Runtime turn identifier. Empty when `queued` is set: a queued prompt
    /// owns no turn until its batch dispatches.
    pub turn_id: String,
    /// Whether the prompt was queued behind a running turn instead of
    /// starting immediately. Absent (legacy services) means immediate.
    #[serde(default)]
    pub queued: bool,
    /// Queue depth after this acceptance.
    #[serde(default)]
    pub queue_depth: usize,
    /// Oldest messages discarded by the queue cap while accepting this one.
    #[serde(default)]
    pub dropped: usize,
}

/// Parameters for cancelling an active turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCancelParams {
    /// Owning session.
    pub session_id: String,
    /// Exact turn identity to cancel.
    pub turn_id: String,
}

/// Parameters for popping the newest queued message back for editing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnQueueTakeLatestParams {
    /// Session that owns the queue.
    pub session_id: String,
}

/// Result of popping the newest queued message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnQueueTakeLatestResult {
    /// Session that owns the queue.
    pub session_id: String,
    /// Popped message text, or `None` when the queue was empty.
    #[serde(default)]
    pub text: Option<String>,
    /// Queue depth after the pop.
    #[serde(default)]
    pub queue_depth: usize,
}

/// Authoritative queued-message snapshot published by the service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnQueueStatus {
    /// Session that owns the queue.
    pub session_id: String,
    /// Number of messages waiting for dispatch.
    pub queue_depth: usize,
    /// Display previews for the oldest queued messages, oldest first, up to
    /// [`MAX_QUEUE_STATUS_PREVIEWS`].
    #[serde(default)]
    pub previews: Vec<String>,
    /// Total oldest messages discarded by the queue cap so far.
    #[serde(default)]
    pub dropped_total: usize,
}
