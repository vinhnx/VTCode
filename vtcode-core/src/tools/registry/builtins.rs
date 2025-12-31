use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;

use super::registration::ToolRegistration;
use super::{ToolInventory, ToolRegistry};

pub(super) fn register_builtin_tools(inventory: &mut ToolInventory) {
    for registration in builtin_tool_registrations() {
        let tool_name = registration.name();
        if let Err(err) = inventory.register_tool(registration) {
            eprintln!("Warning: Failed to register tool '{}': {}", tool_name, err);
        }
    }
}

pub(super) fn builtin_tool_registrations() -> Vec<ToolRegistration> {
    vec![
        // ============================================================
        // SEARCH & DISCOVERY (4 tools - all exposed)
        // ============================================================
        ToolRegistration::new(
            tools::GREP_FILE,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::grep_file_executor,
        ),
        ToolRegistration::new(
            tools::LIST_FILES,
            CapabilityLevel::FileListing,
            false,
            ToolRegistry::list_files_executor,
        ),
        ToolRegistration::new(
            tools::SEARCH_TOOLS,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::search_tools_executor,
        ),
        ToolRegistration::new(
            tools::CODE_INTELLIGENCE,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::code_intelligence_executor,
        ),
        // ============================================================
        // SHELL EXECUTION (1 tool - unified)
        // ============================================================
        ToolRegistration::new(
            tools::UNIFIED_EXEC,
            CapabilityLevel::Bash,
            true,
            ToolRegistry::unified_exec_executor,
        )
        .with_aliases([tools::RUN_PTY_CMD, "exec_pty_cmd", "exec", "shell"]),
        // ============================================================
        // FILE OPERATIONS (5 exposed + 2 deprecated)
        // ============================================================
        ToolRegistration::new(
            tools::READ_FILE,
            CapabilityLevel::FileReading,
            false,
            ToolRegistry::read_file_executor,
        ),
        ToolRegistration::new(
            tools::WRITE_FILE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::write_file_executor,
        ),
        ToolRegistration::new(
            tools::DELETE_FILE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::delete_file_executor,
        ),
        ToolRegistration::new(
            tools::EDIT_FILE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::edit_file_executor,
        ),
        ToolRegistration::new(
            tools::APPLY_PATCH,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::apply_patch_executor,
        ),
        // Deprecated: use write_file with mode=fail_if_exists
        ToolRegistration::new(
            tools::CREATE_FILE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::create_file_executor,
        )
        .with_llm_visibility(false)
        .with_deprecated(true)
        .with_deprecation_message("use write_file with mode=fail_if_exists"),
        // Deprecated: use edit_file instead
        ToolRegistration::new(
            tools::SEARCH_REPLACE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::search_replace_executor,
        )
        .with_llm_visibility(false)
        .with_deprecated(true)
        .with_deprecation_message("use edit_file instead"),
        // ============================================================
        // NETWORK & WEB (1 tool)
        // ============================================================
        ToolRegistration::new(
            tools::WEB_FETCH,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::web_fetch_executor,
        ),
        // ============================================================
        // SPECIAL TOOLS (3 exposed + 2 deprecated)
        // ============================================================
        ToolRegistration::new(
            tools::SKILL,
            CapabilityLevel::Basic,
            false,
            ToolRegistry::skill_executor,
        ),
        ToolRegistration::new(
            tools::EXECUTE_CODE,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::execute_code_executor,
        )
        .with_aliases(["exec_code"]),
        // Merged agent diagnostics tool
        ToolRegistration::new(
            tools::AGENT_INFO,
            CapabilityLevel::Basic,
            false,
            ToolRegistry::agent_info_executor,
        ),
    ]
}
