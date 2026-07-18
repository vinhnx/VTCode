#![allow(missing_docs)]
//! ACP (Agent Communication Protocol) support for VT Code.
//!
//! This crate exposes both the ACP client library and the VT Code Zed bridge.
//! Downstream crates should treat this as the canonical ACP entrypoint.

pub mod capabilities;
pub mod client;
pub mod client_v2;
pub mod discovery;
pub mod error;
pub mod jsonrpc;
pub mod messages;
pub mod permissions;
pub mod reports;
pub mod session;
pub mod tooling;
mod tooling_provider;
pub mod transport;
pub mod workspace;
mod zed;

/// Compatibility facade re-exporting the upstream
/// [`agent_client_protocol`](https://docs.rs/agent-client-protocol) schema types
/// that the ACP bridge uses internally. The protocol schema types live under
/// [`agent_client_protocol::schema::v1`]; the role/transport types are imported
/// directly from the crate root.
pub(crate) mod acp {
    pub(crate) use agent_client_protocol::schema::ProtocolVersion;
    pub(crate) use agent_client_protocol::schema::v1::*;
}

pub use capabilities::{
    AgentCapabilities, AgentFeatures, AgentInfo as AgentInfoV2, AuthCredentials, AuthMethod, AuthRequirements,
    AuthenticateParams, AuthenticateResult, ClientCapabilities, ClientInfo, FilesystemCapabilities, InitializeParams,
    InitializeResult, PROTOCOL_VERSION, SUPPORTED_VERSIONS, TerminalCapabilities, ToolCapability, UiCapabilities,
};
pub use client_v2::{AcpClientV2, AcpClientV2Builder};
pub use discovery::{AgentInfo, AgentRegistry};
pub use error::{AcpError, AcpResult};
pub use jsonrpc::{JSONRPC_VERSION, JsonRpcError, JsonRpcId, JsonRpcRequest, JsonRpcResponse};
pub use session::{
    AcpSession, ConversationTurn, PermissionOption, PromptContent, RequestPermissionParams, RequestPermissionResult,
    SessionCancelParams, SessionLoadParams, SessionLoadResult, SessionNewParams, SessionNewResult, SessionPromptParams,
    SessionPromptResult, SessionState, SessionUpdate, SessionUpdateNotification, ToolCallRecord, TurnStatus,
};
pub use transport::{StdioTransport, StdioTransportOptions};
pub use zed::{StandardAcpAdapter, ZedAcpAdapter};

#[deprecated(since = "0.60.0", note = "Use AcpClientV2 for ACP protocol compliance")]
pub use client::{AcpClient, AcpClientBuilder};
#[deprecated(since = "0.60.0", note = "Use jsonrpc module types instead")]
pub use messages::{AcpMessage, AcpRequest, AcpResponse};

use std::sync::{Arc, OnceLock};

/// Connection context handle for in-handler notification sending.
///
/// In ACP 1.0+ the agent emits session notifications by calling
/// `cx.send_notification(...)` from inside a request handler. To preserve the
/// original "send update from any method" flow, the bridge stores the active
/// [`ConnectionHandle`] in a [`OnceLock`] for the lifetime of the connection.
static ACP_CONNECTION: OnceLock<Arc<zed::connection::ConnectionHandle>> = OnceLock::new();

/// Register the global ACP connection from the host protocol.
///
/// Returns `Err` with the provided connection if one has already been
/// registered. Callers may drop the returned connection or reuse it as needed.
pub fn register_acp_connection(
    connection: Arc<zed::connection::ConnectionHandle>,
) -> Result<(), Arc<zed::connection::ConnectionHandle>> {
    ACP_CONNECTION.set(connection)
}

/// Retrieve the registered ACP connection, if available.
pub fn acp_connection() -> Option<Arc<zed::connection::ConnectionHandle>> {
    ACP_CONNECTION.get().cloned()
}
