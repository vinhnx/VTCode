//! Connection context handle for the Agent→Client link.
//!
//! [`ConnectionHandle`] wraps the SACP [`ConnectionTo<Client>`] the agent
//! uses to talk back to its peer (notifications + RPCs), and exposes the
//! small set of client-facing operations the agent bridge actually uses:
//!
//! * [`ConnectionHandle::send_session_notification`] — fire-and-forget
//!   `session/update` notifications.
//! * [`ConnectionHandle::request_permission`] — ask the client whether a
//!   tool call may proceed.
//! * [`ConnectionHandle::read_text_file`] — delegate a `fs/read_text_file`
//!   call to the client.
//! * [`ConnectionHandle::create_terminal`] — ask the client to spin up a
//!   terminal session.
//!
//! Internally all RPC calls route through the SACP `send_request(...).
//! block_task()` future, which is **only safe inside a `cx.spawn(...)`
//! task** (never directly in a `on_receive_*` handler, which would
//! deadlock the dispatch loop). Callers that still want to drive these
//! RPCs synchronously from inside a request handler must therefore wrap
//! the call in `cx.spawn`.
//!
//! In practice, the agent bridge funnels its work through a `cx.spawn`
//! task and reaches the handle through the global [`crate::acp_connection`]
//! registry, so the deadlock constraint is enforced at the call site.

use std::sync::Arc;

use agent_client_protocol::{
    Client, ConnectionTo, Error,
    schema::v1::{
        CreateTerminalRequest, CreateTerminalResponse, ReadTextFileRequest, ReadTextFileResponse,
        RequestPermissionRequest, RequestPermissionResponse, SessionNotification,
    },
};

type AgentCx = ConnectionTo<Client>;

/// Shared handle to the active SACP connection's outgoing side.
///
/// Cheap to clone (`Arc`-backed) and `Send + Sync`, so it can be stored
/// in the agent state, in the global registry, and in spawned tasks.
#[derive(Clone)]
pub struct ConnectionHandle {
    cx: AgentCx,
}

impl ConnectionHandle {
    /// Wrap a freshly built SACP `cx` handle.
    pub fn new(cx: AgentCx) -> Arc<Self> {
        Arc::new(Self { cx })
    }

    /// Borrow the underlying SACP `cx` (escape hatch for advanced uses).
    pub fn cx(&self) -> &AgentCx {
        &self.cx
    }

    /// Send a `session/update` notification.
    ///
    /// Notifications do not block the dispatch loop, so this is safe in
    /// any context (handler, spawned task, or synchronous setup).
    pub fn send_session_notification(
        &self,
        notification: SessionNotification,
    ) -> Result<(), Error> {
        self.cx.send_notification(notification)
    }

    /// Send a `session/request_permission` request and await the response.
    ///
    /// **Only call from a `cx.spawn` task** — calling from a request
    /// handler deadlocks the SACP dispatch loop.
    pub async fn request_permission(
        &self,
        request: RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse, Error> {
        self.cx.send_request(request).block_task().await
    }

    /// Send an `fs/read_text_file` request and await the response.
    ///
    /// **Only call from a `cx.spawn` task** — see
    /// [`ConnectionHandle::request_permission`].
    pub async fn read_text_file(
        &self,
        request: ReadTextFileRequest,
    ) -> Result<ReadTextFileResponse, Error> {
        self.cx.send_request(request).block_task().await
    }

    /// Send a `terminal/create` request and await the response.
    ///
    /// **Only call from a `cx.spawn` task** — see
    /// [`ConnectionHandle::request_permission`].
    pub async fn create_terminal(
        &self,
        request: CreateTerminalRequest,
    ) -> Result<CreateTerminalResponse, Error> {
        self.cx.send_request(request).block_task().await
    }
}

impl std::fmt::Debug for ConnectionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionHandle").finish_non_exhaustive()
    }
}
