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
use crate::tools::web_search::{WEB_SEARCH_DESCRIPTION, WebSearchTool};
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
#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
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
// WEB SEARCH (built-in, query -> ranked results, keyless DuckDuckGo only)
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
        .with_parameter_schema(json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query (a topic, question, or keywords)."
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 8, max: 20)."
                }
            },
            "required": ["query"],
            "additionalProperties": false
        }))
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

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
fn with_builtin_behavior(registration: ToolRegistration) -> ToolRegistration {
    if let Some(behavior) = builtin_tool_behavior(registration.name()) {
        registration.with_behavior(behavior)
    } else {
        registration
    }
}

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
mod tests {
    use super::*;

    #[test]
    fn distributed_slice_contains_all_builtin_tools() {
        use crate::tools::registry::distributed::BUILTIN_TOOLS;
        // Consolidated action tools keep the factory count bounded.
        // This catches accidentally missing #[distributed_slice] annotations.
        assert!(
            BUILTIN_TOOLS.len() >= 24,
            "expected at least 24 distributed tool factories, found {}",
            BUILTIN_TOOLS.len()
        );
    }

    #[test]
    fn tool_backed_builtins_register_native_cgp_factories() {
        let plan_state = PlanningWorkflowState::new(PathBuf::from("/workspace"));
        let registrations = builtin_tool_registrations(Some(&plan_state));

        for tool_name in [tools::REQUEST_USER_INPUT, tools::START_PLANNING, tools::TASK_TRACKER] {
            let registration = registrations
                .iter()
                .find(|registration| registration.name() == tool_name)
                .expect("builtin registration should exist");
            assert!(registration.native_cgp_factory().is_some(), "expected native CGP factory for {tool_name}");
        }

        assert!(
            registrations
                .iter()
                .all(|registration| registration.name() != tools::UNIFIED_SEARCH
                    && registration.name() != tools::UNIFIED_FILE
                    && registration.name() != tools::UNIFIED_EXEC),
            "removed unified tools must not have builtin registrations"
        );
    }

    #[test]
    fn task_tracker_builtin_registration_matches_workflow_metadata() {
        let plan_state = PlanningWorkflowState::new(PathBuf::from("/workspace"));

        let standard = builtin_tool_registrations(Some(&plan_state))
            .into_iter()
            .find(|registration| registration.name() == tools::TASK_TRACKER)
            .expect("standard task_tracker registration should exist");
        assert_eq!(standard.metadata().description(), Some(task_tracker_description_for_workflow(false)));
        assert_eq!(
            standard.metadata().parameter_schema().expect("standard schema")["properties"]["index"]["minimum"],
            0
        );

        plan_state.enable();
        let planning = builtin_tool_registrations(Some(&plan_state))
            .into_iter()
            .find(|registration| registration.name() == tools::TASK_TRACKER)
            .expect("planning task_tracker registration should exist");
        assert_eq!(planning.metadata().description(), Some(task_tracker_description_for_workflow(true)));
        assert_eq!(
            planning.metadata().parameter_schema().expect("planning schema")["properties"]["index"]["minimum"],
            1
        );
    }

    #[test]
    fn codex_baseline_builtins_are_canonical_public_tools() {
        let plan_state = PlanningWorkflowState::new(PathBuf::from("/workspace"));
        let registrations = builtin_tool_registrations(Some(&plan_state));

        for tool_name in [tools::EXEC_COMMAND, tools::WRITE_STDIN] {
            let registration = registrations
                .iter()
                .find(|registration| registration.name() == tool_name)
                .expect("canonical public registration should exist");
            assert!(registration.expose_in_llm(), "{tool_name} should be public");
            assert!(registration.metadata().aliases().is_empty(), "{tool_name} should not rely on aliases");
        }

        let code_search = registrations
            .iter()
            .find(|registration| registration.name() == tools::CODE_SEARCH)
            .expect("advanced public code_search registration should exist");
        assert!(code_search.expose_in_llm(), "code_search should be public");
        assert!(code_search.metadata().aliases().is_empty(), "code_search should not rely on aliases");
        let schema = code_search.metadata().parameter_schema().expect("code_search schema");
        assert_eq!(schema["required"], json!(["query"]));
        let mut property_names = schema["properties"]
            .as_object()
            .expect("properties")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        property_names.sort_unstable();
        assert_eq!(property_names, ["file_types", "max_results", "path", "query", "result_types"]);

        for tool_name in [tools::UNIFIED_SEARCH, tools::UNIFIED_EXEC, tools::UNIFIED_FILE] {
            assert!(
                registrations.iter().all(|registration| registration.name() != tool_name),
                "{tool_name} must not have a builtin registration"
            );
        }
    }

    #[test]
    fn web_search_schema_requires_canonical_query() {
        let registrations = builtin_tool_registrations(None);
        let web_search = registrations
            .iter()
            .find(|registration| registration.name() == tools::WEB_SEARCH)
            .expect("web_search registration should exist");
        let schema = web_search.metadata().parameter_schema().expect("web_search schema");

        assert_eq!(schema["required"], json!(["query"]));
        assert_eq!(schema["additionalProperties"], json!(false));
        let mut property_names = schema["properties"]
            .as_object()
            .expect("properties")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        property_names.sort_unstable();
        assert_eq!(property_names, ["max_results", "query"]);
    }

    #[test]
    fn multi_agent_builtins_expose_updated_descriptions() {
        let plan_state = PlanningWorkflowState::new(PathBuf::from("/workspace"));
        let registrations = builtin_tool_registrations(Some(&plan_state));

        let agent = registrations
            .iter()
            .find(|registration| registration.name() == tools::AGENT)
            .expect("agent registration should exist");
        assert!(
            agent
                .metadata()
                .description()
                .expect("agent description")
                .contains("delegated child agents")
        );
        for alias in [
            tools::SPAWN_AGENT,
            tools::SPAWN_BACKGROUND_SUBPROCESS,
            tools::SEND_INPUT,
            tools::RESUME_AGENT,
            tools::WAIT_AGENT,
            tools::CLOSE_AGENT,
        ] {
            assert!(agent.metadata().aliases().iter().any(|candidate| candidate == alias));
        }
    }

    #[test]
    fn web_fetch_builtin_description_guides_agents_to_try_llms_txt_first() {
        let registrations = builtin_tool_registrations(None);
        let web_fetch = registrations
            .iter()
            .find(|registration| registration.name() == tools::WEB_FETCH)
            .expect("web_fetch registration should exist");
        let description = web_fetch.metadata().description().expect("web_fetch description");

        assert!(description.contains("/llms.txt"));
        assert!(description.contains("abc.com"));
        assert!(description.contains("https://abc.com/llms.txt"));
        assert!(description.contains("traverse"));
    }

    #[test]
    fn web_fetch_builtin_description_matches_preview_and_temp_file_result() {
        let registrations = builtin_tool_registrations(None);
        let web_fetch = registrations
            .iter()
            .find(|registration| registration.name() == tools::WEB_FETCH)
            .expect("web_fetch registration should exist");
        let description = web_fetch.metadata().description().expect("web_fetch description");

        // The default mode returns a preview plus a temp_file path; it does
        // not produce an analyzed summary.
        assert!(description.contains("`preview`"));
        assert!(description.contains("`temp_file`"));
        assert!(description.contains("format=markdown"));
        assert!(!description.contains("analyzed summary"));
        assert!(!description.contains("Accepts:"));
        assert!(!description.contains("Do NOT"));
    }

    #[test]
    fn web_fetch_schema_accepts_markdown_format() {
        let registrations = builtin_tool_registrations(None);
        let web_fetch = registrations
            .iter()
            .find(|registration| registration.name() == tools::WEB_FETCH)
            .expect("web_fetch registration should exist");
        let schema = web_fetch.metadata().parameter_schema().expect("web_fetch schema");

        assert_eq!(schema["properties"]["format"]["enum"], json!(["summary", "markdown"]));
        assert_eq!(schema["additionalProperties"], json!(false));
        let validator = jsonschema::validator_for(schema).expect("web_fetch schema should compile");
        assert!(validator.is_valid(&json!({"url": "https://example.com", "format": "markdown"})));
        assert!(validator.is_valid(&json!({"url": "https://example.com", "format": "summary"})));
        assert!(!validator.is_valid(&json!({"url": "https://example.com", "format": "html"})));
    }

    /// Tool descriptions are part of the prompt and directly drive tool
    /// selection accuracy (Section 18.3.4 of the agentic-AI guide). This test
    /// enforces a structural contract so that regressions in description
    /// quality are caught at `cargo test` time rather than via observed
    /// agent misbehavior.
    ///
    /// Every LLM-visible tool with a description must satisfy the rules
    /// below. Rules 2 and 3 are checked against the description sent in the
    /// default Progressive documentation mode, which must be the complete
    /// source description:
    /// 1. Length is between 40 and 1500 characters.
    /// 2. Contains at least one verb cue ("Use", "Create", "List", "Fetch",
    ///    "Search", "Send", "Apply", "Read", "Edit", etc.) so the model can
    ///    recognize the action the tool performs.
    /// 3. For tools that mutate state, network-call, schedule work, or
    ///    require confirmation, the description must contain a constraint cue
    ///    ("max ", "rate-limit", "session", "blocks", "timeout",
    ///    "requires approval", etc.) so the model knows the limits and side
    ///    effects. Prohibition phrasing ("Do NOT", "Avoid", "never") does not
    ///    satisfy this rule: models that follow descriptions literally
    ///    over-apply it, so descriptions state the concrete limit instead.
    ///
    /// Tools exempted from rule 3 are simple read-only helpers where the
    /// model can safely call them without explicit guard-rails.
    #[test]
    fn tool_descriptions_satisfy_documented_contract() {
        use crate::config::ToolDocumentationMode;
        use crate::tools::handlers::compact::compact_tool_description;
        use crate::tools::handlers::{SessionSurface, SessionToolCatalog, SessionToolsConfig, ToolModelCapabilities};

        let plan_state = PlanningWorkflowState::new(PathBuf::from("/workspace"));
        let registrations = builtin_tool_registrations(Some(&plan_state));

        let verb_cues = [
            "Use ",
            "Create ",
            "Control ",
            "List ",
            "Fetch ",
            "Search ",
            "Send ",
            "Apply ",
            "Read ",
            "Write ",
            "Edit ",
            "Patch ",
            "Delete ",
            "Move ",
            "Copy ",
            "Spawn ",
            "Launch ",
            "Close ",
            "Resume ",
            "Wait ",
            "Connect ",
            "Disconnect ",
            "Schedule ",
            "Inspect ",
            "Persist ",
            "Request ",
            "Open ",
            "Stop ",
            "Run ",
            "Track ",
            "Update ",
        ];
        let constraint_cues = [
            "max ",
            "rate-limit",
            "rate limit",
            "session",
            "blocks",
            "timeout",
            "cap ",
            "outlives",
            "inherits",
            "expires",
            "limited to",
            "max_bytes",
            "max_results",
            "max_lines",
            "max chars",
            "max size",
            "once per",
            "requires ",
            "requires approval",
            "permission",
            "approval",
            "exceeds",
            "scoped",
        ];
        // Read-only / single-action helpers where a constraint cue is not
        // strictly required. Entries are registration names; aliases such as
        // cron_list or mcp_search_tools never reach this check.
        let rule3_allowlist: &[&str] = &[
            tools::REQUEST_USER_INPUT,
            tools::SEARCH_TOOLS,
            tools::TASK_TRACKER,
            tools::START_PLANNING,
            tools::CODE_SEARCH,
        ];

        for allowed in rule3_allowlist {
            assert!(
                registrations.iter().any(|registration| registration.name() == *allowed),
                "rule3 allowlist entry {allowed} is not a builtin registration name"
            );
        }

        // Rules 2 and 3 are checked against the text the model receives in
        // the default Progressive documentation mode, not just the source
        // string, so a projection that drops later sentences cannot hide a
        // missing cue.
        let progressive_catalog =
            SessionToolCatalog::rebuild_from_registrations(builtin_tool_registrations(Some(&plan_state)));
        let progressive_entries = progressive_catalog.schema_entries(SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Progressive,
            ToolModelCapabilities::default(),
        ));
        assert!(!progressive_entries.is_empty(), "Progressive catalog must expose builtin tools");

        for registration in &registrations {
            if !registration.expose_in_llm() {
                continue;
            }
            let Some(source_description) = registration.metadata().description() else {
                continue;
            };
            let tool_name = registration.name();

            // Rule 1: length of the source description.
            let len = source_description.chars().count();
            assert!((40..=1500).contains(&len), "{tool_name}: description length {len} outside [40, 1500]");

            let projected = compact_tool_description(source_description, ToolDocumentationMode::Progressive, None);
            let sent = progressive_entries
                .iter()
                .find(|entry| entry.name == tool_name)
                .map_or(projected.as_str(), |entry| entry.description.as_str());
            assert_eq!(
                sent,
                compact_tool_description(source_description, ToolDocumentationMode::Full, None),
                "{tool_name}: Progressive mode must send the complete builtin description"
            );
            let description = sent;

            // Rule 2: verb cue (case-sensitive "Use " is the most common).
            let has_verb = verb_cues.iter().any(|cue| description.contains(cue));
            assert!(
                has_verb,
                "{tool_name}: description must contain a verb cue like 'Use ', 'Create ', 'Fetch ', etc.\nDescription: {description}"
            );

            // Rule 3: constraint cue for side-effect tools.
            if rule3_allowlist.contains(&tool_name) {
                continue;
            }
            let has_constraint = constraint_cues.iter().any(|cue| description.contains(cue));
            assert!(
                has_constraint,
                "{tool_name}: side-effect description must state a concrete constraint cue ('max ', 'rate-limit', \
                 'session', 'timeout', 'requires approval', 'inherits', ...).\nDescription: {description}"
            );
        }
    }
    #[test]
    fn default_config_exposed_tool_count_within_cap() {
        // Regression guard for the tool-consolidation work: the number of
        // LLM-exposed built-in tools must stay bounded so the model does not
        // waste attention choosing between near-duplicates. Any new
        // registration must either consolidate an existing tool, be deferred
        // behind the deferred-loading path, or deliberately raise this cap in
        // review.
        let registrations = builtin_tool_registrations(None);
        let exposed: usize = registrations.iter().filter(|registration| registration.expose_in_llm()).count();
        // The optional canonical public-decision surface is the fifteenth builtin.
        assert!(
            exposed <= 15,
            "exposed built-in tool count is {exposed}; expected <= 15. \
             Consolidate, defer, or raise the cap in review."
        );
    }

    #[test]
    fn public_decision_is_available_in_interactive_and_headless_planning_and_execution() {
        use crate::config::ToolDocumentationMode;
        use crate::tools::handlers::{SessionSurface, SessionToolCatalog, SessionToolsConfig, ToolModelCapabilities};
        let catalog = SessionToolCatalog::rebuild_from_registrations(builtin_tool_registrations(None));
        for surface in [SessionSurface::Interactive, SessionSurface::AgentRunner] {
            for planning in [false, true] {
                let config = SessionToolsConfig::full_public(
                    surface,
                    CapabilityLevel::CodeSearch,
                    ToolDocumentationMode::Full,
                    ToolModelCapabilities::default(),
                )
                .with_planning_active(planning);
                let names = catalog.public_tool_names(config);
                assert!(
                    names.iter().any(|name| name == tools::RECORD_DECISION),
                    "decision missing on {surface:?}, planning={planning}"
                );
            }
        }
    }

    /// End-to-end regression for the tool-consolidation work: builds the real
    /// `SessionToolCatalog` from the actual builtin registrations (not a
    /// synthetic subset) and asserts the number of tool definitions/function
    /// declarations the model actually sees, for a default non-native-memory
    /// config, stays within the post-fold cap. This complements
    /// `default_config_exposed_tool_count_within_cap` (which only counts
    /// `expose_in_llm()` registrations) by exercising the full catalog
    /// pipeline, including dedup-by-public-name and deferred-loading
    /// collapsing.
    #[test]
    fn emitted_model_tool_count_stays_within_cap_for_default_config() {
        use crate::config::ToolDocumentationMode;
        use crate::tools::handlers::{SessionSurface, SessionToolCatalog, SessionToolsConfig, ToolModelCapabilities};

        let registrations = builtin_tool_registrations(None);
        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let config = SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        );

        let model_tools = catalog.model_tools(config.clone());
        assert!(
            model_tools.len() <= 14,
            "emitted model_tools count is {}; expected <= 14. \
             Consolidate, defer, or raise the cap in review.",
            model_tools.len()
        );

        let function_declarations = catalog.function_declarations(config);
        assert!(
            function_declarations.len() <= 14,
            "emitted function_declarations count is {}; expected <= 14. \
             Consolidate, defer, or raise the cap in review.",
            function_declarations.len()
        );
    }

    /// End-to-end regression for the first-request token budget: the actual
    /// builtin tool schemas sent in Progressive mode must fit in a small
    /// token envelope, leaving room for the system prompt and conversation.
    #[test]
    fn emitted_model_tool_schema_fits_within_first_request_budget() {
        use crate::config::ToolDocumentationMode;
        use crate::tools::handlers::{SessionSurface, SessionToolCatalog, SessionToolsConfig, ToolModelCapabilities};
        use serde::Serialize;

        #[derive(Serialize)]
        struct ToolSchemaEstimate<'a> {
            name: &'a str,
            description: &'a str,
            parameters: &'a serde_json::Value,
        }

        let registrations = builtin_tool_registrations(None);
        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let config = SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Progressive,
            ToolModelCapabilities::default(),
        );

        let schema_entries = catalog.schema_entries(config);
        let total_tokens: usize = schema_entries
            .iter()
            .map(|entry| {
                let estimate = ToolSchemaEstimate {
                    name: &entry.name,
                    description: &entry.description,
                    parameters: &entry.parameters,
                };
                serde_json::to_string(&estimate).map(|s| s.len() / 4).unwrap_or(0)
            })
            .sum();

        // Progressive mode sends complete builtin tool descriptions and keeps
        // parameter descriptions (trimming only long tails), because models
        // that follow tool definitions literally act on the whole text. The
        // cap is the harness-tax gate (HarnessTax): keep the emitted schema
        // envelope tight so first-call fixed overhead stays low. Measured
        // ~2,336 on the Codex-4 default profile with public decision recording
        // in the initial catalog and discovery surface. Retain a bounded
        // 2,400-token envelope.
        assert!(
            total_tokens <= 2_400,
            "emitted model tool schema tokens in Progressive mode is {total_tokens}; expected <= 2_400"
        );
    }

    /// End-to-end regression for the combined first-request token budget:
    /// the effective-default system prompt (Minimal) plus Progressive tool
    /// schemas, instruction appendix, and welcome addendum must stay under
    /// 6k tokens with no MCP, and under 8k tokens with 5 simulated MCP servers.
    #[test]
    fn first_request_total_token_budget_within_limit() {
        use crate::config::ToolDocumentationMode;
        use crate::config::types::SystemPromptMode;
        use crate::prompts::system::static_profile_prompt;
        use crate::tools::handlers::{SessionSurface, SessionToolCatalog, SessionToolsConfig, ToolModelCapabilities};
        use serde::Serialize;

        #[derive(Serialize)]
        struct ToolSchemaEstimate<'a> {
            name: &'a str,
            description: &'a str,
            parameters: &'a serde_json::Value,
        }

        let registrations = builtin_tool_registrations(None);
        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let config = SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Progressive,
            ToolModelCapabilities::default(),
        );

        let schema_entries = catalog.schema_entries(config);
        let tool_schema_tokens: usize = schema_entries
            .iter()
            .map(|entry| {
                let estimate = ToolSchemaEstimate {
                    name: &entry.name,
                    description: &entry.description,
                    parameters: &entry.parameters,
                };
                serde_json::to_string(&estimate).map(|s| s.len() / 4).unwrap_or(0)
            })
            .sum();

        // Measure the configured default profile (Minimal after lean-defaults).
        let system_prompt = static_profile_prompt(SystemPromptMode::default());
        let system_prompt_tokens = system_prompt.len() / 4;

        let instruction_appendix_tokens = 250;
        let welcome_addendum_tokens = 200;

        let total_no_mcp =
            system_prompt_tokens + tool_schema_tokens + instruction_appendix_tokens + welcome_addendum_tokens;

        assert!(
            total_no_mcp <= 6_000,
            "first-request token budget exceeded: {total_no_mcp} tokens (system={system_prompt_tokens}, tools={tool_schema_tokens}, instructions={instruction_appendix_tokens}, addendum={welcome_addendum_tokens}); expected <= 6_000"
        );

        let mcp_tool_count = 5;
        let estimated_mcp_schema_tokens = mcp_tool_count * 300;
        let total_with_mcp = total_no_mcp + estimated_mcp_schema_tokens;

        assert!(
            total_with_mcp <= 8_000,
            "first-request token budget with MCP exceeded: {total_with_mcp} tokens; expected <= 8_000"
        );

        // Explicit Default-mode profile must also stay inside the no-MCP
        // ceiling when selected (S2-prompt).
        let default_prompt = static_profile_prompt(SystemPromptMode::Default);
        let total_default_mode =
            default_prompt.len() / 4 + tool_schema_tokens + instruction_appendix_tokens + welcome_addendum_tokens;
        assert!(
            total_default_mode <= 6_000,
            "Default-mode first-request budget exceeded: {total_default_mode} tokens; expected <= 6_000"
        );
    }

    /// Cache-stability guard: building the same system prompt + Progressive
    /// tool schema twice must produce byte-identical output. Any non-determinism
    /// (random ordering, uninitialized memory, race) invalidates provider
    /// prompt caches and re-pays full input cost on every turn.
    #[test]
    fn system_prompt_and_tools_prefix_is_byte_stable_across_rebuilds() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        use crate::config::ToolDocumentationMode;
        use crate::prompts::system::default_system_prompt;
        use crate::tools::handlers::{SessionSurface, SessionToolCatalog, SessionToolsConfig, ToolModelCapabilities};
        use serde::Serialize;

        #[derive(Serialize)]
        struct ToolSchemaEstimate<'a> {
            name: &'a str,
            description: &'a str,
            parameters: &'a serde_json::Value,
        }

        fn hash_system_prompt_and_tools() -> u64 {
            let mut hasher = DefaultHasher::new();

            let system_prompt = default_system_prompt();
            system_prompt.hash(&mut hasher);

            let registrations = builtin_tool_registrations(None);
            let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
            let config = SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Progressive,
                ToolModelCapabilities::default(),
            );

            let schema_entries = catalog.schema_entries(config);
            for entry in &schema_entries {
                let estimate = ToolSchemaEstimate {
                    name: &entry.name,
                    description: &entry.description,
                    parameters: &entry.parameters,
                };
                if let Ok(json) = serde_json::to_string(&estimate) {
                    json.hash(&mut hasher);
                }
            }

            hasher.finish()
        }

        let hash1 = hash_system_prompt_and_tools();
        let hash2 = hash_system_prompt_and_tools();

        assert_eq!(hash1, hash2, "system prompt + tool schema prefix is not byte-stable across rebuilds");
    }
}
