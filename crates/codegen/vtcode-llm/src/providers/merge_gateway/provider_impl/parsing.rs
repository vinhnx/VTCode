use super::*;

impl MergeGatewayProvider {
    pub(super) fn parse_native_response_payload(json: Value, model: String) -> Result<LLMResponse, LLMError> {
        let mut content = String::new();
        let mut tool_calls = Vec::new();
        let mut finish_reason = FinishReason::Stop;

        if let Some(output) = json.get("output").and_then(Value::as_array) {
            for item in output {
                Self::parse_native_output_item(item, &mut content, &mut tool_calls, &mut finish_reason)?;
            }
        } else if json.get("content").is_some() || json.get("text").is_some() || json.get("tool_use").is_some() {
            Self::parse_native_output_item(&json, &mut content, &mut tool_calls, &mut finish_reason)?;
        } else {
            return Err(provider_error("Invalid response from Merge Gateway: missing output"));
        }

        if matches!(finish_reason, FinishReason::Stop) && !tool_calls.is_empty() {
            finish_reason = FinishReason::ToolCalls;
        }

        Ok(LLMResponse {
            content: if content.is_empty() { None } else { Some(content) },
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            model: json
                .get("model")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(model),
            usage: Self::parse_native_usage(json.get("usage")),
            finish_reason,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: Self::extract_request_id(&json),
            organization_id: None,
            compaction: None,
        })
    }

    fn parse_native_output_item(
        item: &Value,
        content: &mut String,
        tool_calls: &mut Vec<ToolCall>,
        finish_reason: &mut FinishReason,
    ) -> Result<(), LLMError> {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        match item_type {
            "message" | "" => {
                if let Some(role) = item.get("role").and_then(Value::as_str)
                    && role != "assistant"
                {
                    return Ok(());
                }

                if let Some(reason) = item.get("finish_reason").and_then(Value::as_str) {
                    *finish_reason = Self::map_finish_reason(reason);
                }

                if let Some(content_value) = item.get("content") {
                    Self::parse_native_content_value(content_value, content, tool_calls, finish_reason)?;
                } else {
                    Self::parse_native_content_value(item, content, tool_calls, finish_reason)?;
                }
            }
            "tool_use" | "function_call" => {
                if let Some(tool_call) = Self::tool_call_from_value(item)? {
                    tool_calls.push(tool_call);
                    if matches!(finish_reason, FinishReason::Stop) {
                        *finish_reason = FinishReason::ToolCalls;
                    }
                }
            }
            "refusal" => {
                if let Some(text) = item.get("refusal").and_then(Value::as_str) {
                    content.push_str(text);
                    *finish_reason = FinishReason::Refusal;
                }
            }
            _ => {
                Self::parse_native_content_value(item, content, tool_calls, finish_reason)?;
            }
        }
        Ok(())
    }

    fn parse_native_content_value(
        value: &Value,
        content: &mut String,
        tool_calls: &mut Vec<ToolCall>,
        finish_reason: &mut FinishReason,
    ) -> Result<(), LLMError> {
        match value {
            Value::String(text) => content.push_str(text),
            Value::Array(parts) => {
                for part in parts {
                    Self::parse_native_content_part(part, content, tool_calls, finish_reason)?;
                }
            }
            Value::Object(_) => {
                Self::parse_native_content_part(value, content, tool_calls, finish_reason)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn parse_native_content_part(
        part: &Value,
        content: &mut String,
        tool_calls: &mut Vec<ToolCall>,
        finish_reason: &mut FinishReason,
    ) -> Result<(), LLMError> {
        let part_type = part.get("type").and_then(Value::as_str).unwrap_or("");
        match part_type {
            "text" => {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    content.push_str(text);
                } else if let Some(text) = part.get("output_text").and_then(Value::as_str) {
                    content.push_str(text);
                }
            }
            "tool_use" | "function_call" => {
                if let Some(tool_call) = Self::tool_call_from_value(part)? {
                    tool_calls.push(tool_call);
                    *finish_reason = FinishReason::ToolCalls;
                }
            }
            "refusal" => {
                if let Some(text) = part.get("refusal").and_then(Value::as_str) {
                    content.push_str(text);
                    *finish_reason = FinishReason::Refusal;
                }
            }
            _ => {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    content.push_str(text);
                } else if let Some(text) = part.get("output_text").and_then(Value::as_str) {
                    content.push_str(text);
                } else if let Some(text) = part.as_str() {
                    content.push_str(text);
                }

                if let Some(inner) = part.get("content") {
                    Self::parse_native_content_value(inner, content, tool_calls, finish_reason)?;
                }

                if let Some(tool_call) = Self::tool_call_from_value(part)? {
                    tool_calls.push(tool_call);
                    *finish_reason = FinishReason::ToolCalls;
                }
            }
        }
        Ok(())
    }

    fn tool_call_from_value(value: &Value) -> Result<Option<ToolCall>, LLMError> {
        let id = value
            .get("id")
            .or_else(|| value.get("call_id"))
            .or_else(|| value.get("tool_use_id"))
            .or_else(|| value.get("tool_call_id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(generate_tool_call_id);

        let name = value
            .get("name")
            .or_else(|| value.get("function").and_then(|func| func.get("name")))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            return Ok(None);
        }

        let arguments = match value.get("input").or_else(|| value.get("arguments")) {
            Some(Value::Null) | None => "{}".to_string(),
            Some(input) => Self::value_to_arguments(input)?,
        };

        Ok(Some(ToolCall::function(id, name, arguments)))
    }

    fn map_finish_reason(reason: &str) -> FinishReason {
        match reason.to_ascii_lowercase().as_str() {
            "stop" => FinishReason::Stop,
            "length" | "max_tokens" => FinishReason::Length,
            "tool_use" | "tool_calls" => FinishReason::ToolCalls,
            "content_filter" => FinishReason::ContentFilter,
            "refusal" => FinishReason::Refusal,
            "error" => FinishReason::Error("Merge Gateway reported an error".to_string()),
            _ => FinishReason::Stop,
        }
    }

    pub(crate) fn parse_native_usage(value: Option<&Value>) -> Option<Usage> {
        let usage = value?;
        let prompt_tokens = usage_u32_from_keys(usage, RESPONSES_PROMPT_TOKEN_KEYS);
        let completion_tokens = usage_u32_from_keys(usage, RESPONSES_COMPLETION_TOKEN_KEYS);
        let total_tokens = usage
            .get("total_tokens")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or_else(|| prompt_tokens.saturating_add(completion_tokens));

        // Surface gateway cache signals instead of hard-coding None. The
        // shared Responses helpers cover OpenAI-style
        // `input_tokens_details.cached_tokens` /
        // `prompt_tokens_details.cached_tokens` / `prompt_cache_hit_tokens` /
        // `cached_tokens` shapes; the explicit fallbacks cover gateway
        // variants (`cache_read_tokens`, OpenRouter-style
        // `prompt_cache_read_tokens`, Anthropic-style
        // `cache_read_input_tokens`).
        let openai_cached_prompt_tokens = parse_cached_prompt_tokens_from_usage(usage, true);
        let cache_creation_tokens = parse_cache_write_tokens_from_usage(usage, true)
            .or_else(|| {
                usage
                    .get("prompt_cache_write_tokens")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
            })
            .or_else(|| {
                usage
                    .get("cache_creation_input_tokens")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
            });
        let cache_read_tokens = usage
            .get("cache_read_tokens")
            .or_else(|| usage.get("prompt_cache_read_tokens"))
            .or_else(|| usage.get("cache_read_input_tokens"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .or(openai_cached_prompt_tokens);
        // Merge native Responses often reports only Anthropic-style cache-read
        // fields. Surface that count in `cached_prompt_tokens` so trajectory
        // and status metrics are not misleading zeros when the gateway cached.
        // Cost accounting already prefers `cache_read_tokens` via
        // `Usage::billable_totals`; this mapping is observability only.
        let cached_prompt_tokens = openai_cached_prompt_tokens.or(cache_read_tokens);

        Some(Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
            cached_prompt_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            iterations: None,
        })
    }

    pub(super) fn extract_request_id(value: &Value) -> Option<String> {
        value
            .get("id")
            .or_else(|| value.get("request_id"))
            .or_else(|| value.get("provider_request_id"))
            .or_else(|| value.get("routing").and_then(|routing| routing.get("request_id")))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    }
}
