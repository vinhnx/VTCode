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
const PREVIOUS_RESPONSE_NOT_FOUND_CODE: &str = "previous_response_not_found";
const WEBSOCKET_AUTH_RETRY_STATUSES: [&str; 2] = ["401", "403"];

#[cfg(test)]
pub(super) fn is_websocket_connection_limit_error(err: &LLMError) -> bool {
    let message = match err {
        LLMError::Provider { message, .. } | LLMError::Network { message, .. } => message,
        LLMError::Authentication { .. }
        | LLMError::RateLimit { .. }
        | LLMError::InvalidRequest { .. } => return false,
    };

    message.contains(WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE)
}

pub(super) fn is_websocket_previous_response_not_found_error(err: &LLMError) -> bool {
    let message = match err {
        LLMError::Provider { message, .. } | LLMError::Network { message, .. } => message,
        LLMError::Authentication { .. }
        | LLMError::RateLimit { .. }
        | LLMError::InvalidRequest { .. } => return false,
    };

    message.contains(PREVIOUS_RESPONSE_NOT_FOUND_CODE)
}

pub(super) fn is_websocket_reconnect_error(err: &LLMError) -> bool {
    matches!(err, LLMError::Network { .. })
}

fn is_websocket_auth_retryable(error: &tokio_tungstenite::tungstenite::Error) -> bool {
    let message = error.to_string();
    WEBSOCKET_AUTH_RETRY_STATUSES
        .iter()
        .any(|status| message.contains(status))
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OpenAIResponsesWebSocketContinuationCache {
    response_id: String,
    full_input: Vec<Value>,
    model: String,
    instructions: Option<String>,
    tools: Option<Value>,
}

impl OpenAIResponsesWebSocketContinuationCache {
    fn from_response(
        response_json: &Value,
        request: &LLMRequest,
        prepared: &PreparedWebSocketEvent,
        fallback_model: &str,
    ) -> Option<Self> {
        let response_id = response_json
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?
            .to_string();

        let model = if request.model.trim().is_empty() {
            fallback_model.to_string()
        } else {
            request.model.clone()
        };
        let instructions = response_json
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
        let tools = prepared.event.get("tools").cloned();

        Some(Self {
            response_id,
            full_input: prepared.full_input.clone(),
            model,
            instructions,
            tools,
        })
    }

    fn can_continue_from(&self, payload: &Value) -> bool {
        if self.response_id.is_empty() {
            return false;
        }

        let Some(current_model) = payload.get("model").and_then(Value::as_str) else {
            return false;
        };
        if self.model != current_model {
            return false;
        }

        let current_instructions = payload
            .get("instructions")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if self.instructions != current_instructions {
            return false;
        }

        let current_tools = payload.get("tools").cloned();
        if self.tools != current_tools {
            return false;
        }

        let Some(current_input) = payload.get("input").and_then(Value::as_array) else {
            return false;
        };
        input_is_incremental(self.full_input.as_slice(), current_input.as_slice())
    }
}

#[derive(Debug)]
pub(crate) struct OpenAIResponsesWebSocketSession {
    socket: ResponsesSocket,
}

impl OpenAIResponsesWebSocketSession {
    fn new(socket: ResponsesSocket) -> Self {
        Self { socket }
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
        let payload = self.convert_to_openai_responses_format(request)?;
        let mut retried_reconnect = false;
        let mut retried_new_chain = false;

        loop {
            let needs_warmup = self.websocket_continuation_snapshot().is_none();
            let mut session_guard = self.websocket_session.lock().await;
            let session = self
                .ensure_websocket_session(&mut session_guard, request)
                .await?;

            if needs_warmup {
                let warmup_prepared = prepare_websocket_event(payload.clone(), None, true)?;
                match send_websocket_event(session, &warmup_prepared.event).await {
                    Ok(response_json) => {
                        self.update_websocket_continuation(
                            &response_json,
                            request,
                            &warmup_prepared,
                            &self.model,
                        );
                    }
                    Err(err) => {
                        *session_guard = None;
                        if !retried_reconnect && is_websocket_reconnect_error(&err) {
                            retried_reconnect = true;
                            continue;
                        }
                        return Err(err);
                    }
                }
            }

            let continuation = self.websocket_continuation_snapshot();
            let prepared = prepare_websocket_event(payload.clone(), continuation.as_ref(), false)?;

            match send_websocket_event(session, &prepared.event).await {
                Ok(response_json) => {
                    let parsed = self.parse_openai_responses_response(
                        response_json.clone(),
                        request.model.clone(),
                    )?;
                    self.update_websocket_continuation(
                        &response_json,
                        request,
                        &prepared,
                        &self.model,
                    );
                    return Ok(parsed);
                }
                Err(err) => {
                    *session_guard = None;
                    if !retried_new_chain
                        && prepared.used_previous_response_id
                        && is_websocket_previous_response_not_found_error(&err)
                    {
                        self.clear_websocket_continuation();
                        retried_new_chain = true;
                        continue;
                    }
                    if !retried_reconnect && is_websocket_reconnect_error(&err) {
                        retried_reconnect = true;
                        continue;
                    }
                    return Err(err);
                }
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
            let build_request = |api_key: &str| -> Result<_, LLMError> {
                let mut ws_request = ws_url.clone().into_client_request().map_err(|err| {
                    format_provider_error(format!("Invalid OpenAI WebSocket request: {err}"))
                })?;

                ws_request.headers_mut().insert(
                    "Authorization",
                    HeaderValue::from_str(&format!("Bearer {}", api_key)).map_err(|err| {
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

                Ok(ws_request)
            };

            let api_key = self.current_api_key().await?;
            let ws_request = build_request(&api_key)?;
            let socket = match connect_async(ws_request).await {
                Ok((socket, _)) => socket,
                Err(err) if self.uses_chatgpt_auth() && is_websocket_auth_retryable(&err) => {
                    let retry_api_key = self.refresh_api_key_for_retry().await?;
                    let retry_request = build_request(&retry_api_key)?;
                    let (socket, _) = connect_async(retry_request).await.map_err(|retry_err| {
                        format_network_error(format!(
                            "Failed to connect OpenAI WebSocket: {retry_err}"
                        ))
                    })?;
                    socket
                }
                Err(err) => {
                    return Err(format_network_error(format!(
                        "Failed to connect OpenAI WebSocket: {err}"
                    )));
                }
            };
            *session_guard = Some(OpenAIResponsesWebSocketSession::new(socket));
        }

        session_guard.as_mut().ok_or_else(|| {
            format_provider_error("OpenAI WebSocket session unexpectedly missing".to_string())
        })
    }

    fn websocket_continuation_snapshot(&self) -> Option<OpenAIResponsesWebSocketContinuationCache> {
        self.websocket_continuation_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn update_websocket_continuation(
        &self,
        response_json: &Value,
        request: &LLMRequest,
        prepared: &PreparedWebSocketEvent,
        fallback_model: &str,
    ) {
        let cache = OpenAIResponsesWebSocketContinuationCache::from_response(
            response_json,
            request,
            prepared,
            fallback_model,
        );
        *self
            .websocket_continuation_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = cache;
    }

    fn clear_websocket_continuation(&self) {
        *self
            .websocket_continuation_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

fn input_is_incremental(last_input: &[Value], current_input: &[Value]) -> bool {
    if current_input.len() < last_input.len() {
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
    continuation: Option<&OpenAIResponsesWebSocketContinuationCache>,
    warmup: bool,
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
    if !warmup
        && let Some(continuation) = continuation
        && continuation.can_continue_from(&Value::Object(request_obj.clone()))
    {
        if !continuation.response_id.is_empty() {
            request_obj.insert(
                "previous_response_id".to_string(),
                Value::String(continuation.response_id.clone()),
            );
            let incremental = full_input[continuation.full_input.len()..].to_vec();
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
        OPENAI_BETA_RESPONSES_WEBSOCKET_V2, OpenAIResponsesWebSocketContinuationCache,
        PREVIOUS_RESPONSE_NOT_FOUND_CODE, WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE,
        apply_generate_mode, input_is_incremental, is_websocket_connection_limit_error,
        is_websocket_previous_response_not_found_error, prepare_websocket_event,
        responses_websocket_url,
    };
    use crate::config::core::OpenAIConfig;
    use crate::llm::provider::LLMError;
    use crate::llm::provider::{LLMProvider, LLMRequest, Message as ProviderMessage};
    use crate::llm::providers::openai::OpenAIProvider;
    use futures::{SinkExt, StreamExt};
    use serde_json::{Map, Value, json};
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use tokio_tungstenite::accept_async;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::protocol::CloseFrame;
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

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
    fn websocket_previous_response_not_found_error_is_detected() {
        let err = LLMError::Provider {
            message: format!(
                "OpenAI error: {PREVIOUS_RESPONSE_NOT_FOUND_CODE}: previous response missing"
            ),
            metadata: None,
        };
        assert!(is_websocket_previous_response_not_found_error(&err));
    }

    #[test]
    fn websocket_incremental_input_allows_empty_delta_for_v2_chaining() {
        let input = vec![Value::String("a".to_string())];
        assert!(input_is_incremental(&input, &input));
    }

    #[test]
    fn websocket_incremental_input_requires_prefix_match() {
        let previous = vec![Value::String("a".to_string())];
        let current = vec![Value::String("b".to_string())];
        assert!(!input_is_incremental(&previous, &current));
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

    #[test]
    fn websocket_prepare_event_starts_new_chain_without_continuation() {
        let payload = json!({
            "model": "gpt-5.2",
            "input": [{"role": "user", "content": "hello"}],
            "previous_response_id": "resp_prev",
            "stream": false,
            "background": false,
        });

        let prepared = prepare_websocket_event(payload, None, false).expect("event should prepare");

        assert_eq!(
            prepared.event.get("previous_response_id"),
            None,
            "fresh websocket chain should not reuse caller-supplied response ids"
        );
        assert_eq!(
            prepared.event.get("input").and_then(Value::as_array),
            Some(&prepared.full_input)
        );
        assert!(!prepared.used_previous_response_id);
    }

    #[test]
    fn websocket_prepare_event_reuses_matching_continuation_incrementally() {
        let full_input = vec![
            json!({"role": "user", "content": "hello"}),
            json!({"role": "user", "content": "continue"}),
        ];
        let continuation = OpenAIResponsesWebSocketContinuationCache {
            response_id: "resp_prev".to_string(),
            full_input: vec![full_input[0].clone()],
            model: "gpt-5.2".to_string(),
            instructions: None,
            tools: None,
        };
        let payload = json!({
            "model": "gpt-5.2",
            "input": full_input,
        });

        let prepared = prepare_websocket_event(payload, Some(&continuation), false).expect("event");

        assert_eq!(
            prepared
                .event
                .get("previous_response_id")
                .and_then(Value::as_str),
            Some("resp_prev")
        );
        assert_eq!(
            prepared.event.get("input").and_then(Value::as_array),
            Some(&vec![json!({"role": "user", "content": "continue"})])
        );
        assert!(prepared.used_previous_response_id);
    }

    #[test]
    fn websocket_prepare_event_starts_new_chain_on_non_prefix_input() {
        let continuation = OpenAIResponsesWebSocketContinuationCache {
            response_id: "resp_prev".to_string(),
            full_input: vec![json!({"role": "user", "content": "hello"})],
            model: "gpt-5.2".to_string(),
            instructions: None,
            tools: None,
        };
        let payload = json!({
            "model": "gpt-5.2",
            "input": [json!({"role": "user", "content": "different"})],
            "previous_response_id": "resp_prev",
        });

        let prepared = prepare_websocket_event(payload, Some(&continuation), false).expect("event");

        assert!(prepared.event.get("previous_response_id").is_none());
        assert_eq!(
            prepared.event.get("input").and_then(Value::as_array),
            Some(&prepared.full_input)
        );
        assert!(!prepared.used_previous_response_id);
    }

    #[derive(Clone)]
    enum ScriptedReply {
        Completed {
            response_id: &'static str,
            text: &'static str,
        },
        Error {
            code: &'static str,
            message: &'static str,
        },
        Close {
            reason: &'static str,
        },
    }

    async fn spawn_scripted_websocket_server(
        sessions: Vec<Vec<ScriptedReply>>,
    ) -> (
        String,
        Arc<StdMutex<Vec<Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("listener addr");
        let recorded = Arc::new(StdMutex::new(Vec::new()));
        let recorded_handle = Arc::clone(&recorded);
        let (ready_tx, ready_rx) = oneshot::channel();

        let handle = tokio::spawn(async move {
            let _ = ready_tx.send(());
            for session_script in sessions {
                let (stream, _) = listener.accept().await.expect("accept");
                let mut websocket = accept_async(stream).await.expect("handshake");
                for reply in session_script {
                    let message = websocket
                        .next()
                        .await
                        .expect("request should be present")
                        .expect("request should parse");
                    let Message::Text(text) = message else {
                        panic!("expected text websocket payload");
                    };
                    let payload: Value =
                        serde_json::from_str(text.as_ref()).expect("payload should be valid json");
                    recorded_handle.lock().expect("recorded lock").push(payload);

                    match reply {
                        ScriptedReply::Completed { response_id, text } => {
                            let event = json!({
                                "type": "response.completed",
                                "response": {
                                    "id": response_id,
                                    "output": [{
                                        "type": "message",
                                        "content": [{
                                            "type": "output_text",
                                            "text": text,
                                        }]
                                    }]
                                }
                            });
                            websocket
                                .send(Message::Text(event.to_string().into()))
                                .await
                                .expect("response event");
                        }
                        ScriptedReply::Error { code, message } => {
                            let event = json!({
                                "type": "error",
                                "status": 400,
                                "error": {
                                    "code": code,
                                    "message": message,
                                }
                            });
                            websocket
                                .send(Message::Text(event.to_string().into()))
                                .await
                                .expect("error event");
                            break;
                        }
                        ScriptedReply::Close { reason } => {
                            websocket
                                .send(Message::Close(Some(CloseFrame {
                                    code: CloseCode::Away,
                                    reason: reason.to_string().into(),
                                })))
                                .await
                                .expect("close frame");
                            break;
                        }
                    }
                }
            }
        });

        ready_rx.await.expect("server ready");
        (
            format!("http://api.openai.com@127.0.0.1:{}/v1", addr.port()),
            recorded,
            handle,
        )
    }

    fn websocket_test_provider(base_url: String) -> OpenAIProvider {
        OpenAIProvider::from_config(
            Some("test-key".to_string()),
            None,
            Some("gpt-5.2".to_string()),
            Some(base_url),
            None,
            None,
            None,
            Some(OpenAIConfig {
                websocket_mode: true,
                ..Default::default()
            }),
            None,
        )
    }

    fn websocket_test_request() -> LLMRequest {
        LLMRequest {
            model: "gpt-5.2".to_string(),
            messages: vec![ProviderMessage::user("hello".to_string())],
            ..Default::default()
        }
    }

    fn seed_continuation_cache(provider: &OpenAIProvider, request: &LLMRequest, response_id: &str) {
        let payload = provider
            .convert_to_openai_responses_format(request)
            .expect("payload should serialize");
        let full_input = payload
            .get("input")
            .and_then(Value::as_array)
            .cloned()
            .expect("full input");

        provider.update_websocket_continuation(
            &json!({ "id": response_id }),
            request,
            &super::PreparedWebSocketEvent {
                event: payload,
                full_input,
                used_previous_response_id: false,
            },
            "gpt-5.2",
        );
    }

    #[tokio::test]
    async fn websocket_reconnects_after_connection_limit_error() {
        let (base_url, recorded, handle) = spawn_scripted_websocket_server(vec![
            vec![ScriptedReply::Error {
                code: WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE,
                message: "Responses websocket connection limit reached (60 minutes).",
            }],
            vec![ScriptedReply::Completed {
                response_id: "resp_reconnected",
                text: "ok",
            }],
        ])
        .await;
        let provider = websocket_test_provider(base_url);
        let request = websocket_test_request();
        seed_continuation_cache(&provider, &request, "resp_cached");

        let response = LLMProvider::generate(&provider, request)
            .await
            .expect("websocket retry should succeed");

        assert_eq!(response.content.as_deref(), Some("ok"));
        {
            let recorded = recorded.lock().expect("recorded lock");
            assert_eq!(recorded.len(), 2);
            assert_eq!(
                recorded[0]
                    .get("previous_response_id")
                    .and_then(Value::as_str),
                Some("resp_cached")
            );
            assert_eq!(
                recorded[1]
                    .get("previous_response_id")
                    .and_then(Value::as_str),
                Some("resp_cached")
            );
        }
        handle.await.expect("server task");
    }

    #[tokio::test]
    async fn websocket_previous_response_not_found_restarts_new_chain() {
        let (base_url, recorded, handle) = spawn_scripted_websocket_server(vec![
            vec![ScriptedReply::Error {
                code: PREVIOUS_RESPONSE_NOT_FOUND_CODE,
                message: "Previous response with id 'resp_cached' not found.",
            }],
            vec![
                ScriptedReply::Completed {
                    response_id: "resp_warmup",
                    text: "",
                },
                ScriptedReply::Completed {
                    response_id: "resp_final",
                    text: "new chain ok",
                },
            ],
        ])
        .await;
        let provider = websocket_test_provider(base_url);
        let request = websocket_test_request();
        seed_continuation_cache(&provider, &request, "resp_cached");

        let response = LLMProvider::generate(&provider, request)
            .await
            .expect("provider should recover by starting a new chain");

        assert_eq!(response.content.as_deref(), Some("new chain ok"));
        {
            let recorded = recorded.lock().expect("recorded lock");
            assert_eq!(recorded.len(), 3);
            assert_eq!(
                recorded[0]
                    .get("previous_response_id")
                    .and_then(Value::as_str),
                Some("resp_cached")
            );
            assert!(recorded[1].get("previous_response_id").is_none());
            assert_eq!(
                recorded[1].get("generate").and_then(Value::as_bool),
                Some(false)
            );
            assert_eq!(
                recorded[2]
                    .get("previous_response_id")
                    .and_then(Value::as_str),
                Some("resp_warmup")
            );
        }
        handle.await.expect("server task");
    }

    #[tokio::test]
    async fn websocket_reconnects_after_close_frame() {
        let (base_url, recorded, handle) = spawn_scripted_websocket_server(vec![
            vec![ScriptedReply::Close {
                reason: "limit reached",
            }],
            vec![ScriptedReply::Completed {
                response_id: "resp_after_close",
                text: "closed then ok",
            }],
        ])
        .await;
        let provider = websocket_test_provider(base_url);
        let request = websocket_test_request();
        seed_continuation_cache(&provider, &request, "resp_cached");

        let response = LLMProvider::generate(&provider, request)
            .await
            .expect("close-frame retry should succeed");

        assert_eq!(response.content.as_deref(), Some("closed then ok"));
        {
            let recorded = recorded.lock().expect("recorded lock");
            assert_eq!(recorded.len(), 2);
            assert_eq!(
                recorded[1]
                    .get("previous_response_id")
                    .and_then(Value::as_str),
                Some("resp_cached")
            );
        }
        handle.await.expect("server task");
    }
}
