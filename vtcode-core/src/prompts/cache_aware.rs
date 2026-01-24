use crate::llm::provider::ToolDefinition;

/// Sort tool definitions deterministically to improve prompt cache hit rates.
pub fn sort_tool_definitions(mut tools: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
    tools.sort_by(|a, b| {
        let a_name = a
            .function
            .as_ref()
            .map(|func| func.name.as_str())
            .unwrap_or("");
        let b_name = b
            .function
            .as_ref()
            .map(|func| func.name.as_str())
            .unwrap_or("");
        let name_cmp = a_name.cmp(b_name);
        if name_cmp != std::cmp::Ordering::Equal {
            return name_cmp;
        }
        a.tool_type.cmp(&b.tool_type)
    });
    tools
}

#[cfg(test)]
mod tests {
    use super::sort_tool_definitions;
    use crate::llm::provider::ToolDefinition;

    #[test]
    fn sort_tool_definitions_orders_by_name() {
        let tools = vec![
            ToolDefinition::function("b_tool".to_string(), "b".to_string(), serde_json::json!({})),
            ToolDefinition::function("a_tool".to_string(), "a".to_string(), serde_json::json!({})),
        ];

        let sorted = sort_tool_definitions(tools);
        let names: Vec<&str> = sorted
            .iter()
            .filter_map(|tool| tool.function.as_ref().map(|func| func.name.as_str()))
            .collect();

        assert_eq!(names, vec!["a_tool", "b_tool"]);
    }
}
