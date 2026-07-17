use super::*;
use crate::provider::tool::sanitize_tool_description;
use serde_json::json;

#[test]
fn sanitize_tool_description_trims_padding() {
    let input = "\n\nLine 1\nLine 2 \n";
    assert_eq!(sanitize_tool_description(input), "Line 1\nLine 2");
}

#[test]
fn sanitize_tool_description_preserves_internal_blank_lines() {
    let input = "Line 1\n\nLine 3";
    assert_eq!(sanitize_tool_description(input), input);
}

#[test]
fn tool_definition_function_uses_sanitized_description() {
    let tool = ToolDefinition::function(
        "demo".to_owned(),
        "  Line 1  \n".to_owned(),
        json!({"type": "object", "properties": {}}),
    );
    assert_eq!(tool.function.as_ref().unwrap().description, "Line 1");
}

#[test]
fn file_search_tool_definition_requires_object_config() {
    ToolDefinition::file_search(json!({"vector_store_ids": ["vs_docs"]}))
        .validate()
        .unwrap();
    assert!(ToolDefinition::file_search(json!(["vs_docs"])).validate().is_err());
}

#[test]
fn mcp_tool_definition_requires_object_config() {
    ToolDefinition::mcp(json!({
        "server_label": "dmcp",
        "server_url": "https://dmcp-server.deno.dev/sse",
        "require_approval": "never"
    }))
    .validate()
    .unwrap();
    assert!(ToolDefinition::mcp(json!("dmcp")).validate().is_err());
}

#[test]
fn google_maps_tool_definition_requires_object_config() {
    ToolDefinition::google_maps(json!({"center": "sf"})).validate().unwrap();
    assert!(ToolDefinition::google_maps(json!(["sf"])).validate().is_err());
}

#[test]
fn url_context_tool_definition_requires_object_config() {
    ToolDefinition::url_context(json!({"urls": ["https://example.com"]}))
        .validate()
        .unwrap();
    assert!(ToolDefinition::url_context(json!("https://example.com")).validate().is_err());
}

#[test]
fn code_execution_tool_definition_requires_object_config() {
    ToolDefinition::code_execution(json!({})).validate().unwrap();
    assert!(ToolDefinition::code_execution(json!("enabled")).validate().is_err());
}

#[test]
fn anthropic_web_search_tool_definition_accepts_object_config() {
    let tool = ToolDefinition {
        tool_type: "web_search_20250305".to_string(),
        function: None,
        allowed_callers: None,
        input_examples: None,
        web_search: Some(json!({
            "max_uses": 5,
            "allowed_domains": ["docs.rs"]
        })),
        hosted_tool_config: None,
        shell: None,
        grammar: None,
        strict: None,
        defer_loading: None,
        namespace: None,
        advisor: None,
    };

    tool.validate().unwrap();
}

#[test]
fn anthropic_web_search_tool_definition_rejects_non_object_config() {
    let tool = ToolDefinition {
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
    };

    assert!(tool.validate().is_err());
}

#[test]
fn anthropic_web_search_tool_definition_rejects_mixed_domain_filters() {
    let tool = ToolDefinition {
        tool_type: "web_search_20250305".to_string(),
        function: None,
        allowed_callers: None,
        input_examples: None,
        web_search: Some(json!({
            "allowed_domains": ["docs.rs"],
            "blocked_domains": ["example.com"]
        })),
        hosted_tool_config: None,
        shell: None,
        grammar: None,
        strict: None,
        defer_loading: None,
        namespace: None,
        advisor: None,
    };

    assert!(tool.validate().is_err());
}

#[test]
fn anthropic_code_execution_tool_definition_is_supported() {
    let tool = ToolDefinition {
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
    };

    tool.validate().unwrap();
    assert!(tool.is_anthropic_code_execution());
}

#[test]
fn anthropic_memory_tool_definition_is_supported() {
    let tool = ToolDefinition {
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
    };

    tool.validate().unwrap();
    assert!(tool.is_anthropic_memory_tool());
    assert_eq!(tool.function_name(), "memory");
}
