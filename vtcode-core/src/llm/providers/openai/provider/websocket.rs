use super::OpenAIProvider;
use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMRequest, LLMResponse};
use futures::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

type ResponsesSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
const OPENAI_BETA_RESPONSES_WEBSOCKET_V2: &str = "responses=v2";
const WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE: &str = "websocket_connection_limit_reached";

pub(super) fn is_websocket_connection_limit_error(err: &LLMError) -> bool {
    let message = match err {
        LLMError::Provider { message, .. } | LLMError::Network { message, .. } => message,
        LLMError::Authentication { .. }
        | LLMError::RateLimit { .. }
        | LLMError::InvalidRequest { .. } => return false,
    };

    message.contains(WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE)
}

#[derive(Debug)]
pub(crate) struct OpenAIResponsesWebSocketSession {
    socket: ResponsesSocket,
    last_response_id: Option<String>,
    last_input: Vec<Value>,
    last_model: Option<String>,
    last_instructions: Option<String>,
    last_tools: Option<Value>,
}

impl OpenAIResponsesWebSocketSession {
    fn new(socket: ResponsesSocket) -> Self {
        Self {
            socket,
            last_response_id: None,
            last_input: Vec::new(),
            last_model: None,
            last_instructions: None,
            last_tools: None,
        }
    }

    fn can_continue_from(&self, payload: &Value, allow_empty_delta: bool) -> bool {
        let Some(previous_response_id) = self.last_response_id.as_ref() else {
            return false;
        };
        if previous_response_id.is_empty() {
            return false;
        }

        let Some(current_model) = payload.get("model").and_then(Value::as_str) else {
            return false;
        };
        if self.last_model.as_deref() != Some(current_model) {
            return false;
        }

        let current_instructions = payload
            .get("instructions")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if self.last_instructions != current_instructions {
            return false;
        }

        let current_tools = payload.get("tools").cloned();
        if self.last_tools != current_tools {
            return false;
        }

        let Some(current_input) = payload.get("input").and_then(Value::as_array) else {
            return false;
        };
        input_is_incremental(
            self.last_input.as_slice(),
            current_input.as_slice(),
            allow_empty_delta,
        )
    }

    fn clear_chain(&mut self) {
        self.last_response_id = None;
        self.last_input.clear();
        self.last_model = None;
        self.last_instructions = None;
        self.last_tools = None;
    }
}

#[derive(Debug)]
struct PreparedWebSocketEvent {
    event: Value,
    full_input: Vec<Value>,
    used_previous_response_id: bool,
}

impl OpenAIProvider {
    pub(super) async fn generate_via_responses_websocket(
        &self,
        request: &LLMRequest,
    ) -> Result<LLMResponse, LLMError> {
        let allow_empty_delta = true;
        let mut session_guard = self.websocket_session.lock().await;
        let session = self
            .ensure_websocket_session(&mut session_guard, request)
            .await?;

        let payload = self.convert_to_openai_responses_format(request)?;
        let needs_warmup = session
            .last_response_id
            .as_deref()
            .map(str::is_empty)
            .unwrap_or(true);
        if needs_warmup {
            let warmup_prepared = prepare_websocket_event(payload.clone(), session, true, false)?;
            match send_websocket_event(session, &warmup_prepared.event).await {
                Ok(response_json) => {
                    update_session_from_response(
                        session,
                        &response_json,
                        request,
                        &warmup_prepared,
                        &self.model,
                    );
                }
                Err(err) => {
                    session.clear_chain();
                    *session_guard = None;
                    return Err(err);
                }
            }
        }

        let prepared = prepare_websocket_event(payload, session, false, allow_empty_delta)?;
        let sent_with_previous = prepared.used_previous_response_id;

        match send_websocket_event(session, &prepared.event).await {
            Ok(response_json) => {
                let parsed = self.parse_openai_responses_response(
                    response_json.clone(),
                    request.model.clone(),
                )?;
                update_session_from_response(
                    session,
                    &response_json,
                    request,
                    &prepared,
                    &self.model,
                );
                Ok(parsed)
            }
            Err(err) => {
                if sent_with_previous {
                    session.clear_chain();
                }
                *session_guard = None;
                Err(err)
            }
        }
    }

    async fn ensure_websocket_session<'a>(
        &self,
        session_guard: &'a mut Option<OpenAIResponsesWebSocketSession>,
        request: &LLMRequest,
    ) -> Result<&'a mut OpenAIResponsesWebSocketSession, LLMError> {
        if session_guard.is_none() {
            let ws_url = responses_websocket_url(&self.base_url)?;
            let mut ws_request = ws_url.into_client_request().map_err(|err| {
                format_provider_error(format!("Invalid OpenAI WebSocket request: {err}"))
            })?;

            ws_request.headers_mut().insert(
                "Authorization",
                HeaderValue::from_str(&format!("Bearer {}", self.api_key)).map_err(|err| {
                    format_provider_error(format!("Invalid OpenAI authorization header: {err}"))
                })?,
            );
            ws_request.headers_mut().insert(
                "OpenAI-Beta",
                HeaderValue::from_static(OPENAI_BETA_RESPONSES_WEBSOCKET_V2),
            );
            if let Some(metadata) = &request.metadata
                && let Ok(metadata_str) = serde_json::to_string(metadata)
                && let Ok(value) = HeaderValue::from_str(&metadata_str)
            {
                ws_request.headers_mut().insert("X-Turn-Metadata", value);
            }

            let (socket, _) = connect_async(ws_request).await.map_err(|err| {
                format_network_error(format!("Failed to connect OpenAI WebSocket: {err}"))
            })?;
            *session_guard = Some(OpenAIResponsesWebSocketSession::new(socket));
        }

        session_guard.as_mut().ok_or_else(|| {
            format_provider_error("OpenAI WebSocket session unexpectedly missing".to_string())
        })
    }
}

fn update_session_from_response(
    session: &mut OpenAIResponsesWebSocketSession,
    response_json: &Value,
    request: &LLMRequest,
    prepared: &PreparedWebSocketEvent,
    fallback_model: &str,
) {
    session.last_response_id = response_json
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    session.last_input = prepared.full_input.clone();
    session.last_model = request
        .model
        .trim()
        .is_empty()
        .then(|| fallback_model.to_string())
        .or_else(|| Some(request.model.clone()));
    session.last_instructions = response_json
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            prepared
                .event
                .get("instructions")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    session.last_tools = prepared.event.get("tools").cloned();
}

fn input_is_incremental(
    last_input: &[Value],
    current_input: &[Value],
    allow_empty_delta: bool,
) -> bool {
    if current_input.len() < last_input.len() {
        return false;
    }
    if !allow_empty_delta && current_input.len() == last_input.len() {
        return false;
    }
    current_input.starts_with(last_input)
}

fn apply_generate_mode(request_obj: &mut Map<String, Value>, warmup: bool) {
    if warmup {
        request_obj.insert("generate".to_string(), Value::Bool(false));
    } else {
        request_obj.remove("generate");
    }
}

fn prepare_websocket_event(
    payload: Value,
    session: &OpenAIResponsesWebSocketSession,
    warmup: bool,
    allow_empty_delta: bool,
) -> Result<PreparedWebSocketEvent, LLMError> {
    let mut request_obj = payload
        .as_object()
        .cloned()
        .ok_or_else(|| format_provider_error("Invalid Responses payload".to_string()))?;

    request_obj.remove("stream");
    request_obj.remove("background");
    request_obj
        .entry("store".to_string())
        .or_insert(Value::Bool(false));
    apply_generate_mode(&mut request_obj, warmup);

    let full_input = request_obj
        .get("input")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| format_provider_error("Responses payload missing input".to_string()))?;

    let mut used_previous_response_id = false;
    if session.can_continue_from(&Value::Object(request_obj.clone()), allow_empty_delta) {
        if let Some(previous_response_id) = session.last_response_id.clone() {
            request_obj.insert(
                "previous_response_id".to_string(),
                Value::String(previous_response_id),
            );
            let incremental = full_input[session.last_input.len()..].to_vec();
            request_obj.insert("input".to_string(), Value::Array(incremental));
            used_previous_response_id = true;
        }
    } else {
        request_obj.remove("previous_response_id");
    }

    let event = Value::Object(
        std::iter::once((
            "type".to_string(),
            Value::String("response.create".to_string()),
        ))
        .chain(request_obj)
        .collect(),
    );

    Ok(PreparedWebSocketEvent {
        event,
        full_input,
        used_previous_response_id,
    })
}

async fn send_websocket_event(
    session: &mut OpenAIResponsesWebSocketSession,
    event: &Value,
) -> Result<Value, LLMError> {
    session
        .socket
        .send(Message::Text(event.to_string().into()))
        .await
        .map_err(|err| {
            format_network_error(format!("Failed to send OpenAI WebSocket payload: {err}"))
        })?;
    read_websocket_response(session).await
}

async fn read_websocket_response(
    session: &mut OpenAIResponsesWebSocketSession,
) -> Result<Value, LLMError> {
    while let Some(message) = session.socket.next().await {
        let message = message.map_err(|err| {
            format_network_error(format!("OpenAI WebSocket receive failed: {err}"))
        })?;

        match message {
            Message::Text(text) => {
                let event: Value = serde_json::from_str(text.as_ref()).map_err(|err| {
                    format_provider_error(format!("Invalid OpenAI WebSocket event JSON: {err}"))
                })?;

                let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
                match event_type {
                    "response.completed" => {
                        if let Some(response) = event.get("response").cloned() {
                            return Ok(response);
                        }
                        return Err(format_provider_error(
                            "OpenAI WebSocket completed event missing response".to_string(),
                        ));
                    }
                    "response.failed" | "error" => {
                        let code = event
                            .get("error")
                            .and_then(|error| error.get("code"))
                            .and_then(Value::as_str)
                            .unwrap_or("unknown_error");
                        let message = event
                            .get("error")
                            .and_then(|error| error.get("message"))
                            .and_then(Value::as_str)
                            .unwrap_or("OpenAI WebSocket request failed");
                        let formatted = format!("{code}: {message}");
                        if code == WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE {
                            return Err(format_network_error(formatted));
                        }
                        return Err(format_provider_error(formatted));
                    }
                    _ => {}
                }
            }
            Message::Ping(payload) => {
                session
                    .socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|err| {
                        format_network_error(format!(
                            "Failed to reply to OpenAI WebSocket ping: {err}"
                        ))
                    })?;
            }
            Message::Close(frame) => {
                let reason = frame
                    .map(|frame| frame.reason.to_string())
                    .unwrap_or_else(|| "connection closed".to_string());
                return Err(format_network_error(format!(
                    "OpenAI WebSocket connection closed: {reason}"
                )));
            }
            _ => {}
        }
    }

    Err(format_network_error(
        "OpenAI WebSocket stream ended unexpectedly".to_string(),
    ))
}

fn responses_websocket_url(base_url: &str) -> Result<String, LLMError> {
    let mut url = url::Url::parse(base_url).map_err(|err| {
        format_provider_error(format!("Invalid OpenAI base URL for WebSocket mode: {err}"))
    })?;

    match url.scheme() {
        "https" => {
            let _ = url.set_scheme("wss");
        }
        "http" => {
            let _ = url.set_scheme("ws");
        }
        "wss" | "ws" => {}
        other => {
            return Err(format_provider_error(format!(
                "Unsupported URL scheme for WebSocket mode: {other}"
            )));
        }
    }

    if !url.path().ends_with("/responses") {
        let mut path = url.path().trim_end_matches('/').to_string();
        if path.is_empty() {
            path.push('/');
        }
        path.push_str("/responses");
        url.set_path(&path);
    }

    Ok(url.to_string())
}

fn format_provider_error(message: String) -> LLMError {
    LLMError::Provider {
        message: error_display::format_llm_error("OpenAI", &message),
        metadata: None,
    }
}

fn format_network_error(message: String) -> LLMError {
    LLMError::Network {
        message: error_display::format_llm_error("OpenAI", &message),
        metadata: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        OPENAI_BETA_RESPONSES_WEBSOCKET_V2, WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE,
        apply_generate_mode, input_is_incremental, is_websocket_connection_limit_error,
        responses_websocket_url,
    };
    use crate::llm::provider::LLMError;
    use serde_json::{Map, Value};

    #[test]
    fn websocket_url_is_derived_from_http_base() {
        let ws = responses_websocket_url("https://api.openai.com/v1")
            .expect("websocket url should be built");
        assert_eq!(ws, "wss://api.openai.com/v1/responses");
    }

    #[test]
    fn websocket_beta_header_prefers_v2_protocol() {
        assert_eq!(OPENAI_BETA_RESPONSES_WEBSOCKET_V2, "responses=v2");
    }

    #[test]
    fn websocket_connection_limit_error_is_detected() {
        let err = LLMError::Network {
            message: format!(
                "OpenAI error: {WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE}: limit reached"
            ),
            metadata: None,
        };
        assert!(is_websocket_connection_limit_error(&err));
    }

    #[test]
    fn websocket_non_connection_limit_error_is_not_detected() {
        let err = LLMError::Provider {
            message: "OpenAI error: invalid_request".to_string(),
            metadata: None,
        };
        assert!(!is_websocket_connection_limit_error(&err));
    }

    #[test]
    fn websocket_incremental_input_disallows_empty_delta_by_default() {
        let input = vec![Value::String("a".to_string())];
        assert!(!input_is_incremental(&input, &input, false));
    }

    #[test]
    fn websocket_incremental_input_allows_empty_delta_when_enabled() {
        let input = vec![Value::String("a".to_string())];
        assert!(input_is_incremental(&input, &input, true));
    }

    #[test]
    fn websocket_incremental_input_requires_prefix_match() {
        let previous = vec![Value::String("a".to_string())];
        let current = vec![Value::String("b".to_string())];
        assert!(!input_is_incremental(&previous, &current, true));
    }

    #[test]
    fn websocket_warmup_sets_generate_false() {
        let mut obj = Map::new();
        apply_generate_mode(&mut obj, true);
        assert_eq!(obj.get("generate"), Some(&Value::Bool(false)));
    }

    #[test]
    fn websocket_non_warmup_removes_generate_flag() {
        let mut obj = Map::new();
        obj.insert("generate".to_string(), Value::Bool(false));
        apply_generate_mode(&mut obj, false);
        assert!(obj.get("generate").is_none());
    }
}
