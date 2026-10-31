//! Turn submission, acceptance, and cancellation DTOs.

use serde::{Deserialize, Serialize};

/// A turn was rejected before delivery because a startup interaction is pending.
pub const STARTUP_INTERACTION_PENDING_ERROR: i32 = -32001;

/// Fallback turn id for interactions raised with no active turn (startup
/// decisions). Produced by the service, matched by the desktop client.
pub const SESSION_START_TURN_ID: &str = "session-start";

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
    /// Runtime turn identifier.
    pub turn_id: String,
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
