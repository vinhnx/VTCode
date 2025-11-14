use super::AnthropicProvider;
use crate::config::TimeoutsConfig;
use crate::config::constants::models;
use crate::config::core::PromptCachingConfig;
use crate::llm::client::LLMClient;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    ToolCall, ToolDefinition,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::HashMap;

const TOOL_CALL_BLOCK_PATTERN: &str = r"(?s)<minimax:tool_call>(.*?)</minimax:tool_call>";
const INVOKE_BLOCK_PATTERN: &str = r#"(?s)<invoke\s+name=("[^"]*"|'[^']*'|[^\s>]+)>(.*?)</invoke>"#;
const PARAMETER_BLOCK_PATTERN: &str =
    r#"(?s)<parameter\s+name=("[^"]*"|'[^']*'|[^\s>]+)>(.*?)</parameter>"#;

pub struct MinimaxProvider {
    inner: AnthropicProvider,
}

impl MinimaxProvider {
    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
    ) -> Self {
        let effective_model = model.unwrap_or_else(|| models::minimax::MINIMAX_M2.to_string());

        let inner = AnthropicProvider::from_config(
            api_key,
            Some(effective_model),
            base_url,
            prompt_cache,
            timeouts,
        );

        Self { inner }
    }
}

#[async_trait]
impl LLMProvider for MinimaxProvider {
    fn name(&self) -> &str {
        "minimax"
    }

    fn supports_streaming(&self) -> bool {
        self.inner.supports_streaming()
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        self.inner.supports_reasoning(model)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        self.inner.supports_reasoning_effort(model)
    }

    fn supports_tools(&self, model: &str) -> bool {
        self.inner.supports_tools(model)
    }

    fn supports_parallel_tool_config(&self, model: &str) -> bool {
        self.inner.supports_parallel_tool_config(model)
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let tools = request.tools.clone();
        let response = self.inner.generate(request).await?;
        Ok(post_process_response(
            response,
            tools.as_ref().map(|defs| defs.as_slice()),
        ))
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let tool_defs = request.tools.clone();
        let mut inner_stream = self.inner.stream(request).await?;

        let stream = try_stream! {
            while let Some(event) = inner_stream.next().await {
                let event = event?;
                match event {
                    LLMStreamEvent::Completed { response } => {
                        let processed = post_process_response(
                            response,
                            tool_defs
                                .as_ref()
                                .map(|defs| defs.as_slice()),
                        );
                        yield LLMStreamEvent::Completed { response: processed };
                    }
                    other => {
                        yield other;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    fn supported_models(&self) -> Vec<String> {
        vec![models::minimax::MINIMAX_M2.to_string()]
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        self.inner.validate_request(request)
    }
}

#[async_trait]
impl LLMClient for MinimaxProvider {
    async fn generate(&mut self, prompt: &str) -> Result<crate::llm::types::LLMResponse, LLMError> {
        LLMClient::generate(&mut self.inner, prompt).await
    }

    fn backend_kind(&self) -> crate::llm::types::BackendKind {
        LLMClient::backend_kind(&self.inner)
    }

    fn model_id(&self) -> &str {
        LLMClient::model_id(&self.inner)
    }
}

fn post_process_response(
    mut response: LLMResponse,
    tools: Option<&[ToolDefinition]>,
) -> LLMResponse {
    if response
        .tool_calls
        .as_ref()
        .map_or(false, |calls| !calls.is_empty())
    {
        return response;
    }

    if let Some(content) = response.content.clone() {
        let (tool_calls, cleaned_content) = parse_minimax_tool_calls(&content, tools);

        if !tool_calls.is_empty() {
            response.tool_calls = Some(tool_calls);

            let trimmed = cleaned_content.trim();
            response.content = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };

            if !matches!(response.finish_reason, FinishReason::ToolCalls) {
                response.finish_reason = FinishReason::ToolCalls;
            }
        }
    }

    response
}

fn parse_minimax_tool_calls(
    text: &str,
    tools: Option<&[ToolDefinition]>,
) -> (Vec<ToolCall>, String) {
    let tool_call_regex = Regex::new(TOOL_CALL_BLOCK_PATTERN).ok();
    let invoke_regex = Regex::new(INVOKE_BLOCK_PATTERN).ok();
    let parameter_regex = Regex::new(PARAMETER_BLOCK_PATTERN).ok();

    if tool_call_regex.is_none() || invoke_regex.is_none() || parameter_regex.is_none() {
        return (Vec::new(), text.to_string());
    }

    let tool_call_regex = tool_call_regex.unwrap();
    let invoke_regex = invoke_regex.unwrap();
    let parameter_regex = parameter_regex.unwrap();

    let mut tool_calls = Vec::new();
    let mut call_index = 1u32;

    let param_types = build_parameter_type_map(tools);

    for block_capture in tool_call_regex.captures_iter(text) {
        if let Some(block_inner) = block_capture.get(1) {
            for invoke_capture in invoke_regex.captures_iter(block_inner.as_str()) {
                let function_name_raw = invoke_capture.get(1).map(|m| m.as_str()).unwrap_or("");
                let function_name = extract_name(function_name_raw);
                let body = invoke_capture.get(2).map(|m| m.as_str()).unwrap_or("");

                if function_name.is_empty() {
                    continue;
                }

                let param_config = param_types.get(&function_name);
                let mut arguments_map = Map::new();

                for parameter_capture in parameter_regex.captures_iter(body) {
                    let param_name_raw = parameter_capture.get(1).map(|m| m.as_str()).unwrap_or("");
                    let param_name = extract_name(param_name_raw);
                    if param_name.is_empty() {
                        continue;
                    }

                    let raw_value = parameter_capture.get(2).map(|m| m.as_str()).unwrap_or("");
                    let cleaned_value = raw_value.trim_matches('\n').trim();

                    let param_type = param_config
                        .and_then(|cfg| cfg.get(&param_name))
                        .map(|s| s.as_str())
                        .unwrap_or("string");

                    let converted = convert_param_value(cleaned_value, param_type);
                    arguments_map.insert(param_name, converted);
                }

                let arguments_json = Value::Object(arguments_map);
                if let Ok(serialized) = serde_json::to_string(&arguments_json) {
                    let tool_call = ToolCall::function(
                        format!("call_{}", call_index),
                        function_name,
                        serialized,
                    );
                    tool_calls.push(tool_call);
                    call_index += 1;
                }
            }
        }
    }

    let cleaned = tool_call_regex.replace_all(text, "").to_string();
    (tool_calls, cleaned)
}

fn build_parameter_type_map(
    tools: Option<&[ToolDefinition]>,
) -> HashMap<String, HashMap<String, String>> {
    let mut map = HashMap::new();

    if let Some(defs) = tools {
        for tool in defs {
            let mut param_map = HashMap::new();
            if let Some(properties) = tool
                .function
                .parameters
                .get("properties")
                .and_then(|props| props.as_object())
            {
                for (name, schema) in properties {
                    if let Some(param_type) = schema.get("type") {
                        if let Some(type_str) = param_type.as_str() {
                            param_map.insert(name.clone(), type_str.to_string());
                            continue;
                        }

                        if let Some(array) = param_type.as_array() {
                            if let Some(first) = array.iter().find_map(|value| value.as_str()) {
                                param_map.insert(name.clone(), first.to_string());
                                continue;
                            }
                        }
                    }

                    param_map.insert(name.clone(), "string".to_string());
                }
            }

            map.insert(tool.function.name.clone(), param_map);
        }
    }

    map
}

fn extract_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('\"') && trimmed.ends_with('\"') && trimmed.len() >= 2 {
        trimmed[1..trimmed.len() - 1].to_string()
    } else if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

fn convert_param_value(value: &str, param_type: &str) -> Value {
    if value.eq_ignore_ascii_case("null") {
        return Value::Null;
    }

    match param_type.to_ascii_lowercase().as_str() {
        "string" | "str" | "text" => Value::String(value.to_string()),
        "integer" | "int" => value
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(value.to_string())),
        "number" | "float" => value
            .parse::<f64>()
            .ok()
            .and_then(|num| serde_json::Number::from_f64(num))
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(value.to_string())),
        "boolean" | "bool" => {
            Value::Bool(matches!(value.to_ascii_lowercase().as_str(), "true" | "1"))
        }
        "object" | "array" => {
            serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()))
        }
        _ => serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_tool_definition() -> ToolDefinition {
        ToolDefinition::function(
            "search_web".to_string(),
            "Search function.".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "query_list": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "query_tag": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "radius": { "type": "number" },
                    "verbose": { "type": "boolean" }
                }
            }),
        )
    }

    #[test]
    fn parses_minimax_tool_calls_and_arguments() {
        let text = r#"Assistant thinking...
<minimax:tool_call>
<invoke name="search_web">
<parameter name="query_list">["rust", "news"]</parameter>
<parameter name='query_tag'>["technology"]</parameter>
<parameter name="radius">12.5</parameter>
<parameter name="verbose">true</parameter>
</invoke>
</minimax:tool_call>
Continuing response."#;

        let (tool_calls, cleaned) =
            parse_minimax_tool_calls(text, Some(&[sample_tool_definition()]));

        assert_eq!(tool_calls.len(), 1);
        let call = &tool_calls[0];
        assert_eq!(call.function.name, "search_web");

        let parsed_args: Value = serde_json::from_str(&call.function.arguments).unwrap();
        assert_eq!(parsed_args["query_list"], json!(["rust", "news"]));
        assert_eq!(parsed_args["query_tag"], json!(["technology"]));
        assert_eq!(parsed_args["radius"], json!(12.5));
        assert_eq!(parsed_args["verbose"], json!(true));

        assert!(!cleaned.contains("<minimax:tool_call>"));
        assert!(cleaned.contains("Assistant thinking"));
        assert!(cleaned.contains("Continuing response."));
    }

    #[test]
    fn post_processing_infers_tool_calls() {
        let text = r#"Some text before
<minimax:tool_call>
<invoke name="search_web">
<parameter name="query_list">["minimax"]</parameter>
<parameter name="query_tag">["ai"]</parameter>
</invoke>
</minimax:tool_call>
"#;

        let response = LLMResponse {
            content: Some(text.to_string()),
            tool_calls: None,
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
        };

        let processed = post_process_response(response, Some(&[sample_tool_definition()]));

        let tool_calls = processed.tool_calls.expect("expected tool calls");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "search_web");
        assert!(matches!(processed.finish_reason, FinishReason::ToolCalls));
        assert!(
            processed
                .content
                .unwrap_or_default()
                .contains("Some text before")
        );
    }

    #[test]
    fn parse_handles_missing_blocks() {
        let text = "No tool calls here.";
        let (tool_calls, cleaned) =
            parse_minimax_tool_calls(text, Some(&[sample_tool_definition()]));
        assert!(tool_calls.is_empty());
        assert_eq!(cleaned, text);
    }
}
