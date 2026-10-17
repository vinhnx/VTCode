const WEBSOCKET_ACTIVE_RESPONSE_ERROR_PREFIX: &str = "Conversation already has an active response in progress:";
const OPENAI_BETA_RESPONSES_WEBSOCKET_V2: &str = "responses=v2";
const WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE: &str = "websocket_connection_limit_reached";
const PREVIOUS_RESPONSE_NOT_FOUND_CODE: &str = "previous_response_not_found";
const WEBSOCKET_AUTH_RETRY_STATUSES: [&str; 2] = ["401", "403"];

// Retained custom WebSocket transport while VTCode migrates the optional path
// to Rig. Rig 0.40 exposes `client.responses_websocket()` behind its
// `websocket` feature, including warm-up requests, but it does not yet cover
// VTCode's custom auth refresh and retry policy. Keep this path separate from
// normal HTTP/SSE request shaping: it may use `previous_response_id` only as
// connection-local state, and every websocket request still sends `store=false`.
use crate::provider::LLMError;

fn websocket_tls_connector() -> Result<tokio_tungstenite::Connector, LLMError> {
    // Other dependencies enable both Rustls providers. Select one per connection
    // instead of relying on ambiguous process-wide provider inference.
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let root_store = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|err| format_network_error(format!("Invalid OpenAI WebSocket TLS configuration: {err}")))?
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(tokio_tungstenite::Connector::Rustls(std::sync::Arc::new(config)))
}

fn is_websocket_active_response_error(err: &LLMError) -> bool {
    let message = match err {
        LLMError::Provider { message, .. } | LLMError::Network { message, .. } => message,
        LLMError::Authentication { .. } | LLMError::RateLimit { .. } | LLMError::InvalidRequest { .. } => return false,
    };

    message.contains(WEBSOCKET_ACTIVE_RESPONSE_ERROR_PREFIX)
}

fn is_websocket_connection_limit_error(err: &LLMError) -> bool {
    let message = match err {
        LLMError::Provider { message, .. } | LLMError::Network { message, .. } => message,
        LLMError::Authentication { .. } | LLMError::RateLimit { .. } | LLMError::InvalidRequest { .. } => return false,
    };

    message.contains(WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE)
}

fn is_websocket_previous_response_not_found_error(err: &LLMError) -> bool {
    let message = match err {
        LLMError::Provider { message, .. } | LLMError::Network { message, .. } => message,
        LLMError::Authentication { .. } | LLMError::RateLimit { .. } | LLMError::InvalidRequest { .. } => return false,
    };

    message.contains(PREVIOUS_RESPONSE_NOT_FOUND_CODE)
}

fn is_websocket_reconnect_error(err: &LLMError) -> bool {
    matches!(err, LLMError::Network { .. })
}

fn is_websocket_auth_retryable(error: &tokio_tungstenite::tungstenite::Error) -> bool {
    let message = error.to_string();
    WEBSOCKET_AUTH_RETRY_STATUSES.iter().any(|status| message.contains(status))
}

mod events;
mod provider;
mod session;

#[cfg(test)]
use events::apply_generate_mode;
use events::{
    canonical_history, completed_replay_output, format_network_error, format_provider_error, input_is_incremental,
    prepare_websocket_event, read_websocket_event, responses_websocket_url,
};
use session::PreparedWebSocketEvent;
pub(super) use session::{OpenAIResponsesWebSocketContinuationCache, OpenAIResponsesWebSocketSession};
use session::{WebSocketLease, completed_websocket_stream, websocket_deadline_error};

#[cfg(test)]
mod tests;
