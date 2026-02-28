use crate::llm::provider::LLMRequest;
use crate::llm::providers::anthropic_types::{
    AnthropicFunctionTool, AnthropicTool, AnthropicToolSearchTool, AnthropicWebSearchTool,
    CacheControl, ThinkingConfig,
};
use serde_json::{Value, json};

use super::super::capabilities::supports_structured_output;

pub(crate) fn build_tools(
    request: &LLMRequest,
    cache_control: &Option<CacheControl>,
    breakpoints_remaining: &mut usize,
) -> Option<Vec<AnthropicTool>> {
    let request_tools = request.tools.as_ref()?;
    if request_tools.is_empty() {
        return None;
    }

    let mut built_tools: Vec<AnthropicTool> = request_tools
        .iter()
        .filter_map(|tool| {
            if tool.is_tool_search() {
                let func = tool.function.as_ref()?;
                return Some(AnthropicTool::ToolSearch(AnthropicToolSearchTool {
                    tool_type: tool.tool_type.clone(),
                    name: func.name.clone(),
                }));
            }

            if tool.is_anthropic_web_search() {
                return Some(AnthropicTool::WebSearch(AnthropicWebSearchTool {
                    tool_type: tool.tool_type.clone(),
                    name: "web_search".to_string(),
                }));
            }

            let func = tool.function.as_ref()?;
            Some(AnthropicTool::Function(AnthropicFunctionTool {
                name: func.name.clone(),
                description: func.description.clone(),
                input_schema: func.parameters.clone(),
                cache_control: None,
                defer_loading: tool.defer_loading,
            }))
        })
        .collect();

    if *breakpoints_remaining > 0
        && let Some(cc) = cache_control.as_ref()
        && let Some(last_tool) = built_tools.last_mut()
        && let AnthropicTool::Function(func_tool) = last_tool
    {
        func_tool.cache_control = Some(cc.clone());
        *breakpoints_remaining -= 1;
    }

    if built_tools.is_empty() {
        None
    } else {
        Some(built_tools)
    }
}

pub(crate) fn append_structured_output_tool(
    request: &LLMRequest,
    tools: Option<Vec<AnthropicTool>>,
    default_model: &str,
) -> Option<Vec<AnthropicTool>> {
    let Some(schema) = &request.output_format else {
        return tools;
    };
    if !supports_structured_output(&request.model, default_model) {
        return tools;
    }

    let structured_tool = AnthropicTool::Function(AnthropicFunctionTool {
        name: "structured_output".to_string(),
        description:
            "Forces Claude to respond in a specific JSON format according to the provided schema"
                .to_string(),
        input_schema: schema.clone(),
        cache_control: None,
        defer_loading: None,
    });

    match tools {
        Some(mut tools_vec) => {
            tools_vec.push(structured_tool);
            Some(tools_vec)
        }
        None => Some(vec![structured_tool]),
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

    if request.output_format.is_some() && thinking_val.is_none() {
        final_tool_choice = Some(json!({
            "type": "tool",
            "name": "structured_output"
        }));
    }

    final_tool_choice
}
