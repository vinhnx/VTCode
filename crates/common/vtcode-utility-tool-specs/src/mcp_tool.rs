use rmcp::model::Tool;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedMcpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[must_use]
pub fn parse_mcp_tool(tool: &Tool) -> ParsedMcpTool {
    ParsedMcpTool {
        name: tool.name.to_string(),
        description: tool.description.clone().unwrap_or_default().to_string(),
        input_schema: serde_json::to_value(&tool.input_schema).unwrap_or(Value::Null),
    }
}

#[cfg(test)]
mod tests {
    use super::{ParsedMcpTool, parse_mcp_tool};
    use rmcp::model::Tool;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn parse_mcp_tool_preserves_name_description_and_input_schema() {
        let input_schema = Arc::new(
            serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                }
            }))
            .expect("json object"),
        );
        let tool = Tool::new("search-docs", "Search documentation", input_schema);

        let parsed = parse_mcp_tool(&tool);
        assert_eq!(
            parsed,
            ParsedMcpTool {
                name: "search-docs".to_string(),
                description: "Search documentation".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    }
                }),
            }
        );
    }

    #[test]
    fn parse_mcp_tool_defaults_missing_description() {
        let input_schema = Arc::new(serde_json::from_value(json!({})).expect("json object"));
        let tool = Tool::new_with_raw("search-docs", None, input_schema);

        let parsed = parse_mcp_tool(&tool);
        assert_eq!(parsed.description, "");
        assert_eq!(parsed.input_schema, json!({}));
    }
}
