//! Conversion between MCP tool descriptors and the canonical
//! [`ToolDefinition`](vtcode_llm::provider::ToolDefinition) model.
//!
//! MCP tools arrive as JSON Schema 2020-12 payloads (see
//! [`McpToolInfo`](crate::types::McpToolInfo)) and have to be projected into the
//! unified `ToolDefinition` so the runloop can hand them to whichever provider
//! formatter is active. Historically this conversion happened inline inside the
//! core registry (`crates/codegen/vtcode-core/src/tools/registry/mcp_facade.rs`); this module
//! factors it into a named, testable adapter.
//!
//! See *The Hitchhiker's Guide to Agentic AI* §18.4.5 for the underlying MCP
//! architecture (servers expose tools; clients discover them via `tools/list`
//! and invoke via `tools/call`).

use serde_json::{Map, Value};

use crate::types::McpToolInfo;
use vtcode_llm::provider::ToolDefinition;

/// Default prefix applied to MCP tool names so they don't collide with
/// built-in tools in the unified catalog.
///
/// The separator mirrors the runtime convention used in
/// `crates/codegen/vtcode-core/src/tools/mcp.rs::build_mcp_registration` (i.e. `mcp::provider::tool`)
/// so the tool catalog stays consistent end-to-end.
pub const DEFAULT_MCP_TOOL_NAME_PREFIX: &str = "mcp";

/// Separator used between `prefix`, `provider`, and `tool_name` segments in the
/// canonical MCP tool name. Kept in sync with the parser in
/// `crates/codegen/vtcode-core/src/tools/mcp.rs::parse_canonical_mcp_tool_name`.
pub const MCP_TOOL_NAME_SEPARATOR: &str = "::";

/// Convert an MCP tool descriptor into the canonical `ToolDefinition`.
///
/// When the input schema is not a JSON object (MCP permits null / boolean
/// schemas for tools with no parameters), we coerce it to an empty
/// `{"type": "object"}` shape so downstream formatters don't have to
/// special-case it.
#[must_use]
pub fn mcp_tool_to_definition(info: &McpToolInfo) -> ToolDefinition {
    mcp_tool_to_definition_with_prefix(info, DEFAULT_MCP_TOOL_NAME_PREFIX)
}

/// Like [`mcp_tool_to_definition`] but lets the caller override the prefix
/// segment of the canonical name.
#[must_use]
pub fn mcp_tool_to_definition_with_prefix(info: &McpToolInfo, prefix: &str) -> ToolDefinition {
    let canonical_name = canonical_tool_name(prefix, &info.provider, &info.name);
    let parameters = normalize_input_schema(info.input_schema.clone());
    let description = info.description.trim().to_owned();

    ToolDefinition::function(canonical_name, description, parameters)
}

/// Best-effort conversion from a canonical `ToolDefinition` back into an MCP tool
/// descriptor. Used by tests, MCP-style export, and the `mcp_tool_info` re-export
/// that the core registry expects.
///
/// Returns `None` when the tool has no function payload (hosted / native tools
/// are not representable as MCP tools today).
#[must_use]
pub fn definition_to_mcp_tool(tool: &ToolDefinition, provider: &str) -> Option<McpToolInfo> {
    let func = tool.function.as_ref()?;
    let name = strip_canonical_prefix(&func.name, DEFAULT_MCP_TOOL_NAME_PREFIX, provider)?;
    Some(McpToolInfo {
        name: name.to_owned(),
        description: func.description.clone(),
        provider: provider.to_owned(),
        input_schema: func.parameters.clone(),
    })
}

/// Build the canonical MCP tool name from a prefix, provider identifier, and
/// tool name (e.g. `("mcp", "fetch", "fetch") -> "mcp::fetch::fetch"`).
#[must_use]
pub fn canonical_tool_name(prefix: &str, provider: &str, tool_name: &str) -> String {
    let mut out = String::with_capacity(prefix.len() + provider.len() + tool_name.len() + 4);
    out.push_str(prefix);
    out.push_str(MCP_TOOL_NAME_SEPARATOR);
    out.push_str(provider);
    out.push_str(MCP_TOOL_NAME_SEPARATOR);
    out.push_str(tool_name);
    out
}

fn strip_canonical_prefix<'a>(full: &'a str, prefix: &str, provider: &str) -> Option<&'a str> {
    let expected = format!("{prefix}{MCP_TOOL_NAME_SEPARATOR}{provider}{MCP_TOOL_NAME_SEPARATOR}");
    full.strip_prefix(expected.as_str())
}

fn normalize_input_schema(value: Value) -> Value {
    match value {
        Value::Object(map) => ensure_object_schema(map),
        // MCP permits `null` or `true`/`false` schemas; coerce to a permissive
        // empty object so providers downstream don't have to special-case them.
        Value::Null | Value::Bool(_) => ensure_object_schema(Map::new()),
        // Arrays / scalars aren't valid input schemas for our purposes; fall
        // back to an empty object. Callers that need the original can recover
        // it from the audit log.
        _ => ensure_object_schema(Map::new()),
    }
}

fn ensure_object_schema(mut map: Map<String, Value>) -> Value {
    let needs_type = matches!(map.get("type"), None | Some(Value::Null));
    if needs_type {
        map.insert("type".to_owned(), Value::String("object".to_owned()));
    }
    if !map.contains_key("properties") {
        map.insert("properties".to_owned(), Value::Object(Map::new()));
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_mcp_tool() -> McpToolInfo {
        McpToolInfo {
            name: "fetch".to_owned(),
            description: "Fetch a URL and return its contents.".to_owned(),
            provider: "fetch".to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string"}
                },
                "required": ["url"]
            }),
        }
    }

    #[test]
    fn mcp_tool_to_definition_uses_canonical_name() {
        let tool = mcp_tool_to_definition(&sample_mcp_tool());
        assert_eq!(tool.tool_type, "function");
        let func = tool.function.expect("function definition");
        assert_eq!(func.name, "mcp::fetch::fetch");
        assert_eq!(func.description, "Fetch a URL and return its contents.");
    }

    #[test]
    fn mcp_tool_to_definition_normalizes_missing_schema() {
        let mut info = sample_mcp_tool();
        info.input_schema = Value::Null;
        let tool = mcp_tool_to_definition(&info);
        let func = tool.function.expect("function definition");
        assert_eq!(func.parameters["type"], "object");
        assert!(func.parameters["properties"].is_object());
    }

    #[test]
    fn mcp_tool_to_definition_preserves_object_schema_unchanged() {
        let info = sample_mcp_tool();
        let tool = mcp_tool_to_definition(&info);
        let func = tool.function.expect("function definition");
        assert_eq!(func.parameters["required"][0], "url");
    }

    #[test]
    fn mcp_tool_to_definition_with_custom_prefix() {
        let info = sample_mcp_tool();
        let tool = mcp_tool_to_definition_with_prefix(&info, "ext");
        let func = tool.function.expect("function definition");
        assert_eq!(func.name, "ext::fetch::fetch");
    }

    #[test]
    fn definition_to_mcp_tool_round_trips() {
        let info = sample_mcp_tool();
        let tool = mcp_tool_to_definition(&info);
        let recovered = definition_to_mcp_tool(&tool, "fetch").expect("reverse direction");
        assert_eq!(recovered.name, "fetch");
        assert_eq!(recovered.description, info.description);
        assert_eq!(recovered.provider, "fetch");
        assert_eq!(recovered.input_schema, info.input_schema);
    }

    #[test]
    fn definition_to_mcp_tool_rejects_native_tools() {
        let tool = ToolDefinition::web_search(json!({ "max_uses": 5 }));
        assert!(definition_to_mcp_tool(&tool, "fetch").is_none());
    }

    #[test]
    fn definition_to_mcp_tool_rejects_unrelated_canonical_names() {
        let tool =
            ToolDefinition::function("search_docs".to_owned(), "Search docs".to_owned(), json!({"type": "object"}));
        // The canonical name doesn't start with `mcp::fetch::`, so the reverse
        // conversion refuses it (instead of returning a bogus prefix-stripped
        // name).
        assert!(definition_to_mcp_tool(&tool, "fetch").is_none());
    }

    #[test]
    fn normalize_input_schema_handles_boolean_schema() {
        let normalized = normalize_input_schema(Value::Bool(true));
        assert_eq!(normalized["type"], "object");
        assert!(normalized["properties"].is_object());
    }

    #[test]
    fn normalize_input_schema_preserves_partial_object() {
        let value = json!({"properties": {"x": {"type": "number"}}});
        let normalized = normalize_input_schema(value);
        assert_eq!(normalized["type"], "object");
        assert_eq!(normalized["properties"]["x"]["type"], "number");
    }
}
