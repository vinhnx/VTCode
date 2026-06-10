// The `linkme::distributed_slice` macro uses `link_section` internally,
// which triggers the `unsafe_code` lint. This is inherent to the crate's
// mechanism and cannot be avoided at the call site.
#![allow(unsafe_code)]

use std::path::PathBuf;

use linkme::distributed_slice;

use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::tool_policy::ToolPolicy;
use crate::tools::handlers::{
    EnterPlanModeTool, ExitPlanModeTool, PlanModeState, PlanTaskTrackerTool, TaskTrackerTool,
};
use crate::tools::native_memory;
use crate::tools::request_user_input::RequestUserInputTool;
use crate::tools::tool_intent::builtin_tool_behavior;
use serde_json::json;
use vtcode_collaboration_tool_specs::{
    close_agent_parameters, resume_agent_parameters, send_input_parameters, spawn_agent_parameters,
    spawn_background_subprocess_parameters, wait_agent_parameters,
};
use vtcode_utility_tool_specs::{
    apply_patch_parameters, cron_create_parameters, cron_delete_parameters, cron_list_parameters,
    list_files_parameters, read_file_parameters, unified_exec_parameters, unified_file_parameters,
    unified_search_parameters,
};

use super::distributed::BUILTIN_TOOLS;
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

/// Build builtin tool registrations from the distributed slice.
///
/// Each tool self-registers via `#[distributed_slice(BUILTIN_TOOLS)]` in this
/// file. The linker collects all annotated factory functions into a contiguous
/// slice; this function iterates it to produce the final `Vec<ToolRegistration>`.
///
/// In metadata-only contexts (e.g., declaration building), callers may pass
/// `None`, and a placeholder `PlanModeState` will be used.
pub(super) fn builtin_tool_registrations(
    plan_mode_state: Option<&PlanModeState>,
) -> Vec<ToolRegistration> {
    let mut registrations: Vec<ToolRegistration> = BUILTIN_TOOLS
        .iter()
        .map(|factory| factory(plan_mode_state))
        .map(with_builtin_behavior)
        .map(|registration| registration.with_catalog_source(ToolCatalogSource::Builtin))
        .collect();

    // Sort so that tools with aliases register before tools without aliases.
    // This prevents alias conflicts: e.g., `unified_search` has alias "list_files"
    // which would conflict if the internal tool named "list_files" is registered first.
    // The linker does not guarantee source order for distributed slices.
    // Secondary sort by name ensures deterministic ordering across builds.
    registrations.sort_by(|a, b| {
        let a_has_aliases = !a.metadata().aliases().is_empty();
        let b_has_aliases = !b.metadata().aliases().is_empty();
        b_has_aliases
            .cmp(&a_has_aliases)
            .then_with(|| a.name().cmp(b.name()))
    });

    registrations
}

// ===========================================================================
// Distributed tool registrations.
//
// Each function below is annotated with `#[distributed_slice(BUILTIN_TOOLS)]`
// so the linker collects it into the `BUILTIN_TOOLS` slice at load time.
// The function body runs at startup (not at link time) when
// `builtin_tool_registrations()` iterates the slice.
// ===========================================================================

// ---------------------------------------------------------------------------
// HUMAN-IN-THE-LOOP (HITL)
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_request_user_input(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    let request_user_input_factory = native_cgp_tool_factory(|| RequestUserInputTool);
    ToolRegistration::from_tool_instance(
        tools::REQUEST_USER_INPUT,
        CapabilityLevel::Basic,
        RequestUserInputTool,
    )
    .with_native_cgp_factory(request_user_input_factory)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_memory(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::MEMORY,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::memory_executor,
    )
    .with_description(
        "Access VT Code persistent memory files under /memories. Use view before reading or updating notes; writes are limited to preferences.md, repository-facts.md, and notes/**.",
    )
    .with_parameter_schema(native_memory::parameter_schema())
    .with_permission(ToolPolicy::Allow)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_cron_create(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
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
    .with_aliases(["schedule_task", "loop_create"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_cron_list(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::CRON_LIST,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::cron_list_executor,
    )
    .with_description("List session-scoped scheduled prompts for the current VT Code process.")
    .with_parameter_schema(cron_list_parameters())
    .with_aliases(["scheduled_tasks"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_cron_delete(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::CRON_DELETE,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::cron_delete_executor,
    )
    .with_description("Delete a session-scoped scheduled prompt by id.")
    .with_parameter_schema(cron_delete_parameters())
    .with_aliases(["cancel_scheduled_task"])
}

// ---------------------------------------------------------------------------
// PLAN MODE (enter/exit)
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_enter_plan_mode(plan_state: Option<&PlanModeState>) -> ToolRegistration {
    let plan_state = plan_state
        .cloned()
        .unwrap_or_else(|| PlanModeState::new(PathBuf::new()));
    let factory_state = plan_state.clone();
    ToolRegistration::from_tool_instance(
        tools::ENTER_PLAN_MODE,
        CapabilityLevel::Basic,
        EnterPlanModeTool::new(plan_state),
    )
    .with_native_cgp_factory(native_cgp_tool_factory(move || {
        EnterPlanModeTool::new(factory_state.clone())
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
    ])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_exit_plan_mode(plan_state: Option<&PlanModeState>) -> ToolRegistration {
    let plan_state = plan_state
        .cloned()
        .unwrap_or_else(|| PlanModeState::new(PathBuf::new()));
    let factory_state = plan_state.clone();
    ToolRegistration::from_tool_instance(
        tools::EXIT_PLAN_MODE,
        CapabilityLevel::Basic,
        ExitPlanModeTool::new(plan_state),
    )
    .with_native_cgp_factory(native_cgp_tool_factory(move || {
        ExitPlanModeTool::new(factory_state.clone())
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
    ])
}

// ---------------------------------------------------------------------------
// TASK TRACKER (NL2Repo-Bench: Explicit Task Planning)
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_task_tracker(plan_state: Option<&PlanModeState>) -> ToolRegistration {
    let plan_state = plan_state
        .cloned()
        .unwrap_or_else(|| PlanModeState::new(PathBuf::new()));
    let factory_state = plan_state.clone();
    ToolRegistration::from_tool_instance(
        tools::TASK_TRACKER,
        CapabilityLevel::Basic,
        TaskTrackerTool::new(
            plan_state.workspace_root().unwrap_or_else(PathBuf::new),
            plan_state,
        ),
    )
    .with_native_cgp_factory(native_cgp_tool_factory(move || {
        TaskTrackerTool::new(
            factory_state.workspace_root().unwrap_or_else(PathBuf::new),
            factory_state.clone(),
        )
    }))
    .with_aliases(["plan_manager", "track_tasks", "checklist"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_plan_task_tracker(plan_state: Option<&PlanModeState>) -> ToolRegistration {
    let plan_state = plan_state
        .cloned()
        .unwrap_or_else(|| PlanModeState::new(PathBuf::new()));
    let factory_state = plan_state.clone();
    ToolRegistration::from_tool_instance(
        tools::PLAN_TASK_TRACKER,
        CapabilityLevel::Basic,
        PlanTaskTrackerTool::new(plan_state),
    )
    .with_native_cgp_factory(native_cgp_tool_factory(move || {
        PlanTaskTrackerTool::new(factory_state.clone())
    }))
    .with_aliases(["plan_checklist", "plan_tasks"])
}

// ---------------------------------------------------------------------------
// MULTI-AGENT TOOLS
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_spawn_agent(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::SPAWN_AGENT,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::spawn_agent_executor,
    )
    .with_description(
        "Spawn a delegated child agent for a scoped task. The child inherits the current toolset, can spawn its own child agents, and returns its agent id plus current status.",
    )
    .with_parameter_schema(spawn_agent_parameters())
    .with_aliases(["agent", "delegate", "subagent"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_spawn_background_subprocess(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::SPAWN_BACKGROUND_SUBPROCESS,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::spawn_background_subprocess_executor,
    )
    .with_description(
        "Launch a managed background subprocess for a background-enabled subagent. Use this for durable helpers that should outlive the current foreground turn instead of calling spawn_agent(background=true).",
    )
    .with_parameter_schema(spawn_background_subprocess_parameters())
    .with_aliases(["background_subagent", "launch_background_helper"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_send_input(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::SEND_INPUT,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::send_input_executor,
    )
    .with_description(
        "Send follow-up input to an existing child agent. When interrupt is false, running work keeps going and the new input is queued for the next turn; when true, current work is aborted and restarted with the new input.",
    )
    .with_parameter_schema(send_input_parameters())
    .with_aliases(["message_agent", "continue_agent"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_wait_agent(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::WAIT_AGENT,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::wait_agent_executor,
    )
    .with_description(
        "Wait for one or more child agents to reach a terminal state. Returns completion status for the first target that finishes, or completed=false if the wait times out.",
    )
    .with_parameter_schema(wait_agent_parameters())
    .with_aliases(["wait_subagent"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_resume_agent(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::RESUME_AGENT,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::resume_agent_executor,
    )
    .with_description(
        "Resume a previously completed or closed child agent subtree from its saved context.",
    )
    .with_parameter_schema(resume_agent_parameters())
    .with_aliases(["resume_subagent"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_close_agent(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::CLOSE_AGENT,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::close_agent_executor,
    )
    .with_description(
        "Close a child agent subtree, cancelling any active work and marking the thread closed.",
    )
    .with_parameter_schema(close_agent_parameters())
    .with_aliases(["close_subagent"])
}

// ---------------------------------------------------------------------------
// SEARCH & DISCOVERY
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_unified_search(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
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
    ])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_mcp_search_tools(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::MCP_SEARCH_TOOLS,
        CapabilityLevel::CodeSearch,
        false,
        ToolRegistry::mcp_search_tools_executor,
    )
    .with_description(
        "Search only MCP tool catalogs with progressive detail levels. Use this to discover MCP capabilities without loading full schemas for every tool.",
    )
    .with_parameter_schema(mcp_search_tools_parameters())
    .with_permission(ToolPolicy::Allow)
    .with_aliases(["mcp_tool_search"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_mcp_get_tool_details(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::MCP_GET_TOOL_DETAILS,
        CapabilityLevel::CodeSearch,
        false,
        ToolRegistry::mcp_get_tool_details_executor,
    )
    .with_description(
        "Fetch full MCP tool details for one specific MCP tool name, including its input schema.",
    )
    .with_parameter_schema(mcp_get_tool_details_parameters())
    .with_permission(ToolPolicy::Allow)
    .with_aliases(["mcp_tool_details"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_mcp_list_servers(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::MCP_LIST_SERVERS,
        CapabilityLevel::CodeSearch,
        false,
        ToolRegistry::mcp_list_servers_executor,
    )
    .with_description("List configured MCP servers and their current connection state.")
    .with_parameter_schema(mcp_list_servers_parameters())
    .with_permission(ToolPolicy::Allow)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_mcp_connect_server(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::MCP_CONNECT_SERVER,
        CapabilityLevel::CodeSearch,
        false,
        ToolRegistry::mcp_connect_server_executor,
    )
    .with_description("Connect one configured MCP server by name.")
    .with_parameter_schema(mcp_server_name_parameters())
    .with_permission(ToolPolicy::Prompt)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_mcp_disconnect_server(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::MCP_DISCONNECT_SERVER,
        CapabilityLevel::CodeSearch,
        false,
        ToolRegistry::mcp_disconnect_server_executor,
    )
    .with_description("Disconnect one active MCP server by name.")
    .with_parameter_schema(mcp_server_name_parameters())
    .with_permission(ToolPolicy::Prompt)
}

// ---------------------------------------------------------------------------
// SHELL EXECUTION
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_unified_exec(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::UNIFIED_EXEC,
        CapabilityLevel::Bash,
        false,
        ToolRegistry::unified_exec_executor,
    )
    .with_description(
        "Run commands, manage command sessions, or execute fresh Python/JavaScript snippets with `action=code`. Runs are pipe-first by default; set `tty=true` for PTY behavior. Use continue for one-call send+read, inspect for one-call output preview/filtering from session or spool file, and set `language` when `action=code` should use JavaScript instead of the default Python.",
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
    ])
}

// ---------------------------------------------------------------------------
// FILE OPERATIONS
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_unified_file(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
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
    ])
}

// ---------------------------------------------------------------------------
// INTERNAL TOOLS (Hidden from LLM, used by unified tools)
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_read_file(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
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
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_list_files(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
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
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_write_file(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::WRITE_FILE,
        CapabilityLevel::Editing,
        false,
        ToolRegistry::write_file_executor,
    )
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_edit_file(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::EDIT_FILE,
        CapabilityLevel::Editing,
        false,
        ToolRegistry::edit_file_executor,
    )
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_run_pty_cmd(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::RUN_PTY_CMD,
        CapabilityLevel::Bash,
        false,
        ToolRegistry::run_pty_cmd_executor,
    )
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_send_pty_input(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::SEND_PTY_INPUT,
        CapabilityLevel::Bash,
        false,
        ToolRegistry::send_pty_input_executor,
    )
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_read_pty_session(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::READ_PTY_SESSION,
        CapabilityLevel::Bash,
        false,
        ToolRegistry::read_pty_session_executor,
    )
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_create_pty_session(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::CREATE_PTY_SESSION,
        CapabilityLevel::Bash,
        false,
        ToolRegistry::create_pty_session_executor,
    )
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_list_pty_sessions(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::LIST_PTY_SESSIONS,
        CapabilityLevel::Bash,
        false,
        ToolRegistry::list_pty_sessions_executor,
    )
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_close_pty_session(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::CLOSE_PTY_SESSION,
        CapabilityLevel::Bash,
        false,
        ToolRegistry::close_pty_session_executor,
    )
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_get_errors(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::GET_ERRORS,
        CapabilityLevel::CodeSearch,
        false,
        ToolRegistry::get_errors_executor,
    )
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_apply_patch(_plan_state: Option<&PlanModeState>) -> ToolRegistration {
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
    .with_llm_visibility(false)
}

// ---------------------------------------------------------------------------
// SKILL MANAGEMENT TOOLS (3 tools)
// ---------------------------------------------------------------------------
// Note: These tools are created dynamically in session_setup.rs
// because they depend on runtime context (skills map, tool registry).
// They are NOT registered here; instead they are registered
// on-demand in session initialization.
//
// Tools created in session_setup.rs:
// - list_skills
// - load_skill
// - load_skill_resource

fn mcp_search_tools_parameters() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Natural language query describing MCP capability to find."
            },
            "detail_level": {
                "type": "string",
                "enum": ["name", "name_description", "full"],
                "description": "Response detail level: names only, names with descriptions, or full schema excerpts."
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 25,
                "description": "Maximum number of results to return."
            }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

fn mcp_get_tool_details_parameters() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "description": "Exact MCP tool name to inspect."
            }
        },
        "required": ["name"],
        "additionalProperties": false
    })
}

fn mcp_list_servers_parameters() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

fn mcp_server_name_parameters() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "description": "Configured MCP server name."
            }
        },
        "required": ["name"],
        "additionalProperties": false
    })
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
    fn distributed_slice_contains_all_builtin_tools() {
        use crate::tools::registry::distributed::BUILTIN_TOOLS;
        // The distributed slice should contain at least 30 tool factories.
        // This catches accidentally missing #[distributed_slice] annotations.
        assert!(
            BUILTIN_TOOLS.len() >= 30,
            "expected at least 30 distributed tool factories, found {}",
            BUILTIN_TOOLS.len()
        );
    }

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

    #[test]
    fn multi_agent_builtins_expose_updated_descriptions() {
        let plan_state = PlanModeState::new(PathBuf::from("/workspace"));
        let registrations = builtin_tool_registrations(Some(&plan_state));

        let spawn_agent = registrations
            .iter()
            .find(|registration| registration.name() == tools::SPAWN_AGENT)
            .expect("spawn_agent registration should exist");
        assert!(
            spawn_agent
                .metadata()
                .description()
                .expect("spawn_agent description")
                .contains("inherits the current toolset")
        );

        let send_input = registrations
            .iter()
            .find(|registration| registration.name() == tools::SEND_INPUT)
            .expect("send_input registration should exist");
        assert!(
            send_input
                .metadata()
                .description()
                .expect("send_input description")
                .contains("queued for the next turn")
        );

        let wait_agent = registrations
            .iter()
            .find(|registration| registration.name() == tools::WAIT_AGENT)
            .expect("wait_agent registration should exist");
        assert!(
            wait_agent
                .metadata()
                .description()
                .expect("wait_agent description")
                .contains("completed=false if the wait times out")
        );

        let spawn_background = registrations
            .iter()
            .find(|registration| registration.name() == tools::SPAWN_BACKGROUND_SUBPROCESS)
            .expect("spawn_background_subprocess registration should exist");
        assert!(
            spawn_background
                .metadata()
                .description()
                .expect("spawn_background_subprocess description")
                .contains("spawn_agent(background=true)")
        );
    }
}
