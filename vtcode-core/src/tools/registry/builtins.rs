use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;

use super::registration::ToolRegistration;
use super::{ToolInventory, ToolRegistry};

pub(super) fn register_builtin_tools(inventory: &mut ToolInventory, todo_planning_enabled: bool) {
    for registration in builtin_tool_registrations() {
        if !todo_planning_enabled && registration.name() == tools::UPDATE_PLAN {
            continue;
        }
        if registration.name() == tools::AST_GREP_SEARCH && inventory.ast_grep_engine().is_none() {
            continue;
        }

        let tool_name = registration.name();
        if let Err(err) = inventory.register_tool(registration) {
            eprintln!("Warning: Failed to register tool '{}': {}", tool_name, err);
        }
    }
}

pub(super) fn builtin_tool_registrations() -> Vec<ToolRegistration> {
    vec![
        ToolRegistration::new(
            tools::GREP_SEARCH,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::grep_search_executor,
        ),
        ToolRegistration::new(
            tools::LIST_FILES,
            CapabilityLevel::FileListing,
            false,
            ToolRegistry::list_files_executor,
        ),
        ToolRegistration::new(
            tools::UPDATE_PLAN,
            CapabilityLevel::Basic,
            false,
            ToolRegistry::update_plan_executor,
        ),
        ToolRegistration::new(
            tools::RUN_TERMINAL_CMD,
            CapabilityLevel::Bash,
            true,
            ToolRegistry::run_terminal_cmd_executor,
        ),
        ToolRegistration::new(
            tools::RUN_PTY_CMD,
            CapabilityLevel::Bash,
            true,
            ToolRegistry::run_pty_cmd_executor,
        ),
        ToolRegistration::new(
            tools::CREATE_PTY_SESSION,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::create_pty_session_executor,
        ),
        ToolRegistration::new(
            tools::LIST_PTY_SESSIONS,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::list_pty_sessions_executor,
        ),
        ToolRegistration::new(
            tools::CLOSE_PTY_SESSION,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::close_pty_session_executor,
        ),
        ToolRegistration::new(
            tools::SEND_PTY_INPUT,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::send_pty_input_executor,
        ),
        ToolRegistration::new(
            tools::READ_PTY_SESSION,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::read_pty_session_executor,
        ),
        ToolRegistration::new(
            tools::RESIZE_PTY_SESSION,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::resize_pty_session_executor,
        ),
        ToolRegistration::new(
            tools::CURL,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::curl_executor,
        ),
        ToolRegistration::new(
            tools::READ_FILE,
            CapabilityLevel::FileReading,
            false,
            ToolRegistry::read_file_executor,
        ),
        ToolRegistration::new(
            tools::GIT_DIFF,
            CapabilityLevel::FileReading,
            false,
            ToolRegistry::git_diff_executor,
        ),
        ToolRegistration::new(
            tools::WRITE_FILE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::write_file_executor,
        ),
        ToolRegistration::new(
            tools::EDIT_FILE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::edit_file_executor,
        ),
        ToolRegistration::new(
            tools::AST_GREP_SEARCH,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::ast_grep_executor,
        ),
        ToolRegistration::new(
            tools::SIMPLE_SEARCH,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::simple_search_executor,
        ),
        ToolRegistration::new(
            tools::BASH,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::bash_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::APPLY_PATCH,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::apply_patch_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::SRGN,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::srgn_executor,
        ),
    ]
}
