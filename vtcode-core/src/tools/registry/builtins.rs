use std::path::PathBuf;

use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::tool_policy::ToolPolicy;
use crate::tools::handlers::session_tool_catalog::{
    apply_patch_parameters, unified_exec_parameters, unified_file_parameters,
    unified_search_parameters,
};
use crate::tools::handlers::{
    EnterPlanModeTool, ExitPlanModeTool, PlanModeState, PlanTaskTrackerTool, TaskTrackerTool,
};
use crate::tools::request_user_input::RequestUserInputTool;

use super::registration::ToolRegistration;
use super::{ToolInventory, ToolRegistry};

/// Register all builtin tools into the inventory using the shared plan mode state.
pub(super) fn register_builtin_tools(inventory: &ToolInventory, plan_mode_state: &PlanModeState) {
    for registration in builtin_tool_registrations(Some(plan_mode_state)) {
        let tool_name = registration.name().to_string();
        if let Err(err) = inventory.register_tool(registration) {
            tracing::warn!(tool = %tool_name, %err, "Failed to register builtin tool");
        }
    }
}

/// Build builtin tool registrations. In metadata-only contexts (e.g., declaration building),
/// callers may pass `None`, and a placeholder PlanModeState will be used.
pub(super) fn builtin_tool_registrations(
    plan_mode_state: Option<&PlanModeState>,
) -> Vec<ToolRegistration> {
    let plan_state = plan_mode_state
        .cloned()
        .unwrap_or_else(|| PlanModeState::new(PathBuf::new()));

    vec![
        // ============================================================
        // HUMAN-IN-THE-LOOP (HITL)
        // ============================================================
        ToolRegistration::from_tool_instance(
            tools::REQUEST_USER_INPUT,
            CapabilityLevel::Basic,
            RequestUserInputTool,
        ),
        // ============================================================
        // PLAN MODE (enter/exit)
        // ============================================================
        ToolRegistration::from_tool_instance(
            tools::ENTER_PLAN_MODE,
            CapabilityLevel::Basic,
            EnterPlanModeTool::new(plan_state.clone()),
        )
        .with_aliases([
            "plan_mode",
            "enter_plan",
            "start_planning",
            "plan_on",
            "plan_start",
            "switch_to_plan_mode",
            "switch_plan_mode",
            "mode_plan",
            "planner_mode",
            "/plan",
        ]),
        ToolRegistration::from_tool_instance(
            tools::EXIT_PLAN_MODE,
            CapabilityLevel::Basic,
            ExitPlanModeTool::new(plan_state.clone()),
        )
        .with_aliases([
            "exit_plan",
            "plan_exit",
            "start_implementation",
            "implement_plan",
            "plan_off",
            "switch_to_edit_mode",
            "switch_edit_mode",
            "mode_edit",
            "resume_edit_mode",
            "coder_mode",
            "/plan_off",
            "/edit",
        ]),
        // ============================================================
        // TASK TRACKER (NL2Repo-Bench: Explicit Task Planning)
        // ============================================================
        ToolRegistration::from_tool_instance(
            tools::TASK_TRACKER,
            CapabilityLevel::Basic,
            TaskTrackerTool::new(
                plan_state.workspace_root().unwrap_or_else(PathBuf::new),
                plan_state.clone(),
            ),
        )
        .with_aliases(["plan_manager", "track_tasks", "checklist"]),
        ToolRegistration::from_tool_instance(
            tools::PLAN_TASK_TRACKER,
            CapabilityLevel::Basic,
            PlanTaskTrackerTool::new(plan_state.clone()),
        )
        .with_aliases(["plan_checklist", "plan_tasks"]),
        // ============================================================
        // SEARCH & DISCOVERY (1 tool - unified)
        // ============================================================
        ToolRegistration::new(
            tools::UNIFIED_SEARCH,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::unified_search_executor,
        )
        .with_description(
            "Unified discovery tool: grep, list, tool discovery, errors, agent status, web fetch, and skills.",
        )
        .with_parameter_schema(unified_search_parameters())
        .with_permission(ToolPolicy::Allow)
        .with_aliases([
            tools::GREP_FILE,
            tools::LIST_FILES,
            "grep",
            "search text",
            "list files",
            "list tools",
            "list errors",
            "show agent info",
            "fetch",
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
        .with_description(
            "Run commands and manage PTY sessions. Use continue for one-call send+read, or inspect for one-call output preview/filtering from session or spool file.",
        )
        .with_parameter_schema(unified_exec_parameters())
        .with_aliases([
            tools::RUN_PTY_CMD,
            tools::EXECUTE_CODE,
            tools::CREATE_PTY_SESSION,
            tools::LIST_PTY_SESSIONS,
            tools::CLOSE_PTY_SESSION,
            tools::SEND_PTY_INPUT,
            tools::READ_PTY_SESSION,
            "bash",
            "container.exec",
            "exec code",
            "run code",
            "run command",
            "run command (pty)",
            "send pty input",
            "read pty session",
            "list pty sessions",
            "close pty session",
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
        .with_description(
            "Unified file ops: read, write, edit, patch, delete, move, copy. For edit, `old_str` must match exactly. For patch, use VT Code patch format (`*** Begin Patch`), not unified diff.",
        )
        .with_parameter_schema(unified_file_parameters())
        .with_aliases([
            tools::READ_FILE,
            tools::WRITE_FILE,
            tools::DELETE_FILE,
            tools::EDIT_FILE,
            tools::CREATE_FILE,
            tools::MOVE_FILE,
            tools::COPY_FILE,
            tools::FILE_OP,
            "repo_browser.read_file",
            "repo_browser.write_file",
            "read file",
            "write file",
            "edit file",
            "apply patch",
            "delete file",
            "move file",
            "copy file",
            "file operation",
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
            tools::GET_ERRORS,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::get_errors_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::APPLY_PATCH,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::apply_patch_executor,
        )
        .with_description(
            "Apply patches to files. IMPORTANT: Use VT Code patch format (*** Begin Patch, *** Update File: path, @@ hunks with -/+ lines, *** End Patch), NOT standard unified diff (---/+++ format).",
        )
        .with_parameter_schema(apply_patch_parameters())
        .with_permission(ToolPolicy::Prompt)
        .with_llm_visibility(false),
        // ============================================================
        // SKILL MANAGEMENT TOOLS (3 tools)
        // ============================================================
        // Note: These tools are created dynamically in session_setup.rs
        // because they depend on runtime context (skills map, tool registry).
        // They are NOT registered here; instead they are registered
        // on-demand in session initialization.
        //
        // Tools created in session_setup.rs:
        // - list_skills
        // - load_skill
        // - load_skill_resource
    ]
}
