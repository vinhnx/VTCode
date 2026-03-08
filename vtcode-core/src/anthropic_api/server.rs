//! Anthropic API compatibility server
//!
//! Provides compatibility with the Anthropic Messages API to help connect existing
//! applications to VT Code, including tools like Claude Code.

use crate::llm::provider::{
    ContentPart, LLMProvider, LLMRequest, LLMStreamEvent, Message, MessageContent, MessageRole,
    ParallelToolConfig, ToolChoice, ToolDefinition,
};
use crate::llm::providers::anthropic_types::{AnthropicOutputConfig, AnthropicOutputFormat};
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
    pub tool_choice: Option<Value>,
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
    Text {
        text: String,
        #[serde(default)]
        citations: Option<Value>,
        #[serde(default)]
        cache_control: Option<Value>,
    },
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
    Thinking { thinking: String },
    #[serde(rename = "server_tool_use")]
    ServerToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "container_upload")]
    ContainerUpload { file_id: String },
    #[serde(rename = "code_execution_tool_result")]
    CodeExecutionToolResult { tool_use_id: String, content: Value },
    #[serde(rename = "bash_code_execution_tool_result")]
    BashCodeExecutionToolResult { tool_use_id: String, content: Value },
    #[serde(rename = "text_editor_code_execution_tool_result")]
    TextEditorCodeExecutionToolResult { tool_use_id: String, content: Value },
    #[serde(rename = "web_search_tool_result")]
    WebSearchToolResult { tool_use_id: String, content: Value },
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
        #[serde(default)]
        input_examples: Option<Vec<Value>>,
        #[serde(default)]
        strict: Option<bool>,
        #[serde(default)]
        allowed_callers: Option<Vec<String>>,
    },
    Native {
        #[serde(rename = "type")]
        tool_type: String,
        name: String,
        #[serde(flatten, default)]
        options: serde_json::Map<String, Value>,
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
    ThinkingDelta { thinking: String },
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
                                    citations: None,
                                    cache_control: None,
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
                                    thinking: delta.clone(),
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
                                                thinking: delta.clone(),
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
                            LLMStreamEvent::ReasoningStage { .. } => {}
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
    let (tool_choice, parallel_tool_config) =
        parse_anthropic_tool_choice(request.tool_choice.as_ref());
    let effort = request
        .output_config
        .as_ref()
        .and_then(|config| config.effort.clone());
    let output_format = request.output_config.as_ref().and_then(|config| {
        config
            .format
            .as_ref()
            .map(|AnthropicOutputFormat::JsonSchema { schema }| schema.clone())
    });

    // Add system message if present
    if let Some(system_prompt) = request.system {
        let system_text = match system_prompt {
            AnthropicSystemPrompt::Text(text) => text,
            AnthropicSystemPrompt::Blocks(blocks) => {
                // Extract text from blocks
                blocks
                    .iter()
                    .filter_map(|block| {
                        if let AnthropicContentBlock::Text { text, .. } = block {
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
                phase: None,
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
            AnthropicContent::Text(text) => MessageContent::Text(text),
            AnthropicContent::Blocks(blocks) => {
                message_content_from_anthropic_blocks(&blocks, &mut messages)
            }
        };

        if !content.is_empty() {
            messages.push(Message {
                role,
                content,
                tool_calls: None,
                tool_call_id: None,
                phase: None,
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
                    input_examples,
                    strict,
                    allowed_callers,
                } => {
                    let mut tool = ToolDefinition::function(
                        name,
                        description.unwrap_or_default(),
                        input_schema,
                    );
                    tool.input_examples = input_examples;
                    tool.strict = strict;
                    tool.allowed_callers = allowed_callers;
                    tool
                }
                AnthropicTool::Native {
                    tool_type, options, ..
                } => {
                    if tool_type.starts_with("web_search_") {
                        ToolDefinition {
                            tool_type,
                            function: None,
                            allowed_callers: None,
                            input_examples: None,
                            web_search: (!options.is_empty()).then_some(Value::Object(options)),
                            hosted_tool_config: None,
                            shell: None,
                            grammar: None,
                            strict: None,
                            defer_loading: None,
                        }
                    } else if tool_type.starts_with("code_execution_")
                        || tool_type.starts_with("memory_")
                    {
                        ToolDefinition {
                            tool_type,
                            function: None,
                            allowed_callers: None,
                            input_examples: None,
                            web_search: None,
                            hosted_tool_config: None,
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
        tool_choice,
        parallel_tool_calls: None,
        parallel_tool_config,
        reasoning_effort: None,
        effort,
        output_format,
        character_reinforcement: false,
        character_name: None,
        coding_agent_settings: None,
        top_p: request.top_p,
        top_k: request.top_k,
        do_sample: None,
        presence_penalty: None,
        frequency_penalty: None,
        stop_sequences: request.stop_sequences,
        verbosity: None,
        betas: request.betas,
        context_management: request.context_management,
        thinking_budget: None,
        prefill: None,
        metadata: None,
        previous_response_id: None,
        response_store: None,
        responses_include: None,
        service_tier: None,
        prompt_cache_key: None,
    })
}

fn parse_anthropic_tool_choice(
    tool_choice: Option<&Value>,
) -> (Option<ToolChoice>, Option<Box<ParallelToolConfig>>) {
    let Some(choice) = tool_choice else {
        return (None, None);
    };
    let Some(choice_obj) = choice.as_object() else {
        return (None, None);
    };

    let disable_parallel_tool_use = choice_obj
        .get("disable_parallel_tool_use")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let parsed_tool_choice = match choice_obj.get("type").and_then(Value::as_str) {
        Some("auto") => Some(ToolChoice::Auto),
        Some("none") => Some(ToolChoice::None),
        Some("any") => Some(ToolChoice::Any),
        Some("tool") => choice_obj
            .get("name")
            .and_then(Value::as_str)
            .map(|name| ToolChoice::function(name.to_string())),
        _ => None,
    };

    let parallel_tool_config = disable_parallel_tool_use.then(|| {
        Box::new(ParallelToolConfig {
            disable_parallel_tool_use: true,
            max_parallel_tools: Some(1),
            encourage_parallel: false,
        })
    });

    (parsed_tool_choice, parallel_tool_config)
}

fn message_content_from_anthropic_blocks(
    blocks: &[AnthropicContentBlock],
    messages: &mut Vec<Message>,
) -> MessageContent {
    let mut parts = Vec::new();

    for block in blocks {
        match block {
            AnthropicContentBlock::Text { text, .. } => {
                parts.push(ContentPart::text(text.clone()));
            }
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
                            if let AnthropicContentBlock::Text { text, .. } = inner_block {
                                Some(text.clone())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                messages.push(Message {
                    role: MessageRole::Tool,
                    content: content_text.into(),
                    tool_call_id: Some(tool_use_id.clone()),
                    tool_calls: None,
                    phase: None,
                    reasoning_details: None,
                    origin_tool: None,
                    reasoning: None,
                });
            }
            AnthropicContentBlock::ContainerUpload { file_id } => {
                parts.push(ContentPart::file_from_id(file_id.clone()));
            }
            AnthropicContentBlock::Image { .. }
            | AnthropicContentBlock::Thinking { .. }
            | AnthropicContentBlock::ToolUse { .. }
            | AnthropicContentBlock::ServerToolUse { .. }
            | AnthropicContentBlock::CodeExecutionToolResult { .. }
            | AnthropicContentBlock::BashCodeExecutionToolResult { .. }
            | AnthropicContentBlock::TextEditorCodeExecutionToolResult { .. }
            | AnthropicContentBlock::WebSearchToolResult { .. } => {}
        }
    }

    if parts.len() == 1
        && let ContentPart::Text { text } = &parts[0]
    {
        return MessageContent::Text(text.clone());
    }

    MessageContent::Parts(parts)
}

/// Convert VT Code LLM response to Anthropic response
fn convert_llm_to_anthropic_response(
    response: crate::llm::provider::LLMResponse,
) -> Result<AnthropicMessagesResponse, ()> {
    use uuid::Uuid;

    let mut content_blocks = Vec::new();

    // Add text content blocks
    if let Some(content) = response.content
        && !content.is_empty()
    {
        content_blocks.push(AnthropicContentBlock::Text {
            text: content,
            citations: None,
            cache_control: None,
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn convert_anthropic_to_llm_request_preserves_web_search_options() {
        let request = AnthropicMessagesRequest {
            model: "claude-opus-4-6".to_string(),
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

        let llm_request = convert_anthropic_to_llm_request(request).expect("request conversion");
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
            model: "claude-opus-4-6".to_string(),
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

        let llm_request = convert_anthropic_to_llm_request(request).expect("request conversion");
        let tools = llm_request.tools.expect("tools");
        assert_eq!(
            tools[0].allowed_callers.as_ref(),
            Some(&vec!["code_execution_20250825".to_string()])
        );
    }

    #[test]
    fn convert_anthropic_to_llm_request_preserves_strict_and_input_examples() {
        let request = AnthropicMessagesRequest {
            model: "claude-opus-4-6".to_string(),
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

        let llm_request = convert_anthropic_to_llm_request(request).expect("request conversion");
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
            model: "claude-opus-4-6".to_string(),
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

        let llm_request = convert_anthropic_to_llm_request(request).expect("request conversion");
        let tools = llm_request.tools.expect("tools");
        assert_eq!(tools[0].tool_type, "code_execution_20250825");
    }

    #[test]
    fn convert_anthropic_to_llm_request_accepts_native_memory_tool() {
        let request = AnthropicMessagesRequest {
            model: "claude-opus-4-6".to_string(),
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

        let llm_request = convert_anthropic_to_llm_request(request).expect("request conversion");
        let tools = llm_request.tools.expect("tools");
        assert_eq!(tools[0].tool_type, "memory_20250818");
    }

    #[test]
    fn convert_anthropic_to_llm_request_maps_container_upload_to_file_part() {
        let request = AnthropicMessagesRequest {
            model: "claude-opus-4-6".to_string(),
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

        let llm_request = convert_anthropic_to_llm_request(request).expect("request conversion");
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
            model: "claude-opus-4-6".to_string(),
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

        let llm_request = convert_anthropic_to_llm_request(request).expect("request conversion");
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
    fn convert_anthropic_to_llm_request_maps_disable_parallel_tool_use() {
        let request = AnthropicMessagesRequest {
            model: "claude-opus-4-6".to_string(),
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

        let llm_request = convert_anthropic_to_llm_request(request).expect("request conversion");
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
}
