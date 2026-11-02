use super::*;

impl OllamaProvider {
    pub(super) fn build_payload(&self, request: &LLMRequest, stream: bool) -> Result<OllamaChatRequest, LLMError> {
        let mut messages = Vec::new();
        let mut tool_names: HashMap<String, String> = HashMap::new();
        let minimax_tool_followup_compat = Self::minimax_tool_followup_compat_mode(request);

        if let Some(system) = Self::merged_system_prompt(request) {
            messages.push(OllamaChatMessage {
                role: "system".to_string(),
                content: Some(system),
                thinking: None,
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
                images: None,
            });
        }

        for message in request.messages.iter() {
            let interleaved_content = assistant_interleaved_history_text(message, &request.model);
            let used_interleaved_content = interleaved_content.is_some();
            let (content_text, images) = if let Some(interleaved_content) = interleaved_content {
                (interleaved_content, None)
            } else {
                Self::extract_content_and_images(&message.content)
            };
            match message.role {
                MessageRole::System => continue,
                MessageRole::Tool => {
                    let tool_name = message.tool_call_id.as_ref().and_then(|id| tool_names.get(id).cloned());
                    let tool_name = tool_name.or_else(|| message.origin_tool.clone());
                    let tool_call_id = if minimax_tool_followup_compat && tool_name.is_some() {
                        None
                    } else {
                        message.tool_call_id.clone()
                    };
                    messages.push(OllamaChatMessage {
                        role: "tool".to_string(),
                        content: Some(content_text),
                        thinking: None,
                        tool_calls: None,
                        tool_call_id,
                        tool_name,
                        images: None,
                    });
                }
                _ => {
                    let thinking = if used_interleaved_content {
                        None
                    } else {
                        Self::assistant_thinking_history_text(message)
                    };
                    let mut payload_message = OllamaChatMessage {
                        role: message.role.as_generic_str().to_string(),
                        content: Some(content_text),
                        thinking,
                        tool_calls: None,
                        tool_call_id: None,
                        tool_name: None,
                        images,
                    };

                    if let Some(tool_calls) = message.get_tool_calls() {
                        let mut converted = Vec::new();
                        for (index, tool_call) in tool_calls.iter().enumerate() {
                            if let Some(ref func) = tool_call.function {
                                if !tool_call.id.is_empty() {
                                    tool_names.entry(tool_call.id.clone()).or_insert_with(|| func.name.clone());
                                }

                                let arguments =
                                    tool_call.execution_arguments().map_err(|err| LLMError::InvalidRequest {
                                        message: format!("Failed to parse tool arguments for Ollama: {err}"),
                                        metadata: None,
                                    })?;
                                converted.push(OllamaToolCall {
                                    call_type: tool_call.call_type.clone(),
                                    function: OllamaToolFunctionCall {
                                        name: func.name.clone(),
                                        arguments: Some(arguments),
                                        index: Some(index as u32),
                                    },
                                });
                            }
                        }

                        if !converted.is_empty() {
                            payload_message.tool_calls = Some(converted);
                            if payload_message.content.is_none() {
                                payload_message.content = Some(String::new());
                            }
                        }
                    }

                    messages.push(payload_message);
                }
            }
        }

        let options = if request.temperature.is_some() || request.max_tokens.is_some() {
            Some(OllamaChatOptions {
                temperature: request.temperature,
                num_predict: request.max_tokens,
            })
        } else {
            None
        };

        // Ollama has no wire-level tool_choice, so a tool-free request must
        // omit the callable catalog. Historical calls and results stay paired.
        let tools = request
            .tools
            .as_ref()
            .filter(|_| !matches!(request.tool_choice, Some(ToolChoice::None)))
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|tool| {
                        tool.function.as_ref().map(|func| {
                            ToolDefinition::function(
                                func.name.clone(),
                                func.description.clone(),
                                func.parameters.clone(),
                            )
                        })
                    })
                    .collect()
            });

        Ok(OllamaChatRequest {
            model: request.model.clone(),
            messages,
            stream,
            format: request.output_format.clone(),
            options,
            tools,
            think: Self::think_value(request),
        })
    }

    fn assistant_thinking_history_text(message: &Message) -> Option<String> {
        if message.role != MessageRole::Assistant {
            return None;
        }

        message
            .reasoning
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                message
                    .reasoning_details
                    .as_deref()
                    .and_then(extract_reasoning_text_from_detail_values)
            })
    }

    fn extract_content_and_images(content: &MessageContent) -> (String, Option<Vec<String>>) {
        let mut images = Vec::new();
        if let MessageContent::Parts(parts) = content {
            for part in parts {
                if let ContentPart::Image { data, .. } = part {
                    images.push(data.clone());
                }
            }
        }

        let text = content.as_text().into_owned();
        let images = if images.is_empty() { None } else { Some(images) };
        (text, images)
    }

    fn think_value(request: &LLMRequest) -> Option<Value> {
        let model_id = request.model.as_str();
        if Self::minimax_tool_followup_compat_mode(request) {
            return None;
        }
        if !models::ollama::REASONING_MODELS.contains(&model_id) {
            return None;
        }

        if models::ollama::REASONING_LEVEL_MODELS.contains(&model_id) {
            request.reasoning_effort.map(|effort| Value::String(effort.to_string()))
        } else {
            Some(Value::Bool(true))
        }
    }

    fn minimax_tool_followup_compat_mode(request: &LLMRequest) -> bool {
        is_minimax_m2_model(&request.model)
            && request
                .messages
                .iter()
                .any(|message| message.role == MessageRole::Tool || message.has_tool_calls())
    }
}
