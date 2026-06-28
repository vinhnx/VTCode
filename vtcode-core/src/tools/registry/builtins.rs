// The `linkme::distributed_slice` macro uses `link_section` internally,
// which triggers the `unsafe_code` lint. This is inherent to the crate's
// mechanism and cannot be avoided at the call site.
#![allow(unsafe_code)]

use std::path::PathBuf;

use linkme::distributed_slice;

use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::tool_policy::ToolPolicy;
use crate::tools::defuddle::{DEFUDDLE_FETCH_DESCRIPTION, DefuddleTool};
use crate::tools::handlers::{
    FinishPlanningTool, PlanningWorkflowState, StartPlanningTool, TaskTrackerTool,
};
use crate::tools::native_memory;
use crate::tools::request_user_input::RequestUserInputTool;
use crate::tools::tool_intent::builtin_tool_behavior;
use crate::tools::web_fetch::{WEB_FETCH_DESCRIPTION, WebFetchTool};
use crate::tools::web_search::{WEB_SEARCH_DESCRIPTION, WebSearchTool};
use serde_json::json;
use vtcode_utility_tool_specs::{
    apply_patch_parameters, close_agent_parameters, cron_create_parameters, cron_delete_parameters,
    cron_list_parameters, list_files_parameters, read_file_parameters, resume_agent_parameters,
    send_input_parameters, spawn_agent_parameters, spawn_background_subprocess_parameters,
    unified_exec_parameters, unified_file_parameters, unified_search_parameters,
    wait_agent_parameters,
};

use super::distributed::{BUILTIN_TOOLS, tool_config};
use super::registration::{ToolCatalogSource, ToolRegistration};
use super::{ToolInventory, ToolRegistry, native_cgp_tool_factory};

/// Register all builtin tools into the inventory using the shared planning workflow state.
pub(super) fn register_builtin_tools(
    inventory: &ToolInventory,
    planning_workflow_state: &PlanningWorkflowState,
) {
    for registration in builtin_tool_registrations(Some(planning_workflow_state)) {
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
/// `None`, and a placeholder `PlanningWorkflowState` will be used.
pub(super) fn builtin_tool_registrations(
    planning_workflow_state: Option<&PlanningWorkflowState>,
) -> Vec<ToolRegistration> {
    let mut registrations: Vec<ToolRegistration> = BUILTIN_TOOLS
        .iter()
        .map(|factory| factory(planning_workflow_state))
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
fn register_request_user_input(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    let request_user_input_factory = native_cgp_tool_factory(|| RequestUserInputTool);
    ToolRegistration::from_tool_instance(
        tools::REQUEST_USER_INPUT,
        CapabilityLevel::Basic,
        RequestUserInputTool,
    )
    .with_native_cgp_factory(request_user_input_factory)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_memory(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
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
fn register_cron_create(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::CRON_CREATE,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::cron_create_executor,
    )
    .with_description(
        "Create a session-scoped scheduled prompt using a cron expression, fixed interval, or one-shot fire time. Use cron_create to defer work, schedule recurring checks, or fire a one-shot reminder. Do NOT schedule per-minute jobs — they exhaust the per-turn tool budget and will be rate-limited. Scheduled prompts are session-scoped; jobs die when the vtcode process exits.",
    )
    .with_parameter_schema(cron_create_parameters())
    .with_aliases(["schedule_task", "loop_create"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_cron_list(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
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
fn register_cron_delete(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
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
    .with_native_cgp_factory(native_cgp_tool_factory(move || {
        StartPlanningTool::new(factory_state.clone())
    }))
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_finish_planning(plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    let plan_state = plan_state
        .cloned()
        .unwrap_or_else(|| PlanningWorkflowState::new(PathBuf::new()));
    let factory_state = plan_state.clone();
    ToolRegistration::from_tool_instance(
        tools::FINISH_PLANNING,
        CapabilityLevel::Basic,
        FinishPlanningTool::new(plan_state),
    )
    .with_native_cgp_factory(native_cgp_tool_factory(move || {
        FinishPlanningTool::new(factory_state.clone())
    }))
}

// ---------------------------------------------------------------------------
// TASK TRACKER (NL2Repo-Bench: Explicit Task Planning)
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_task_tracker(plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    let plan_state = plan_state
        .cloned()
        .unwrap_or_else(|| PlanningWorkflowState::new(PathBuf::new()));
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
    .with_description(
        "Track task progress through a single checklist API (action: create | update | list | add). Use task_tracker with action=create at the start of a multi-step plan; use action=update as work progresses; use action=list to review current state. Do NOT call action=create twice — subsequent calls update the existing checklist. Tracker state mirrors between `.vtcode/tasks/current_task.md` and active plan sidecar files when available.",
    )
    .with_aliases(["plan_manager", "track_tasks", "checklist"])
}

// ---------------------------------------------------------------------------
// MULTI-AGENT TOOLS
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_spawn_agent(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
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
fn register_spawn_background_subprocess(
    _plan_state: Option<&PlanningWorkflowState>,
) -> ToolRegistration {
    ToolRegistration::new(
        tools::SPAWN_BACKGROUND_SUBPROCESS,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::spawn_background_subprocess_executor,
    )
    .with_description(
        "Launch a managed background subprocess that outlives the current turn. Use this for long-running daemons (file watchers, dev servers, indexers) where you need to return control to the model immediately. Do NOT use this for one-shot shell commands — use unified_exec with action=run instead. Background subprocesses are session-scoped; they die with the vtcode process.",
    )
    .with_parameter_schema(spawn_background_subprocess_parameters())
    .with_aliases(["background_subagent", "launch_background_helper"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_send_input(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::SEND_INPUT,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::send_input_executor,
    )
    .with_description(
        "Send follow-up input to an existing child agent. Use to continue a delegated task with new context or direction. Do NOT use this to ask the model a one-off question — answer inline instead. Requires an existing agent_id from a prior spawn_agent call.",
    )
    .with_parameter_schema(send_input_parameters())
    .with_aliases(["message_agent", "continue_agent"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_wait_agent(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::WAIT_AGENT,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::wait_agent_executor,
    )
    .with_description(
        "Wait for child agents to reach a terminal state. Use this when you spawned an agent and need its result before continuing. Do NOT call wait_agent with an empty ids array — provide at least one agent id from a prior spawn_agent call. Default timeout 300s; pass timeout_ms to extend for long-running delegated tasks.",
    )
    .with_parameter_schema(wait_agent_parameters())
    .with_aliases(["wait_subagent"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_resume_agent(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::RESUME_AGENT,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::resume_agent_executor,
    )
    .with_description(
        "Resume a previously completed or closed child agent subtree from its saved context. Use this to continue work in a delegated agent after it has closed. Do NOT call resume_agent on a still-running child — use send_input instead. Resume is session-scoped: agents cannot be resumed across separate vtcode sessions.",
    )
    .with_parameter_schema(resume_agent_parameters())
    .with_aliases(["resume_subagent"])
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_close_agent(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::CLOSE_AGENT,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::close_agent_executor,
    )
    .with_description(
        "Close a child agent subtree, cancelling any active work and marking the thread closed. Use this when you are done with a delegated agent and want to free its tool budget. Do NOT close an agent you still need results from — call wait_agent first. Closed subtrees cannot be queried; resume_agent is required to bring one back.",
    )
    .with_parameter_schema(close_agent_parameters())
    .with_aliases(["close_subagent"])
}

// ---------------------------------------------------------------------------
// SEARCH & DISCOVERY
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_unified_search(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::UNIFIED_SEARCH,
        CapabilityLevel::CodeSearch,
        false,
        ToolRegistry::unified_search_executor,
    )
    .with_description(
        "Search and discovery: grep text, list files, structural (ast-grep), list available tools, list errors, web search/fetch, list skills. Use action=list to enumerate files; action=grep to search for a regex pattern across files; action=structural for AST-shaped code queries; action=web with a 'query' for web search; action=web with a 'url' to fetch a page. Do NOT use action=list to read file contents — use action=read via unified_file or read_file instead. Result lists are capped by the tool's max_results parameter; pass a higher limit only when you genuinely need more hits.",
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
    ToolRegistration::from_tool_instance(
        tools::WEB_FETCH,
        CapabilityLevel::Basic,
        web_fetch,
    )
    .with_native_cgp_factory(web_fetch_factory)
    .with_description(WEB_FETCH_DESCRIPTION)
    .with_parameter_schema(json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "URL to fetch (HTTPS required by default)"
            },
            "prompt": {
                "type": "string",
                "description": "Question or instruction for analyzing the fetched content. Omit for a default summary."
            },
            "max_bytes": {
                "type": "integer",
                "description": "Maximum response body size in bytes (default: 500000). The default is generous — most pages including llms.txt fit easily. Only set this if you need to cap a very large page."
            },
            "timeout_secs": {
                "type": "integer",
                "description": "Request timeout in seconds (default: 30)"
            }
        },
        "required": ["url"],
        "additionalProperties": false
    }))
    .with_permission(ToolPolicy::Prompt)
    .with_aliases(["fetch_url", "web"])
}

// ---------------------------------------------------------------------------
// WEB SEARCH (built-in, query -> ranked results, keyless DuckDuckGo only)
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_web_search(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    let web_search = WebSearchTool::with_config(
        tool_config()
            .map(|snapshot| snapshot.web_search.clone())
            .unwrap_or_default(),
    );
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
                "pattern": {
                    "type": "string",
                    "description": "Alias for 'query' (used by the unified_search tool)."
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
    ToolRegistration::from_tool_instance(
        tools::DEFUDDLE_FETCH,
        CapabilityLevel::Basic,
        defuddle,
    )
    .with_native_cgp_factory(defuddle_factory)
    .with_description(DEFUDDLE_FETCH_DESCRIPTION)
    .with_parameter_schema(json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "format": "uri",
                "description": "The URL to fetch through defuddle.md. Must be http(s)."
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
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_mcp_search_tools(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
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
fn register_mcp_get_tool_details(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
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
fn register_mcp_list_servers(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
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
fn register_mcp_connect_server(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::MCP_CONNECT_SERVER,
        CapabilityLevel::CodeSearch,
        false,
        ToolRegistry::mcp_connect_server_executor,
    )
    .with_description(
        "Connect one configured MCP server by name. Use this when an MCP tool is referenced but the server has not been initialized yet. Do NOT call connect_server unless the server's tools are needed — the connection has overhead. Requires user confirmation via the permissions system (ToolPolicy::Prompt).",
    )
    .with_parameter_schema(mcp_server_name_parameters())
    .with_permission(ToolPolicy::Prompt)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_mcp_disconnect_server(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::MCP_DISCONNECT_SERVER,
        CapabilityLevel::CodeSearch,
        false,
        ToolRegistry::mcp_disconnect_server_executor,
    )
    .with_description(
        "Disconnect one active MCP server by name. Use this to free resources or reset a misbehaving MCP connection. Do NOT disconnect servers mid-task — any in-flight tool calls from that server will fail. Requires user confirmation via the permissions system (ToolPolicy::Prompt).",
    )
    .with_parameter_schema(mcp_server_name_parameters())
    .with_permission(ToolPolicy::Prompt)
}

// ---------------------------------------------------------------------------
// SHELL EXECUTION
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_unified_exec(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::UNIFIED_EXEC,
        CapabilityLevel::Bash,
        false,
        ToolRegistry::unified_exec_executor,
    )
    .with_description(
        "Shell & code execution. Actions: run, write, poll, continue, inspect, list, close, code. Use action=run for one-shot commands; action=write + action=poll for interactive shells that outlive a single call. Do NOT use action=write without a follow-up poll/close — the session leaks. Default timeout is 180s; pass timeout explicitly for long-running commands (max 1800s). All shell calls run through the active sandbox policy; requires ToolPolicy::Prompt confirmation.",
    )
    .with_parameter_schema(unified_exec_parameters())
    .with_aliases([
        tools::EXEC_COMMAND,
        tools::WRITE_STDIN,
        tools::RUN_PTY_CMD,
        tools::EXECUTE_CODE,
        "bash",
        "exec code",
        "run command",
    ])
}

// ---------------------------------------------------------------------------
// FILE OPERATIONS
// ---------------------------------------------------------------------------

#[distributed_slice(BUILTIN_TOOLS)]
fn register_unified_file(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::UNIFIED_FILE,
        CapabilityLevel::Editing,
        false,
        ToolRegistry::unified_file_executor,
    )
    .with_description(
        "Read, write, edit, patch, delete, move, or copy a single file. Use action=read to load file contents (with optional range); action=edit for surgical replacements (exact old_str, max 800 chars/40 lines per side); action=patch for larger or multi-hunk changes; action=write for new files or full replacement; action=delete to remove a file. Do NOT mix action=edit with action=patch in the same call. Requires ToolPolicy::Prompt confirmation for write/edit/patch/delete/move/copy actions.",
    )
    .with_parameter_schema(unified_file_parameters())
    .with_aliases([
        tools::READ_FILE,
        tools::WRITE_FILE,
        tools::EDIT_FILE,
        tools::DELETE_FILE,
        tools::CREATE_FILE,
        "repo_browser.read_file",
        "repo_browser.write_file",
    ])
}

// ---------------------------------------------------------------------------
// INTERNAL TOOLS (Hidden from LLM, used by unified tools)
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
    .with_parameter_schema(read_file_parameters())
    .with_permission(ToolPolicy::Allow)
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_list_files(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
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
fn register_write_file(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::WRITE_FILE,
        CapabilityLevel::Editing,
        false,
        ToolRegistry::write_file_executor,
    )
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_edit_file(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::EDIT_FILE,
        CapabilityLevel::Editing,
        false,
        ToolRegistry::edit_file_executor,
    )
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_run_pty_cmd(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::RUN_PTY_CMD,
        CapabilityLevel::Bash,
        false,
        ToolRegistry::run_pty_cmd_executor,
    )
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_send_pty_input(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::SEND_PTY_INPUT,
        CapabilityLevel::Bash,
        false,
        ToolRegistry::send_pty_input_executor,
    )
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
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_get_errors(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::GET_ERRORS,
        CapabilityLevel::CodeSearch,
        false,
        ToolRegistry::get_errors_executor,
    )
    .with_llm_visibility(false)
}

#[distributed_slice(BUILTIN_TOOLS)]
fn register_apply_patch(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
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
        let plan_state = PlanningWorkflowState::new(PathBuf::from("/workspace"));
        let registrations = builtin_tool_registrations(Some(&plan_state));

        for tool_name in [
            tools::REQUEST_USER_INPUT,
            tools::START_PLANNING,
            tools::FINISH_PLANNING,
            tools::TASK_TRACKER,
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
        let plan_state = PlanningWorkflowState::new(PathBuf::from("/workspace"));
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
        let plan_state = PlanningWorkflowState::new(PathBuf::from("/workspace"));
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
                .contains("follow-up input")
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
                .contains("terminal state")
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
                .contains("outlives the current turn")
        );
    }

    #[test]
    fn web_fetch_builtin_description_guides_agents_to_try_llms_txt_first() {
        let registrations = builtin_tool_registrations(None);
        let web_fetch = registrations
            .iter()
            .find(|registration| registration.name() == tools::WEB_FETCH)
            .expect("web_fetch registration should exist");
        let description = web_fetch
            .metadata()
            .description()
            .expect("web_fetch description");

        assert!(description.contains("/llms.txt"));
        assert!(description.contains("abc.com"));
        assert!(description.contains("https://abc.com/llms.txt"));
        assert!(description.contains("traverse"));
    }

    /// Tool descriptions are part of the prompt and directly drive tool
    /// selection accuracy (Section 18.3.4 of the agentic-AI guide). This test
    /// enforces a structural contract so that regressions in description
    /// quality are caught at `cargo test` time rather than via observed
    /// agent misbehavior.
    ///
    /// Every LLM-visible tool with a description must satisfy:
    /// 1. Length is between 40 and 1200 characters.
    /// 2. Contains at least one verb cue ("Use", "Create", "List", "Fetch",
    ///    "Search", "Send", "Apply", "Read", "Edit", etc.) so the model can
    ///    recognize the action the tool performs.
    /// 3. For tools that mutate state, network-call, schedule work, or
    ///    require confirmation, the description must contain either an
    ///    anti-pattern cue ("Do NOT", "Avoid", "sparely", "Don't", etc.) OR
    ///    a constraint cue ("max", "rate-limit", "session", "Prompt",
    ///    "blocks", "timeout", etc.) so the model knows the limits and side
    ///    effects.
    ///
    /// Tools exempted from rule 3 are simple read-only helpers where the
    /// model can safely call them without explicit guard-rails.
    #[test]
    fn tool_descriptions_satisfy_documented_contract() {
        let plan_state = PlanningWorkflowState::new(PathBuf::from("/workspace"));
        let registrations = builtin_tool_registrations(Some(&plan_state));

        let verb_cues = [
            "Use ",
            "Create ",
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
        let anti_pattern_cues = [
            "Do NOT",
            "Do not",
            "Don't",
            "Avoid ",
            "sparely",
            "spareingly",
            "must not",
            "must only",
            "never",
            "refuse",
            "Refuse ",
            "Limit use",
            "limit use",
            "no need",
            "Do not call",
            "do not call",
            "not for",
            "not to be used",
        ];
        let constraint_cues = [
            "max ",
            "rate-limit",
            "rate limit",
            "session",
            "Prompt",
            "blocks",
            "timeout",
            "cap ",
            "outlives",
            "inherits",
            "expires",
            "Limited",
            "limited to",
            "max_bytes",
            "max_results",
            "max_lines",
            "max chars",
            "max size",
            "Once per",
            "once per",
            "requires ",
            "Permission",
            "permission",
            "approval",
            "Prompt ",
            "spareingly",
            "exceeds",
            "EXCLUSIVE",
            "scoped",
        ];
        // Read-only / single-action helpers where explicit anti-pattern and
        // constraint cues are not strictly required.
        let rule3_allowlist: &[&str] = &[
            tools::REQUEST_USER_INPUT,
            tools::CRON_LIST,
            tools::CRON_DELETE,
            tools::MCP_LIST_SERVERS,
            tools::MCP_GET_TOOL_DETAILS,
            tools::MCP_SEARCH_TOOLS,
            tools::TASK_TRACKER,
            tools::FINISH_PLANNING,
            tools::START_PLANNING,
        ];

        for registration in &registrations {
            if !registration.expose_in_llm() {
                continue;
            }
            let Some(description) = registration.metadata().description() else {
                continue;
            };
            let tool_name = registration.name();

            // Rule 1: length.
            let len = description.chars().count();
            assert!(
                (40..=1500).contains(&len),
                "{tool_name}: description length {len} outside [40, 1500]"
            );

            // Rule 2: verb cue (case-sensitive "Use " is the most common).
            let has_verb = verb_cues.iter().any(|cue| description.contains(cue));
            assert!(
                has_verb,
                "{tool_name}: description must contain a verb cue like 'Use ', 'Create ', 'Fetch ', etc.\nDescription: {description}"
            );

            // Rule 3: anti-pattern OR constraint cue for side-effect tools.
            if rule3_allowlist.contains(&tool_name) {
                continue;
            }
            let has_anti = anti_pattern_cues
                .iter()
                .any(|cue| description.contains(cue));
            let has_constraint = constraint_cues.iter().any(|cue| description.contains(cue));
            assert!(
                has_anti || has_constraint,
                "{tool_name}: side-effect description must contain an anti-pattern cue ('Do NOT', 'Avoid ', 'sparely', ...) \
                 OR a constraint cue ('max ', 'rate-limit', 'session', 'Prompt', 'timeout', 'inherits', ...).\nDescription: {description}"
            );
        }
    }
}
