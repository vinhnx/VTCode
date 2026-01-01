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
            tools::AGENT_INFO,
            tools::WEB_FETCH,
            tools::SKILL,
            tools::SEARCH_TOOLS,
            tools::SEARCH,
            tools::FIND,
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
        .with_aliases([
            tools::RUN_PTY_CMD,
            tools::EXECUTE_CODE,
            tools::CREATE_PTY_SESSION,
            tools::LIST_PTY_SESSIONS,
            tools::CLOSE_PTY_SESSION,
            tools::SEND_PTY_INPUT,
            tools::READ_PTY_SESSION,
            tools::EXEC_PTY_CMD,
            tools::EXEC,
            tools::SHELL,
        ]),
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
            tools::MOVE_FILE,
            tools::COPY_FILE,
            tools::FILE_OP,
        ]),
        // ============================================================
        // INTERNAL TOOLS (Hidden from LLM, used by unified tools)
        // ============================================================
        ToolRegistration::new(
            tools::READ_FILE,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::read_file_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::WRITE_FILE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::write_file_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::EDIT_FILE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::edit_file_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::GREP_FILE,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::grep_file_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::LIST_FILES,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::list_files_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::RUN_PTY_CMD,
            CapabilityLevel::Bash,
            true,
            ToolRegistry::run_pty_cmd_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::SEND_PTY_INPUT,
            CapabilityLevel::Bash,
            true,
            ToolRegistry::send_pty_input_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::READ_PTY_SESSION,
            CapabilityLevel::Bash,
            true,
            ToolRegistry::read_pty_session_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::LIST_PTY_SESSIONS,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::list_pty_sessions_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::CLOSE_PTY_SESSION,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::close_pty_session_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::CODE_INTELLIGENCE,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::code_intelligence_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::GET_ERRORS,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::get_errors_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::AGENT_INFO,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::agent_info_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::SEARCH_TOOLS,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::search_tools_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::APPLY_PATCH,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::apply_patch_executor,
        )
        .with_llm_visibility(false),
    ]
}
