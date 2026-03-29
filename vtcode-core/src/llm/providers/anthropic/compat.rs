use crate::llm::provider::{
    ContentPart, FinishReason, LLMRequest, LLMResponse, Message, MessageContent, MessageRole,
    ParallelToolConfig, ToolCall, ToolChoice, ToolDefinition,
};
use crate::llm::providers::anthropic_types::{AnthropicOutputConfig, AnthropicOutputFormat};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

/// Anthropic Messages API request.
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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicContent,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AnthropicImageSource {
    pub r#type: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum AnthropicSystemPrompt {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

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

#[derive(Debug, Serialize, Clone)]
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

#[derive(Debug, Serialize, Clone)]
pub struct AnthropicUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

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

#[derive(Debug, Serialize)]
pub struct AnthropicDelta {
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AnthropicError {
    pub r#type: String,
    pub message: String,
}

#[derive(Default)]
struct ConvertedAnthropicBlocks {
    content_parts: Vec<ContentPart>,
    tool_calls: Vec<ToolCall>,
    reasoning_chunks: Vec<String>,
    emitted_messages: Vec<Message>,
}

pub fn convert_anthropic_to_llm_request(request: AnthropicMessagesRequest) -> LLMRequest {
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

    let system_prompt = request
        .system
        .map(extract_system_prompt_text)
        .filter(|value| !value.is_empty())
        .map(Arc::new);

    let mut messages = Vec::new();
    for anthropic_msg in request.messages {
        let role = anthropic_role_to_message_role(&anthropic_msg.role);

        match anthropic_msg.content {
            AnthropicContent::Text(text) => {
                if !text.is_empty() {
                    messages.push(Message::base(role, MessageContent::Text(text)));
                }
            }
            AnthropicContent::Blocks(blocks) => {
                let converted = convert_anthropic_blocks(&blocks);
                messages.extend(converted.emitted_messages);

                if !converted.content_parts.is_empty()
                    || !converted.tool_calls.is_empty()
                    || !converted.reasoning_chunks.is_empty()
                {
                    let mut message =
                        Message::base(role, message_content_from_parts(converted.content_parts));
                    if !converted.tool_calls.is_empty() {
                        message.tool_calls = Some(converted.tool_calls);
                    }
                    if !converted.reasoning_chunks.is_empty()
                        && message.role == MessageRole::Assistant
                    {
                        message.reasoning = Some(converted.reasoning_chunks.join("\n"));
                    }
                    messages.push(message);
                }
            }
        }
    }

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

    LLMRequest {
        messages,
        system_prompt,
        tools,
        model: request.model,
        max_tokens: Some(request.max_tokens),
        temperature: request.temperature,
        stream: request.stream,
        output_format,
        tool_choice,
        parallel_tool_calls: None,
        parallel_tool_config,
        reasoning_effort: None,
        effort,
        verbosity: None,
        do_sample: None,
        top_p: request.top_p,
        top_k: request.top_k,
        presence_penalty: None,
        frequency_penalty: None,
        stop_sequences: request.stop_sequences,
        thinking_budget: None,
        betas: request.betas,
        context_management: request.context_management,
        prefill: None,
        character_reinforcement: false,
        character_name: None,
        coding_agent_settings: None,
        metadata: None,
        previous_response_id: None,
        response_store: None,
        responses_include: None,
        service_tier: None,
        prompt_cache_key: None,
    }
}

pub fn convert_llm_to_anthropic_response(response: LLMResponse) -> AnthropicMessagesResponse {
    use uuid::Uuid;

    let mut content_blocks = Vec::new();

    if let Some(reasoning) = response.reasoning.as_ref()
        && !reasoning.trim().is_empty()
    {
        content_blocks.push(AnthropicContentBlock::Thinking {
            thinking: reasoning.clone(),
        });
    }

    if let Some(content) = response.content.as_ref()
        && !content.is_empty()
    {
        content_blocks.push(AnthropicContentBlock::Text {
            text: content.clone(),
            citations: None,
            cache_control: None,
        });
    }

    if let Some(tool_calls) = response.tool_calls.as_ref() {
        for call in tool_calls {
            if let Some(func) = &call.function {
                let input = call
                    .parsed_arguments()
                    .unwrap_or_else(|_| Value::String(func.arguments.clone()));
                content_blocks.push(AnthropicContentBlock::ToolUse {
                    id: call.id.clone(),
                    name: func.name.clone(),
                    input,
                });
            }
        }
    }

    let usage = response.usage.unwrap_or_default();
    let model = if response.model.trim().is_empty() {
        "unknown".to_string()
    } else {
        response.model
    };

    AnthropicMessagesResponse {
        id: Uuid::new_v4().to_string(),
        r#type: "message".to_string(),
        role: "assistant".to_string(),
        model,
        content: content_blocks,
        stop_reason: Some(anthropic_stop_reason(response.finish_reason)),
        stop_sequence: None,
        usage: AnthropicUsage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
        },
    }
}

pub(crate) fn anthropic_stop_reason(finish_reason: FinishReason) -> String {
    match finish_reason {
        FinishReason::Stop => "end_turn".to_string(),
        FinishReason::Length => "max_tokens".to_string(),
        FinishReason::ToolCalls => "tool_use".to_string(),
        FinishReason::ContentFilter => "content_filter".to_string(),
        FinishReason::Pause => "pause_turn".to_string(),
        FinishReason::Refusal => "refusal".to_string(),
        FinishReason::Error(message) => message,
    }
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

fn anthropic_role_to_message_role(role: &str) -> MessageRole {
    match role {
        "assistant" => MessageRole::Assistant,
        "system" => MessageRole::System,
        "tool" => MessageRole::Tool,
        _ => MessageRole::User,
    }
}

fn extract_system_prompt_text(system_prompt: AnthropicSystemPrompt) -> String {
    match system_prompt {
        AnthropicSystemPrompt::Text(text) => text,
        AnthropicSystemPrompt::Blocks(blocks) => blocks
            .iter()
            .map(anthropic_block_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn convert_anthropic_blocks(blocks: &[AnthropicContentBlock]) -> ConvertedAnthropicBlocks {
    let mut converted = ConvertedAnthropicBlocks::default();

    for block in blocks {
        match block {
            AnthropicContentBlock::Text { text, .. } => {
                converted
                    .content_parts
                    .push(ContentPart::text(text.clone()));
            }
            AnthropicContentBlock::Image { source } => {
                converted.content_parts.push(ContentPart::image(
                    source.data.clone(),
                    source.media_type.clone(),
                ));
            }
            AnthropicContentBlock::ToolUse { id, name, input }
            | AnthropicContentBlock::ServerToolUse { id, name, input } => {
                converted.tool_calls.push(ToolCall::function(
                    id.clone(),
                    name.clone(),
                    input.to_string(),
                ));
            }
            AnthropicContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } => converted.emitted_messages.push(Message::tool_response(
                tool_use_id.clone(),
                anthropic_content_text(content),
            )),
            AnthropicContentBlock::Thinking { thinking } => {
                converted.reasoning_chunks.push(thinking.clone());
            }
            AnthropicContentBlock::ContainerUpload { file_id } => {
                converted
                    .content_parts
                    .push(ContentPart::file_from_id(file_id.clone()));
            }
            AnthropicContentBlock::CodeExecutionToolResult {
                tool_use_id,
                content,
            }
            | AnthropicContentBlock::BashCodeExecutionToolResult {
                tool_use_id,
                content,
            }
            | AnthropicContentBlock::TextEditorCodeExecutionToolResult {
                tool_use_id,
                content,
            }
            | AnthropicContentBlock::WebSearchToolResult {
                tool_use_id,
                content,
            } => converted.emitted_messages.push(Message::tool_response(
                tool_use_id.clone(),
                serialize_value(content),
            )),
        }
    }

    converted
}

fn message_content_from_parts(parts: Vec<ContentPart>) -> MessageContent {
    if parts.len() == 1
        && let ContentPart::Text { text } = &parts[0]
    {
        return MessageContent::Text(text.clone());
    }

    MessageContent::Parts(parts)
}

fn anthropic_content_text(content: &AnthropicContent) -> String {
    match content {
        AnthropicContent::Text(text) => text.clone(),
        AnthropicContent::Blocks(blocks) => blocks
            .iter()
            .map(anthropic_block_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn anthropic_block_text(block: &AnthropicContentBlock) -> String {
    match block {
        AnthropicContentBlock::Text { text, .. } => text.clone(),
        AnthropicContentBlock::Thinking { thinking } => thinking.clone(),
        AnthropicContentBlock::Image { .. } => "[Image]".to_string(),
        AnthropicContentBlock::ContainerUpload { file_id } => format!("[File: {file_id}]"),
        AnthropicContentBlock::ToolUse { name, input, .. }
        | AnthropicContentBlock::ServerToolUse { name, input, .. } => {
            format!("[Tool call: {name} with args: {input}]")
        }
        AnthropicContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } => format!(
            "[Tool result {}: {}]",
            tool_use_id,
            anthropic_content_text(content)
        ),
        AnthropicContentBlock::CodeExecutionToolResult {
            tool_use_id,
            content,
        }
        | AnthropicContentBlock::BashCodeExecutionToolResult {
            tool_use_id,
            content,
        }
        | AnthropicContentBlock::TextEditorCodeExecutionToolResult {
            tool_use_id,
            content,
        }
        | AnthropicContentBlock::WebSearchToolResult {
            tool_use_id,
            content,
        } => {
            format!(
                "[Tool result {}: {}]",
                tool_use_id,
                serialize_value(content)
            )
        }
    }
}

fn serialize_value(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| json!({ "value": value }).to_string())
}
