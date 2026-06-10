use std::sync::Arc;

pub(crate) type ToolCatalogState = vtcode_core::tools::registry::SessionToolCatalogState;

pub(crate) fn tool_catalog_change_notifier(
    tool_catalog: &Arc<ToolCatalogState>,
) -> Arc<dyn Fn(&'static str) + Send + Sync> {
    tool_catalog.change_notifier()
}

#[expect(dead_code)]
pub(crate) fn should_expose_tool_in_mode(
    tool: &vtcode_core::llm::provider::ToolDefinition,
    planning_active: bool,
    request_user_input_enabled: bool,
) -> bool {
    vtcode_core::core::agent::harness_kernel::should_expose_tool_in_mode(
        tool,
        planning_active,
        request_user_input_enabled,
    )
}

#[expect(dead_code)]
pub(crate) fn filter_tools_for_mode(
    tools: Option<Arc<Vec<vtcode_core::llm::provider::ToolDefinition>>>,
    planning_active: bool,
    request_user_input_enabled: bool,
) -> Option<Arc<Vec<vtcode_core::llm::provider::ToolDefinition>>> {
    vtcode_core::core::agent::harness_kernel::filter_tool_definitions_for_mode(
        tools,
        planning_active,
        request_user_input_enabled,
    )
}
