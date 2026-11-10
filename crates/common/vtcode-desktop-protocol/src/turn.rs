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

/// Bounded identity token for one queue entry, minted by the service so the
/// client can address one message for take-latest and remove without relying
/// on positional order (which races against concurrent submits).
pub const MAX_QUEUE_ENTRY_ID_LEN: usize = 64;

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
    /// Display path used for labeling: workspace-relative for workspace
    /// files, absolute for files attached from anywhere else.
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
    /// Discard queued turns behind the cancelled one (running-switch flow:
    /// the terminal pump auto-dispatches the queue, so a deferred session
    /// switch can only land on an empty queue). Absent (legacy clients)
    /// means keep the queue: a plain stop lets queued prompts run
    /// afterwards (TUI parity).
    #[serde(default)]
    pub clear_queue: bool,
    /// Skip the cooperative-cancellation grace period and start the
    /// teardown escalation immediately. The desktop client sets this on a
    /// repeat Stop press while a cancel is already outstanding; the first
    /// press stays cooperative so a healthy turn stops without a runtime
    /// restart. Absent (legacy clients) means cooperative with the
    /// standard grace backstop.
    #[serde(default)]
    pub force: bool,
}

/// Parameters for reversing one turn's exact file changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnUndoParams {
    /// Session that owns the turn.
    pub session_id: String,
    /// Canonical turn identity (event `context.turn_id`).
    pub turn_id: String,
}

/// One reversed file change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnUndoFile {
    /// Workspace-relative path.
    pub path: String,
    /// Kind of the reversed change.
    pub kind: vtcode_exec_events::PatchChangeKind,
    /// Reversed added-line count.
    #[serde(default)]
    pub additions: u64,
    /// Reversed deleted-line count.
    #[serde(default)]
    pub deletions: u64,
}

/// Outcome of a turn undo request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnUndoResult {
    /// Session that owns the turn.
    pub session_id: String,
    /// Canonical turn identity.
    pub turn_id: String,
    /// Whether every recorded change was reversed. `false` carries the
    /// refusal reason; nothing is partially reversed without disclosure.
    pub undone: bool,
    /// Reversed changes in recorded order: empty on clean refusals, holding
    /// the applied prefix when a later file aborts the flush.
    #[serde(default)]
    pub files: Vec<TurnUndoFile>,
    /// Refusal or failure explanation; empty on success.
    #[serde(default)]
    pub reason: String,
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
    /// Entry identity tokens matching `previews` one-to-one (when the service
    /// supplies them). Absent or shorter (legacy services) means the client
    /// falls back to newest-first take-latest and hides per-row actions.
    #[serde(default)]
    pub entry_ids: Vec<String>,
    /// Seconds since each entry was queued, oldest first, matching `previews`
    /// one-to-one. Absent (legacy services) hides the elapsed-time column.
    #[serde(default)]
    pub queued_for_secs: Vec<u64>,
    /// Total oldest messages discarded by the queue cap so far.
    #[serde(default)]
    pub dropped_total: usize,
}

/// Parameters for removing one queued message by entry id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnQueueRemoveParams {
    /// Session that owns the queue.
    pub session_id: String,
    /// Entry identity token from the latest `turn/queue-status` snapshot.
    pub entry_id: String,
}

/// Outcome of a remove-one-queued-message request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnQueueRemoveResult {
    /// Session that owns the queue.
    pub session_id: String,
    /// Whether the entry id matched a queued message and it was removed.
    pub removed: bool,
    /// Queue depth after the removal.
    pub queue_depth: usize,
}
