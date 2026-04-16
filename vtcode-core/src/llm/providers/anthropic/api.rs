//! Anthropic API compatibility server
//!
//! Provides compatibility with the Anthropic Messages API to help connect existing
//! applications to VT Code, including tools like Claude Code.

use crate::llm::provider::{LLMProvider, LLMStreamEvent};
use crate::llm::providers::anthropic::compat::{
    AnthropicContentBlock, AnthropicContentDelta, AnthropicDelta, AnthropicError,
    AnthropicMessagesRequest, AnthropicMessagesResponse, AnthropicStreamEvent, AnthropicUsage,
    anthropic_stop_reason, convert_anthropic_to_llm_request, convert_llm_to_anthropic_response,
};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, sse::Event},
};
use futures::StreamExt;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::CorsLayer;

type AnthropicSseEvent = Result<Event, axum::Error>;
type AnthropicSseSender = tokio::sync::mpsc::Sender<AnthropicSseEvent>;

/// Server state containing shared resources
#[derive(Clone)]
pub struct AnthropicApiServerState {
    /// The LLM provider to use for requests
    pub provider: Arc<dyn LLMProvider>,
    /// Model name to use
    pub model: String,
}

impl AnthropicApiServerState {
    pub fn new(provider: Arc<dyn LLMProvider>, model: String) -> Self {
        Self { provider, model }
    }
}

/// Create the Anthropic API router
pub fn create_router(state: AnthropicApiServerState) -> Router {
    Router::new()
        .route("/v1/messages", axum::routing::post(messages_handler))
        .with_state(state)
        .layer(CorsLayer::permissive())
}

fn merge_header_betas(request: &mut AnthropicMessagesRequest, headers: &HeaderMap) {
    let Some(header_betas) = headers
        .get("anthropic-beta")
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|beta| !beta.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|betas| !betas.is_empty())
    else {
        return;
    };

    let request_betas = request.betas.get_or_insert_with(Vec::new);
    for beta in header_betas {
        if !request_betas.contains(&beta) {
            request_betas.push(beta);
        }
    }
}

async fn send_stream_event(tx: &AnthropicSseSender, event: AnthropicStreamEvent) -> bool {
    tx.send(Event::default().json_data(event)).await.is_ok()
}

async fn send_content_block(
    tx: &AnthropicSseSender,
    index: u32,
    content_block: AnthropicContentBlock,
    delta: AnthropicContentDelta,
) -> bool {
    if !send_stream_event(
        tx,
        AnthropicStreamEvent::ContentBlockStart {
            index,
            content_block,
        },
    )
    .await
    {
        return false;
    }

    if !send_stream_event(tx, AnthropicStreamEvent::ContentBlockDelta { index, delta }).await {
        return false;
    }

    send_stream_event(tx, AnthropicStreamEvent::ContentBlockStop { index }).await
}

/// Handle messages endpoint
pub async fn messages_handler(
    State(state): State<AnthropicApiServerState>,
    headers: HeaderMap,
    Json(request): Json<AnthropicMessagesRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut request = request;
    merge_header_betas(&mut request, &headers);

    let is_stream = request.stream;
    let llm_request = convert_anthropic_to_llm_request(request);

    if is_stream {
        // Handle streaming response
        let stream = match state.provider.stream(llm_request).await {
            Ok(s) => s,
            Err(_) => {
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, "Stream error").into_response());
            }
        };

        // Create a channel to bridge the stream
        let (tx, rx) = tokio::sync::mpsc::channel(100);

        // Spawn a task to convert the stream
        tokio::spawn(async move {
            let mut stream = Box::pin(stream);
            let mut content_block_idx = 0u32;

            // Send message_start event
            let initial_response = AnthropicMessagesResponse {
                id: uuid::Uuid::new_v4().to_string(),
                r#type: "message".to_string(),
                role: "assistant".to_string(),
                model: state.model.clone(),
                content: vec![],
                stop_reason: None,
                stop_sequence: None,
                usage: AnthropicUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                },
            };

            if !send_stream_event(
                &tx,
                AnthropicStreamEvent::MessageStart {
                    message: initial_response,
                },
            )
            .await
            {
                return;
            }

            while let Some(event_result) = stream.next().await {
                match event_result {
                    Ok(provider_event) => {
                        match provider_event {
                            LLMStreamEvent::Token { delta } => {
                                let content_block = AnthropicContentBlock::Text {
                                    text: delta.clone(),
                                    citations: None,
                                    cache_control: None,
                                };

                                if !send_content_block(
                                    &tx,
                                    content_block_idx,
                                    content_block,
                                    AnthropicContentDelta::TextDelta { text: delta },
                                )
                                .await
                                {
                                    break;
                                }

                                content_block_idx += 1;
                            }
                            LLMStreamEvent::Reasoning { delta } => {
                                let content_block = AnthropicContentBlock::Thinking {
                                    thinking: delta.clone(),
                                };

                                if !send_content_block(
                                    &tx,
                                    content_block_idx,
                                    content_block,
                                    AnthropicContentDelta::ThinkingDelta { thinking: delta },
                                )
                                .await
                                {
                                    break;
                                }

                                content_block_idx += 1;
                            }
                            LLMStreamEvent::ReasoningStage { .. } => {}
                            LLMStreamEvent::Completed { response } => {
                                let usage = response.usage.unwrap_or_default();
                                let delta = AnthropicDelta {
                                    stop_reason: Some(anthropic_stop_reason(
                                        response.finish_reason,
                                    )),
                                    stop_sequence: None,
                                };

                                if !send_stream_event(
                                    &tx,
                                    AnthropicStreamEvent::MessageDelta {
                                        delta,
                                        usage: AnthropicUsage {
                                            input_tokens: usage.prompt_tokens,
                                            output_tokens: usage.completion_tokens,
                                        },
                                    },
                                )
                                .await
                                {
                                    break;
                                }

                                if !send_stream_event(&tx, AnthropicStreamEvent::MessageStop).await
                                {
                                    break;
                                }

                                break; // Exit the stream
                            }
                        }
                    }
                    Err(e) => {
                        let error_event = AnthropicStreamEvent::Error {
                            error: AnthropicError {
                                r#type: "error".to_string(),
                                message: e.to_string(),
                            },
                        };

                        if !send_stream_event(&tx, error_event).await {
                            break;
                        }
                        break;
                    }
                }
            }
        });

        Ok(axum::response::Sse::new(ReceiverStream::new(rx)).into_response())
    } else {
        // Handle non-streaming response
        let response = match state.provider.generate(llm_request).await {
            Ok(r) => r,
            Err(_) => {
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, "Generation error").into_response());
            }
        };

        let anthropic_response = convert_llm_to_anthropic_response(response);
        Ok(Json(anthropic_response).into_response())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::{ContentPart, MessageContent, ToolChoice};
    use crate::llm::providers::anthropic::compat::{
        AnthropicContent, AnthropicMessage, AnthropicTool,
    };
    use crate::llm::providers::anthropic_types::{AnthropicOutputConfig, AnthropicOutputFormat};
    use serde_json::json;

    #[test]
    fn convert_anthropic_to_llm_request_preserves_web_search_options() {
        let request = AnthropicMessagesRequest {
            model: "claude-opus-4-7".to_string(),
            max_tokens: 1024,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Text("search docs".to_string()),
            }],
            system: None,
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: Some(vec![AnthropicTool::Native {
                tool_type: "web_search_20260209".to_string(),
                name: "web_search".to_string(),
                options: json!({
                    "allowed_callers": ["direct"]
                })
                .as_object()
                .cloned()
                .expect("object config"),
            }]),
            tool_choice: None,
            thinking: None,
            betas: None,
            context_management: None,
            output_config: None,
        };

        let llm_request = convert_anthropic_to_llm_request(request);
        let tools = llm_request.tools.expect("tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool_type, "web_search_20260209");
        assert_eq!(
            tools[0].web_search.as_ref(),
            Some(&json!({
                "allowed_callers": ["direct"]
            }))
        );
    }

    #[test]
    fn convert_anthropic_to_llm_request_preserves_function_allowed_callers() {
        let request = AnthropicMessagesRequest {
            model: "claude-opus-4-7".to_string(),
            max_tokens: 1024,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Text("find warmest city".to_string()),
            }],
            system: None,
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: Some(vec![AnthropicTool::Function {
                name: "get_weather".to_string(),
                description: Some("Get weather for a city".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "city": {"type": "string"}
                    },
                    "required": ["city"]
                }),
                input_examples: None,
                strict: None,
                allowed_callers: Some(vec!["code_execution_20250825".to_string()]),
            }]),
            tool_choice: None,
            thinking: None,
            betas: None,
            context_management: None,
            output_config: None,
        };

        let llm_request = convert_anthropic_to_llm_request(request);
        let tools = llm_request.tools.expect("tools");
        assert_eq!(
            tools[0].allowed_callers.as_ref(),
            Some(&vec!["code_execution_20250825".to_string()])
        );
    }

    #[test]
    fn convert_anthropic_to_llm_request_preserves_strict_and_input_examples() {
        let request = AnthropicMessagesRequest {
            model: "claude-opus-4-7".to_string(),
            max_tokens: 1024,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Text("find warmest city".to_string()),
            }],
            system: None,
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: Some(vec![AnthropicTool::Function {
                name: "get_weather".to_string(),
                description: Some("Get weather for a city".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "city": {"type": "string"}
                    },
                    "required": ["city"]
                }),
                input_examples: Some(vec![json!({
                    "input": "Weather in Paris",
                    "tool_use": {
                        "city": "Paris"
                    }
                })]),
                strict: Some(true),
                allowed_callers: None,
            }]),
            tool_choice: None,
            thinking: None,
            betas: None,
            context_management: None,
            output_config: None,
        };

        let llm_request = convert_anthropic_to_llm_request(request);
        let tools = llm_request.tools.expect("tools");
        assert_eq!(tools[0].strict, Some(true));
        assert_eq!(
            tools[0].input_examples.as_ref(),
            Some(&vec![json!({
                "input": "Weather in Paris",
                "tool_use": {
                    "city": "Paris"
                }
            })])
        );
    }

    #[test]
    fn convert_anthropic_to_llm_request_accepts_native_code_execution_tool() {
        let request = AnthropicMessagesRequest {
            model: "claude-opus-4-7".to_string(),
            max_tokens: 1024,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Text("run python".to_string()),
            }],
            system: None,
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: Some(vec![AnthropicTool::Native {
                tool_type: "code_execution_20250825".to_string(),
                name: "code_execution".to_string(),
                options: serde_json::Map::new(),
            }]),
            tool_choice: None,
            thinking: None,
            betas: None,
            context_management: None,
            output_config: None,
        };

        let llm_request = convert_anthropic_to_llm_request(request);
        let tools = llm_request.tools.expect("tools");
        assert_eq!(tools[0].tool_type, "code_execution_20250825");
    }

    #[test]
    fn convert_anthropic_to_llm_request_accepts_native_memory_tool() {
        let request = AnthropicMessagesRequest {
            model: "claude-opus-4-7".to_string(),
            max_tokens: 1024,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Text("remember this preference".to_string()),
            }],
            system: None,
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: Some(vec![AnthropicTool::Native {
                tool_type: "memory_20250818".to_string(),
                name: "memory".to_string(),
                options: serde_json::Map::new(),
            }]),
            tool_choice: None,
            thinking: None,
            betas: None,
            context_management: None,
            output_config: None,
        };

        let llm_request = convert_anthropic_to_llm_request(request);
        let tools = llm_request.tools.expect("tools");
        assert_eq!(tools[0].tool_type, "memory_20250818");
    }

    #[test]
    fn convert_anthropic_to_llm_request_maps_container_upload_to_file_part() {
        let request = AnthropicMessagesRequest {
            model: "claude-opus-4-7".to_string(),
            max_tokens: 1024,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Blocks(vec![
                    AnthropicContentBlock::Text {
                        text: "Analyze this CSV".to_string(),
                        citations: None,
                        cache_control: None,
                    },
                    AnthropicContentBlock::ContainerUpload {
                        file_id: "file_abc123".to_string(),
                    },
                ]),
            }],
            system: None,
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
            betas: None,
            context_management: None,
            output_config: None,
        };

        let llm_request = convert_anthropic_to_llm_request(request);
        match &llm_request.messages[0].content {
            MessageContent::Parts(parts) => {
                assert!(matches!(
                    &parts[0],
                    ContentPart::Text { text } if text == "Analyze this CSV"
                ));
                assert!(matches!(
                    &parts[1],
                    ContentPart::File {
                        file_id: Some(file_id),
                        ..
                    } if file_id == "file_abc123"
                ));
            }
            other => panic!("expected multipart content, got {other:?}"),
        }
    }

    #[test]
    fn convert_anthropic_to_llm_request_maps_native_structured_output_config() {
        let request = AnthropicMessagesRequest {
            model: "claude-opus-4-7".to_string(),
            max_tokens: 1024,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Text("answer in json".to_string()),
            }],
            system: None,
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
            betas: None,
            context_management: None,
            output_config: Some(AnthropicOutputConfig {
                effort: Some("medium".to_string()),
                task_budget: None,
                format: Some(AnthropicOutputFormat::JsonSchema {
                    schema: json!({
                        "type": "object",
                        "properties": {
                            "answer": {"type": "string"}
                        },
                        "required": ["answer"],
                        "additionalProperties": false
                    }),
                }),
            }),
        };

        let llm_request = convert_anthropic_to_llm_request(request);
        assert_eq!(llm_request.effort.as_deref(), Some("medium"));
        assert_eq!(
            llm_request.output_format,
            Some(json!({
                "type": "object",
                "properties": {
                    "answer": {"type": "string"}
                },
                "required": ["answer"],
                "additionalProperties": false
            }))
        );
    }

    #[test]
    fn convert_anthropic_to_llm_request_preserves_assistant_tool_calls_and_reasoning() {
        let request = AnthropicMessagesRequest {
            model: "claude-opus-4-7".to_string(),
            max_tokens: 1024,
            messages: vec![AnthropicMessage {
                role: "assistant".to_string(),
                content: AnthropicContent::Blocks(vec![
                    AnthropicContentBlock::Thinking {
                        thinking: "inspect files".to_string(),
                    },
                    AnthropicContentBlock::Text {
                        text: "Calling read_file".to_string(),
                        citations: None,
                        cache_control: None,
                    },
                    AnthropicContentBlock::ToolUse {
                        id: "call_123".to_string(),
                        name: "read_file".to_string(),
                        input: json!({"path": "src/main.rs"}),
                    },
                ]),
            }],
            system: None,
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
            betas: None,
            context_management: None,
            output_config: None,
        };

        let llm_request = convert_anthropic_to_llm_request(request);
        assert_eq!(llm_request.messages.len(), 1);
        let message = &llm_request.messages[0];
        assert_eq!(message.reasoning.as_deref(), Some("inspect files"));
        assert_eq!(message.content.as_text().as_ref(), "Calling read_file");
        assert_eq!(
            message
                .tool_calls
                .as_ref()
                .and_then(|calls| calls.first())
                .and_then(|call| call.function.as_ref())
                .map(|function| function.name.as_str()),
            Some("read_file")
        );
    }

    #[test]
    fn convert_anthropic_to_llm_request_maps_disable_parallel_tool_use() {
        let request = AnthropicMessagesRequest {
            model: "claude-opus-4-7".to_string(),
            max_tokens: 1024,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Text("use one tool at a time".to_string()),
            }],
            system: None,
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: None,
            tool_choice: Some(json!({
                "type": "auto",
                "disable_parallel_tool_use": true
            })),
            thinking: None,
            betas: None,
            context_management: None,
            output_config: None,
        };

        let llm_request = convert_anthropic_to_llm_request(request);
        assert!(matches!(llm_request.tool_choice, Some(ToolChoice::Auto)));
        assert!(
            llm_request
                .parallel_tool_config
                .as_ref()
                .is_some_and(|config| config.disable_parallel_tool_use)
        );
    }

    #[test]
    fn anthropic_content_block_thinking_uses_anthropic_wire_field() {
        let block = AnthropicContentBlock::Thinking {
            thinking: "plan".to_string(),
        };

        let serialized = serde_json::to_value(block).expect("serialize thinking block");
        assert_eq!(serialized["type"], "thinking");
        assert_eq!(serialized["thinking"], "plan");
        assert!(serialized.get("text").is_none());
    }

    #[test]
    fn anthropic_content_delta_thinking_uses_anthropic_wire_field() {
        let delta = AnthropicContentDelta::ThinkingDelta {
            thinking: "draft".to_string(),
        };

        let serialized = serde_json::to_value(delta).expect("serialize thinking delta");
        assert_eq!(serialized["type"], "thinking_delta");
        assert_eq!(serialized["thinking"], "draft");
        assert!(serialized.get("text").is_none());
    }

    #[test]
    fn convert_llm_to_anthropic_response_preserves_reasoning_and_model() {
        let response = crate::llm::provider::LLMResponse {
            content: Some("Done".to_string()),
            model: "claude-opus-4-7".to_string(),
            reasoning: Some("inspect files".to_string()),
            ..Default::default()
        };

        let anthropic = convert_llm_to_anthropic_response(response);
        assert_eq!(anthropic.model, "claude-opus-4-7");
        assert!(matches!(
            anthropic.content.first(),
            Some(AnthropicContentBlock::Thinking { thinking }) if thinking == "inspect files"
        ));
        assert!(matches!(
            anthropic.content.get(1),
            Some(AnthropicContentBlock::Text { text, .. }) if text == "Done"
        ));
    }
}
