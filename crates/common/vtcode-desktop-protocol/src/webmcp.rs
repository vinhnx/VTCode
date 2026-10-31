//! WebMCP bridge status and pairing DTOs.

use serde::{Deserialize, Serialize};

/// WebMCP bridge state exposed to the desktop client without pairing secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebmcpBridgeStatus {
    /// Whether the listener is running in this desktop service process.
    pub running: bool,
    /// Loopback WebSocket endpoint when the listener is running.
    pub endpoint: Option<String>,
    /// Exact origins allowed to pair. Empty means the next explicit pair request binds one origin.
    pub allowed_origins: Vec<String>,
    /// Selected durable session, if any.
    pub session_id: Option<String>,
    /// Whether an active desktop runtime can accept a browser turn now.
    pub turns_available: bool,
    /// Canonical workspace used by the bridge.
    pub workspace: String,
}

/// Request to issue a one-time WebMCP pairing code for one exact browser origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebmcpPairParams {
    /// Exact browser origin, including scheme and optional port, without a path.
    pub origin: String,
    /// Revoke existing browser sessions before issuing the replacement code.
    #[serde(default)]
    pub replace: bool,
}

/// One-time pairing material. The code is returned only by the pair operation and is never persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebmcpPairResult {
    /// Loopback WebSocket endpoint for the paired browser.
    pub endpoint: String,
    /// Exact origin bound to the pairing code.
    pub origin: String,
    /// Short-lived, single-use pairing code.
    pub pairing_code: String,
    /// Remaining lifetime of the pairing code.
    pub expires_in_secs: u64,
}
