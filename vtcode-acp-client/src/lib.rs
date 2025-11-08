//! ACP (Agent Communication Protocol) client for inter-agent communication
//!
//! This module provides:
//! - HTTP-based communication with remote agents
//! - Agent discovery (online and offline)
//! - Request/response message handling
//! - Async-first design with optional sync support

pub mod client;
pub mod discovery;
pub mod error;
pub mod messages;

pub use client::{AcpClient, AcpClientBuilder};
pub use discovery::{AgentInfo, AgentRegistry};
pub use error::{AcpError, AcpResult};
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
