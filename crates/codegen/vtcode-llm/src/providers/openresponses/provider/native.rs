//! Native Responses request serialization and completed-response parsing.

use super::OpenResponsesProvider;
use crate::provider::{FinishReason, LLMError, LLMRequest, LLMResponse, ToolCall};
use crate::providers::common::append_normalized_reasoning_detail_items;
use crate::providers::shared::{
    collect_tool_references_from_tool_search_output, function_output_value_from_message_content,
};
use anyhow::Result;
use serde_json::{Value, json};

impl OpenResponsesProvider {
    pub(super) fn parse_native_response_payload(json: Value, model: String) -> Result<LLMResponse, LLMError> {
        let output = json
            .get("output")
            .and_then(|o| o.as_array())
            .ok_or_else(|| LLMError::Provider {
                message: "Invalid response from OpenResponses: missing output".to_string(),
                metadata: None,
            })?;

        let mut content = String::new();
        let mut tool_calls = Vec::new();
        let mut reasoning = None;
        let mut tool_references = Vec::new();
        let mut replay_items = Vec::new();

        for item_val in output {
            let item_type = item_val.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match item_type {
                "message" => {
                    if let Some(content_parts) = item_val.get("content").and_then(|c| c.as_array()) {
                        for part in content_parts {
                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                content.push_str(text);
                            }
                        }
                    }
                }
                "reasoning" => {
                    replay_items.push(item_val.clone());
                    if let Some(text) = item_val.get("content").and_then(|t| t.as_str()) {
                        reasoning = Some(text.to_string());
                    }
                }
                "function_call" => {
                    let id = item_val.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let name = item_val.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let arguments = item_val
                        .get("arguments")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "{}".to_string());
                    let namespace = item_val.get("namespace").and_then(|v| v.as_str()).map(ToOwned::to_owned);
                    tool_calls.push(ToolCall::function_with_namespace(id, namespace, name, arguments));
                }
                "tool_search_output" => {
                    collect_tool_references_from_tool_search_output(item_val, &mut tool_references);
                }
                _ if !item_type.is_empty() => replay_items.push(item_val.clone()),
                _ => {}
            }
        }

        let mut reasoning_details = if replay_items.is_empty() {
            None
        } else {
            Some(replay_items.into_iter().map(|item| item.to_string()).collect())
        };
        let (final_reasoning, final_content) = if reasoning.is_none() && !content.is_empty() {
            let (reasoning_parts, cleaned_content) = crate::utils::extract_reasoning_content(&content);
            if reasoning_parts.is_empty() {
                (None, Some(content))
            } else {
                crate::providers::common::preserve_interleaved_content_in_reasoning_details(
                    &mut reasoning_details,
                    &content,
                );
                (Some(reasoning_parts.join("\n\n")), cleaned_content.or(Some(content)))
            }
        } else {
            (reasoning, Some(content))
        };

        let finish_reason = match json.get("status").and_then(|s| s.as_str()) {
            Some("completed") => FinishReason::Stop,
            Some("incomplete") => FinishReason::Length,
            _ => FinishReason::Stop,
        };

        Ok(LLMResponse {
            content: final_content.filter(|c| !c.is_empty()),
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            model,
            usage: None,
            finish_reason,
            reasoning: final_reasoning,
            reasoning_details,
            tool_references,
            request_id: json.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()),
            organization_id: None,
            compaction: None,
        })
    }

    fn output_item_to_value(item: crate::open_responses::OutputItem) -> Result<Value, LLMError> {
        serde_json::to_value(item).map_err(|e| LLMError::Provider {
            message: format!("Failed to serialize Open Responses input item: {e}"),
            metadata: None,
        })
    }

    pub(super) fn build_native_payload(&self, request: &LLMRequest, stream: bool) -> Result<Value, LLMError> {
        use crate::open_responses::{
            ContentPart, ImageDetail, InputFileContent, InputImageContent, MessageRole, OutputItem, Request,
        };

        let mut input: Vec<Value> = Vec::new();

        if let Some(system) = &request.system_prompt {
            input.push(Self::output_item_to_value(OutputItem::completed_message(
                "msg_system",
                MessageRole::System,
                vec![ContentPart::input_text(system.as_ref())],
            ))?);
        }

        for (i, message) in request.messages.iter().enumerate() {
            if let Some(reasoning_details) = &message.reasoning_details {
                append_normalized_reasoning_detail_items(&mut input, reasoning_details);
            }

            let role = match message.role.as_generic_str() {
                "user" => Some(MessageRole::User),
                "assistant" => Some(MessageRole::Assistant),
                "system" => Some(MessageRole::System),
                // Tool responses are represented by function_call_output items below.
                "tool" => None,
                _ => Some(MessageRole::User),
            };

            if let Some(role) = role {
                let id = format!("msg_{i}");
                let mut content = Vec::new();
                match &message.content {
                    crate::provider::MessageContent::Text(text) => {
                        if !text.trim().is_empty() {
                            content.push(ContentPart::input_text(text.as_str()));
                        }
                    }
                    crate::provider::MessageContent::Parts(parts) => {
                        for part in parts {
                            match part {
                                crate::provider::ContentPart::Text { text } => {
                                    if !text.trim().is_empty() {
                                        content.push(ContentPart::input_text(text.as_str()));
                                    }
                                }
                                crate::provider::ContentPart::Image { data, mime_type, .. } => {
                                    // Providers accept only JPEG/PNG/GIF/WebP. Anything else
                                    // (notably SVG auto-attached from quoted paths in diffs)
                                    // fails the whole request with 400, so drop it here
                                    // instead of letting one bad part poison the turn.
                                    if !vtcode_commons::image::is_supported_image_mime_type(mime_type) {
                                        tracing::warn!(
                                            mime_type = %mime_type,
                                            "dropping unsupported image MIME type for Responses API"
                                        );
                                        continue;
                                    }
                                    content.push(ContentPart::InputImage(InputImageContent {
                                        image_url: format!("data:{mime_type};base64,{data}"),
                                        detail: Some(ImageDetail::Auto),
                                    }));
                                }
                                crate::provider::ContentPart::File {
                                    filename, file_id, file_data, file_url, ..
                                } => {
                                    content.push(ContentPart::InputFile(InputFileContent {
                                        filename: filename.clone(),
                                        file_id: file_id.clone(),
                                        file_data: file_data.clone(),
                                        file_url: file_url.clone(),
                                    }));
                                }
                            }
                        }
                    }
                }
                if content.is_empty() {
                    let content_text = message.content.as_text();
                    if !content_text.trim().is_empty() {
                        content.push(ContentPart::input_text(content_text.to_string()));
                    }
                }
                if !content.is_empty() {
                    input.push(Self::output_item_to_value(OutputItem::completed_message(id, role, content))?);
                }
            }

            // Handle tool calls and outputs if present in message history
            if let Some(tool_calls) = &message.tool_calls {
                for (j, tc) in tool_calls.iter().enumerate() {
                    if let Some(f) = &tc.function {
                        input.push(Self::output_item_to_value(OutputItem::function_call(
                            format!("fc_{i}_{j}"),
                            &f.name,
                            tc.parsed_arguments().unwrap_or(Value::Null),
                        ))?);
                    }
                }
            }

            if let Some(tool_call_id) = &message.tool_call_id {
                // If this message is a tool output, add it as FunctionCallOutput
                input.push(json!({
                    "type": "function_call_output",
                    "id": format!("fco_{i}"),
                    "status": "completed",
                    "call_id": tool_call_id,
                    "output": function_output_value_from_message_content(&message.content),
                }));
            }
        }

        let mut req = Request::new(&request.model, Vec::new());
        req.stream = stream;
        req.temperature = request.temperature.map(crate::providers::common::sampling_param_f64);
        req.max_output_tokens = request.max_tokens.map(|t| t as u64);
        req.previous_response_id = request
            .previous_response_id
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        req.store = request.response_store;
        req.include = request.responses_include.as_ref().and_then(|fields| {
            let values: Vec<String> = fields
                .iter()
                .map(|field| field.trim())
                .filter(|field| !field.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            if values.is_empty() { None } else { Some(values) }
        });

        if let Some(tools) = &request.tools {
            req.tools = Some(tools.as_ref().clone());
        }

        let mut payload = serde_json::to_value(req).map_err(|e| LLMError::Provider {
            message: format!("Failed to serialize Open Responses request: {e}"),
            metadata: None,
        })?;
        if let Some(map) = payload.as_object_mut() {
            map.insert("input".to_string(), Value::Array(input));
        }

        if let Some(context_management) = &request.context_management
            && let Some(map) = payload.as_object_mut()
        {
            map.insert("context_management".to_string(), context_management.clone());
        }

        Ok(payload)
    }
}
