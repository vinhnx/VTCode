use vtcode_core::llm::provider as uni;
use vtcode_core::mcp::McpToolInfo;

fn build_single_mcp_tool_definition(tool: &McpToolInfo) -> uni::ToolDefinition {
    let parameters = vtcode_core::llm::providers::gemini::sanitize_function_parameters(
        tool.input_schema.clone(),
    );
    let description = if tool.description.trim().is_empty() {
        format!("MCP tool from provider '{}'", tool.provider)
    } else {
        format!(
            "MCP tool from provider '{}': {}",
            tool.provider, tool.description
        )
    };

    uni::ToolDefinition::function(format!("mcp_{}", tool.name), description, parameters)
}

pub fn build_mcp_tool_definitions(tools: &[McpToolInfo]) -> Vec<uni::ToolDefinition> {
    tools.iter().map(build_single_mcp_tool_definition).collect()
}
