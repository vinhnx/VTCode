//! VT Code ACP extension methods for session lifecycle management.
//!
//! The Agent Client Protocol covers `session/new`, `session/load`,
//! `session/prompt`, and `session/cancel`. Programmatic hosts (IDE bridges,
//! test harnesses, automation) additionally need the lifecycle operations
//! Codex's app-server exposes over its proprietary JSON-RPC dialect: fork,
//! rollback, and compact. This module implements them as **VT Code ACP
//! extensions** on the same connection, so one protocol surface serves
//! everything instead of maintaining a parallel native app-server (the
//! `vtcode app-server` Codex proxy keeps serving `provider = "codex"`).
//!
//! Wire contract (also emitted by `vtcode schema acp`):
//!
//! | Method             | Params                            | Result                                   |
//! |--------------------|-----------------------------------|------------------------------------------|
//! | `session/fork`     | `{session_id}`                    | `{session_id}` (new independent session) |
//! | `session/rollback` | `{session_id, keep_last_turns}`   | `{remaining_messages}`                   |
//! | `session/compact`  | `{session_id}`                    | `{original_messages, compacted_messages}`|
//!
//! ACP clients that don't know these methods are unaffected: they simply
//! never call them.

use std::sync::Arc;

use crate::acp::Error as SdkError;
use agent_client_protocol::v1::schema::SessionId;
use agent_client_protocol::v1::{Builder, Responder};
use agent_client_protocol::{
    Agent, Client, HandleDispatchFrom, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, RunWithConnectionTo,
    UntypedMessage, on_receive_request,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use vtcode_core::compaction::{CompactionConfig, compact_history};
use vtcode_core::core::threads::ThreadBootstrap;
use vtcode_core::llm::provider::{Message, MessageRole};

use super::super::constants::SESSION_PREFIX;
use super::ZedAgent;
use super::handlers::build_session_provider;

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

/// `session/fork` — branch an existing session into an independent one that
/// inherits the full conversation history and configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionForkRequest {
    /// Session to branch from.
    pub session_id: SessionId,
}

/// Result of `session/fork`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionForkResponse {
    /// The new session's id; it can be driven with `session/prompt` like any
    /// other session.
    pub session_id: SessionId,
}

/// `session/rollback` — drop the trailing conversation turns of a session.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionRollbackRequest {
    /// Session to roll back.
    pub session_id: SessionId,
    /// Number of most-recent user turns to keep. `0` clears the conversation.
    pub keep_last_turns: u32,
}

/// Result of `session/rollback`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionRollbackResponse {
    /// Messages left in the session after the rollback.
    pub remaining_messages: usize,
}

/// `session/compact` — summarize the session history in place to reclaim
/// context budget, preserving task continuity.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionCompactRequest {
    /// Session to compact.
    pub session_id: SessionId,
}

/// Result of `session/compact`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionCompactResponse {
    /// Message count before compaction.
    pub original_messages: usize,
    /// Message count after compaction.
    pub compacted_messages: usize,
}

macro_rules! impl_lifecycle_request {
    ($req:ty, $res:ty, $method:literal) => {
        impl JsonRpcMessage for $req {
            fn matches_method(method: &str) -> bool {
                method == $method
            }

            fn method(&self) -> &'static str {
                $method
            }

            fn to_untyped_message(&self) -> Result<UntypedMessage, agent_client_protocol::Error> {
                UntypedMessage::new($method, self)
            }

            fn parse_message(method: &str, params: &serde_json::Value) -> Result<Self, agent_client_protocol::Error> {
                if !Self::matches_method(method) {
                    return Err(agent_client_protocol::Error::method_not_found());
                }
                serde_json::from_value(params.clone())
                    .map_err(|error| agent_client_protocol::Error::invalid_params().data(error.to_string()))
            }
        }

        impl JsonRpcRequest for $req {
            type Response = $res;
        }
    };
}

impl_lifecycle_request!(SessionForkRequest, SessionForkResponse, "session/fork");
impl_lifecycle_request!(SessionRollbackRequest, SessionRollbackResponse, "session/rollback");
impl_lifecycle_request!(SessionCompactRequest, SessionCompactResponse, "session/compact");

macro_rules! impl_lifecycle_response {
    ($res:ty) => {
        impl JsonRpcResponse for $res {
            fn into_json(self, _method: &str) -> Result<serde_json::Value, agent_client_protocol::Error> {
                serde_json::to_value(self)
                    .map_err(|error| agent_client_protocol::Error::internal_error().data(error.to_string()))
            }

            fn from_json(_method: &str, value: serde_json::Value) -> Result<Self, agent_client_protocol::Error> {
                serde_json::from_value(value)
                    .map_err(|error| agent_client_protocol::Error::invalid_params().data(error.to_string()))
            }
        }
    };
}

impl_lifecycle_response!(SessionForkResponse);
impl_lifecycle_response!(SessionRollbackResponse);
impl_lifecycle_response!(SessionCompactResponse);

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

impl ZedAgent {
    /// Allocate the next session id without registering a thread.
    fn allocate_session_id(&self) -> SessionId {
        let raw_id = self.next_session_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        SessionId::new(Arc::from(format!("{SESSION_PREFIX}-{raw_id}")))
    }

    /// Branch `parent_id` into a new independent session with copied history.
    pub(crate) async fn fork_session(&self, parent_id: &SessionId) -> Result<SessionId, SdkError> {
        let parent = self
            .session_handle(parent_id)
            .ok_or_else(|| SdkError::invalid_params().data(json!({ "reason": "unknown_session" })))?;
        let snapshot = {
            let Ok(data) = parent.data.lock() else {
                return Err(SdkError::internal_error());
            };
            data.thread.snapshot()
        };

        let fork_id = self.allocate_session_id();
        let thread = self
            .thread_manager
            .start_thread_with_identifier(fork_id.0.to_string(), ThreadBootstrap::from_snapshot(snapshot));
        let handle = self.build_session_handle(fork_id.clone(), thread);
        if let Ok(mut guard) = self.sessions.lock() {
            drop(guard.insert(fork_id.clone(), handle));
        }
        Ok(fork_id)
    }

    /// Drop trailing user turns from a session, keeping `keep_last_turns`.
    pub(crate) fn rollback_session(&self, session_id: &SessionId, keep_last_turns: u32) -> Result<usize, SdkError> {
        let session = self
            .session_handle(session_id)
            .ok_or_else(|| SdkError::invalid_params().data(json!({ "reason": "unknown_session" })))?;
        let Ok(data) = session.data.lock() else {
            return Err(SdkError::internal_error());
        };
        let messages = data.thread.messages();
        let boundary = turn_boundary_index(&messages, keep_last_turns);
        let kept = messages.get(boundary..).unwrap_or_default();
        data.thread.replace_messages(kept.to_vec());
        Ok(kept.len())
    }

    /// Summarize a session's history in place via the configured model.
    pub(crate) async fn compact_session(&self, session_id: &SessionId) -> Result<(usize, usize), SdkError> {
        let session = self
            .session_handle(session_id)
            .ok_or_else(|| SdkError::invalid_params().data(json!({ "reason": "unknown_session" })))?;
        let (provider_name, model, history) = {
            let Ok(data) = session.data.lock() else {
                return Err(SdkError::internal_error());
            };
            (data.provider.clone(), data.model.clone(), data.thread.messages())
        };
        // The data lock is released before the LLM call (no lock across await).
        let provider = build_session_provider(self, &provider_name, &model)?;
        let original = history.len();
        let compacted = compact_history(provider.as_ref(), &model, &history, &CompactionConfig::default())
            .await
            .map_err(|error| SdkError::internal_error().data(error.to_string()))?;
        let compacted_len = compacted.len();
        let Ok(data) = session.data.lock() else {
            return Err(SdkError::internal_error());
        };
        // The lock was released during the LLM call, so a concurrent
        // `session/prompt` or `session/rollback` may have mutated the history
        // the summary was built from. Replacing it wholesale would silently
        // drop those turns — fail closed instead of compacting stale state.
        ensure_history_unchanged(&data.thread.messages(), &history)?;
        data.thread.replace_messages(compacted);
        Ok((original, compacted_len))
    }
}

/// Verify `current` still equals the `snapshot` a long-running operation was
/// built from, so the operation can safely replace the history in place.
/// Returns an error naming the concurrent mutation when they diverge.
fn ensure_history_unchanged(current: &[Message], snapshot: &[Message]) -> Result<(), SdkError> {
    if current == snapshot {
        return Ok(());
    }
    Err(SdkError::internal_error().data(json!({
        "reason": "session_changed_during_operation",
        "snapshot_messages": snapshot.len(),
        "current_messages": current.len(),
    })))
}

/// Index of the first message of the trailing `keep_last_turns` user turns.
///
/// A turn starts at its user message; everything before the kept window is
/// rolled back. `keep_last_turns == 0` removes the whole conversation, and
/// histories with fewer user turns than requested are left untouched.
fn turn_boundary_index(messages: &[Message], keep_last_turns: u32) -> usize {
    let user_positions: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.role == MessageRole::User)
        .map(|(index, _)| index)
        .collect();
    if keep_last_turns == 0 {
        return messages.len();
    }
    match user_positions.len().checked_sub(keep_last_turns as usize) {
        Some(offset) => user_positions.get(offset).copied().unwrap_or(0),
        // Fewer user turns than requested: nothing to roll back.
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// Schema export
// ---------------------------------------------------------------------------

/// Method descriptions for the lifecycle extensions (shared by the schema
/// document builder and the crate docs).
pub const LIFECYCLE_METHODS: &[(&str, &str)] = &[
    (
        "session/fork",
        "Branch an existing session into a new independent session with copied history; returns {session_id}.",
    ),
    (
        "session/rollback",
        "Drop the trailing conversation turns of a session, keeping the requested number of most-recent user turns; returns {remaining_messages}.",
    ),
    (
        "session/compact",
        "Summarize a session's history in place via the configured model to reclaim context budget; returns {original_messages, compacted_messages}.",
    ),
];

/// JSON-Schema document describing the VT Code lifecycle extension methods —
/// the wire contract emitted by `vtcode schema acp`.
#[must_use]
pub fn lifecycle_schema_document() -> serde_json::Value {
    let methods = [
        ("session/fork", schema_for_type::<SessionForkRequest>(), schema_for_type::<SessionForkResponse>()),
        (
            "session/rollback",
            schema_for_type::<SessionRollbackRequest>(),
            schema_for_type::<SessionRollbackResponse>(),
        ),
        (
            "session/compact",
            schema_for_type::<SessionCompactRequest>(),
            schema_for_type::<SessionCompactResponse>(),
        ),
    ];
    let methods = methods
        .into_iter()
        .map(|(method, params, result)| {
            let description = LIFECYCLE_METHODS
                .iter()
                .find(|(name, _)| *name == method)
                .map(|(_, description)| (*description).to_string())
                .unwrap_or_default();
            json!({
                "method": method,
                "description": description,
                "params": params,
                "result": result,
            })
        })
        .collect::<Vec<_>>();

    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "kind": "vtcode-acp-lifecycle-extensions",
        "note": "VT Code ACP extension methods served on the standard ACP connection; standard ACP clients are unaffected.",
        "methods": methods,
    })
}

fn schema_for_type<T: JsonSchema>() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(T)).unwrap_or(serde_json::Value::Null)
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register the VT Code lifecycle extension handlers onto the SACP builder.
pub fn install_lifecycle_handlers<H, R>(
    builder: Builder<Agent, H, R>,
    agent: Arc<ZedAgent>,
) -> Builder<Agent, impl HandleDispatchFrom<Client>, R>
where
    H: HandleDispatchFrom<Client>,
    R: RunWithConnectionTo<Client>,
{
    builder
        .on_receive_request(
            {
                let agent = Arc::clone(&agent);
                move |req: SessionForkRequest, request_cx: Responder<SessionForkResponse>, _cx| {
                    let agent = Arc::clone(&agent);
                    async move {
                        request_cx.respond_with_result(
                            agent
                                .fork_session(&req.session_id)
                                .await
                                .map(|session_id| SessionForkResponse { session_id }),
                        )
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let agent = Arc::clone(&agent);
                move |req: SessionRollbackRequest, request_cx: Responder<SessionRollbackResponse>, _cx| {
                    let agent = Arc::clone(&agent);
                    async move {
                        request_cx.respond_with_result(
                            agent
                                .rollback_session(&req.session_id, req.keep_last_turns)
                                .map(|remaining_messages| SessionRollbackResponse { remaining_messages }),
                        )
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let agent = Arc::clone(&agent);
                move |req: SessionCompactRequest, request_cx: Responder<SessionCompactResponse>, _cx| {
                    let agent = Arc::clone(&agent);
                    async move {
                        request_cx.respond_with_result(agent.compact_session(&req.session_id).await.map(
                            |(original_messages, compacted_messages)| SessionCompactResponse {
                                original_messages,
                                compacted_messages,
                            },
                        ))
                    }
                }
            },
            on_receive_request!(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp;
    use assert_fs::TempDir;
    use vtcode_core::llm::provider::Message;

    use super::super::test_support::build_agent;

    fn message(role: MessageRole, text: &str) -> Message {
        Message {
            role,
            content: text.to_string().into(),
            ..Message::default()
        }
    }

    #[test]
    fn turn_boundary_keeps_exactly_the_requested_trailing_turns() {
        // Two user turns, each with a tool message: [u, t, a, u, t, a]
        let messages = vec![
            message(MessageRole::User, "one"),
            message(MessageRole::Tool, "tool-1"),
            message(MessageRole::Assistant, "reply-1"),
            message(MessageRole::User, "two"),
            message(MessageRole::Tool, "tool-2"),
            message(MessageRole::Assistant, "reply-2"),
        ];
        // keep=1: boundary at the second user message, so its tool message and
        // reply stay attached to the kept turn.
        assert_eq!(turn_boundary_index(&messages, 1), 3);
        // keep=2: nothing removed.
        assert_eq!(turn_boundary_index(&messages, 2), 0);
        // keep=3 (more turns than exist): nothing removed.
        assert_eq!(turn_boundary_index(&messages, 3), 0);
    }

    #[test]
    fn turn_boundary_zero_clears_and_userless_history_is_untouched() {
        let messages = vec![
            message(MessageRole::User, "one"),
            message(MessageRole::Assistant, "reply"),
        ];
        assert_eq!(turn_boundary_index(&messages, 0), messages.len());

        // No user turns at all (e.g. system-only history): rollback is a no-op
        // rather than silently clearing the window.
        let system_only = vec![message(MessageRole::System, "instructions")];
        assert_eq!(turn_boundary_index(&system_only, 1), 0);
        assert_eq!(turn_boundary_index(&system_only, 0), system_only.len());
    }

    #[tokio::test]
    async fn fork_clones_history_into_an_independent_session() {
        let temp = TempDir::new().expect("workspace");
        let agent = build_agent(temp.path()).await;
        let parent = agent
            .new_session(acp::NewSessionRequest::new(temp.path().to_path_buf()))
            .await
            .expect("parent session");
        agent.push_message(
            &agent.session_handle(&parent.session_id).expect("parent handle"),
            message(MessageRole::User, "hi"),
        );

        let forked_id = agent.fork_session(&parent.session_id).await.expect("fork");
        assert_ne!(forked_id, parent.session_id, "fork must be a new session id");

        let parent_messages = agent
            .session_handle(&parent.session_id)
            .expect("parent handle")
            .data
            .lock()
            .expect("lock")
            .thread
            .messages();
        let forked_messages = agent
            .session_handle(&forked_id)
            .expect("forked handle")
            .data
            .lock()
            .expect("lock")
            .thread
            .messages();
        assert_eq!(parent_messages.len(), forked_messages.len(), "fork inherits history");
        assert_eq!(forked_messages.last().map(|m| m.role), Some(MessageRole::User));
    }

    #[tokio::test]
    async fn rollback_truncates_the_requested_turns() {
        let temp = TempDir::new().expect("workspace");
        let agent = build_agent(temp.path()).await;
        let session = agent
            .new_session(acp::NewSessionRequest::new(temp.path().to_path_buf()))
            .await
            .expect("session");
        let handle = agent.session_handle(&session.session_id).expect("handle");
        agent.push_message(&handle, message(MessageRole::User, "turn-1"));
        agent.push_message(&handle, message(MessageRole::Assistant, "reply-1"));
        agent.push_message(&handle, message(MessageRole::User, "turn-2"));
        agent.push_message(&handle, message(MessageRole::Assistant, "reply-2"));

        let remaining = agent.rollback_session(&session.session_id, 1).expect("rollback");
        assert_eq!(remaining, 2, "only the last user turn and its reply remain");
        let messages = handle.data.lock().expect("lock").thread.messages();
        assert_eq!(messages[0].role, MessageRole::User);
        assert_eq!(messages.len(), 2);

        // Clearing the whole conversation is expressed as keep=0.
        let remaining = agent.rollback_session(&session.session_id, 0).expect("rollback");
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn lifecycle_operations_reject_unknown_sessions() {
        let temp = TempDir::new().expect("workspace");
        let agent = build_agent(temp.path()).await;
        let unknown = SessionId::new(Arc::from("vtcode-zed-session-404"));

        let fork_error = agent.fork_session(&unknown).await.expect_err("unknown fork");
        assert!(
            fork_error.data.as_ref().is_some_and(|data| data["reason"] == "unknown_session"),
            "fork must report unknown_session: {fork_error:?}"
        );
        let rollback_error = agent.rollback_session(&unknown, 1).expect_err("unknown rollback");
        assert!(
            rollback_error
                .data
                .as_ref()
                .is_some_and(|data| data["reason"] == "unknown_session"),
            "rollback must report unknown_session: {rollback_error:?}"
        );
    }

    #[test]
    fn history_unchanged_guard_accepts_only_exact_snapshots() {
        let snapshot = vec![
            message(MessageRole::User, "one"),
            message(MessageRole::Assistant, "reply"),
        ];

        // Identical history: the operation may replace it in place.
        assert!(ensure_history_unchanged(&snapshot, &snapshot).is_ok());

        // Appended turn: replacing would drop the new messages.
        let appended = {
            let mut messages = snapshot.clone();
            messages.push(message(MessageRole::User, "two"));
            messages
        };
        assert!(ensure_history_unchanged(&appended, &snapshot).is_err());

        // Rolled-back history: replacing would resurrect dropped turns.
        let truncated = vec![message(MessageRole::User, "one")];
        assert!(ensure_history_unchanged(&truncated, &snapshot).is_err());

        // Same length, different content: a length-only check would miss this.
        let mutated = vec![
            message(MessageRole::User, "one"),
            message(MessageRole::Assistant, "different reply"),
        ];
        assert!(ensure_history_unchanged(&mutated, &snapshot).is_err());
    }

    #[test]
    fn lifecycle_wire_types_parse_their_own_json() {
        let session_id = SessionId::new(Arc::from("vtcode-zed-session-1"));
        let fork = SessionForkRequest { session_id: session_id.clone() };
        let fork_json = serde_json::to_value(&fork).expect("serialize fork");
        let parsed: SessionForkRequest = SessionForkRequest::parse_message("session/fork", &fork_json).expect("parse");
        assert_eq!(parsed.session_id, session_id);
        assert!(SessionForkRequest::matches_method("session/fork"));
        assert!(!SessionForkRequest::matches_method("session/rollback"));
        assert!(SessionForkRequest::parse_message("session/rollback", &fork_json).is_err());

        let rollback = SessionRollbackRequest { session_id, keep_last_turns: 2 };
        let rollback_json = serde_json::to_value(&rollback).expect("serialize rollback");
        let parsed: SessionRollbackRequest =
            SessionRollbackRequest::parse_message("session/rollback", &rollback_json).expect("parse");
        assert_eq!(parsed.keep_last_turns, 2);
    }
}
