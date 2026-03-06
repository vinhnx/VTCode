use super::*;
use crate::llm::provider::tool::sanitize_tool_description;
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
    assert!(
        ToolDefinition::file_search(json!({"vector_store_ids": ["vs_docs"]}))
            .validate()
            .is_ok()
    );
    assert!(
        ToolDefinition::file_search(json!(["vs_docs"]))
            .validate()
            .is_err()
    );
}

#[test]
fn mcp_tool_definition_requires_object_config() {
    assert!(
        ToolDefinition::mcp(json!({
            "server_label": "dmcp",
            "server_url": "https://dmcp-server.deno.dev/sse",
            "require_approval": "never"
        }))
        .validate()
        .is_ok()
    );
    assert!(ToolDefinition::mcp(json!("dmcp")).validate().is_err());
}
