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
        // SEARCH & DISCOVERY (1 tool - unified)
        // ============================================================
        ToolRegistration::new(
            tools::UNIFIED_SEARCH,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::unified_search_executor,
        )
        .with_aliases([
            tools::GREP_FILE,
            tools::LIST_FILES,
            tools::CODE_INTELLIGENCE,
            "search",
            "find",
        ]),
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
        // FILE OPERATIONS (1 tool - unified)
        // ============================================================
        ToolRegistration::new(
            tools::UNIFIED_FILE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::unified_file_executor,
        )
        .with_aliases([
            tools::READ_FILE,
            tools::WRITE_FILE,
            tools::DELETE_FILE,
            tools::EDIT_FILE,
            tools::APPLY_PATCH,
            tools::CREATE_FILE,
            "file_op",
        ]),
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
    ]
}
