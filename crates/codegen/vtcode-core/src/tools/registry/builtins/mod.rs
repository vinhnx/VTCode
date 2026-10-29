// The `linkme::distributed_slice` macro uses `link_section` internally,
// which triggers the `unsafe_code` lint. This is inherent to the crate's
// mechanism and cannot be avoided at the call site.
#![allow(
    unsafe_code,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]

use std::path::PathBuf;

use linkme::distributed_slice;

use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::tool_policy::ToolPolicy;
use crate::tools::defuddle::{DEFUDDLE_FETCH_DESCRIPTION, DefuddleTool};
use crate::tools::handlers::task_tracker::{
    task_tracker_description_for_workflow, task_tracker_parameter_schema_for_workflow,
};
use crate::tools::handlers::{PlanningWorkflowState, StartPlanningTool, TaskTrackerTool};
use crate::tools::native_memory;
use crate::tools::request_user_input::RequestUserInputTool;
use crate::tools::tool_intent::builtin_tool_behavior;
use crate::tools::web_fetch::{WEB_FETCH_DESCRIPTION, WebFetchTool, web_fetch_parameter_schema};
use crate::tools::web_search::{WEB_SEARCH_DESCRIPTION, WebSearchTool, web_search_parameter_schema};
use serde_json::json;
use vtcode_utility_tool_specs::{
    AGENT_DESCRIPTION, EXEC_COMMAND_DESCRIPTION, MCP_DESCRIPTION, SEARCH_TOOLS_DESCRIPTION, WRITE_STDIN_DESCRIPTION,
    agent_parameters, apply_patch_parameters, code_search_parameters, cron_parameters, exec_command_parameters,
    list_files_parameters, mcp_parameters, search_tools_parameters, write_stdin_parameters,
};

use super::distributed::{BUILTIN_TOOLS, tool_config};
use super::registration::{ToolCatalogSource, ToolRegistration};
use super::{ToolRegistry, native_cgp_tool_factory};

/// Build builtin tool registrations from the distributed slice.
///
/// Each tool self-registers via `#[distributed_slice(BUILTIN_TOOLS)]` in this
/// file. The linker collects all annotated factory functions into a contiguous
/// slice; this function iterates it to produce the final `Vec<ToolRegistration>`.
///
/// In metadata-only contexts (e.g., declaration building), callers may pass
/// `None`, and a placeholder `PlanningWorkflowState` will be used.
#[allow(
    dead_code,
    reason = "Test-only reference implementation: registry::pack assembly is verified against this loop."
)]
pub(super) fn builtin_tool_registrations(
    planning_workflow_state: Option<&PlanningWorkflowState>,
) -> Vec<ToolRegistration> {
    let mut registrations: Vec<ToolRegistration> = BUILTIN_TOOLS
        .iter()
        .map(|factory| factory(planning_workflow_state))
        .map(with_builtin_behavior)
        .map(with_builtin_network_access)
        .map(|registration| registration.with_catalog_source(ToolCatalogSource::Builtin))
        .collect();

    // Sort so that tools with aliases register before tools without aliases.
    // This prevents alias conflicts when an alias matches another registration name.
    // The linker does not guarantee source order for distributed slices.
    // Secondary sort by name ensures deterministic ordering across builds.
    registrations.sort_by(|a, b| {
        let a_has_aliases = !a.metadata().aliases().is_empty();
        let b_has_aliases = !b.metadata().aliases().is_empty();
        b_has_aliases.cmp(&a_has_aliases).then_with(|| a.name().cmp(b.name()))
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
fn register_request_user_input(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    let request_user_input_factory = native_cgp_tool_factory(|| RequestUserInputTool);
    ToolRegistration::from_tool_instance(tools::REQUEST_USER_INPUT, CapabilityLevel::Basic, RequestUserInputTool)
        .with_native_cgp_factory(request_user_input_factory)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_memory(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::MEMORY, CapabilityLevel::Basic, false, ToolRegistry::memory_executor)
        .with_description(native_memory::MEMORY_TOOL_DESCRIPTION)
        .with_parameter_schema(native_memory::parameter_schema())
        .with_permission(ToolPolicy::Allow)
}

#[distributed_slice(BUILTIN_TOOLS)]
pub(super) fn register_record_decision(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::RECORD_DECISION, CapabilityLevel::Basic, false, ToolRegistry::record_decision_executor)
        .with_description("Use to record consequential choices, rejected approaches, or recovery changes with public rationale. Optional evidence IDs must belong to the current session task. Rationale is agent-reported; ordinary commands need no record.")
        .with_parameter_schema(vtcode_utility_tool_specs::record_decision_parameters())
        .with_permission(ToolPolicy::Allow)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_cron(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::CRON,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::cron_executor,
    )
    .with_description(
        "Create, list, or delete session-scoped scheduled prompts. Use action=create to schedule a prompt, action=list to show scheduled prompts, or action=delete to remove one by id. Do not schedule per-minute jobs because they exhaust the per-turn tool budget. Scheduled prompts end when the vtcode process exits.",
    )
    .with_parameter_schema(cron_parameters())
    .with_aliases([
        tools::CRON_CREATE,
        tools::CRON_LIST,
        tools::CRON_DELETE,
        "schedule_task",
        "loop_create",
        "scheduled_tasks",
        "cancel_scheduled_task",
    ])
}

// ---------------------------------------------------------------------------
// PLANNING WORKFLOW (start/finish)
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_start_planning(plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    let plan_state = plan_state
        .cloned()
        .unwrap_or_else(|| PlanningWorkflowState::new(PathBuf::new()));
    let factory_state = plan_state.clone();
    ToolRegistration::from_tool_instance(
        tools::START_PLANNING,
        CapabilityLevel::Basic,
        StartPlanningTool::new(plan_state),
    )
    .with_native_cgp_factory(native_cgp_tool_factory(move || StartPlanningTool::new(factory_state.clone())))
}

// ---------------------------------------------------------------------------
// TASK TRACKER (NL2Repo-Bench: Explicit Task Planning)
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_task_tracker(plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    let planning_active = plan_state.is_some_and(PlanningWorkflowState::is_active);
    let plan_state = plan_state
        .cloned()
        .unwrap_or_else(|| PlanningWorkflowState::new(PathBuf::new()));
    let factory_state = plan_state.clone();
    ToolRegistration::from_tool_instance(
        tools::TASK_TRACKER,
        CapabilityLevel::Basic,
        TaskTrackerTool::new(plan_state.workspace_root().unwrap_or_else(PathBuf::new), plan_state),
    )
    .with_native_cgp_factory(native_cgp_tool_factory(move || {
        TaskTrackerTool::new(factory_state.workspace_root().unwrap_or_else(PathBuf::new), factory_state.clone())
    }))
    .with_description(task_tracker_description_for_workflow(planning_active))
    .with_parameter_schema(task_tracker_parameter_schema_for_workflow(planning_active))
    .with_aliases(["plan_manager", "track_tasks", "checklist"])
}

// ---------------------------------------------------------------------------
// MULTI-AGENT TOOLS
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_agent(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::AGENT, CapabilityLevel::Basic, false, ToolRegistry::agent_executor)
        .with_description(AGENT_DESCRIPTION)
        .with_parameter_schema(agent_parameters())
        .with_aliases([
            tools::SPAWN_AGENT,
            tools::SPAWN_BACKGROUND_SUBPROCESS,
            tools::SEND_INPUT,
            tools::RESUME_AGENT,
            tools::WAIT_AGENT,
            tools::CLOSE_AGENT,
            "delegate",
            "subagent",
            "background_subagent",
            "launch_background_helper",
            "message_agent",
            "continue_agent",
            "resume_subagent",
            "wait_subagent",
            "close_subagent",
        ])
}

// ---------------------------------------------------------------------------
// SEARCH & DISCOVERY
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_code_search(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::CODE_SEARCH,
        CapabilityLevel::CodeSearch,
        false,
        ToolRegistry::code_search_executor,
    )
    .with_description(
        "Search workspace code with one literal query (or `|`-separated literal alternatives like \"tokio|async-std|runtime\"). Use optional path, file_types, result_types, and max_results filters to find definitions, syntactic usages, text matches, and matching paths.",
    )
    .with_parameter_schema(code_search_parameters())
    .with_permission(ToolPolicy::Allow)
}

// ---------------------------------------------------------------------------
// WEB FETCH (built-in, sandbox-bypassing network fetch)
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_web_fetch(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    let web_fetch = tool_config()
        .map(|snapshot| WebFetchTool::from_config(&snapshot.web_fetch))
        .unwrap_or_default();
    let web_fetch_for_factory = web_fetch.clone();
    let web_fetch_factory = native_cgp_tool_factory(move || web_fetch_for_factory.clone());
    ToolRegistration::from_tool_instance(tools::WEB_FETCH, CapabilityLevel::Basic, web_fetch)
        .with_native_cgp_factory(web_fetch_factory)
        .with_description(WEB_FETCH_DESCRIPTION)
        .with_parameter_schema(web_fetch_parameter_schema())
        .with_permission(ToolPolicy::Prompt)
        .with_aliases(["fetch_url", "web"])
}

// ---------------------------------------------------------------------------
// WEB SEARCH (built-in, query -> ranked results, keyless DuckDuckGo)
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_web_search(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    let web_search =
        WebSearchTool::with_config(tool_config().map(|snapshot| snapshot.web_search.clone()).unwrap_or_default());
    let web_search_for_factory = web_search.clone();
    let web_search_factory = native_cgp_tool_factory(move || web_search_for_factory.clone());
    ToolRegistration::from_tool_instance(tools::WEB_SEARCH, CapabilityLevel::Basic, web_search)
        .with_native_cgp_factory(web_search_factory)
        .with_description(WEB_SEARCH_DESCRIPTION)
        .with_parameter_schema(web_search_parameter_schema())
        .with_permission(ToolPolicy::Prompt)
        .with_aliases(["search_web", "websearch"])
}

// ---------------------------------------------------------------------------
// DEFUDDLE FETCH (built-in, one-shot markdown extraction via defuddle.md)
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_defuddle_fetch(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    let defuddle = DefuddleTool::new();
    let defuddle_for_factory = defuddle.clone();
    let defuddle_factory = native_cgp_tool_factory(move || defuddle_for_factory.clone());
    ToolRegistration::from_tool_instance(tools::DEFUDDLE_FETCH, CapabilityLevel::Basic, defuddle)
        .with_native_cgp_factory(defuddle_factory)
        .with_description(DEFUDDLE_FETCH_DESCRIPTION)
        .with_parameter_schema(json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "format": "uri",
                    "pattern": "^https?://",
                    "description": "REMOTE web page URL (http:// or https:// ONLY). Do NOT use for local file paths."
                },
                "max_bytes": {
                    "type": "integer",
                    "description": "Hard cap on the returned markdown size in bytes (default: 262144, max: 262144)."
                }
            },
            "required": ["url"],
            "additionalProperties": false
        }))
        .with_permission(ToolPolicy::Prompt)
        .with_aliases(["defuddle", "extract_markdown"])
        .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_mcp(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::MCP, CapabilityLevel::CodeSearch, false, ToolRegistry::mcp_executor)
        .with_description(MCP_DESCRIPTION)
        .with_parameter_schema(mcp_parameters())
        .with_permission(ToolPolicy::Allow)
        .with_aliases([
            tools::MCP_SEARCH_TOOLS,
            tools::MCP_GET_TOOL_DETAILS,
            tools::MCP_LIST_SERVERS,
            tools::MCP_CONNECT_SERVER,
            tools::MCP_DISCONNECT_SERVER,
            "mcp_tool_search",
            "mcp_tool_details",
        ])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_search_tools(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::SEARCH_TOOLS, CapabilityLevel::Basic, false, ToolRegistry::search_tools_executor)
        .with_description(SEARCH_TOOLS_DESCRIPTION)
        .with_parameter_schema(search_tools_parameters())
        .with_permission(ToolPolicy::Allow)
}

// ---------------------------------------------------------------------------
// SHELL EXECUTION
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_exec_command(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::EXEC_COMMAND, CapabilityLevel::Bash, false, ToolRegistry::exec_command_executor)
        .with_description(EXEC_COMMAND_DESCRIPTION)
        .with_parameter_schema(exec_command_parameters())
        .with_permission(ToolPolicy::Allow)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_exec_pty_cmd(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::EXEC_PTY_CMD, CapabilityLevel::Bash, false, ToolRegistry::run_pty_cmd_executor)
        .with_description(
            "Execute a shell command attached to a PTY (pseudo-terminal) so interactive and \
         TTY-aware programs behave as in a real terminal. Use this when the command needs a \
         controlling terminal (e.g. pagers, prompts, curses UIs). Returns output, exit status, \
         and a reusable session id when the command is still running.",
        )
        .with_parameter_schema(exec_command_parameters())
        .with_permission(ToolPolicy::Allow)
        .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_write_stdin(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::WRITE_STDIN, CapabilityLevel::Bash, false, ToolRegistry::write_stdin_executor)
        .with_description(WRITE_STDIN_DESCRIPTION)
        .with_parameter_schema(write_stdin_parameters())
        .with_permission(ToolPolicy::Allow)
}

// ---------------------------------------------------------------------------
// INTERNAL TOOLS (Hidden from LLM, reused by public tools and harnesses)
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_read_file(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::READ_FILE,
        CapabilityLevel::CodeSearch,
        false,
        ToolRegistry::read_file_executor,
    )
    .with_description(
        "Read file contents with chunked ranges or indentation-aware block selection. Exposed as a first-class browse tool for the harness surface.",
    )
    .with_permission(ToolPolicy::Allow)
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_list_files(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::LIST_FILES, CapabilityLevel::CodeSearch, false, ToolRegistry::list_files_executor)
        .with_description(
            "List files and directories with pagination. Exposed as a first-class browse tool for the harness surface.",
        )
        .with_parameter_schema(list_files_parameters())
        .with_permission(ToolPolicy::Allow)
        .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_write_file(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::WRITE_FILE, CapabilityLevel::Editing, false, ToolRegistry::write_file_executor)
        .with_description("Write or overwrite a file with new content. Internal file helper.")
        .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_edit_file(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::EDIT_FILE, CapabilityLevel::Editing, false, ToolRegistry::edit_file_executor)
        .with_description("Apply a surgical text replacement in a file. Internal file helper.")
        .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_run_pty_cmd(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::RUN_PTY_CMD, CapabilityLevel::Bash, false, ToolRegistry::run_pty_cmd_executor)
        .with_description("Run a one-shot PTY command. Internal execution helper.")
        .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_send_pty_input(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::SEND_PTY_INPUT, CapabilityLevel::Bash, false, ToolRegistry::send_pty_input_executor)
        .with_description("Send stdin to an active PTY session. Internal execution helper.")
        .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_read_pty_session(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::READ_PTY_SESSION,
        CapabilityLevel::Bash,
        false,
        ToolRegistry::read_pty_session_executor,
    )
    .with_description("Read buffered output from a PTY session. Internal execution helper.")
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_create_pty_session(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::CREATE_PTY_SESSION,
        CapabilityLevel::Bash,
        false,
        ToolRegistry::create_pty_session_executor,
    )
    .with_description("Create an interactive PTY session. Internal execution helper.")
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_list_pty_sessions(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::LIST_PTY_SESSIONS,
        CapabilityLevel::Bash,
        false,
        ToolRegistry::list_pty_sessions_executor,
    )
    .with_description("List all active PTY sessions. Internal execution helper.")
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_close_pty_session(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::CLOSE_PTY_SESSION,
        CapabilityLevel::Bash,
        false,
        ToolRegistry::close_pty_session_executor,
    )
    .with_description("Close a PTY session by ID. Internal execution helper.")
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_get_errors(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::GET_ERRORS, CapabilityLevel::CodeSearch, false, ToolRegistry::get_errors_executor)
        .with_description(
            "Retrieve compilation/lint errors from the most recent run. Internal — used by the harness surface.",
        )
        .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_apply_patch(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(tools::APPLY_PATCH, CapabilityLevel::Editing, false, ToolRegistry::apply_patch_executor)
        .with_description(crate::tools::apply_patch::with_semantic_anchor_guidance(
            crate::tools::apply_patch::APPLY_PATCH_TOOL_DESCRIPTION,
        ))
        .with_parameter_schema(apply_patch_parameters())
        .with_permission(ToolPolicy::Prompt)
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

#[allow(
    dead_code,
    reason = "Test-only reference implementation: registry::pack assembly is verified against this loop."
)]
fn with_builtin_behavior(registration: ToolRegistration) -> ToolRegistration {
    if let Some(behavior) = builtin_tool_behavior(registration.name()) {
        registration.with_behavior(behavior)
    } else {
        registration
    }
}

#[allow(
    dead_code,
    reason = "Test-only reference implementation: registry::pack assembly is verified against this loop."
)]
fn with_builtin_network_access(registration: ToolRegistration) -> ToolRegistration {
    use super::ToolNetworkAccess;
    let access = match registration.name() {
        "web_fetch" | "web_search" | "defuddle_fetch" | "exec_command" | "exec_pty_cmd" | "write_stdin"
        | "run_pty_cmd" | "send_pty_input" | "create_pty_session" | "mcp" | "agent" | "cron" => {
            ToolNetworkAccess::Network
        }
        "read_file"
        | "list_files"
        | "write_file"
        | "edit_file"
        | "apply_patch"
        | "code_search"
        | "request_user_input"
        | "memory"
        | "start_planning"
        | "task_tracker"
        | tools::RECORD_DECISION
        | "search_tools"
        | "read_pty_session"
        | "list_pty_sessions"
        | "close_pty_session"
        | "get_errors" => ToolNetworkAccess::Local,
        _ => ToolNetworkAccess::Unknown,
    };
    registration.with_network_access(access)
}

#[cfg(test)]
mod tests;
