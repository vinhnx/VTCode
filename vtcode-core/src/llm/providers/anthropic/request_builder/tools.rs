use crate::llm::provider::{LLMError, LLMRequest, ToolDefinition};
use crate::llm::providers::anthropic_types::{
    AnthropicCodeExecutionTool, AnthropicFunctionTool, AnthropicMemoryTool, AnthropicTool,
    AnthropicToolSearchTool, AnthropicWebSearchTool, CacheControl, ThinkingConfig,
};
use serde_json::{Map, Value, json};

pub(crate) fn build_tools(
    request: &LLMRequest,
    cache_control: &Option<CacheControl>,
    breakpoints_remaining: &mut usize,
) -> Result<Option<Vec<AnthropicTool>>, LLMError> {
    let Some(request_tools) = request.tools.as_ref() else {
        return Ok(None);
    };
    if request_tools.is_empty() {
        return Ok(None);
    }

    let mut built_tools = Vec::with_capacity(request_tools.len());
    for tool in request_tools.iter() {
        if tool.is_tool_search() {
            let Some(func) = tool.function.as_ref() else {
                continue;
            };
            built_tools.push(AnthropicTool::ToolSearch(AnthropicToolSearchTool {
                tool_type: tool.tool_type.clone(),
                name: func.name.clone(),
            }));
            continue;
        }

        if tool.is_anthropic_web_search() {
            built_tools.push(AnthropicTool::WebSearch(AnthropicWebSearchTool {
                tool_type: tool.tool_type.clone(),
                name: "web_search".to_string(),
                options: anthropic_web_search_options(tool)?,
            }));
            continue;
        }

        if tool.is_anthropic_code_execution() {
            built_tools.push(AnthropicTool::CodeExecution(AnthropicCodeExecutionTool {
                tool_type: tool.tool_type.clone(),
                name: "code_execution".to_string(),
            }));
            continue;
        }

        if tool.is_anthropic_memory_tool() {
            built_tools.push(AnthropicTool::Memory(AnthropicMemoryTool {
                tool_type: tool.tool_type.clone(),
                name: "memory".to_string(),
            }));
            continue;
        }

        let Some(func) = tool.function.as_ref() else {
            continue;
        };
        built_tools.push(AnthropicTool::Function(AnthropicFunctionTool {
            name: func.name.clone(),
            description: func.description.clone(),
            input_schema: func.parameters.clone(),
            input_examples: tool.input_examples.clone(),
            strict: tool.strict,
            allowed_callers: tool.allowed_callers.clone(),
            cache_control: None,
            defer_loading: tool.defer_loading,
        }));
    }

    if *breakpoints_remaining > 0
        && let Some(cc) = cache_control.as_ref()
        && let Some(last_tool) = built_tools.last_mut()
        && let AnthropicTool::Function(func_tool) = last_tool
    {
        func_tool.cache_control = Some(cc.clone());
        *breakpoints_remaining -= 1;
    }

    if built_tools.is_empty() {
        Ok(None)
    } else {
        Ok(Some(built_tools))
    }
}

fn anthropic_web_search_options(tool: &ToolDefinition) -> Result<Map<String, Value>, LLMError> {
    match tool.web_search.as_ref() {
        Some(Value::Object(config)) => {
            if config.contains_key("allowed_domains") && config.contains_key("blocked_domains") {
                return Err(LLMError::Provider {
                    message: "anthropic web_search tools cannot set both allowed_domains and blocked_domains".to_string(),
                    metadata: None,
                });
            }

            Ok(config.clone())
        }
        Some(_) => Err(LLMError::Provider {
            message: format!(
                "{} tool configuration must be a JSON object",
                tool.tool_type
            ),
            metadata: None,
        }),
        None => Ok(Map::new()),
    }
}

pub(crate) fn build_tool_choice(
    request: &LLMRequest,
    thinking_val: &Option<ThinkingConfig>,
) -> Option<Value> {
    let mut final_tool_choice = request
        .tool_choice
        .as_ref()
        .map(|tc| tc.to_provider_format("anthropic"));

    if thinking_val.is_some()
        && let Some(ref choice) = final_tool_choice
    {
        let choice_type = choice.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if choice_type != "auto" && choice_type != "none" && !choice_type.is_empty() {
            final_tool_choice = Some(json!({"type": "auto"}));
        }
    }

    if request
        .parallel_tool_config
        .as_ref()
        .is_some_and(|config| config.disable_parallel_tool_use)
    {
        let mut tool_choice = final_tool_choice.unwrap_or_else(|| json!({"type": "auto"}));
        if let Some(tool_choice_obj) = tool_choice.as_object_mut() {
            tool_choice_obj.insert("disable_parallel_tool_use".to_string(), Value::Bool(true));
        }
        final_tool_choice = Some(tool_choice);
    }

    final_tool_choice
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::models;
    use crate::llm::provider::{
        LLMRequest, Message, ParallelToolConfig, ToolChoice, ToolDefinition,
    };
    use std::sync::Arc;

    #[test]
    fn build_tools_keeps_apply_patch_as_function_tool() {
        let request = LLMRequest {
            messages: vec![Message::user("patch this file".to_string())],
            tools: Some(Arc::new(vec![ToolDefinition::apply_patch(
                "Apply VT Code patches".to_string(),
            )])),
            model: models::anthropic::DEFAULT_MODEL.to_string(),
            ..Default::default()
        };

        let tools = build_tools(&request, &None, &mut 0)
            .expect("tool build")
            .expect("tools should exist");
        assert_eq!(tools.len(), 1);
        assert!(matches!(
            &tools[0],
            AnthropicTool::Function(function) if function.name == "apply_patch"
        ));
    }

    #[test]
    fn build_tools_preserves_anthropic_web_search_options() {
        let request = LLMRequest {
            messages: vec![Message::user("search docs".to_string())],
            tools: Some(Arc::new(vec![ToolDefinition {
                tool_type: "web_search_20250305".to_string(),
                function: None,
                allowed_callers: None,
                input_examples: None,
                web_search: Some(json!({
                    "max_uses": 5,
                    "allowed_domains": ["docs.rs"],
                    "user_location": {
                        "type": "approximate",
                        "city": "San Francisco",
                        "region": "California",
                        "country": "US",
                        "timezone": "America/Los_Angeles"
                    }
                })),
                hosted_tool_config: None,
                shell: None,
                grammar: None,
                strict: None,
                defer_loading: None,
            }])),
            model: models::anthropic::DEFAULT_MODEL.to_string(),
            ..Default::default()
        };

        let tools = build_tools(&request, &None, &mut 0)
            .expect("tool build")
            .expect("tools should exist");
        assert!(matches!(
            &tools[0],
            AnthropicTool::WebSearch(web_search)
                if web_search.options["max_uses"] == json!(5)
                    && web_search.options["allowed_domains"] == json!(["docs.rs"])
                    && web_search.options["user_location"]["timezone"]
                        == json!("America/Los_Angeles")
        ));
    }

    #[test]
    fn build_tools_rejects_non_object_anthropic_web_search_options() {
        let request = LLMRequest {
            messages: vec![Message::user("search docs".to_string())],
            tools: Some(Arc::new(vec![ToolDefinition {
                tool_type: "web_search_20260209".to_string(),
                function: None,
                allowed_callers: None,
                input_examples: None,
                web_search: Some(json!(["direct"])),
                hosted_tool_config: None,
                shell: None,
                grammar: None,
                strict: None,
                defer_loading: None,
            }])),
            model: models::anthropic::DEFAULT_MODEL.to_string(),
            ..Default::default()
        };

        assert!(build_tools(&request, &None, &mut 0).is_err());
    }

    #[test]
    fn build_tools_includes_native_code_execution_tool() {
        let request = LLMRequest {
            messages: vec![Message::user("run code".to_string())],
            tools: Some(Arc::new(vec![ToolDefinition {
                tool_type: "code_execution_20250825".to_string(),
                function: None,
                allowed_callers: None,
                input_examples: None,
                web_search: None,
                hosted_tool_config: None,
                shell: None,
                grammar: None,
                strict: None,
                defer_loading: None,
            }])),
            model: models::anthropic::DEFAULT_MODEL.to_string(),
            ..Default::default()
        };

        let tools = build_tools(&request, &None, &mut 0)
            .expect("tool build")
            .expect("tools should exist");
        assert!(matches!(
            &tools[0],
            AnthropicTool::CodeExecution(code_execution)
                if code_execution.tool_type == "code_execution_20250825"
                    && code_execution.name == "code_execution"
        ));
    }

    #[test]
    fn build_tools_includes_native_memory_tool() {
        let request = LLMRequest {
            messages: vec![Message::user("remember this preference".to_string())],
            tools: Some(Arc::new(vec![ToolDefinition {
                tool_type: "memory_20250818".to_string(),
                function: None,
                allowed_callers: None,
                input_examples: None,
                web_search: None,
                hosted_tool_config: None,
                shell: None,
                grammar: None,
                strict: None,
                defer_loading: None,
            }])),
            model: models::anthropic::DEFAULT_MODEL.to_string(),
            ..Default::default()
        };

        let tools = build_tools(&request, &None, &mut 0)
            .expect("tool build")
            .expect("tools should exist");
        assert!(matches!(
            &tools[0],
            AnthropicTool::Memory(memory)
                if memory.tool_type == "memory_20250818" && memory.name == "memory"
        ));
    }

    #[test]
    fn build_tools_preserves_allowed_callers_for_function_tools() {
        let mut tool = ToolDefinition::function(
            "get_weather".to_string(),
            "Get weather for a city".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string"}
                },
                "required": ["city"]
            }),
        );
        tool.allowed_callers = Some(vec!["code_execution_20250825".to_string()]);

        let request = LLMRequest {
            messages: vec![Message::user("find warmest city".to_string())],
            tools: Some(Arc::new(vec![tool])),
            model: models::anthropic::DEFAULT_MODEL.to_string(),
            ..Default::default()
        };

        let tools = build_tools(&request, &None, &mut 0)
            .expect("tool build")
            .expect("tools should exist");
        assert!(matches!(
            &tools[0],
            AnthropicTool::Function(function)
                if function.allowed_callers.as_ref()
                    == Some(&vec!["code_execution_20250825".to_string()])
        ));
    }

    #[test]
    fn build_tools_preserves_strict_and_input_examples_for_function_tools() {
        let tool = ToolDefinition::function(
            "get_weather".to_string(),
            "Get weather for a city".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string"}
                },
                "required": ["city"]
            }),
        )
        .with_strict(true)
        .with_input_examples(vec![json!({
            "input": "Weather in Paris",
            "tool_use": {
                "city": "Paris"
            }
        })]);

        let request = LLMRequest {
            messages: vec![Message::user("find warmest city".to_string())],
            tools: Some(Arc::new(vec![tool])),
            model: models::anthropic::DEFAULT_MODEL.to_string(),
            ..Default::default()
        };

        let tools = build_tools(&request, &None, &mut 0)
            .expect("tool build")
            .expect("tools should exist");
        assert!(matches!(
            &tools[0],
            AnthropicTool::Function(function)
                if function.strict == Some(true)
                    && function.input_examples.as_ref()
                        == Some(&vec![json!({
                            "input": "Weather in Paris",
                            "tool_use": {
                                "city": "Paris"
                            }
                        })])
        ));
    }

    #[test]
    fn build_tool_choice_disables_parallel_tool_use_when_requested() {
        let request = LLMRequest {
            messages: vec![Message::user("hi".to_string())],
            model: models::anthropic::DEFAULT_MODEL.to_string(),
            tool_choice: Some(ToolChoice::auto()),
            parallel_tool_config: Some(Box::new(ParallelToolConfig::sequential_only())),
            ..Default::default()
        };

        assert_eq!(
            build_tool_choice(&request, &None),
            Some(json!({
                "type": "auto",
                "disable_parallel_tool_use": true
            }))
        );
    }
}
