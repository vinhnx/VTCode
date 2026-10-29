use super::*;

impl HuggingFaceProvider {
    pub(super) fn format_for_responses_api(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut input = Vec::new();

        for msg in request.messages.iter() {
            let convert_parts = |parts: &[crate::provider::ContentPart]| -> Value {
                let parts_json: Vec<Value> = parts
                    .iter()
                    .map(|part| match part {
                        crate::provider::ContentPart::Text { text } => {
                            json!({ "type": "input_text", "text": text })
                        }
                        crate::provider::ContentPart::Image { data, mime_type, .. } => {
                            json!({
                                "type": "input_image",
                                "image_url": format!("data:{};base64,{}", mime_type, data)
                            })
                        }
                        crate::provider::ContentPart::File { filename, file_id, file_url, .. } => {
                            let fallback = filename
                                .clone()
                                .or_else(|| file_id.clone())
                                .or_else(|| file_url.clone())
                                .unwrap_or_else(|| "attached file".to_string());
                            json!({
                                "type": "input_text",
                                "text": format!("[File input not directly supported: {}]", fallback)
                            })
                        }
                    })
                    .collect();
                json!(parts_json)
            };

            match msg.role {
                MessageRole::System | MessageRole::User => {
                    if msg.role == MessageRole::System && request.system_prompt.is_some() {
                        if let crate::provider::MessageContent::Text(text) = &msg.content {
                            if request.system_prompt.as_ref().map(|s| s.as_ref()) == Some(text.as_str()) {
                                continue;
                            }
                        }
                    }

                    let role = if msg.role == MessageRole::System {
                        "system"
                    } else {
                        "user"
                    };

                    let mut message_obj = json!({
                        "type": "message",
                        "role": role,
                    });

                    match &msg.content {
                        crate::provider::MessageContent::Text(text) => {
                            message_obj["content"] = json!(text);
                        }
                        crate::provider::MessageContent::Parts(parts) => {
                            message_obj["content"] = convert_parts(parts);
                        }
                    }

                    input.push(message_obj);
                }
                MessageRole::Assistant => {
                    let has_content = match &msg.content {
                        crate::provider::MessageContent::Text(text) => !text.is_empty(),
                        crate::provider::MessageContent::Parts(parts) => !parts.is_empty(),
                    };

                    if has_content {
                        let mut message_obj = json!({
                            "type": "message",
                            "role": "assistant",
                        });

                        match &msg.content {
                            crate::provider::MessageContent::Text(text) => {
                                message_obj["content"] = json!(text);
                            }
                            crate::provider::MessageContent::Parts(parts) => {
                                message_obj["content"] = convert_parts(parts);
                            }
                        }

                        input.push(message_obj);
                    }

                    if let Some(tool_calls) = &msg.tool_calls {
                        for tc in tool_calls {
                            if let Some(func) = &tc.function {
                                input.push(json!({
                                    "type": "function_call",
                                    "call_id": tc.id,
                                    "name": func.name,
                                    "arguments": func.arguments
                                }));
                            }
                        }
                    }
                }
                MessageRole::Tool => {
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": msg.tool_call_id.clone().unwrap_or_default(),
                        "output": function_output_value_from_message_content(&msg.content)
                    }));
                }
            }
        }

        let mut payload = json!({
            "model": request.model,
            "input": input,
            "stream": request.stream,
        });

        if let Some(system_prompt) = &request.system_prompt {
            payload["instructions"] = json!(system_prompt);
        }

        if let Some(effort) = request.reasoning_effort {
            use vtcode_config::types::ReasoningEffortLevel;
            if effort != ReasoningEffortLevel::None {
                payload["reasoning"] = json!({ "effort": effort.as_str() });
            }
        }

        if let Some(max_tokens) = request.max_tokens {
            payload["max_tokens"] = json!(max_tokens);
        }
        if let Some(temperature) = request.temperature {
            payload["temperature"] = json!(super::super::common::sampling_param_f64(temperature));
        }
        if let Some(top_p) = request.top_p {
            payload["top_p"] = json!(super::super::common::sampling_param_f64(top_p));
        }
        if let Some(top_k) = request.top_k {
            payload["top_k"] = json!(top_k);
        }

        if let Some(tools) = &request.tools {
            if let Some(serialized) = self.serialize_tools_huggingface(tools) {
                payload["tools"] = json!(serialized);

                if let Some(choice) = &request.tool_choice {
                    payload["tool_choice"] = choice.to_provider_format("openai");
                }
            }
        }

        if request.output_format.is_some() || request.tools.is_some() {
            self.add_json_instruction(&mut payload)?;
        }

        if request.output_format.is_some() && !self.is_glm_model(&request.model) {
            payload["response_format"] = json!({ "type": "json_object" });
        }

        Ok(payload)
    }
}
