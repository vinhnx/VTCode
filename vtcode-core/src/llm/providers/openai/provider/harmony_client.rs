use super::super::errors::format_openai_error;
use super::super::harmony;
use super::super::headers;
use super::OpenAIProvider;
use crate::config::types::ReasoningEffortLevel;
use crate::llm::error_display;
use crate::llm::provider;
use openai_harmony::chat::{
    Author as HarmonyAuthor, Content as HarmonyContent, Conversation, DeveloperContent,
    Message as HarmonyMessage, ReasoningEffort, Role as HarmonyRole, SystemContent,
    ToolDescription,
};
use openai_harmony::{HarmonyEncodingName, load_harmony_encoding};
use serde_json::{Value, json};
use std::collections::HashMap;
use tokio::task::spawn_blocking;

impl OpenAIProvider {
    fn convert_to_harmony_conversation(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<Conversation, provider::LLMError> {
        let mut harmony_messages = Vec::with_capacity(request.messages.len() + 4); // +4 for system, developer, and potential splits
        let mut tool_call_authors: HashMap<String, String> = HashMap::with_capacity(16);

        // 1. Add standard system message as per Harmony spec
        let current_date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let reasoning_effort = match request.reasoning_effort {
            Some(ReasoningEffortLevel::Low) => ReasoningEffort::Low,
            Some(ReasoningEffortLevel::Medium) => ReasoningEffort::Medium,
            Some(ReasoningEffortLevel::High) => ReasoningEffort::High,
            _ => ReasoningEffort::Medium,
        };

        let system_content = SystemContent::new()
            .with_conversation_start_date(&current_date)
            .with_reasoning_effort(reasoning_effort);

        // Note: The identity and valid channels are typically handled by the SystemContent renderer
        // in openai-harmony, but we can also add them to instructions if needed.

        harmony_messages.push(HarmonyMessage::from_role_and_content(
            HarmonyRole::System,
            system_content,
        ));

        // 2. Add developer message (instructions + tools)
        let mut developer_content = DeveloperContent::new();
        if let Some(system_prompt) = &request.system_prompt {
            developer_content = developer_content.with_instructions(system_prompt.as_str());
        }

        if let Some(tools) = &request.tools {
            let tool_descriptions: Vec<ToolDescription> = tools
                .iter()
                .filter_map(|tool| {
                    if tool.tool_type != "function" {
                        return None;
                    }
                    let func = tool.function.as_ref()?;
                    Some(ToolDescription::new(
                        &func.name,
                        &func.description,
                        Some(func.parameters.clone()),
                    ))
                })
                .collect();

            if !tool_descriptions.is_empty() {
                developer_content = developer_content.with_function_tools(tool_descriptions);
            }
        }

        harmony_messages.push(HarmonyMessage::from_role_and_content(
            HarmonyRole::Developer,
            developer_content,
        ));

        // Convert messages
        for (i, msg) in request.messages.iter().enumerate() {
            match msg.role {
                provider::MessageRole::System => {
                    // Additional system messages (rare in vtcode)
                    harmony_messages.push(HarmonyMessage::from_role_and_content(
                        HarmonyRole::System,
                        msg.content.as_text(),
                    ));
                }
                provider::MessageRole::User => {
                    harmony_messages.push(HarmonyMessage::from_role_and_content(
                        HarmonyRole::User,
                        msg.content.as_text(),
                    ));
                }
                provider::MessageRole::Assistant => {
                    let has_final = !msg.content.as_text().is_empty();
                    let is_last = i == request.messages.len() - 1;

                    // Spec: Drop CoT (analysis) if the response ended in a 'final' message,
                    // as it's no longer needed for subsequent turns.
                    // Keep it if there are tool calls (as they are part of the CoT flow)
                    // or if it's the last message and has no final content yet.
                    let should_keep_analysis = msg.tool_calls.is_some() || (is_last && !has_final);

                    // 1. Handle reasoning (analysis channel)
                    if let Some(reasoning) = &msg.reasoning {
                        if should_keep_analysis {
                            harmony_messages.push(
                                HarmonyMessage::from_role_and_content(
                                    HarmonyRole::Assistant,
                                    reasoning.clone(),
                                )
                                .with_channel("analysis"),
                            );
                        }
                    }

                    // 2. Handle tool calls (commentary channel)
                    if let Some(tool_calls) = &msg.tool_calls {
                        for call in tool_calls {
                            if let Some(ref func) = call.function {
                                let recipient = format!("functions.{}", func.name);
                                tool_call_authors.insert(call.id.clone(), recipient.clone());

                                harmony_messages.push(
                                    HarmonyMessage::from_role_and_content(
                                        HarmonyRole::Assistant,
                                        func.arguments.clone(),
                                    )
                                    .with_channel("commentary")
                                    .with_recipient(&recipient)
                                    .with_content_type("<|constrain|> json"),
                                );
                            }
                        }
                    } else {
                        // 3. Handle final content (final channel)
                        let text = msg.content.as_text();
                        if !text.is_empty() {
                            harmony_messages.push(
                                HarmonyMessage::from_role_and_content(HarmonyRole::Assistant, text)
                                    .with_channel("final"),
                            );
                        }
                    }
                }
                provider::MessageRole::Tool => {
                    let author_name = msg
                        .tool_call_id
                        .as_ref()
                        .and_then(|id| tool_call_authors.get(id))
                        .cloned()
                        .or_else(|| msg.tool_call_id.clone());

                    let author = author_name
                        .map(|name| HarmonyAuthor::new(HarmonyRole::Tool, name))
                        .unwrap_or_else(|| HarmonyAuthor::from(HarmonyRole::Tool));

                    harmony_messages.push(
                        HarmonyMessage::from_author_and_content(author, msg.content.as_text())
                            .with_channel("commentary")
                            .with_recipient("assistant"),
                    );
                }
            }
        }

        Ok(Conversation::from_messages(harmony_messages))
    }

    pub(super) async fn generate_with_harmony(
        &self,
        request: provider::LLMRequest,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        // Load harmony encoding off the async runtime to avoid blocking drop panics
        let encoding = spawn_blocking(|| load_harmony_encoding(HarmonyEncodingName::HarmonyGptOss))
            .await
            .map_err(|join_err| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!("Failed to load harmony encoding (task join): {}", join_err),
                );
                provider::LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                }
            })?
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!("Failed to load harmony encoding: {}", e),
                );
                provider::LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        // Convert to harmony conversation
        let conversation = self.convert_to_harmony_conversation(&request)?;

        // Render conversation for completion
        let prompt_tokens = encoding
            .render_conversation_for_completion(&conversation, HarmonyRole::Assistant, None)
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!("Failed to render conversation: {}", e),
                );
                provider::LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        // Send tokens to inference server
        let completion_tokens = self
            .send_harmony_tokens_to_inference_server(&prompt_tokens, request.temperature)
            .await?;

        // Parse completion tokens back into messages
        let parsed_messages = encoding
            .parse_messages_from_completion_tokens(
                completion_tokens.clone(),
                Some(HarmonyRole::Assistant),
            )
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!("Failed to parse completion tokens: {}", e),
                );
                provider::LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        // Extract content from parsed messages
        let mut content = None;
        let mut tool_calls = Vec::with_capacity(8); // Typical tool call count in harmony responses

        let extract_text_content = |parts: &[HarmonyContent]| -> Option<String> {
            let text = parts
                .iter()
                .filter_map(|part| match part {
                    HarmonyContent::Text(text_part) => Some(text_part.text.clone()),
                    _ => None,
                })
                .collect::<String>();

            if text.is_empty() { None } else { Some(text) }
        };

        let normalize_json_arguments = |raw: String| -> String {
            match serde_json::from_str::<Value>(&raw) {
                Ok(parsed) => parsed.to_string(),
                Err(_) => raw,
            }
        };

        for message in parsed_messages {
            match message.author.role {
                HarmonyRole::Assistant => {
                    if let Some(channel) = &message.channel {
                        match channel.as_str() {
                            "final" => {
                                // This is the final response content
                                // Extract text from content Vec<Content>
                                if let Some(text_content) = extract_text_content(&message.content) {
                                    content = Some(text_content);
                                }
                            }
                            "commentary" => {
                                // Check if this is a tool call
                                if let Some(recipient) = &message.recipient {
                                    if recipient.starts_with("functions.") {
                                        // This is a tool call with functions. prefix
                                        let function_name = recipient
                                            .strip_prefix("functions.")
                                            .unwrap_or(recipient);
                                        let arguments = extract_text_content(&message.content)
                                            .map(normalize_json_arguments)
                                            .unwrap_or_else(|| "{}".to_owned());

                                        tool_calls.push(provider::ToolCall::function(
                                            format!("call_{}", tool_calls.len()),
                                            function_name.to_string(),
                                            arguments,
                                        ));
                                    } else {
                                        // Check if this is a harmony format tool call (to=tool_name)
                                        // The recipient might be the tool name directly
                                        let tool_name = Self::parse_harmony_tool_name(recipient);
                                        if !tool_name.is_empty() {
                                            let arguments = extract_text_content(&message.content)
                                                .map(normalize_json_arguments)
                                                .unwrap_or_else(|| "{}".to_string());

                                            tool_calls.push(provider::ToolCall::function(
                                                format!("call_{}", tool_calls.len()),
                                                tool_name,
                                                arguments,
                                            ));
                                        }
                                    }
                                } else {
                                    // Check if the content itself contains harmony tool call format
                                    if let Some(text_content) =
                                        extract_text_content(&message.content)
                                    {
                                        if let Some((tool_name, args)) =
                                            Self::parse_harmony_tool_call_from_text(&text_content)
                                        {
                                            let arguments = serde_json::to_string(&args)
                                                .unwrap_or_else(|_| "{}".to_string());

                                            tool_calls.push(provider::ToolCall::function(
                                                format!("call_{}", tool_calls.len()),
                                                tool_name,
                                                arguments,
                                            ));
                                        }
                                    }
                                }
                            }
                            _ => {} // Other channels like "analysis" are for reasoning
                        }
                    }
                }
                _ => {} // Skip other message types for now
            }
        }

        let tool_calls = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

        Ok(provider::LLMResponse {
            content,
            model: request.model.clone(),
            tool_calls,
            usage: Some(crate::llm::provider::Usage {
                prompt_tokens: prompt_tokens.len().try_into().unwrap_or(u32::MAX),
                completion_tokens: completion_tokens.len().try_into().unwrap_or(u32::MAX),
                total_tokens: (prompt_tokens.len() + completion_tokens.len())
                    .try_into()
                    .unwrap_or(u32::MAX),
                cached_prompt_tokens: None,
                cache_creation_tokens: None,
                cache_read_tokens: None,
            }),
            finish_reason: crate::llm::provider::FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        })
    }

    /// Sends harmony-formatted tokens to an inference server for GPT-OSS models.
    ///
    /// This method handles the HTTP communication with inference servers that support
    /// harmony-formatted token inputs (such as vLLM or Transformers serve).
    ///
    /// # Configuration
    ///
    /// Set the `HARMONY_INFERENCE_SERVER_URL` environment variable to configure
    /// the inference server endpoint. Defaults to `http://localhost:8000` for local vLLM.
    ///
    /// # Supported Servers
    ///
    /// - **vLLM**: Set `HARMONY_INFERENCE_SERVER_URL=http://localhost:8000`
    /// - **Transformers serve**: Configure appropriate endpoint URL
    /// - **Custom servers**: Any server accepting `{"prompt_token_ids": [...], "max_tokens": N, ...}`
    ///
    /// # Example
    ///
    /// ```bash
    /// export HARMONY_INFERENCE_SERVER_URL=http://localhost:8000
    /// vtcode ask --model openai/gpt-oss-20b "Explain quantum computing"
    /// ```
    async fn send_harmony_tokens_to_inference_server(
        &self,
        tokens: &[u32],
        temperature: Option<f32>,
    ) -> Result<Vec<u32>, provider::LLMError> {
        // Get harmony inference server URL from environment variable
        // Default to localhost vLLM server if not configured
        let server_url = std::env::var("HARMONY_INFERENCE_SERVER_URL")
            .unwrap_or_else(|_| "http://localhost:8000".to_owned());

        // Load harmony encoding to get stop tokens
        let encoding = load_harmony_encoding(HarmonyEncodingName::HarmonyGptOss).map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Failed to load harmony encoding for stop tokens: {}", e),
            );
            provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

        let stop_token_ids = encoding.stop_tokens_for_assistant_actions().map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Failed to get stop tokens: {}", e),
            );
            provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

        // Convert HashSet to Vec for JSON serialization
        let stop_token_ids_vec: Vec<u32> = stop_token_ids.into_iter().collect();

        // Prepare request body for vLLM-style inference server
        let request_body = json!({
            "prompt_token_ids": tokens,
            "temperature": temperature.unwrap_or(0.7),
            "stop_token_ids": stop_token_ids_vec,
            // Additional parameters that might be needed
            "stream": false,
            "logprobs": null,
            "echo": false
        });

        // Send HTTP request to inference server
        let response = headers::apply_json_content_type(
            self.http_client.post(format!("{}/generate", server_url)),
        )
        .json(&request_body)
        .send()
        .await
        .map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!(
                    "Failed to send request to harmony inference server at {}: {}",
                    server_url, e
                ),
            );
            provider::LLMError::Network {
                message: formatted_error,
                metadata: None,
            }
        })?;

        // Check response status
        if !response.status().is_success() {
            let status = response.status();
            let headers = response.headers().clone();
            let error_text = response.text().await.unwrap_or_default();
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format_openai_error(
                    status,
                    &error_text,
                    &headers,
                    "Harmony inference server error",
                ),
            );
            return Err(provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        // Parse response JSON
        let response_json: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Failed to parse harmony inference response: {}", e),
            );
            provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

        // Extract completion tokens from response
        // vLLM returns tokens in different formats depending on the response structure
        let completion_tokens =
            if let Some(tokens_array) = response_json.get("tokens").and_then(|t| t.as_array()) {
                // Direct tokens array
                tokens_array
                    .iter()
                    .filter_map(|v| v.as_u64().and_then(|u| u32::try_from(u).ok()))
                    .collect::<Vec<u32>>()
            } else if let Some(outputs) = response_json.get("outputs").and_then(|o| o.as_array()) {
                // vLLM nested outputs format
                outputs
                    .first()
                    .and_then(|output| output.get("token_ids"))
                    .and_then(|token_ids| token_ids.as_array())
                    .map(|token_ids| {
                        token_ids
                            .iter()
                            .filter_map(|v| v.as_u64().and_then(|u| u32::try_from(u).ok()))
                            .collect::<Vec<u32>>()
                    })
                    .unwrap_or_default()
            } else {
                // Fallback: try to find tokens in any nested structure
                let mut found_tokens = Vec::new();
                if let Some(obj) = response_json.as_object() {
                    for (_, value) in obj {
                        if let Some(arr) = value.as_array() {
                            if arr.iter().all(|v| v.is_u64()) {
                                found_tokens = arr
                                    .iter()
                                    .filter_map(|v| v.as_u64().and_then(|u| u32::try_from(u).ok()))
                                    .collect();
                                break;
                            }
                        }
                    }
                }
                found_tokens
            };

        if completion_tokens.is_empty() {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                "No completion tokens received from harmony inference server",
            );
            return Err(provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        Ok(completion_tokens)
    }

    /// Parse harmony tool name from recipient or tool reference
    pub(crate) fn parse_harmony_tool_name(recipient: &str) -> String {
        harmony::parse_harmony_tool_name(recipient)
    }

    /// Parse harmony tool call from raw text content
    pub(crate) fn parse_harmony_tool_call_from_text(
        text: &str,
    ) -> Option<(String, serde_json::Value)> {
        harmony::parse_harmony_tool_call_from_text(text)
    }
}
