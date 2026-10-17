use serde::{Deserialize, Serialize};
use serde_json::Value;
use vtcode_exec_events::VersionedThreadEvent;

/// Current browser/server protocol version.
pub const PROTOCOL_VERSION: &str = "1";

pub(crate) const MAX_REQUEST_ID_BYTES: usize = 256;

pub(crate) fn is_valid_request_id(request_id: &str) -> bool {
    !request_id.is_empty() && request_id.len() <= MAX_REQUEST_ID_BYTES
}

pub(crate) fn response_request_id(request_id: &str) -> &str {
    if is_valid_request_id(request_id) {
        request_id
    } else {
        "unknown"
    }
}

/// A structured browser edit proposal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileChange {
    /// Workspace-relative path.
    pub path: String,
    /// SHA-256 digest of the file the browser edited.
    pub base_digest: String,
    /// Complete proposed file content.
    pub content: String,
}

/// Browser requests accepted by the WebMCP server.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum BridgeRequest {
    /// Query canonical retained execution facts after authentication.
    #[serde(rename = "explanation.get")]
    ExplanationGet {
        /// Response correlation identifier.
        request_id: String,
        /// In-memory pairing token.
        token: String,
        /// Latest task or retained session.
        #[serde(default)]
        scope: vtcode_memory::explanation::ExplanationScope,
        /// Collection page index.
        #[serde(default)]
        offset: usize,
    },
    /// Resolve one content-addressed, paged canonical source.
    #[serde(rename = "explanation.evidence")]
    ExplanationEvidence {
        /// Response correlation identifier.
        request_id: String,
        /// In-memory pairing token.
        token: String,
        /// Content-addressed canonical source.
        reference: vtcode_memory::explanation::EvidenceRef,
        /// UTF-8 evidence page offset.
        #[serde(default)]
        offset: usize,
    },
    /// Focus evidence in the terminal review surface without runtime execution.
    #[serde(rename = "explanation.navigate")]
    ExplanationNavigate {
        /// Response correlation identifier.
        request_id: String,
        /// In-memory pairing token.
        token: String,
        /// Content-addressed canonical source.
        reference: vtcode_memory::explanation::EvidenceRef,
    },
    /// Consume the one-time pairing code.
    #[serde(rename = "pair")]
    Pair {
        /// Correlates the response with the browser request.
        request_id: String,
        /// One-time code displayed by VT Code. Omit when resuming an in-memory session.
        #[serde(default)]
        code: String,
        /// Existing in-memory session token used only to resume a dropped socket.
        #[serde(default)]
        resume_token: Option<String>,
        /// Optional browser-provided origin, checked against the HTTP header.
        #[serde(default)]
        origin: Option<String>,
        /// Replay events after this bridge sequence when reconnecting.
        #[serde(default)]
        after_sequence: Option<u64>,
    },
    /// Return bridge and runtime status.
    #[serde(rename = "status")]
    Status {
        /// Correlates the response with the browser request.
        request_id: String,
        /// In-memory pairing token.
        token: String,
    },
    /// List bounded workspace files.
    #[serde(rename = "workspace.list_files", alias = "list_files")]
    ListFiles {
        /// Correlates the response with the browser request.
        request_id: String,
        /// In-memory pairing token.
        token: String,
    },
    /// Read a workspace file and its digest.
    #[serde(rename = "workspace.read_file", alias = "read_file")]
    ReadFile {
        /// Correlates the response with the browser request.
        request_id: String,
        /// In-memory pairing token.
        token: String,
        /// Workspace-relative path.
        path: String,
    },
    /// Validate and stage structured file changes.
    #[serde(rename = "patch.propose", alias = "propose_changes")]
    ProposeChanges {
        /// Correlates the response with the browser request.
        request_id: String,
        /// In-memory pairing token.
        token: String,
        /// Proposed file changes.
        changes: Vec<FileChange>,
    },
    /// Ask the terminal runtime to approve and apply a staged proposal.
    #[serde(rename = "patch.apply", alias = "apply_proposal")]
    ApplyProposal {
        /// Correlates the response with the browser request.
        request_id: String,
        /// In-memory pairing token.
        token: String,
        /// Proposal identifier returned by `patch.propose`.
        proposal_id: String,
    },
    /// Run a safe, runtime-approved check command.
    #[serde(rename = "checks.run", alias = "run_checks")]
    RunChecks {
        /// Correlates the response with the browser request.
        request_id: String,
        /// In-memory pairing token.
        token: String,
        /// Command text parsed without invoking a shell.
        command: String,
    },
    /// Revert the most recent applied bridge change.
    #[serde(rename = "patch.revert", alias = "revert_last_change")]
    RevertLastChange {
        /// Correlates the response with the browser request.
        request_id: String,
        /// In-memory pairing token.
        token: String,
        /// Change identity returned by `patch.apply`.
        change_id: String,
    },
    /// Send a prompt to the active VT Code runtime.
    #[serde(rename = "turn.request", alias = "request_turn")]
    RequestTurn {
        /// Correlates the response with the browser request.
        request_id: String,
        /// In-memory pairing token.
        token: String,
        /// Optional server-validated proposal to include in the active turn handoff.
        #[serde(default)]
        proposal_id: Option<String>,
        /// Prompt to submit.
        prompt: String,
    },
    /// Cancel an active request or turn.
    #[serde(rename = "cancel")]
    Cancel {
        /// Correlates the response with the browser request.
        request_id: String,
        /// In-memory pairing token.
        token: String,
        /// Request or turn identifier to cancel.
        target_id: String,
    },
}

impl BridgeRequest {
    /// Returns the request correlation identifier.
    pub fn request_id(&self) -> &str {
        match self {
            Self::ExplanationGet { request_id, .. }
            | Self::ExplanationEvidence { request_id, .. }
            | Self::ExplanationNavigate { request_id, .. } => request_id,
            Self::Pair { request_id, .. }
            | Self::Status { request_id, .. }
            | Self::ListFiles { request_id, .. }
            | Self::ReadFile { request_id, .. }
            | Self::ProposeChanges { request_id, .. }
            | Self::ApplyProposal { request_id, .. }
            | Self::RunChecks { request_id, .. }
            | Self::RevertLastChange { request_id, .. }
            | Self::RequestTurn { request_id, .. }
            | Self::Cancel { request_id, .. } => request_id,
        }
    }

    /// Returns the session token for authenticated requests.
    pub fn token(&self) -> Option<&str> {
        match self {
            Self::ExplanationGet { token, .. }
            | Self::ExplanationEvidence { token, .. }
            | Self::ExplanationNavigate { token, .. } => Some(token),
            Self::Pair { .. } => None,
            Self::Status { token, .. }
            | Self::ListFiles { token, .. }
            | Self::ReadFile { token, .. }
            | Self::ProposeChanges { token, .. }
            | Self::ApplyProposal { token, .. }
            | Self::RunChecks { token, .. }
            | Self::RevertLastChange { token, .. }
            | Self::RequestTurn { token, .. }
            | Self::Cancel { token, .. } => Some(token),
        }
    }
}

/// A JSON response envelope sent to the browser.
#[derive(Debug, Clone, Serialize)]
pub struct BridgeResponse {
    /// Response discriminator.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// Request correlation identifier.
    pub request_id: String,
    /// Whether the operation succeeded.
    pub ok: bool,
    /// Successful operation payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    /// Structured error payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<BridgeErrorPayload>,
}

/// Error information returned without exposing secrets.
#[derive(Debug, Clone, Serialize)]
pub struct BridgeErrorPayload {
    /// Stable error code.
    pub code: &'static str,
    /// User-safe message.
    pub message: String,
}

impl BridgeResponse {
    /// Construct a successful response.
    pub fn success(request_id: impl Into<String>, payload: impl Serialize) -> Self {
        Self {
            kind: "response",
            request_id: request_id.into(),
            ok: true,
            payload: serde_json::to_value(payload).ok(),
            error: None,
        }
    }

    /// Construct a failed response.
    pub fn failure(request_id: impl Into<String>, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind: "response",
            request_id: request_id.into(),
            ok: false,
            payload: None,
            error: Some(BridgeErrorPayload { code, message: message.into() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_are_bounded_and_invalid_ids_are_not_echoed() {
        assert!(!is_valid_request_id(""));
        assert!(is_valid_request_id("browser-1"));
        assert!(is_valid_request_id(&"x".repeat(MAX_REQUEST_ID_BYTES)));
        assert!(!is_valid_request_id(&"x".repeat(MAX_REQUEST_ID_BYTES + 1)));
        assert_eq!(response_request_id(""), "unknown");
        assert_eq!(response_request_id(&"x".repeat(MAX_REQUEST_ID_BYTES + 1)), "unknown");
    }

    #[test]
    fn turn_request_can_reference_a_staged_proposal() {
        let request: BridgeRequest = serde_json::from_value(serde_json::json!({
            "type": "turn.request",
            "request_id": "browser-1",
            "token": "session-token",
            "proposal_id": "proposal-1",
            "prompt": "Implement the staged change"
        }))
        .expect("turn request should deserialize");

        assert!(matches!(
            request,
            BridgeRequest::RequestTurn { proposal_id: Some(proposal_id), prompt, .. }
                if proposal_id == "proposal-1" && prompt == "Implement the staged change"
        ));
    }

    #[test]
    fn bridge_settings_are_non_secret() {
        let settings = BridgeSettings {
            host: "127.0.0.1".to_string(),
            port: 4321,
            pairing_ttl_secs: 300,
            max_frame_bytes: 1_048_576,
            max_in_flight_requests: 8,
            remote_enabled: false,
        };
        let serialized = serde_json::to_string(&settings).expect("settings should serialize");
        assert!(serialized.contains("pairing_ttl_secs"));
        assert!(!serialized.contains("token"));
        assert!(!serialized.contains("code"));
    }
}

/// Pairing response payload.
#[derive(Debug, Clone, Serialize)]
pub struct PairPayload {
    /// In-memory token used on subsequent messages.
    pub token: String,
    /// Protocol version negotiated by the server.
    pub protocol_version: &'static str,
    /// Seconds until the session inactivity lease expires.
    pub expires_in_secs: u64,
}

/// Non-secret bridge settings returned to an authenticated browser.
///
/// The origin is returned separately in [`StatusPayload`] because the server
/// may allow more than one configured origin while each session is bound to
/// exactly one request origin. Pairing codes and session tokens are never
/// included in this structure.
#[derive(Debug, Clone, Serialize)]
pub struct BridgeSettings {
    /// Literal address configured for the listener.
    pub host: String,
    /// Configured listener port; zero means that the operating system chooses one.
    pub port: u16,
    /// Pairing-code lifetime and authenticated-session inactivity lease.
    pub pairing_ttl_secs: u64,
    /// Maximum accepted WebSocket frame size.
    pub max_frame_bytes: usize,
    /// Maximum concurrent bridge operations.
    pub max_in_flight_requests: usize,
    /// Whether the server was configured for a TLS-terminating remote proxy.
    pub remote_enabled: bool,
}

/// Status response payload.
#[derive(Debug, Clone, Serialize)]
pub struct StatusPayload {
    /// Protocol version.
    pub protocol_version: &'static str,
    /// Whether this process has a connected runtime adapter.
    pub connected: bool,
    /// Runtime status.
    pub runtime: crate::runtime::RuntimeStatus,
    /// Origin authenticated for this browser session.
    pub authenticated_origin: String,
    /// Terminal-owned, non-secret bridge configuration.
    pub settings: BridgeSettings,
    /// Latest bridge event sequence.
    pub latest_sequence: u64,
}

/// Event envelope sent after a browser pairs or reconnects.
#[derive(Debug, Clone, Serialize)]
pub struct BridgeEventMessage {
    /// Event discriminator.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// Monotonic bridge sequence.
    pub sequence: u64,
    /// Canonical VT Code runtime event.
    pub event: VersionedThreadEvent,
}
