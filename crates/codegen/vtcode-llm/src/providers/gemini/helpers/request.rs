//! GenerateContent and Interactions request construction.

use super::super::sanitize::sanitize_function_parameters;
use super::super::wire::{GenerationConfig, InlineData, StreamingError, ThinkingConfig};
use super::*;
use crate::provider::{ContentPart, MessageContent, ToolDefinition};
use crate::providers::common::{collect_history_system_directives, merge_system_prompt_with_history_directives};
use crate::system_prompt::default_system_prompt;
use std::collections::BTreeMap;

impl GeminiProvider {
    const HISTORY_DIRECTIVES_SECTION_HEADER: &str = "[History Directives]";

    pub(crate) fn convert_to_gemini_request(&self, request: &LLMRequest) -> Result<GenerateContentRequest, LLMError> {
        // Explicit mode is applied in `generate`/`stream` via the
        // `cachedContents` lifecycle (`ensure_explicit_cache`); this converter
        // always emits the full implicit-shaped body so the cache create can
        // reuse the same system/tools/contents payload.

        let mut call_map: HashMap<String, String> = HashMap::with_capacity(request.messages.len());
        for message in request.messages.iter() {
            if message.role == MessageRole::Assistant
                && let Some(tool_calls) = &message.tool_calls
            {
                for tool_call in tool_calls {
                    if let Some(ref func) = tool_call.function {
                        call_map.insert(tool_call.id.clone(), func.name.clone());
                    }
                }
            }
        }

        let mut contents: Vec<Content> = Vec::with_capacity(request.messages.len());
        let history_system_directives = collect_history_system_directives(request);
        for message in request.messages.iter() {
            if message.role == MessageRole::System {
                continue;
            }

            let mut parts = match preserved_gemini_parts_from_message(message) {
                Some(mut preserved) => {
                    // Preserved parts bypass `tool_calls`, so request-only
                    // input clearing would never reach the wire without
                    // reconciling them here.
                    sync_preserved_parts_with_tool_calls(&mut preserved, message);
                    preserved
                }
                None => build_message_parts(message, request.model.as_str()),
            };

            if message.role == MessageRole::Tool {
                if let Some(tool_call_id) = &message.tool_call_id {
                    let func_name = call_map.get(tool_call_id).cloned().unwrap_or_else(|| tool_call_id.clone());
                    let response_text = serde_json::from_str::<Value>(&message.content.as_text())
                        .map(|value| {
                            serde_json::to_string_pretty(&value)
                                .unwrap_or_else(|_| message.content.as_text().into_owned())
                        })
                        .unwrap_or_else(|_| message.content.as_text().into_owned());

                    let response_payload = json!({
                        "name": func_name.clone(),
                        "content": [{
                            "text": response_text
                        }]
                    });

                    parts.push(Part::FunctionResponse {
                        function_response: FunctionResponse {
                            name: func_name,
                            response: response_payload,
                            id: Some(tool_call_id.clone()),
                        },
                        thought_signature: None, // Function responses don't carry thought signatures
                    });
                } else if !message.content.is_empty() {
                    parts.push(Part::Text {
                        text: message.content.as_text().into_owned(),
                        thought_signature: None,
                    });
                }
            }

            if !parts.is_empty() {
                contents.push(Content {
                    role: message.role.as_gemini_str().to_string(),
                    parts,
                });
            }
        }

        // Latest Gemini models reject prefilled model turns (last turn with role "model").
        // A trailing FunctionCall turn is an in-flight tool call, not a prefill:
        // preserve it (and its thought signatures) rather than silently dropping it.
        let trailing_prefill = contents.last().is_some_and(|content| {
            content.role == "model" && !content.parts.iter().any(|part| matches!(part, Part::FunctionCall { .. }))
        });
        if Self::uses_latest_gemini_api(&request.model) && trailing_prefill {
            contents.pop();
        }

        let tool_spec = collect_gemini_tool_spec(request.tools.as_deref().map(|v| v.as_slice()))?;
        let tools = tool_spec.generate_tools;
        let uses_server_side_tools = tool_spec.uses_server_side_tools;

        let generation_config = build_generation_config(self, request);

        // For Gemini 3 Pro, Google recommends keeping temperature at 1.0 default
        if let Some(temp) = request.temperature {
            if Self::is_gemini_3_pro_model(&request.model) && temp < 1.0 {
                tracing::warn!(
                    "When using Gemini 3 Pro with temperature values below 1.0, be aware that this may cause looping or degraded performance on complex tasks. Consider using 1.0 or higher for optimal results."
                );
            }
        }

        let has_tools = request.tools.as_ref().map(|defs| !defs.is_empty()).unwrap_or(false);
        let has_function_tools = tool_spec.has_function_tools;
        let tool_config = if has_tools || request.tool_choice.is_some() {
            let function_calling_config = if has_function_tools {
                Some(match request.tool_choice.as_ref() {
                    Some(ToolChoice::None) => FunctionCallingConfig::none(),
                    Some(ToolChoice::Any) => FunctionCallingConfig::any(),
                    Some(ToolChoice::Specific(spec)) => {
                        let mut config = if uses_server_side_tools {
                            FunctionCallingConfig::validated()
                        } else {
                            FunctionCallingConfig::any()
                        };
                        if spec.tool_type == "function" {
                            config.allowed_function_names = Some(vec![spec.function.name.clone()]);
                        }
                        config
                    }
                    _ => {
                        if uses_server_side_tools {
                            FunctionCallingConfig::validated()
                        } else {
                            FunctionCallingConfig::auto()
                        }
                    }
                })
            } else {
                None
            };

            Some(ToolConfig {
                function_calling_config,
                include_server_side_tool_invocations: uses_server_side_tools.then_some(true),
            })
        } else {
            None
        };

        Ok(GenerateContentRequest {
            contents,
            tools,
            tool_config,
            system_instruction: {
                let owned_prompt;
                let base_system_prompt = if self.prompt_cache_enabled {
                    owned_prompt = Some(default_system_prompt());
                    request
                        .system_prompt
                        .as_ref()
                        .map(|prompt| prompt.as_ref())
                        .or(owned_prompt.as_deref())
                } else {
                    request.system_prompt.as_ref().map(|prompt| prompt.as_ref())
                };
                let merged_system_prompt = merge_system_prompt_with_history_directives(
                    base_system_prompt,
                    &history_system_directives,
                    Self::HISTORY_DIRECTIVES_SECTION_HEADER,
                );

                if self.prompt_cache_enabled
                    && matches!(self.prompt_cache_settings.mode, GeminiPromptCacheMode::Explicit)
                {
                    // Fail-safe: the inline `ttlSeconds` part is not a valid
                    // generateContent shape (see the warn-once above). Emit the
                    // same text-only system instruction as implicit mode.
                    merged_system_prompt.map(SystemInstruction::new)
                } else if request.system_prompt.is_some()
                    || self.prompt_cache_enabled
                    || !history_system_directives.is_empty()
                {
                    merged_system_prompt.map(SystemInstruction::new)
                } else {
                    None
                }
            },
            generation_config: Some(generation_config.into()),
            cached_content: None,
        })
    }

    pub(crate) fn should_use_interactions(&self, request: &LLMRequest) -> bool {
        if request.previous_response_id.is_some() {
            return true;
        }

        let uses_interactions_for_built_in_tools = request.model.contains("gemini-3")
            || request.model.contains("gemini-4")
            || GeminiProvider::uses_latest_gemini_api(&request.model);
        uses_interactions_for_built_in_tools
            && request
                .tools
                .as_deref()
                .is_some_and(|tools| tools.iter().any(|tool| gemini_built_in_tool(tool).is_some()))
    }

    pub(crate) fn convert_to_interaction_request(&self, request: &LLMRequest) -> Result<InteractionRequest, LLMError> {
        let history_system_directives = collect_history_system_directives(request);
        let owned_prompt;
        let base_system_prompt = if self.prompt_cache_enabled {
            owned_prompt = Some(default_system_prompt());
            request
                .system_prompt
                .as_ref()
                .map(|prompt| prompt.as_ref())
                .or(owned_prompt.as_deref())
        } else {
            request.system_prompt.as_ref().map(|prompt| prompt.as_ref())
        };
        let merged_system_prompt = merge_system_prompt_with_history_directives(
            base_system_prompt,
            &history_system_directives,
            Self::HISTORY_DIRECTIVES_SECTION_HEADER,
        );

        let tool_spec = collect_gemini_tool_spec(request.tools.as_deref().map(|v| v.as_slice()))?;
        let generation_config = build_generation_config(self, request);
        let interaction_input = build_interaction_input(request)?;

        Ok(InteractionRequest {
            model: request.model.clone(),
            input: interaction_input,
            tools: tool_spec.interaction_tools,
            system_instruction: merged_system_prompt,
            response_format: request.output_format.clone(),
            response_mime_type: request.output_format.as_ref().map(|_| "application/json".to_string()),
            stream: request.stream.then_some(true),
            store: request.response_store,
            generation_config: Some(generation_config.into()),
            tool_choice: build_interaction_tool_choice(
                request.tool_choice.as_ref(),
                tool_spec.has_function_tools,
                tool_spec.uses_server_side_tools,
            ),
            previous_interaction_id: request.previous_response_id.clone(),
        })
    }
}

pub(crate) struct GeminiToolSpec {
    pub(crate) generate_tools: Option<Vec<Tool>>,
    pub(crate) interaction_tools: Option<Vec<InteractionTool>>,
    pub(crate) uses_server_side_tools: bool,
    pub(crate) has_function_tools: bool,
}

fn parts_from_message_content(content: &MessageContent) -> Vec<Part> {
    match content {
        MessageContent::Text(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![Part::Text { text: text.clone(), thought_signature: None }]
            }
        }
        MessageContent::Parts(parts) => {
            let mut converted = Vec::new();
            for part in parts {
                match part {
                    ContentPart::Text { text } => {
                        if !text.is_empty() {
                            converted.push(Part::Text { text: text.clone(), thought_signature: None });
                        }
                    }
                    ContentPart::Image { data, mime_type, .. } => {
                        converted.push(Part::InlineData {
                            inline_data: InlineData { mime_type: mime_type.clone(), data: data.clone() },
                        });
                    }
                    ContentPart::File { filename, file_id, file_url, .. } => {
                        let fallback = filename
                            .clone()
                            .or_else(|| file_id.clone())
                            .or_else(|| file_url.clone())
                            .unwrap_or_else(|| "attached file".to_string());
                        converted.push(Part::Text {
                            text: format!("[File input not directly supported: {fallback}]"),
                            thought_signature: None,
                        });
                    }
                }
            }
            converted
        }
    }
}

fn build_interaction_content(content: &MessageContent) -> Vec<InteractionContent> {
    match content {
        MessageContent::Text(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![InteractionContent::Text { text: text.clone() }]
            }
        }
        MessageContent::Parts(parts) => {
            let mut converted = Vec::new();
            for part in parts {
                match part {
                    ContentPart::Text { text } => {
                        if !text.is_empty() {
                            converted.push(InteractionContent::Text { text: text.clone() });
                        }
                    }
                    ContentPart::Image { data, mime_type, .. } => {
                        converted.push(InteractionContent::Image { data: data.clone(), mime_type: mime_type.clone() })
                    }
                    ContentPart::File { filename, file_id, file_url, .. } => {
                        let fallback = filename
                            .clone()
                            .or_else(|| file_id.clone())
                            .or_else(|| file_url.clone())
                            .unwrap_or_else(|| "attached file".to_string());
                        converted.push(InteractionContent::Text {
                            text: {
                                let mut s = String::with_capacity(38 + fallback.len());
                                s.push_str("[File input not directly supported: ");
                                s.push_str(&fallback);
                                s.push(']');
                                s
                            },
                        });
                    }
                }
            }
            converted
        }
    }
}

fn build_message_parts(message: &Message, model: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    if message.role != MessageRole::Tool {
        parts.extend(parts_from_message_content(&message.content));
    }

    if message.role == MessageRole::Assistant
        && let Some(tool_calls) = &message.tool_calls
    {
        let is_gemini3 = model.contains("gemini-3");
        for tool_call in tool_calls {
            if let Some(ref func) = tool_call.function {
                let parsed_args = tool_call.parsed_arguments().unwrap_or_else(|_| json!({}));

                let thought_signature = if is_gemini3 && tool_call.thought_signature.is_none() {
                    tracing::trace!(
                        function_name = %func.name,
                        "Gemini 3: using skip_thought_signature_validator fallback"
                    );
                    Some("skip_thought_signature_validator".to_string())
                } else {
                    tool_call.thought_signature.clone()
                };

                parts.push(Part::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: func.name.clone(),
                        args: parsed_args,
                        id: Some(tool_call.id.clone()),
                    },
                    thought_signature,
                });
            }
        }
    }

    parts
}

fn preserved_gemini_parts_from_message(message: &Message) -> Option<Vec<Part>> {
    let details = message.reasoning_details.as_ref()?;
    for detail in details {
        let Some(text) = detail.as_str() else {
            continue;
        };
        let Some(payload) = text.strip_prefix(GEMINI_PRESERVED_PARTS_PREFIX) else {
            continue;
        };
        if let Ok(parts) = serde_json::from_str::<Vec<Part>>(payload) {
            return Some(parts);
        }
    }
    None
}

/// Reconcile preserved parts with the message's current tool calls.
///
/// `clear_old_tool_results` stubs tool-call inputs on `Message.tool_calls`
/// request-only, and preserved Gemini parts bypass `tool_calls` entirely:
/// without this sync the original arguments would ride the wire and
/// `clear_tool_inputs` would be a silent no-op on Gemini. Parts were
/// deserialized into these same tool calls at response time, so the part id
/// (falling back to the function name) is the faithful join. Thought
/// signatures stay on the part — Gemini requires them — and only arguments
/// that still parse as JSON are replaced.
fn sync_preserved_parts_with_tool_calls(parts: &mut [Part], message: &Message) {
    let Some(tool_calls) = message.tool_calls.as_ref() else {
        return;
    };
    if tool_calls.is_empty() {
        return;
    }
    let arguments_by_id: BTreeMap<&str, &str> = tool_calls
        .iter()
        .filter_map(|call| Some((call.id.as_str(), call.function.as_ref()?.arguments.as_str())))
        .collect();
    let arguments_by_name: BTreeMap<&str, &str> = tool_calls
        .iter()
        .filter_map(|call| {
            let function = call.function.as_ref()?;
            Some((function.name.as_str(), function.arguments.as_str()))
        })
        .collect();
    for part in parts.iter_mut() {
        let Part::FunctionCall { function_call, .. } = part else {
            continue;
        };
        let arguments = function_call
            .id
            .as_deref()
            .and_then(|id| arguments_by_id.get(id))
            .or_else(|| arguments_by_name.get(function_call.name.as_str()));
        let Some(arguments) = arguments else {
            continue;
        };
        if let Ok(parsed) = serde_json::from_str::<Value>(arguments)
            && function_call.args != parsed
        {
            function_call.args = parsed;
        }
    }
}

pub(crate) fn preserved_gemini_parts_detail(parts: &[Part]) -> Option<Vec<String>> {
    if !parts_require_roundtrip_history(parts) {
        return None;
    }

    serde_json::to_string(parts)
        .ok()
        .map(|serialized| vec![format!("{GEMINI_PRESERVED_PARTS_PREFIX}{serialized}")])
}

fn parts_require_roundtrip_history(parts: &[Part]) -> bool {
    parts.iter().any(|part| {
        part.thought_signature().is_some()
            || matches!(
                part,
                Part::ToolCall { .. }
                    | Part::ToolResponse { .. }
                    | Part::ExecutableCode { .. }
                    | Part::CodeExecutionResult { .. }
                    | Part::FunctionResponse { .. }
                    | Part::InlineData { .. }
            )
    })
}

fn gemini_built_in_tool(tool: &ToolDefinition) -> Option<Tool> {
    match tool.tool_type.as_str() {
        "web_search" | "google_search" => Some(Tool {
            google_search: Some(tool.web_search.clone().unwrap_or_else(|| json!({}))),
            ..Tool::default()
        }),
        "google_maps" => Some(Tool {
            google_maps: Some(tool.hosted_tool_config.clone().unwrap_or_else(|| json!({}))),
            ..Tool::default()
        }),
        "url_context" => Some(Tool {
            url_context: Some(tool.hosted_tool_config.clone().unwrap_or_else(|| json!({}))),
            ..Tool::default()
        }),
        "file_search" => Some(Tool {
            file_search: Some(tool.hosted_tool_config.clone().unwrap_or_else(|| json!({}))),
            ..Tool::default()
        }),
        "code_execution" => Some(Tool {
            code_execution: Some(tool.hosted_tool_config.clone().unwrap_or_else(|| json!({}))),
            ..Tool::default()
        }),
        other if other.starts_with("code_execution_") => {
            Some(Tool { code_execution: Some(json!({})), ..Tool::default() })
        }
        _ => None,
    }
}

fn gemini_interaction_built_in_tool(tool: &ToolDefinition) -> Result<Option<InteractionTool>, LLMError> {
    let (tool_type, config) = match tool.tool_type.as_str() {
        "web_search" | "google_search" => ("google_search", tool.web_search.as_ref()),
        "google_maps" => ("google_maps", tool.hosted_tool_config.as_ref()),
        "url_context" => ("url_context", tool.hosted_tool_config.as_ref()),
        "file_search" => ("file_search", tool.hosted_tool_config.as_ref()),
        "code_execution" => ("code_execution", tool.hosted_tool_config.as_ref()),
        other if other.starts_with("code_execution_") => ("code_execution", None),
        _ => return Ok(None),
    };

    if let Some(config) = config.and_then(Value::as_object)
        && let Some(key) = ["type", "name", "description", "parameters"]
            .into_iter()
            .find(|key| config.contains_key(*key))
    {
        return Err(LLMError::InvalidRequest {
            message: format!("Gemini built-in tool extension key '{key}' collides with a reserved wire field"),
            metadata: None,
        });
    }

    Ok(Some(InteractionTool::built_in(tool_type, config)))
}

pub(crate) fn collect_gemini_tool_spec(definitions: Option<&[ToolDefinition]>) -> Result<GeminiToolSpec, LLMError> {
    let Some(definitions) = definitions else {
        return Ok(GeminiToolSpec {
            generate_tools: None,
            interaction_tools: None,
            uses_server_side_tools: false,
            has_function_tools: false,
        });
    };

    let mut generate_tools = Vec::new();
    let mut interaction_tools = Vec::new();
    let mut function_declarations = Vec::new();
    let mut seen = hashbrown::HashSet::new();
    let mut uses_server_side_tools = false;
    let mut has_function_tools = false;

    for tool in definitions {
        if let Some(built_in_tool) = gemini_built_in_tool(tool) {
            uses_server_side_tools = true;
            generate_tools.push(built_in_tool);
        }
        if let Some(interaction_tool) = gemini_interaction_built_in_tool(tool)? {
            interaction_tools.push(interaction_tool);
        }

        let Some(func) = tool.function.as_ref() else {
            continue;
        };
        has_function_tools = true;
        let name = func.name.clone();
        if !seen.insert(name.clone()) {
            continue;
        }

        let description = func.description.clone();
        let parameters = sanitize_function_parameters(func.parameters.clone());
        function_declarations.push(FunctionDeclaration {
            name: name.clone(),
            description: description.clone(),
            parameters: parameters.clone(),
        });
        interaction_tools.push(InteractionTool::function(name, description, parameters));
    }

    if !function_declarations.is_empty() {
        generate_tools.push(Tool {
            function_declarations: Some(function_declarations),
            ..Tool::default()
        });
    }

    Ok(GeminiToolSpec {
        generate_tools: (!generate_tools.is_empty()).then_some(generate_tools),
        interaction_tools: (!interaction_tools.is_empty()).then_some(interaction_tools),
        uses_server_side_tools,
        has_function_tools,
    })
}

fn build_generation_config(provider: &GeminiProvider, request: &LLMRequest) -> GenerationConfig {
    let uses_latest = GeminiProvider::uses_latest_gemini_api(&request.model);
    let mut generation_config = GenerationConfig {
        max_output_tokens: request.max_tokens,
        temperature: if uses_latest { None } else { request.temperature },
        top_p: if uses_latest { None } else { request.top_p },
        top_k: if uses_latest { None } else { request.top_k },
        presence_penalty: request.presence_penalty,
        frequency_penalty: request.frequency_penalty,
        stop_sequences: request.stop_sequences.clone(),
        ..Default::default()
    };

    if let Some(format) = &request.output_format {
        generation_config.response_mime_type = Some("application/json".to_string());
        if format.is_object() {
            generation_config.response_schema = Some(format.clone());
        }
    }

    if let Some(effort) = request.reasoning_effort
        && provider.supports_reasoning_effort(&request.model)
    {
        let is_gemini3_flash_preview = request.model.contains("gemini-3-flash-preview");
        let is_gemini3_flash = request.model.contains("gemini-3") && request.model.contains("flash");
        let is_gemini4_argon = request.model.contains("gemini-4-argon");
        let thinking_level = match effort {
            ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown => Some("low"),
            ReasoningEffortLevel::Minimal => {
                if is_gemini3_flash_preview {
                    Some("minimal")
                } else {
                    // Gemini 3.8/3.7 Flash does not support `minimal` — fall back to `low` to avoid API error
                    Some("low")
                }
            }
            ReasoningEffortLevel::Low => Some("low"),
            ReasoningEffortLevel::Medium => {
                if is_gemini3_flash || is_gemini4_argon {
                    // Gemini 3 Flash and Gemini 4 Argon natively support medium
                    Some("medium")
                } else {
                    Some("high")
                }
            }
            ReasoningEffortLevel::High | ReasoningEffortLevel::XHigh | ReasoningEffortLevel::Max => Some("high"),
        };

        if let Some(level) = thinking_level {
            generation_config.thinking_config = Some(ThinkingConfig { thinking_level: Some(level.to_string()) });
        }
    }

    generation_config
}

fn build_interaction_tool_choice(
    tool_choice: Option<&ToolChoice>,
    has_function_tools: bool,
    uses_server_side_tools: bool,
) -> Option<InteractionToolChoice> {
    if !has_function_tools {
        return None;
    }

    let mut choice = match tool_choice {
        Some(ToolChoice::None) => InteractionToolChoice::new("none"),
        Some(ToolChoice::Any) => InteractionToolChoice::new("any"),
        Some(ToolChoice::Specific(spec)) => {
            let mut choice = InteractionToolChoice::new("validated");
            if spec.tool_type == "function" {
                choice.tools = Some(vec![spec.function.name.clone()]);
            }
            choice
        }
        _ => {
            if uses_server_side_tools {
                InteractionToolChoice::new("validated")
            } else {
                InteractionToolChoice::new("auto")
            }
        }
    };

    if choice.tools.as_ref().is_some_and(|tools| tools.is_empty()) {
        choice.tools = None;
    }

    Some(choice)
}

fn build_interaction_input(request: &LLMRequest) -> Result<InteractionInput, LLMError> {
    // Only materialize an owned message vector when a continuation requires the
    // delta slice. In the common (non-continuation) case, borrow the original
    // `Arc<Vec<Message>>` slice directly and skip the deep copy.
    let delta_messages: Vec<Message>;
    let relevant_messages: &[Message] = if request.previous_response_id.is_some() {
        delta_messages = interaction_delta_messages(&request.messages);
        &delta_messages
    } else {
        request.messages.as_ref().as_slice()
    };
    let mut turns = build_interaction_turns(relevant_messages, &request.messages)?;

    // Latest Gemini models reject prefilled model turns (last turn with role "model").
    // A trailing FunctionCall turn is an in-flight tool call, not a prefill:
    // preserve it (and its signatures) rather than silently dropping it.
    let trailing_prefill = turns.last().is_some_and(|turn| {
        turn.role == "model"
            && match &turn.content {
                InteractionTurnContent::Text(_) => true,
                InteractionTurnContent::Content(parts) => {
                    !parts.iter().any(|part| matches!(part, InteractionContent::FunctionCall { .. }))
                }
            }
    });
    if GeminiProvider::uses_latest_gemini_api(&request.model) && trailing_prefill {
        turns.pop();
    }

    if request.previous_response_id.is_none() {
        if let [turn] = turns.as_slice()
            && turn.role == "user"
        {
            return Ok(match &turn.content {
                InteractionTurnContent::Text(text) => InteractionInput::Text(text.clone()),
                InteractionTurnContent::Content(content) => InteractionInput::Content(content.clone()),
            });
        }
        return Ok(InteractionInput::Turns(turns));
    }

    if let [turn] = turns.as_slice()
        && turn.role == "user"
    {
        return Ok(match &turn.content {
            InteractionTurnContent::Text(text) => InteractionInput::Text(text.clone()),
            InteractionTurnContent::Content(content) => InteractionInput::Content(content.clone()),
        });
    }

    Ok(InteractionInput::Turns(turns))
}

fn interaction_delta_messages(messages: &[Message]) -> Vec<Message> {
    let start = messages
        .iter()
        .rposition(|message| message.role == MessageRole::Assistant)
        .map_or(0, |index| index.saturating_add(1));
    messages[start..].to_vec()
}

fn build_interaction_turns(messages: &[Message], full_messages: &[Message]) -> Result<Vec<InteractionTurn>, LLMError> {
    let mut call_map: HashMap<String, String> = HashMap::with_capacity(full_messages.len());
    for message in full_messages {
        if message.role == MessageRole::Assistant
            && let Some(tool_calls) = &message.tool_calls
        {
            for tool_call in tool_calls {
                if let Some(func) = &tool_call.function {
                    call_map.insert(tool_call.id.clone(), func.name.clone());
                }
            }
        }
    }

    let mut turns = Vec::new();
    for message in messages {
        if message.role == MessageRole::System {
            continue;
        }

        let mut content = if message.role == MessageRole::Tool {
            Vec::new()
        } else {
            build_interaction_content(&message.content)
        };
        if message.role == MessageRole::Assistant
            && let Some(tool_calls) = &message.tool_calls
        {
            for tool_call in tool_calls {
                if let Some(func) = &tool_call.function {
                    content.push(InteractionContent::FunctionCall {
                        id: tool_call.id.clone(),
                        name: func.name.clone(),
                        arguments: tool_call.parsed_arguments().unwrap_or(Value::Null),
                        signature: tool_call.thought_signature.clone(),
                    });
                }
            }
        }
        if message.role == MessageRole::Tool {
            let tool_call_id = message.tool_call_id.clone().ok_or_else(|| LLMError::InvalidRequest {
                message: "Gemini interactions require tool_call_id for tool messages".to_string(),
                metadata: None,
            })?;
            content.push(InteractionContent::FunctionResult {
                call_id: tool_call_id.clone(),
                name: call_map.get(&tool_call_id).cloned(),
                result: interaction_result_from_message_content(&message.content),
                is_error: None,
                signature: None,
            });
        }
        if content.is_empty() {
            continue;
        }

        let role = if message.role == MessageRole::Assistant {
            "model"
        } else {
            "user"
        };
        let content = match content.as_slice() {
            [InteractionContent::Text { text }] => InteractionTurnContent::Text(text.clone()),
            _ => InteractionTurnContent::Content(content),
        };
        turns.push(InteractionTurn { role: role.to_string(), content });
    }

    Ok(turns)
}

fn interaction_result_from_message_content(content: &MessageContent) -> InteractionResult {
    match content {
        MessageContent::Text(text) => interaction_result_from_text(text),
        MessageContent::Parts(_) => {
            let parts = build_interaction_content(content);
            if let [InteractionContent::Text { text }] = parts.as_slice() {
                interaction_result_from_text(text)
            } else {
                InteractionResult::Content(parts)
            }
        }
    }
}

fn interaction_result_from_text(text: &str) -> InteractionResult {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return InteractionResult::String(String::new());
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(content) = interaction_result_content_array(&value) {
            return InteractionResult::Content(content);
        }
        if value.is_object() {
            return InteractionResult::Json(value);
        }
    }

    InteractionResult::String(text.to_string())
}

fn interaction_result_content_array(value: &Value) -> Option<Vec<InteractionContent>> {
    let items = value.as_array()?;
    let mut content = Vec::with_capacity(items.len());
    for item in items {
        let item_type = item.get("type")?.as_str()?;
        match item_type {
            "text" => content.push(InteractionContent::Text { text: item.get("text")?.as_str()?.to_string() }),
            "image" => {
                let mime_type = item.get("mime_type")?.as_str()?.to_string();
                let data = item.get("data")?.as_str()?.to_string();
                content.push(InteractionContent::Image { data, mime_type });
            }
            _ => return None,
        }
    }

    Some(content)
}

#[cfg(test)]
mod preserved_parts_sync_tests {
    use super::*;

    const CLEARED_ARGS: &str = "{\"cleared\":\"tool_input\"}";

    fn function_part(id: Option<&str>, name: &str, args: Value) -> Part {
        Part::FunctionCall {
            function_call: GeminiFunctionCall {
                id: id.map(str::to_string),
                name: name.to_string(),
                args,
            },
            thought_signature: Some("sig".to_string()),
        }
    }

    fn tool_call(id: &str, name: &str, arguments: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            call_type: "function".to_string(),
            function: Some(FunctionCall {
                namespace: None,
                name: name.to_string(),
                arguments: arguments.to_string(),
            }),
            text: None,
            thought_signature: None,
        }
    }

    fn assistant_message(tool_calls: Option<Vec<ToolCall>>) -> Message {
        Message {
            tool_calls,
            ..Message::assistant("working".to_string())
        }
    }

    fn part_args(part: &Part) -> Option<&Value> {
        match part {
            Part::FunctionCall { function_call, .. } => Some(&function_call.args),
            _ => None,
        }
    }

    fn part_signature(part: &Part) -> Option<&str> {
        match part {
            Part::FunctionCall { thought_signature, .. } => thought_signature.as_deref(),
            _ => None,
        }
    }

    #[test]
    fn sync_replaces_arguments_by_id_then_name() {
        let mut parts = vec![
            function_part(Some("call_1"), "read_file", json!({"path": "big.rs"})),
            function_part(None, "grep_file", json!({"pattern": "x"})),
        ];
        let message = assistant_message(Some(vec![
            tool_call("call_1", "read_file", CLEARED_ARGS),
            tool_call("call_2", "grep_file", CLEARED_ARGS),
        ]));

        sync_preserved_parts_with_tool_calls(&mut parts, &message);

        assert_eq!(part_args(&parts[0]), Some(&json!({"cleared": "tool_input"})));
        assert_eq!(part_args(&parts[1]), Some(&json!({"cleared": "tool_input"})));
        // Thought signatures stay on the part: Gemini requires them.
        assert_eq!(part_signature(&parts[0]), Some("sig"));
    }

    #[test]
    fn sync_leaves_unmatched_parts_untouched() {
        let original_args = json!({"path": "keep.rs"});
        let mut parts = vec![function_part(Some("call_other"), "write_file", original_args.clone())];
        let message = assistant_message(Some(vec![tool_call("call_1", "read_file", CLEARED_ARGS)]));

        sync_preserved_parts_with_tool_calls(&mut parts, &message);

        assert_eq!(part_args(&parts[0]), Some(&original_args));
    }

    #[test]
    fn sync_keeps_original_when_tool_call_arguments_unparseable() {
        let original_args = json!({"path": "big.rs"});
        let mut parts = vec![function_part(Some("call_1"), "read_file", original_args.clone())];
        let message = assistant_message(Some(vec![tool_call("call_1", "read_file", "not-json")]));

        sync_preserved_parts_with_tool_calls(&mut parts, &message);

        assert_eq!(part_args(&parts[0]), Some(&original_args));
    }

    #[test]
    fn sync_ignores_messages_without_tool_calls() {
        let original_args = json!({"path": "big.rs"});
        let mut parts = vec![function_part(Some("call_1"), "read_file", original_args.clone())];
        let message = assistant_message(None);

        sync_preserved_parts_with_tool_calls(&mut parts, &message);

        assert_eq!(part_args(&parts[0]), Some(&original_args));
    }
}
