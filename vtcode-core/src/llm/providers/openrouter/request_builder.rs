use serde_json::{Value, json};

use crate::config::models::Provider;
use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, MessageRole};
use crate::llm::providers::common::serialize_message_content_openai_for_role;
use crate::llm::rig_adapter::reasoning_parameters_for;

use super::OpenRouterProvider;

impl OpenRouterProvider {
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
            let content_value = serialize_message_content_openai_for_role(&msg.role, &msg.content);

            let mut message = json!({
                "role": role,
                "content": content_value
            });

            if msg.role == MessageRole::Assistant {
                if let Some(tool_calls) = &msg.tool_calls
                    && !tool_calls.is_empty()
                {
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

                if let Some(reasoning_details) = &msg.reasoning_details
                    && !reasoning_details.is_empty()
                {
                    message["reasoning_details"] = Value::Array(reasoning_details.clone());
                }
            }

            if msg.role == MessageRole::Tool {
                match &msg.tool_call_id {
                    Some(tool_call_id) => {
                        message["tool_call_id"] = Value::String(tool_call_id.clone());
                    }
                    None => {
                        let formatted_error = error_display::format_llm_error(
                            "OpenRouter",
                            "Tool response message missing required tool_call_id",
                        );
                        return Err(LLMError::InvalidRequest {
                            message: formatted_error,
                            metadata: None,
                        });
                    }
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

        if let Some(tools) = &request.tools
            && !tools.is_empty()
        {
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

        if let Some(tool_choice) = &request.tool_choice {
            provider_request["tool_choice"] = tool_choice.to_provider_format("openai");
        }

        if let Some(parallel) = request.parallel_tool_calls {
            provider_request["parallel_tool_calls"] = Value::Bool(parallel);
        }

        if let Some(effort) = request.reasoning_effort
            && self.supports_reasoning_effort(resolved_model)
        {
            if let Some(payload) = reasoning_parameters_for(Provider::OpenRouter, effort) {
                provider_request["reasoning"] = payload;
            } else {
                provider_request["reasoning"] = json!({ "effort": effort.as_str() });
            }
        }

        Ok(provider_request)
    }
}
