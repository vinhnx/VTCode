mod catalog;
mod schemas;
mod titles;

pub use super::tooling_provider::ToolRegistryProvider;
pub use catalog::{AcpToolRegistry, SupportedTool, ToolDescriptor};
pub use schemas::{
    TOOL_LIST_FILES_CASE_SENSITIVE_ARG, TOOL_LIST_FILES_CONTENT_PATTERN_ARG,
    TOOL_LIST_FILES_DESCRIPTION, TOOL_LIST_FILES_FILE_EXTENSIONS_ARG,
    TOOL_LIST_FILES_INCLUDE_HIDDEN_ARG, TOOL_LIST_FILES_ITEMS_KEY, TOOL_LIST_FILES_MAX_ITEMS_ARG,
    TOOL_LIST_FILES_MESSAGE_KEY, TOOL_LIST_FILES_MODE_ARG, TOOL_LIST_FILES_NAME_PATTERN_ARG,
    TOOL_LIST_FILES_PAGE_ARG, TOOL_LIST_FILES_PATH_ARG, TOOL_LIST_FILES_PER_PAGE_ARG,
    TOOL_LIST_FILES_RESPONSE_FORMAT_ARG, TOOL_LIST_FILES_RESULT_KEY,
    TOOL_LIST_FILES_SUMMARY_MAX_ITEMS, TOOL_LIST_FILES_URI_ARG, TOOL_READ_FILE_DESCRIPTION,
    TOOL_READ_FILE_LIMIT_ARG, TOOL_READ_FILE_LINE_ARG, TOOL_READ_FILE_PATH_ARG,
    TOOL_READ_FILE_URI_ARG,
};

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;
    use vtcode_core::config::constants::tools;
    use vtcode_core::llm::provider::ToolDefinition;

    use super::{AcpToolRegistry, SupportedTool, ToolDescriptor};

    fn local_definition(name: &str) -> ToolDefinition {
        ToolDefinition::function(
            name.to_string(),
            format!("{name} description"),
            json!({"type": "object"}),
        )
    }

    #[test]
    fn definitions_for_preserve_core_local_tool_order() {
        let local_definitions = vec![
            local_definition(tools::UNIFIED_FILE),
            local_definition(tools::UNIFIED_EXEC),
            local_definition(tools::UNIFIED_SEARCH),
        ];
        let registry =
            AcpToolRegistry::new(Path::new("/tmp/workspace"), true, true, local_definitions);

        let definitions =
            registry.definitions_for(&[SupportedTool::ReadFile, SupportedTool::SwitchMode], true);
        let names = definitions
            .into_iter()
            .map(|definition| definition.function_name().to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                tools::READ_FILE.to_string(),
                "switch_mode".to_string(),
                tools::UNIFIED_FILE.to_string(),
                tools::UNIFIED_EXEC.to_string(),
                tools::UNIFIED_SEARCH.to_string(),
            ]
        );
    }

    #[test]
    fn lookup_checks_native_map_before_local_membership() {
        let registry = AcpToolRegistry::new(
            Path::new("/tmp/workspace"),
            true,
            false,
            vec![local_definition(tools::UNIFIED_SEARCH)],
        );

        assert_eq!(
            registry.lookup(tools::READ_FILE),
            Some(ToolDescriptor::Acp(SupportedTool::ReadFile))
        );
        assert_eq!(
            registry.lookup(tools::UNIFIED_SEARCH),
            Some(ToolDescriptor::Local)
        );
        assert_eq!(registry.lookup("unknown_tool"), None);
    }
}
