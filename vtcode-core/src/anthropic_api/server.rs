//! Anthropic API compatibility server
//!
//! Provides compatibility with the Anthropic Messages API to help connect existing
//! applications to VT Code, including tools like Claude Code.

use crate::llm::provider::{
    LLMProvider, LLMRequest, LLMStreamEvent, Message, MessageRole, ToolDefinition,
};
use crate::llm::providers::anthropic_types::AnthropicOutputConfig;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, sse::Event},
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::CorsLayer;

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

/// Anthropic Messages API request
#[derive(Debug, Deserialize, Clone)]
pub struct AnthropicMessagesRequest {
    pub model: String,
    pub max_tokens: u32,
    pub messages: Vec<AnthropicMessage>,
    #[serde(default)]
    pub system: Option<AnthropicSystemPrompt>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub top_k: Option<i32>,
    #[serde(default)]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(default)]
    pub tools: Option<Vec<AnthropicTool>>,
    #[serde(default)]
    pub thinking: Option<bool>,
    #[serde(default)]
    pub betas: Option<Vec<String>>,
    #[serde(default)]
    pub context_management: Option<Value>,
    #[serde(default)]
    pub output_config: Option<AnthropicOutputConfig>,
}

/// Anthropic Message
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicContent,
}

/// Anthropic Content (can be string or array of content blocks)
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

/// Anthropic Content Block
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type")]
pub enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: AnthropicImageSource },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: AnthropicContent,
        is_error: Option<bool>,
    },
    #[serde(rename = "thinking")]
    Thinking { text: String },
}

/// Anthropic Image Source
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AnthropicImageSource {
    pub r#type: String,
    pub media_type: String,
    pub data: String,
}

/// Anthropic System Prompt (can be string or array of content blocks)
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum AnthropicSystemPrompt {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

/// Anthropic Tool Definition
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum AnthropicTool {
    Function {
        name: String,
        description: Option<String>,
        input_schema: Value,
    },
    Native {
        #[serde(rename = "type")]
        tool_type: String,
        name: String,
    },
}

/// Anthropic Messages API response
#[derive(Debug, Serialize)]
pub struct AnthropicMessagesResponse {
    pub id: String,
    pub r#type: String,
    pub role: String,
    pub model: String,
    pub content: Vec<AnthropicContentBlock>,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: AnthropicUsage,
}

/// Anthropic Usage
#[derive(Debug, Serialize)]
pub struct AnthropicUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Streaming event for Anthropic API
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum AnthropicStreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: AnthropicMessagesResponse },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: u32,
        content_block: AnthropicContentBlock,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        index: u32,
        delta: AnthropicContentDelta,
    },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: u32 },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: AnthropicDelta,
        usage: AnthropicUsage,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(rename = "ping")]
    Ping {},
    #[serde(rename = "error")]
    Error { error: AnthropicError },
}

/// Anthropic Content Delta
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum AnthropicContentDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { text: String },
}

/// Anthropic Delta
#[derive(Debug, Serialize)]
pub struct AnthropicDelta {
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
}

/// Anthropic Error
#[derive(Debug, Serialize)]
pub struct AnthropicError {
    pub r#type: String,
    pub message: String,
}

/// Handle messages endpoint
pub async fn messages_handler(
    State(state): State<AnthropicApiServerState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<AnthropicMessagesRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    // Extract API key from headers (we accept but don't validate it as per Anthropic compatibility)
    let _api_key = headers
        .get("x-api-key")
        .or_else(|| headers.get("authorization"))
        .or_else(|| headers.get("anthropic-version"))
        .map(|val| val.to_str().unwrap_or(""))
        .unwrap_or("");

    // Extract betas from headers
    let header_betas = headers
        .get("anthropic-beta")
        .and_then(|val| val.to_str().ok())
        .map(|s| {
            s.split(',')
                .map(|b| b.trim().to_string())
                .collect::<Vec<String>>()
        });

    let mut request = request;
    if let Some(hb) = header_betas {
        if let Some(ref mut rb) = request.betas {
            for beta in hb {
                if !rb.contains(&beta) {
                    rb.push(beta);
                }
            }
        } else {
            request.betas = Some(hb);
        }
    }

    // Convert Anthropic request to VT Code LLM request
    let llm_request = match convert_anthropic_to_llm_request(request.clone()) {
        Ok(req) => req,
        Err(_) => return Ok((StatusCode::BAD_REQUEST, "Invalid request").into_response()),
    };

    if request.stream {
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

            if tx
                .send(
                    Event::default().json_data(AnthropicStreamEvent::MessageStart {
                        message: initial_response,
                    }),
                )
                .await
                .is_err()
            {
                return;
            }

            while let Some(event_result) = stream.next().await {
                match event_result {
                    Ok(provider_event) => {
                        match provider_event {
                            LLMStreamEvent::Token { delta } => {
                                // Send content block start
                                let content_block = AnthropicContentBlock::Text {
                                    text: delta.clone(),
                                };

                                if tx
                                    .send(Event::default().json_data(
                                        AnthropicStreamEvent::ContentBlockStart {
                                            index: content_block_idx,
                                            content_block: content_block.clone(),
                                        },
                                    ))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }

                                // Send content delta
                                if tx
                                    .send(Event::default().json_data(
                                        AnthropicStreamEvent::ContentBlockDelta {
                                            index: content_block_idx,
                                            delta: AnthropicContentDelta::TextDelta {
                                                text: delta.clone(),
                                            },
                                        },
                                    ))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }

                                // Send content block stop
                                if tx
                                    .send(Event::default().json_data(
                                        AnthropicStreamEvent::ContentBlockStop {
                                            index: content_block_idx,
                                        },
                                    ))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }

                                content_block_idx += 1;
                            }
                            LLMStreamEvent::Reasoning { delta } => {
                                // Handle reasoning/thinking content
                                let content_block = AnthropicContentBlock::Thinking {
                                    text: delta.clone(),
                                };

                                if tx
                                    .send(Event::default().json_data(
                                        AnthropicStreamEvent::ContentBlockStart {
                                            index: content_block_idx,
                                            content_block: content_block.clone(),
                                        },
                                    ))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }

                                if tx
                                    .send(Event::default().json_data(
                                        AnthropicStreamEvent::ContentBlockDelta {
                                            index: content_block_idx,
                                            delta: AnthropicContentDelta::ThinkingDelta {
                                                text: delta.clone(),
                                            },
                                        },
                                    ))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }

                                if tx
                                    .send(Event::default().json_data(
                                        AnthropicStreamEvent::ContentBlockStop {
                                            index: content_block_idx,
                                        },
                                    ))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }

                                content_block_idx += 1;
                            }
                            LLMStreamEvent::Completed { response } => {
                                // Send message delta with stop reason
                                let stop_reason = match response.finish_reason {
                                    crate::llm::provider::FinishReason::Stop => {
                                        Some("end_turn".to_string())
                                    }
                                    crate::llm::provider::FinishReason::Length => {
                                        Some("max_tokens".to_string())
                                    }
                                    crate::llm::provider::FinishReason::ToolCalls => {
                                        Some("tool_use".to_string())
                                    }
                                    crate::llm::provider::FinishReason::Refusal => {
                                        Some("refusal".to_string())
                                    }
                                    _ => Some("end_turn".to_string()),
                                };

                                let usage = response.usage.unwrap_or_default();
                                let delta = AnthropicDelta {
                                    stop_reason,
                                    stop_sequence: None,
                                };

                                // Send message delta
                                if tx
                                    .send(Event::default().json_data(
                                        AnthropicStreamEvent::MessageDelta {
                                            delta,
                                            usage: AnthropicUsage {
                                                input_tokens: usage.prompt_tokens,
                                                output_tokens: usage.completion_tokens,
                                            },
                                        },
                                    ))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }

                                // Send message stop
                                if tx
                                    .send(
                                        Event::default()
                                            .json_data(AnthropicStreamEvent::MessageStop {}),
                                    )
                                    .await
                                    .is_err()
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

                        if tx
                            .send(Event::default().json_data(error_event))
                            .await
                            .is_err()
                        {
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

        let anthropic_response = match convert_llm_to_anthropic_response(response) {
            Ok(resp) => resp,
            Err(_) => {
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, "Conversion error").into_response());
            }
        };
        Ok(Json(anthropic_response).into_response())
    }
}

/// Convert Anthropic request to VT Code LLM request
fn convert_anthropic_to_llm_request(request: AnthropicMessagesRequest) -> Result<LLMRequest, ()> {
    // Convert messages
    let mut messages = Vec::new();

    // Add system message if present
    if let Some(system_prompt) = request.system {
        let system_text = match system_prompt {
            AnthropicSystemPrompt::Text(text) => text,
            AnthropicSystemPrompt::Blocks(blocks) => {
                // Extract text from blocks
                blocks
                    .iter()
                    .filter_map(|block| {
                        if let AnthropicContentBlock::Text { text } = block {
                            Some(text.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        };
        if !system_text.is_empty() {
            messages.push(Message {
                role: MessageRole::System,
                content: system_text.into(),
                tool_calls: None,
                tool_call_id: None,
                reasoning_details: None,
                origin_tool: None,
                reasoning: None,
            });
        }
    }

    // Convert conversation messages
    for anthropic_msg in request.messages {
        let role = match anthropic_msg.role.as_str() {
            "user" => MessageRole::User,
            "assistant" => MessageRole::Assistant,
            "system" => MessageRole::System,
            "tool" => MessageRole::Tool,
            _ => MessageRole::User, // Default fallback
        };

        let content = match anthropic_msg.content {
            AnthropicContent::Text(text) => text,
            AnthropicContent::Blocks(blocks) => {
                // For now, just extract text content from blocks
                blocks
                    .iter()
                    .filter_map(|block| {
                        match block {
                            AnthropicContentBlock::Text { text } => Some(text.clone()),
                            AnthropicContentBlock::ToolResult {
                                content: tool_content,
                                tool_use_id,
                                ..
                            } => {
                                let content_text = match tool_content {
                                    AnthropicContent::Text(text) => text.clone(),
                                    AnthropicContent::Blocks(inner_blocks) => inner_blocks
                                        .iter()
                                        .filter_map(|inner_block| {
                                            if let AnthropicContentBlock::Text { text } =
                                                inner_block
                                            {
                                                Some(text.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                };
                                // Add tool response message
                                messages.push(Message {
                                    role: MessageRole::Tool,
                                    content: content_text.clone().into(),
                                    tool_call_id: Some(tool_use_id.clone()),
                                    tool_calls: None,
                                    reasoning_details: None,
                                    origin_tool: None,
                                    reasoning: None,
                                });
                                None // Don't add to the main content
                            }
                            AnthropicContentBlock::Image { .. } => {
                                // For now, we'll skip image content in the text content
                                None
                            }
                            _ => None,
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        };

        if !content.is_empty() {
            messages.push(Message {
                role,
                content: content.into(),
                tool_calls: None,
                tool_call_id: None,
                reasoning_details: None,
                origin_tool: None,
                reasoning: None,
            });
        }
    }

    // Convert tools if present
    let tools = if let Some(anthropic_tools) = request.tools {
        let mut converted_tools = Vec::new();
        for tool in anthropic_tools {
            let tool_def = match tool {
                AnthropicTool::Function {
                    name,
                    description,
                    input_schema,
                } => ToolDefinition::function(name, description.unwrap_or_default(), input_schema),
                AnthropicTool::Native { tool_type, .. } => {
                    if tool_type.starts_with("web_search_") {
                        ToolDefinition {
                            tool_type,
                            function: None,
                            web_search: None,
                            shell: None,
                            grammar: None,
                            strict: None,
                            defer_loading: None,
                        }
                    } else {
                        continue;
                    }
                }
            };
            converted_tools.push(tool_def);
        }
        if converted_tools.is_empty() {
            None
        } else {
            Some(Arc::new(converted_tools))
        }
    } else {
        None
    };

    Ok(LLMRequest {
        messages,
        system_prompt: None, // Already handled above
        tools,
        model: request.model,
        max_tokens: Some(request.max_tokens),
        temperature: request.temperature,
        stream: request.stream,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
        effort: request.output_config.map(|config| config.effort),
        output_format: None,
        character_reinforcement: false,
        character_name: None,
        coding_agent_settings: None,
        top_p: request.top_p,
        top_k: request.top_k,
        presence_penalty: None,
        frequency_penalty: None,
        stop_sequences: request.stop_sequences,
        verbosity: None,
        betas: request.betas,
        context_management: request.context_management,
        thinking_budget: None,
        prefill: None,
        metadata: None,
    })
}

/// Convert VT Code LLM response to Anthropic response
fn convert_llm_to_anthropic_response(
    response: crate::llm::provider::LLMResponse,
) -> Result<AnthropicMessagesResponse, ()> {
    use uuid::Uuid;

    let mut content_blocks = Vec::new();

    // Add text content blocks
    if let Some(content) = response.content {
        if !content.is_empty() {
            content_blocks.push(AnthropicContentBlock::Text { text: content });
        }
    }

    // Add tool use blocks if present
    if let Some(tool_calls) = response.tool_calls {
        for call in tool_calls {
            if let Some(func) = &call.function {
                let input = serde_json::from_str(&func.arguments).unwrap_or_else(|_| json!({}));
                content_blocks.push(AnthropicContentBlock::ToolUse {
                    id: call.id.clone(),
                    name: func.name.clone(),
                    input,
                });
            }
        }
    }

    let usage = response.usage.unwrap_or_default();

    Ok(AnthropicMessagesResponse {
        id: Uuid::new_v4().to_string(),
        r#type: "message".to_string(),
        role: "assistant".to_string(),
        model: "unknown".to_string(), // Will be filled from response if available
        content: content_blocks,
        stop_reason: Some(match response.finish_reason {
            crate::llm::provider::FinishReason::Stop => "end_turn".to_string(),
            crate::llm::provider::FinishReason::Length => "max_tokens".to_string(),
            crate::llm::provider::FinishReason::ToolCalls => "tool_use".to_string(),
            crate::llm::provider::FinishReason::ContentFilter => "content_filter".to_string(),
            crate::llm::provider::FinishReason::Pause => "pause_turn".to_string(),
            crate::llm::provider::FinishReason::Refusal => "refusal".to_string(),
            crate::llm::provider::FinishReason::Error(msg) => msg,
        }),
        stop_sequence: None,
        usage: AnthropicUsage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
        },
    })
}
