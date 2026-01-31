//! ACP (Agent Communication Protocol) client for inter-agent communication
//!
//! This module provides:
//! - **V2 (Recommended)**: Full ACP protocol compliance with JSON-RPC 2.0
//!   - Session lifecycle (initialize, session/new, session/prompt)
//!   - Capability negotiation
//!   - SSE streaming for real-time updates
//! - **V1 (Legacy)**: HTTP-based communication with remote agents
//!   - Agent discovery (online and offline)
//!   - Request/response message handling
//!
//! # Quick Start (V2)
//!
//! ```rust,ignore
//! use vtcode_acp_client::{AcpClientV2, ClientCapabilities};
//!
//! let client = AcpClientV2::new("http://agent.example.com")?;
//!
//! // Initialize connection and negotiate capabilities
//! let init_result = client.initialize().await?;
//!
//! // Create a session
//! let session = client.session_new(Default::default()).await?;
//!
//! // Send a prompt
//! let response = client.session_prompt(SessionPromptParams {
//!     session_id: session.session_id,
//!     content: vec![PromptContent::text("Hello!")],
//!     ..Default::default()
//! }).await?;
//! ```
//!
//! # Migration from V1
//!
//! The V1 `AcpClient` is deprecated and will be removed in a future version.
//! Migrate to `AcpClientV2` for full ACP protocol compliance.

// V2 modules (ACP compliant)
pub mod capabilities;
pub mod client_v2;
pub mod jsonrpc;
pub mod session;

// V1 modules (legacy, deprecated)
pub mod client;
pub mod discovery;
pub mod error;
pub mod messages;

// V2 exports (recommended)
pub use capabilities::{
    AgentCapabilities, AgentFeatures, AgentInfo as AgentInfoV2, AuthCredentials, AuthMethod,
    AuthRequirements, AuthenticateParams, AuthenticateResult, ClientCapabilities, ClientInfo,
    FilesystemCapabilities, InitializeParams, InitializeResult, PROTOCOL_VERSION,
    SUPPORTED_VERSIONS, TerminalCapabilities, ToolCapability, UiCapabilities,
};
pub use client_v2::{AcpClientV2, AcpClientV2Builder};
pub use jsonrpc::{JSONRPC_VERSION, JsonRpcError, JsonRpcId, JsonRpcRequest, JsonRpcResponse};
pub use session::{
    AcpSession, ConversationTurn, PermissionOption, PromptContent, RequestPermissionParams,
    RequestPermissionResult, SessionCancelParams, SessionLoadParams, SessionLoadResult,
    SessionNewParams, SessionNewResult, SessionPromptParams, SessionPromptResult, SessionState,
    SessionUpdate, SessionUpdateNotification, ToolCallRecord, TurnStatus,
};

// V1 exports (deprecated)
#[deprecated(since = "0.60.0", note = "Use AcpClientV2 for ACP protocol compliance")]
pub use client::{AcpClient, AcpClientBuilder};
pub use discovery::{AgentInfo, AgentRegistry};
pub use error::{AcpError, AcpResult};
#[deprecated(since = "0.60.0", note = "Use jsonrpc module types instead")]
pub use messages::{AcpMessage, AcpRequest, AcpResponse};

use agent_client_protocol::AgentSideConnection;
use std::sync::{Arc, OnceLock};

static ACP_CONNECTION: OnceLock<Arc<AgentSideConnection>> = OnceLock::new();

/// Register the global ACP connection from the host protocol.
///
/// Returns `Err` with the provided connection if one has already been
/// registered. Callers may drop the returned connection or reuse it as needed.
pub fn register_acp_connection(
    connection: Arc<AgentSideConnection>,
) -> Result<(), Arc<AgentSideConnection>> {
    ACP_CONNECTION.set(connection)
}

/// Retrieve the registered ACP connection, if available.
pub fn acp_connection() -> Option<Arc<AgentSideConnection>> {
    ACP_CONNECTION.get().cloned()
}
