use std::path::PathBuf;

use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::tool_policy::ToolPolicy;
use crate::tools::handlers::{
    EnterPlanModeTool, ExitPlanModeTool, PlanModeState, PlanTaskTrackerTool, TaskTrackerTool,
};
use crate::tools::request_user_input::RequestUserInputTool;
use crate::tools::tool_intent::builtin_tool_behavior;
use vtcode_collaboration_tool_specs::{
    close_agent_parameters, resume_agent_parameters, send_input_parameters, spawn_agent_parameters,
    wait_agent_parameters,
};
use vtcode_utility_tool_specs::{
    apply_patch_parameters, cron_create_parameters, cron_delete_parameters, cron_list_parameters,
    list_files_parameters, read_file_parameters, unified_exec_parameters, unified_file_parameters,
    unified_search_parameters,
};

use super::registration::{ToolCatalogSource, ToolRegistration};
use super::{ToolInventory, ToolRegistry, native_cgp_tool_factory};

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
    let request_user_input_factory = native_cgp_tool_factory(|| RequestUserInputTool);
    let enter_plan_state = plan_state.clone();
    let exit_plan_state = plan_state.clone();
    let task_tracker_state = plan_state.clone();
    let plan_task_tracker_state = plan_state.clone();

    vec![
        // ============================================================
        // HUMAN-IN-THE-LOOP (HITL)
        // ============================================================
        ToolRegistration::from_tool_instance(
            tools::REQUEST_USER_INPUT,
            CapabilityLevel::Basic,
            RequestUserInputTool,
        )
        .with_native_cgp_factory(request_user_input_factory),
        ToolRegistration::new(
            tools::CRON_CREATE,
            CapabilityLevel::Basic,
            false,
            ToolRegistry::cron_create_executor,
        )
        .with_description(
            "Create a session-scoped scheduled prompt using a cron expression, fixed interval, or one-shot fire time.",
        )
        .with_parameter_schema(cron_create_parameters())
        .with_aliases(["schedule_task", "loop_create"]),
        ToolRegistration::new(
            tools::CRON_LIST,
            CapabilityLevel::Basic,
            false,
            ToolRegistry::cron_list_executor,
        )
        .with_description("List session-scoped scheduled prompts for the current VT Code process.")
        .with_parameter_schema(cron_list_parameters())
        .with_aliases(["scheduled_tasks"]),
        ToolRegistration::new(
            tools::CRON_DELETE,
            CapabilityLevel::Basic,
            false,
            ToolRegistry::cron_delete_executor,
        )
        .with_description("Delete a session-scoped scheduled prompt by id.")
        .with_parameter_schema(cron_delete_parameters())
        .with_aliases(["cancel_scheduled_task"]),
        // ============================================================
        // PLAN MODE (enter/exit)
        // ============================================================
        ToolRegistration::from_tool_instance(
            tools::ENTER_PLAN_MODE,
            CapabilityLevel::Basic,
            EnterPlanModeTool::new(plan_state.clone()),
        )
        .with_native_cgp_factory(native_cgp_tool_factory(move || {
            EnterPlanModeTool::new(enter_plan_state.clone())
        }))
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
        .with_native_cgp_factory(native_cgp_tool_factory(move || {
            ExitPlanModeTool::new(exit_plan_state.clone())
        }))
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
        .with_native_cgp_factory(native_cgp_tool_factory(move || {
            TaskTrackerTool::new(
                task_tracker_state.workspace_root().unwrap_or_else(PathBuf::new),
                task_tracker_state.clone(),
            )
        }))
        .with_aliases(["plan_manager", "track_tasks", "checklist"]),
        ToolRegistration::from_tool_instance(
            tools::PLAN_TASK_TRACKER,
            CapabilityLevel::Basic,
            PlanTaskTrackerTool::new(plan_state.clone()),
        )
        .with_native_cgp_factory(native_cgp_tool_factory(move || {
            PlanTaskTrackerTool::new(plan_task_tracker_state.clone())
        }))
        .with_aliases(["plan_checklist", "plan_tasks"]),
        ToolRegistration::new(
            tools::SPAWN_AGENT,
            CapabilityLevel::Basic,
            false,
            ToolRegistry::spawn_agent_executor,
        )
        .with_description(
            "Spawn a delegated child agent with isolated context and return its id plus status.",
        )
        .with_parameter_schema(spawn_agent_parameters())
        .with_aliases(["agent", "delegate", "subagent"]),
        ToolRegistration::new(
            tools::SEND_INPUT,
            CapabilityLevel::Basic,
            false,
            ToolRegistry::send_input_executor,
        )
        .with_description(
            "Send follow-up instructions to an existing child agent and optionally interrupt current work.",
        )
        .with_parameter_schema(send_input_parameters())
        .with_aliases(["message_agent", "continue_agent"]),
        ToolRegistration::new(
            tools::WAIT_AGENT,
            CapabilityLevel::Basic,
            false,
            ToolRegistry::wait_agent_executor,
        )
        .with_description(
            "Wait for one or more child agents to reach a terminal state and return the first result.",
        )
        .with_parameter_schema(wait_agent_parameters())
        .with_aliases(["wait_subagent"]),
        ToolRegistration::new(
            tools::RESUME_AGENT,
            CapabilityLevel::Basic,
            false,
            ToolRegistry::resume_agent_executor,
        )
        .with_description("Resume a previously completed child agent from its saved context.")
        .with_parameter_schema(resume_agent_parameters())
        .with_aliases(["resume_subagent"]),
        ToolRegistration::new(
            tools::CLOSE_AGENT,
            CapabilityLevel::Basic,
            false,
            ToolRegistry::close_agent_executor,
        )
        .with_description(
            "Close a child agent, cancelling any active work and marking the thread closed.",
        )
        .with_parameter_schema(close_agent_parameters())
        .with_aliases(["close_subagent"]),
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
            "Unified discovery tool: structural code search, grep text search, list, tool discovery, errors, agent status, web fetch, and skills. Use `action=list` for file discovery before falling back to shell listing. Paths are relative to the workspace root.",
        )
        .with_parameter_schema(unified_search_parameters())
        .with_permission(ToolPolicy::Allow)
        .with_aliases([
            tools::GREP_FILE,
            tools::LIST_FILES,
            "grep",
            "search text",
            "structural search",
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
            false,
            ToolRegistry::unified_exec_executor,
        )
        .with_description(
            "Run commands and manage command sessions. Runs are pipe-first by default; set `tty=true` for PTY behavior. Use continue for one-call send+read, or inspect for one-call output preview/filtering from session or spool file.",
        )
        .with_parameter_schema(unified_exec_parameters())
        .with_aliases([
            tools::EXEC_COMMAND,
            tools::WRITE_STDIN,
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
            "send command input",
            "read command session",
            "list command sessions",
            "close command session",
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
            "Unified file ops: read, write, edit, patch, delete, move, copy. Use `action=read` for file contents instead of shell `cat`/`sed` during normal repo browsing. Paths are relative to the workspace root. For edit, `old_str` must match exactly. For patch, use VT Code patch format (`*** Begin Patch`), not unified diff.",
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
        .with_description(
            "Read file contents with chunked ranges or indentation-aware block selection. Exposed as a first-class browse tool for the harness surface.",
        )
        .with_parameter_schema(read_file_parameters())
        .with_permission(ToolPolicy::Allow)
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::LIST_FILES,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::list_files_executor,
        )
        .with_description(
            "List files and directories with pagination. Exposed as a first-class browse tool for the harness surface.",
        )
        .with_parameter_schema(list_files_parameters())
        .with_permission(ToolPolicy::Allow)
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
            false,
            ToolRegistry::run_pty_cmd_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::SEND_PTY_INPUT,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::send_pty_input_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::READ_PTY_SESSION,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::read_pty_session_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::CREATE_PTY_SESSION,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::create_pty_session_executor,
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
        .with_description(crate::tools::apply_patch::with_semantic_anchor_guidance(
            "Apply patches to files. IMPORTANT: Use VT Code patch format (*** Begin Patch, *** Update File: path, @@ hunks with -/+ lines, *** End Patch), NOT standard unified diff (---/+++ format)."
        ))
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
    .into_iter()
    .map(with_builtin_behavior)
    .map(|registration| registration.with_catalog_source(ToolCatalogSource::Builtin))
    .collect()
}

fn with_builtin_behavior(registration: ToolRegistration) -> ToolRegistration {
    if let Some(behavior) = builtin_tool_behavior(registration.name()) {
        registration.with_behavior(behavior)
    } else {
        registration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_backed_builtins_register_native_cgp_factories() {
        let plan_state = PlanModeState::new(PathBuf::from("/workspace"));
        let registrations = builtin_tool_registrations(Some(&plan_state));

        for tool_name in [
            tools::REQUEST_USER_INPUT,
            tools::ENTER_PLAN_MODE,
            tools::EXIT_PLAN_MODE,
            tools::TASK_TRACKER,
            tools::PLAN_TASK_TRACKER,
        ] {
            let registration = registrations
                .iter()
                .find(|registration| registration.name() == tool_name)
                .expect("builtin registration should exist");
            assert!(
                registration.native_cgp_factory().is_some(),
                "expected native CGP factory for {tool_name}"
            );
        }

        let unified_search = registrations
            .iter()
            .find(|registration| registration.name() == tools::UNIFIED_SEARCH)
            .expect("unified_search registration should exist");
        assert!(unified_search.native_cgp_factory().is_none());
    }

    #[test]
    fn unified_builtins_preserve_public_aliases() {
        let plan_state = PlanModeState::new(PathBuf::from("/workspace"));
        let registrations = builtin_tool_registrations(Some(&plan_state));

        let unified_search = registrations
            .iter()
            .find(|registration| registration.name() == tools::UNIFIED_SEARCH)
            .expect("unified_search registration should exist");
        assert!(unified_search.expose_in_llm());
        for alias in [tools::GREP_FILE, tools::LIST_FILES, "structural search"] {
            assert!(
                unified_search
                    .metadata()
                    .aliases()
                    .iter()
                    .any(|item| item == alias),
                "expected unified_search alias {alias}"
            );
        }

        let unified_exec = registrations
            .iter()
            .find(|registration| registration.name() == tools::UNIFIED_EXEC)
            .expect("unified_exec registration should exist");
        assert!(unified_exec.expose_in_llm());
        for alias in [tools::EXEC_COMMAND, tools::WRITE_STDIN, tools::RUN_PTY_CMD] {
            assert!(
                unified_exec
                    .metadata()
                    .aliases()
                    .iter()
                    .any(|item| item == alias),
                "expected unified_exec alias {alias}"
            );
        }

        let unified_file = registrations
            .iter()
            .find(|registration| registration.name() == tools::UNIFIED_FILE)
            .expect("unified_file registration should exist");
        assert!(unified_file.expose_in_llm());
        for alias in [tools::READ_FILE, tools::WRITE_FILE, tools::EDIT_FILE] {
            assert!(
                unified_file
                    .metadata()
                    .aliases()
                    .iter()
                    .any(|item| item == alias),
                "expected unified_file alias {alias}"
            );
        }
    }
}
