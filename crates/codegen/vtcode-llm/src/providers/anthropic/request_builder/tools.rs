use crate::provider::{LLMError, LLMRequest, ToolDefinition};
use crate::providers::anthropic_types::{
    AnthropicCodeExecutionTool, AnthropicFunctionTool, AnthropicMemoryTool, AnthropicTool, AnthropicToolSearchTool,
    AnthropicWebSearchTool, CacheControl, ThinkingConfig,
};
use serde_json::{Map, Value, json};

/// Format a slice of tool definitions for the Anthropic wire format.
///
/// This is the entry point used by [`crate::providers::tool_format::AnthropicFormatter`]
/// and lets the runloop project tool definitions without first assembling a full
/// `LLMRequest`. Cache control markers are intentionally omitted — callers that
/// need them should use [`build_tools`] directly with an `LLMRequest`.
pub(crate) fn build_tools_via_formatter(tools: &[ToolDefinition]) -> Option<Value> {
    if tools.is_empty() {
        return None;
    }

    let mut built_tools = Vec::with_capacity(tools.len());
    for tool in tools.iter() {
        if let Err(err) = push_anthropic_tool(&mut built_tools, tool) {
            tracing::warn!(
                error = %err,
                tool = %tool.tool_type,
                "AnthropicFormatter: failed to build tool entry, dropping tool"
            );
        }
    }

    serde_json::to_value(built_tools).ok()
}

fn push_anthropic_tool(built_tools: &mut Vec<AnthropicTool>, tool: &ToolDefinition) -> Result<(), LLMError> {
    if tool.is_tool_search() {
        let Some(func) = tool.function.as_ref() else {
            return Ok(());
        };
        built_tools.push(AnthropicTool::ToolSearch(AnthropicToolSearchTool {
            tool_type: tool.tool_type.clone(),
            name: func.name.clone(),
        }));
        return Ok(());
    }

    if tool.is_anthropic_web_search() {
        built_tools.push(AnthropicTool::WebSearch(AnthropicWebSearchTool {
            tool_type: tool.tool_type.clone(),
            name: "web_search".to_string(),
            options: anthropic_web_search_options(tool)?,
        }));
        return Ok(());
    }

    if tool.is_anthropic_code_execution() {
        built_tools.push(AnthropicTool::CodeExecution(AnthropicCodeExecutionTool {
            tool_type: tool.tool_type.clone(),
            name: "code_execution".to_string(),
        }));
        return Ok(());
    }

    if tool.is_anthropic_memory_tool() {
        built_tools.push(AnthropicTool::Memory(AnthropicMemoryTool {
            tool_type: tool.tool_type.clone(),
            name: "memory".to_string(),
        }));
        return Ok(());
    }

    let Some(func) = tool.function.as_ref() else {
        return Ok(());
    };
    built_tools.push(AnthropicTool::Function(AnthropicFunctionTool {
        name: func.name.clone(),
        description: func.description.clone(),
        input_schema: strip_top_level_schema_composition(func.parameters.clone()),
        input_examples: tool.input_examples.clone(),
        strict: tool.strict,
        allowed_callers: tool.allowed_callers.clone(),
        cache_control: None,
        defer_loading: tool.defer_loading,
    }));
    Ok(())
}

/// Strips `oneOf`/`anyOf`/`allOf` when present at the *top level* of an
/// `input_schema`, leaving everything else -- including the same keywords
/// nested inside `properties` -- untouched.
///
/// Anthropic's tool-use API rejects `input_schema` outright when one of
/// these keywords appears at the root (`tools.N.custom.input_schema: input_schema
/// does not support oneOf, allOf, or anyOf at the top level`), but has no
/// such restriction on nested usage: a property like `{"anyOf": [...]}`
/// inside `properties.<name>` is valid and unaffected by this function.
///
/// Some built-in tool schemas author top-level `allOf` with `if`/`then`
/// conditional requirements (e.g. "if action == 'create' then items is
/// required") -- valid JSON Schema, but not something Anthropic's tool-use
/// validator accepts at the root. Dropping just the offending top-level key
/// (rather than the whole schema, or recursively stripping everywhere like
/// the Gemini sanitizer does) keeps `type`/`properties`/`required` and any
/// legitimately nested composition intact -- the model still sees every
/// parameter and its description; only the root-level conditional-validation
/// hint is lost, which Claude's tool-call generation does not depend on.
fn strip_top_level_schema_composition(schema: Value) -> Value {
    let Value::Object(mut map) = schema else {
        return schema;
    };
    for keyword in ["oneOf", "anyOf", "allOf"] {
        map.remove(keyword);
    }
    Value::Object(map)
}

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
        push_anthropic_tool(&mut built_tools, tool)?;
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
                    message: "anthropic web_search tools cannot set both allowed_domains and blocked_domains"
                        .to_string(),
                    metadata: None,
                });
            }

            Ok(config.clone())
        }
        Some(_) => Err(LLMError::Provider {
            message: format!("{} tool configuration must be a JSON object", tool.tool_type),
            metadata: None,
        }),
        None => Ok(Map::new()),
    }
}

pub(crate) fn build_tool_choice(request: &LLMRequest, thinking_val: &Option<ThinkingConfig>) -> Option<Value> {
    let mut final_tool_choice = request.tool_choice.as_ref().map(|tc| tc.to_provider_format("anthropic"));

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
    use crate::provider::{LLMRequest, Message, ParallelToolConfig, ToolChoice, ToolDefinition, ToolNamespace};
    use std::sync::Arc;
    use vtcode_config::constants::models;

    #[test]
    fn build_tools_keeps_apply_patch_as_function_tool() {
        let request = LLMRequest {
            messages: vec![Message::user("patch this file".to_string())].into(),
            tools: Some(Arc::new(vec![ToolDefinition::apply_patch("Apply VT Code patches".to_string())])),
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
            messages: vec![Message::user("search docs".to_string())].into(),
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
                namespace: None,
                advisor: None,
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
            messages: vec![Message::user("search docs".to_string())].into(),
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
                namespace: None,
                advisor: None,
            }])),
            model: models::anthropic::DEFAULT_MODEL.to_string(),
            ..Default::default()
        };

        build_tools(&request, &None, &mut 0).unwrap_err();
    }

    #[test]
    fn build_tools_includes_native_code_execution_tool() {
        let request = LLMRequest {
            messages: vec![Message::user("run code".to_string())].into(),
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
                namespace: None,
                advisor: None,
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
            messages: vec![Message::user("remember this preference".to_string())].into(),
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
                namespace: None,
                advisor: None,
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
            messages: vec![Message::user("find warmest city".to_string())].into(),
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
            messages: vec![Message::user("find warmest city".to_string())].into(),
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
    fn strip_top_level_schema_composition_removes_root_all_of() {
        let schema = json!({
            "type": "object",
            "properties": {"action": {"type": "string"}},
            "required": ["action"],
            "allOf": [
                {"if": {"properties": {"action": {"const": "create"}}}, "then": {"required": ["items"]}}
            ]
        });

        let stripped = strip_top_level_schema_composition(schema);

        assert!(stripped.get("allOf").is_none());
        assert_eq!(stripped["type"], "object");
        assert_eq!(stripped["required"], json!(["action"]));
        assert!(stripped["properties"]["action"].is_object());
    }

    #[test]
    fn strip_top_level_schema_composition_removes_root_one_of_and_any_of() {
        for keyword in ["oneOf", "anyOf"] {
            let schema = json!({"type": "object", keyword: [{"type": "string"}, {"type": "integer"}]});
            let stripped = strip_top_level_schema_composition(schema);
            assert!(stripped.get(keyword).is_none(), "expected {keyword} to be stripped");
            assert_eq!(stripped["type"], "object");
        }
    }

    #[test]
    fn strip_top_level_schema_composition_preserves_nested_any_of_in_property() {
        // Mirrors read_file's `indentation` property: legitimate nested
        // anyOf, which Anthropic accepts and this function must not touch.
        let schema = json!({
            "type": "object",
            "properties": {
                "indentation": {
                    "anyOf": [
                        {"type": "boolean"},
                        {"type": "object", "properties": {"max_levels": {"type": "integer"}}}
                    ]
                }
            }
        });

        let stripped = strip_top_level_schema_composition(schema.clone());

        assert_eq!(stripped, schema, "nested anyOf inside a property must be left untouched");
    }

    #[test]
    fn strip_top_level_schema_composition_noop_on_plain_schema() {
        let schema = json!({"type": "object", "properties": {"city": {"type": "string"}}});
        assert_eq!(strip_top_level_schema_composition(schema.clone()), schema);
    }

    #[test]
    fn build_tools_strips_top_level_all_of_from_function_tool_schema() {
        let tool = ToolDefinition::function(
            "task_tracker".to_string(),
            "Track tasks".to_string(),
            json!({
                "type": "object",
                "properties": {"action": {"type": "string"}},
                "required": ["action"],
                "allOf": [
                    {"if": {"properties": {"action": {"const": "create"}}}, "then": {"required": ["items"]}}
                ]
            }),
        );

        let request = LLMRequest {
            messages: vec![Message::user("track this".to_string())].into(),
            tools: Some(Arc::new(vec![tool])),
            model: models::anthropic::DEFAULT_MODEL.to_string(),
            ..Default::default()
        };

        let tools = build_tools(&request, &None, &mut 0)
            .expect("tool build")
            .expect("tools should exist");
        let AnthropicTool::Function(function) = &tools[0] else {
            panic!("expected function tool");
        };
        assert!(
            function.input_schema.get("allOf").is_none(),
            "top-level allOf must be stripped so Anthropic doesn't reject the tool schema"
        );
        assert_eq!(function.input_schema["required"], json!(["action"]));
    }

    #[test]
    fn build_tool_choice_disables_parallel_tool_use_when_requested() {
        let request = LLMRequest {
            messages: vec![Message::user("hi".to_string())].into(),
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

    /// MANDATORY wire-payload safety test: client-side namespace metadata
    /// (used for local BM25 ranking and `by_group` search results) must
    /// never reach either provider's wire format. Both the Anthropic
    /// formatter and the OpenAI-format serializer build their JSON manually,
    /// field by field, rather than serde-serializing the whole
    /// `ToolDefinition` -- this test guards that invariant against
    /// regression (e.g. someone switching either formatter to
    /// `serde_json::json!(tool)`).
    #[test]
    fn namespace_metadata_never_leaks_onto_the_wire() {
        let namespaced_tool = ToolDefinition::function(
            "context7_search".to_string(),
            "Search context7 docs".to_string(),
            json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"]
            }),
        )
        .with_defer_loading(true)
        .with_namespace(ToolNamespace {
            name: "context7".to_string(),
            description: "Tools provided by MCP server 'context7'".to_string(),
        });
        let tools = vec![namespaced_tool];

        // Anthropic formatter: builds `AnthropicFunctionTool` manually,
        // field by field.
        let anthropic_value = build_tools_via_formatter(&tools)
            .expect("anthropic formatter should produce a value for a non-empty tool list");
        let anthropic_json = serde_json::to_string(&anthropic_value).expect("serialize");
        assert!(
            !anthropic_json.contains("namespace"),
            "Anthropic wire payload must never contain namespace metadata: {anthropic_json}"
        );

        // OpenAI-format serializer: also builds JSON manually, field by
        // field, rather than serde-serializing the whole `ToolDefinition`.
        let openai_value = crate::providers::common::serialize_tools_openai_format(&tools)
            .expect("openai serializer should produce a value for a non-empty tool list");
        let openai_json = serde_json::to_string(&openai_value).expect("serialize");
        assert!(
            !openai_json.contains("namespace"),
            "OpenAI wire payload must never contain namespace metadata: {openai_json}"
        );
    }
}
