use serde_json::{Value, json};

use crate::config::models::Provider;
use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, MessageRole};
use crate::llm::rig_adapter::reasoning_parameters_for;
use crate::prompts::system::default_system_prompt;

use super::OpenRouterProvider;

impl OpenRouterProvider {
    fn build_standard_responses_input(&self, request: &LLMRequest) -> Result<Vec<Value>, LLMError> {
        let mut input = Vec::new();

        if let Some(system_prompt) = &request.system_prompt {
            if !system_prompt.trim().is_empty() {
                input.push(json!({
                    "role": "developer",
                    "content": [{
                        "type": "input_text",
                        "text": system_prompt.clone()
                    }]
                }));
            }
        }

        for msg in &request.messages {
            match msg.role {
                MessageRole::System => {
                    let content_text = msg.content.as_text();
                    if !content_text.trim().is_empty() {
                        input.push(json!({
                            "role": "developer",
                            "content": [{
                                "type": "input_text",
                                "text": content_text
                            }]
                        }));
                    }
                }
                MessageRole::User => {
                    input.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "input_text",
                            "text": msg.content.as_text()
                        }]
                    }));
                }
                MessageRole::Assistant => {
                    let mut content_parts = Vec::new();
                    if !msg.content.is_empty() {
                        content_parts.push(json!({
                            "type": "output_text",
                            "text": msg.content.as_text()
                        }));
                    }

                    if let Some(tool_calls) = &msg.tool_calls {
                        for call in tool_calls {
                            if let Some(ref func) = call.function {
                                content_parts.push(json!({
                                    "type": "tool_call",
                                    "id": call.id.clone(),
                                    "name": func.name.clone(),
                                    "arguments": func.arguments.clone()
                                }));
                            }
                        }
                    }

                    if !content_parts.is_empty() {
                        input.push(json!({
                            "role": "assistant",
                            "content": content_parts
                        }));
                    }
                }
                MessageRole::Tool => {
                    let tool_call_id = msg.tool_call_id.clone().ok_or_else(|| {
                        let formatted_error = error_display::format_llm_error(
                            "OpenRouter",
                            "Tool messages must include tool_call_id for Responses API",
                        );
                        LLMError::InvalidRequest {
                            message: formatted_error,
                            metadata: None,
                        }
                    })?;

                    let mut tool_content = Vec::new();
                    let content_text = msg.content.as_text();
                    if !content_text.trim().is_empty() {
                        tool_content.push(json!({
                            "type": "output_text",
                            "text": content_text
                        }));
                    }

                    let mut tool_result = json!({
                        "type": "tool_result",
                        "tool_call_id": tool_call_id
                    });

                    if !tool_content.is_empty() {
                        if let Value::Object(ref mut map) = tool_result {
                            map.insert("content".to_string(), json!(tool_content));
                        }
                    }

                    input.push(json!({
                        "role": "tool",
                        "content": [tool_result]
                    }));
                }
            }
        }

        Ok(input)
    }

    fn build_codex_responses_input(&self, request: &LLMRequest) -> Result<Vec<Value>, LLMError> {
        let mut additional_guidance = Vec::new();

        if let Some(system_prompt) = &request.system_prompt {
            let trimmed = system_prompt.trim();
            if !trimmed.is_empty() {
                additional_guidance.push(trimmed.to_string());
            }
        }

        let mut input = Vec::new();

        for msg in &request.messages {
            match msg.role {
                MessageRole::System => {
                    let content_text = msg.content.as_text();
                    let trimmed = content_text.trim();
                    if !trimmed.is_empty() {
                        additional_guidance.push(trimmed.to_string());
                    }
                }
                MessageRole::User => {
                    input.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "input_text",
                            "text": msg.content.as_text()
                        }]
                    }));
                }
                MessageRole::Assistant => {
                    let mut content_parts = Vec::new();
                    if !msg.content.is_empty() {
                        content_parts.push(json!({
                            "type": "output_text",
                            "text": msg.content.as_text()
                        }));
                    }

                    if let Some(tool_calls) = &msg.tool_calls {
                        for call in tool_calls {
                            if let Some(ref func) = call.function {
                                content_parts.push(json!({
                                    "type": "tool_call",
                                    "id": call.id.clone(),
                                    "name": func.name.clone(),
                                    "arguments": func.arguments.clone()
                                }));
                            }
                        }
                    }

                    if !content_parts.is_empty() {
                        input.push(json!({
                            "role": "assistant",
                            "content": content_parts
                        }));
                    }
                }
                MessageRole::Tool => {
                    let tool_call_id = msg.tool_call_id.clone().ok_or_else(|| {
                        let formatted_error = error_display::format_llm_error(
                            "OpenRouter",
                            "Tool messages must include tool_call_id for Responses API",
                        );
                        LLMError::InvalidRequest {
                            message: formatted_error,
                            metadata: None,
                        }
                    })?;

                    let mut tool_content = Vec::new();
                    let content_text = msg.content.as_text();
                    if !content_text.trim().is_empty() {
                        tool_content.push(json!({
                            "type": "output_text",
                            "text": content_text
                        }));
                    }

                    let mut tool_result = json!({
                        "type": "tool_result",
                        "tool_call_id": tool_call_id
                    });

                    if !tool_content.is_empty() {
                        if let Value::Object(ref mut map) = tool_result {
                            map.insert("content".to_string(), json!(tool_content));
                        }
                    }

                    input.push(json!({
                        "role": "tool",
                        "content": [tool_result]
                    }));
                }
            }
        }

        // Use collected guidance, or fall back to default system prompt if empty
        let developer_prompt = if additional_guidance.is_empty() {
            default_system_prompt().to_string()
        } else {
            additional_guidance.join("\n\n")
        };
        input.insert(
            0,
            json!({
                "role": "developer",
                "content": [{
                    "type": "input_text",
                    "text": developer_prompt
                }]
            }),
        );

        Ok(input)
    }

    pub(super) fn convert_to_openrouter_responses_format(
        &self,
        request: &LLMRequest,
    ) -> Result<Value, LLMError> {
        let resolved_model = self.resolve_model(request);
        let input = if Self::is_gpt5_codex_model(resolved_model) {
            self.build_codex_responses_input(request)?
        } else {
            self.build_standard_responses_input(request)?
        };

        if input.is_empty() {
            let formatted_error = error_display::format_llm_error(
                "OpenRouter",
                "No messages provided for Responses API",
            );
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        let mut provider_request = json!({
            "model": resolved_model,
            "input": input,
            "stream": request.stream
        });

        if let Some(max_tokens) = request.max_tokens {
            provider_request["max_output_tokens"] = json!(max_tokens);
        }

        if let Some(temperature) = request.temperature {
            provider_request["temperature"] = json!(temperature);
        }

        if let Some(tools) = &request.tools {
            if !tools.is_empty() {
                let tools_json: Vec<Value> = tools
                    .iter()
                    .filter_map(|tool| {
                        let func = tool.function.as_ref()?;
                        Some(json!({
                            "type": "function",
                            "function": {
                                "name": func.name,
                                "description": func.description,
                                "parameters": func.parameters
                            }
                        }))
                    })
                    .collect();
                provider_request["tools"] = Value::Array(tools_json);
            }
        }

        if let Some(tool_choice) = &request.tool_choice {
            provider_request["tool_choice"] = tool_choice.to_provider_format("openai");
        }

        if let Some(parallel) = request.parallel_tool_calls {
            provider_request["parallel_tool_calls"] = Value::Bool(parallel);
        }

        if let Some(effort) = request.reasoning_effort {
            if self.supports_reasoning_effort(resolved_model) {
                if let Some(payload) = reasoning_parameters_for(Provider::OpenRouter, effort) {
                    provider_request["reasoning"] = payload;
                } else {
                    provider_request["reasoning"] = json!({ "effort": effort.as_str() });
                }
            }
        }

        if Self::is_gpt5_codex_model(resolved_model) {
            provider_request["reasoning"] = json!({ "effort": "medium" });
        }

        Ok(provider_request)
    }

    pub(super) fn convert_to_openrouter_format(
        &self,
        request: &LLMRequest,
    ) -> Result<Value, LLMError> {
        let resolved_model = self.resolve_model(request);
        let mut messages = Vec::new();

        if let Some(system_prompt) = &request.system_prompt {
            messages.push(json!({
                "role": crate::config::constants::message_roles::SYSTEM,
                "content": system_prompt
            }));
        }

        for msg in &request.messages {
            let role = msg.role.as_openai_str();
            let mut message = json!({
                "role": role,
                "content": msg.content
            });

            if msg.role == MessageRole::Assistant {
                if let Some(tool_calls) = &msg.tool_calls {
                    if !tool_calls.is_empty() {
                        let tool_calls_json: Vec<Value> = tool_calls
                            .iter()
                            .filter_map(|tc| {
                                tc.function.as_ref().map(|func| {
                                    json!({
                                        "id": tc.id,
                                        "type": "function",
                                        "function": {
                                            "name": func.name,
                                            "arguments": func.arguments
                                        }
                                    })
                                })
                            })
                            .collect();
                        message["tool_calls"] = Value::Array(tool_calls_json);
                    }
                }

                if let Some(reasoning_details) = &msg.reasoning_details {
                    if !reasoning_details.is_empty() {
                        message["reasoning_details"] = Value::Array(reasoning_details.clone());
                    }
                }
            }

            if msg.role == MessageRole::Tool {
                if let Some(tool_call_id) = &msg.tool_call_id {
                    message["tool_call_id"] = Value::String(tool_call_id.clone());
                }
            }

            messages.push(message);
        }

        if messages.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenRouter", "No messages provided");
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        let mut provider_request = json!({
            "model": resolved_model,
            "messages": messages,
            "stream": request.stream
        });

        if let Some(max_tokens) = request.max_tokens {
            provider_request["max_tokens"] = json!(max_tokens);
        }

        if let Some(temperature) = request.temperature {
            provider_request["temperature"] = json!(temperature);
        }

        if let Some(tools) = &request.tools {
            if !tools.is_empty() {
                let tools_json: Vec<Value> = tools
                    .iter()
                    .filter_map(|tool| {
                        let func = tool.function.as_ref()?;
                        Some(json!({
                            "type": "function",
                            "function": {
                                "name": func.name,
                                "description": func.description,
                                "parameters": func.parameters
                            }
                        }))
                    })
                    .collect();
                provider_request["tools"] = Value::Array(tools_json);
            }
        }

        if let Some(tool_choice) = &request.tool_choice {
            provider_request["tool_choice"] = tool_choice.to_provider_format("openai");
        }

        if let Some(parallel) = request.parallel_tool_calls {
            provider_request["parallel_tool_calls"] = Value::Bool(parallel);
        }

        if let Some(effort) = request.reasoning_effort {
            if self.supports_reasoning_effort(resolved_model) {
                if let Some(payload) = reasoning_parameters_for(Provider::OpenRouter, effort) {
                    provider_request["reasoning"] = payload;
                } else {
                    provider_request["reasoning"] = json!({ "effort": effort.as_str() });
                }
            }
        }

        Ok(provider_request)
    }
}
