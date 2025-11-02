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
            tools::UPDATE_PLAN,
            CapabilityLevel::Basic,
            false,
            ToolRegistry::update_plan_executor,
        ),
        ToolRegistration::new(
            tools::RUN_COMMAND,
            CapabilityLevel::Bash,
            true,
            ToolRegistry::run_command_executor,
        )
        .with_deprecated(true)
        .with_deprecation_message("Use PTY session tools (create_pty_session, send_pty_input, read_pty_session) instead for better session management"),
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
            tools::CREATE_FILE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::create_file_executor,
        ),
        ToolRegistration::new(
            tools::DELETE_FILE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::delete_file_executor,
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
            tools::APPLY_PATCH,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::apply_patch_executor,
        )
        .with_llm_visibility(false),
    ]
}
