use agent_client_protocol::AgentSideConnection;
use std::sync::{Arc, OnceLock};

static ACP_CLIENT: OnceLock<Arc<AgentSideConnection>> = OnceLock::new();

/// Register the global ACP client connection.
///
/// Returns `Err` with the provided connection if one has already been
/// registered. Callers may drop the returned connection or reuse it as
/// needed.
pub fn register_acp_client(
    client: Arc<AgentSideConnection>,
) -> Result<(), Arc<AgentSideConnection>> {
    ACP_CLIENT.set(client)
}

/// Retrieve the registered ACP client connection, if available.
pub fn acp_client() -> Option<Arc<AgentSideConnection>> {
    ACP_CLIENT.get().cloned()
}
