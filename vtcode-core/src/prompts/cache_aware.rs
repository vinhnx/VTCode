use crate::llm::provider::ToolDefinition;

/// Priority tools (unified tools + critical controls) that should appear first
const PRIORITY_TOOLS: &[&str] = &[
    "unified_search",
    "unified_file",
    "unified_exec",
    "ask_user_question",
    "task_tracker",
    "exit_plan_mode",
];

/// Sort tool definitions with priority grouping to improve LLM attention.
/// Priority tools appear first (better attention), then alphabetically.
/// This can save ~50-100 tokens per request via improved LLM focus.
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

        // Find priority positions (None = not priority = end of list)
        let a_priority = PRIORITY_TOOLS.iter().position(|&p| p == a_name);
        let b_priority = PRIORITY_TOOLS.iter().position(|&p| p == b_name);

        match (a_priority, b_priority) {
            // Both priority - sort by priority order
            (Some(a_pos), Some(b_pos)) => a_pos.cmp(&b_pos),
            // Only a is priority - a comes first
            (Some(_), None) => std::cmp::Ordering::Less,
            // Only b is priority - b comes first
            (None, Some(_)) => std::cmp::Ordering::Greater,
            // Neither priority - sort alphabetically
            (None, None) => {
                let name_cmp = a_name.cmp(b_name);
                if name_cmp != std::cmp::Ordering::Equal {
                    return name_cmp;
                }
                a.tool_type.cmp(&b.tool_type)
            }
        }
    });
    tools
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::PRIORITY_TOOLS;
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

    #[test]
    fn sort_tool_definitions_prioritizes_unified_tools() {
        let tools = vec![
            ToolDefinition::function(
                "zebra_tool".to_string(),
                "z".to_string(),
                serde_json::json!({}),
            ),
            ToolDefinition::function(
                "unified_search".to_string(),
                "search".to_string(),
                serde_json::json!({}),
            ),
            ToolDefinition::function(
                "ask_user_question".to_string(),
                "ask".to_string(),
                serde_json::json!({}),
            ),
            ToolDefinition::function(
                "alpha_tool".to_string(),
                "a".to_string(),
                serde_json::json!({}),
            ),
            ToolDefinition::function(
                "unified_file".to_string(),
                "file".to_string(),
                serde_json::json!({}),
            ),
        ];

        let sorted = sort_tool_definitions(tools);
        let names: Vec<&str> = sorted
            .iter()
            .filter_map(|tool| tool.function.as_ref().map(|func| func.name.as_str()))
            .collect();

        // Priority tools first (in priority order), then alphabetical
        assert_eq!(
            names,
            vec![
                "unified_search",
                "unified_file",
                "ask_user_question",
                "alpha_tool",
                "zebra_tool"
            ]
        );
    }

    #[test]
    fn priority_tools_are_unique() {
        let unique: HashSet<&str> = PRIORITY_TOOLS.iter().copied().collect();
        assert_eq!(unique.len(), PRIORITY_TOOLS.len());
    }
}
