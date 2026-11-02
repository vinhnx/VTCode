pub(super) use super::{
    OPENAI_BETA_RESPONSES_WEBSOCKET_V2, OpenAIResponsesWebSocketContinuationCache, PREVIOUS_RESPONSE_NOT_FOUND_CODE,
    WEBSOCKET_ACTIVE_RESPONSE_ERROR_PREFIX, WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE, apply_generate_mode,
    input_is_incremental, is_websocket_active_response_error, is_websocket_connection_limit_error,
    is_websocket_previous_response_not_found_error, prepare_websocket_event, responses_websocket_url,
};
pub(super) use crate::provider::LLMError;
pub(super) use crate::provider::{LLMProvider, LLMRequest, Message as ProviderMessage};
pub(super) use crate::providers::openai::OpenAIProvider;
pub(super) use futures::{SinkExt, StreamExt};
pub(super) use serde_json::{Map, Value, json};
pub(super) use std::sync::{Arc, Mutex as StdMutex};
pub(super) use tokio::net::TcpListener;
pub(super) use tokio::sync::oneshot;
pub(super) use tokio_tungstenite::accept_async;
pub(super) use tokio_tungstenite::tungstenite::Message;
pub(super) use tokio_tungstenite::tungstenite::protocol::CloseFrame;
pub(super) use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
pub(super) use vtcode_config::core::OpenAIConfig;

#[derive(Clone)]
enum ScriptedReply {
    Events(Vec<Value>),
    Frames(Vec<Message>),
    Paused {
        events: Vec<Value>,
        gate: Arc<tokio::sync::Notify>,
        completion: Value,
    },
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
) -> Option<(String, Arc<StdMutex<Vec<Value>>>, tokio::task::JoinHandle<()>)> {
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => return None,
        Err(err) => panic!("listener should bind: {err}"),
    };
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
                let payload: Value = serde_json::from_str(text.as_ref()).expect("payload should be valid json");
                recorded_handle.lock().expect("recorded lock").push(payload);

                match reply {
                    ScriptedReply::Events(events) => {
                        for event in events {
                            websocket.send(Message::Text(event.to_string().into())).await.expect("event");
                        }
                    }
                    ScriptedReply::Frames(frames) => {
                        for frame in frames {
                            websocket.send(frame).await.expect("frame");
                        }
                    }
                    ScriptedReply::Paused { events, gate, completion } => {
                        for event in events {
                            websocket.send(Message::Text(event.to_string().into())).await.expect("event");
                        }
                        gate.notified().await;
                        let _ = websocket.send(Message::Text(completion.to_string().into())).await;
                    }
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
    Some((format!("http://api.openai.com@127.0.0.1:{}/v1", addr.port()), recorded, handle))
}

fn websocket_test_provider(base_url: String) -> OpenAIProvider {
    OpenAIProvider::from_config(
        Some("test-key".to_string()),
        None,
        Some("gpt-5.6".to_string()),
        Some(base_url),
        None,
        None,
        None,
        Some(OpenAIConfig { websocket_mode: true, ..Default::default() }),
        None,
    )
}

fn websocket_test_custom_provider(base_url: String) -> OpenAIProvider {
    OpenAIProvider::from_custom_config(
        "mycorp".to_string(),
        "MyCorp".to_string(),
        Some("test-key".to_string()),
        Some("gpt-5.6".to_string()),
        Some(base_url),
        None,
        None,
        Some(OpenAIConfig { websocket_mode: true, ..Default::default() }),
        None,
        None,
        None,
    )
}

fn websocket_test_request() -> LLMRequest {
    LLMRequest {
        model: "gpt-5.6".to_string(),
        messages: vec![ProviderMessage::user("hello".to_string())].into(),
        ..Default::default()
    }
}

fn seed_continuation_cache(provider: &OpenAIProvider, request: &LLMRequest, response_id: &str, store: bool) {
    let payload = provider
        .convert_to_openai_responses_format(request)
        .expect("payload should serialize");
    let full_input = payload.get("input").and_then(Value::as_array).cloned().expect("full input");
    let mut event = payload;
    event["store"] = Value::Bool(store);
    event["generate"] = Value::Bool(false);

    provider.update_websocket_continuation(
        &json!({ "id": response_id }),
        request,
        &super::PreparedWebSocketEvent {
            event,
            full_input,
            used_previous_response_id: false,
        },
        "gpt-5.6",
    );
}

async fn seed_connected_continuation_cache(
    provider: &OpenAIProvider,
    request: &LLMRequest,
    response_id: &str,
    store: bool,
) {
    let mut slot = provider.websocket_session.lock().await;
    provider
        .ensure_websocket_session(&mut slot, request)
        .await
        .expect("seed connection");
    seed_continuation_cache(provider, request, response_id, store);
}

fn completion(id: &str, output: Value) -> Value {
    json!({"type":"response.completed", "response":{
        "id":id, "status":"completed", "output":output,
        "usage":{"input_tokens":17,"output_tokens":9,"total_tokens":26,"input_tokens_details":{"cached_tokens":4}}
    }})
}

fn text_delta(text: &str) -> Value {
    json!({"type":"response.output_text.delta", "delta":text, "item_id":"msg_1", "output_index":0, "content_index":0, "sequence_number":1})
}

fn text_output(text: &str) -> Value {
    json!([{"id":"msg_1", "type":"message", "role":"assistant", "status":"completed", "content":[{"type":"output_text", "text":text,"annotations":[]}]}])
}

async fn final_normalized(mut stream: crate::provider::LLMNormalizedStream) -> crate::provider::LLMResponse {
    while let Some(event) = stream.next().await {
        if let crate::provider::NormalizedStreamEvent::Done { response } = event.expect("stream event") {
            return *response;
        }
    }
    panic!("missing completion");
}

async fn http_stream_server(expected: u64) -> wiremock::MockServer {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let body = format!("data: {}\n\ndata: {}\n\n", text_delta("http"), completion("http_final", text_output("http")));
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .expect(expected)
        .mount(&server)
        .await;
    server
}

async fn final_legacy(mut stream: crate::provider::LLMStream) -> crate::provider::LLMResponse {
    while let Some(event) = stream.next().await {
        if let crate::provider::LLMStreamEvent::Completed { response } = event.expect("legacy event") {
            return *response;
        }
    }
    panic!("missing legacy completion");
}

fn private_reasoning_delta() -> Value {
    json!({"type":"response.reasoning_text.delta", "item_id":"rs_private", "output_index":0,
        "content_index":0, "sequence_number":1, "delta":"private transport reasoning"})
}

mod cache_reconnect;
mod reasoning_misc;
mod resilience;
mod streaming;
mod unit;
