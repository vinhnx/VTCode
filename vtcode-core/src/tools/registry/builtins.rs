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
    FinishPlanningTool, PlanningWorkflowState, StartPlanningTool, TaskTrackerTool,
};
use crate::tools::native_memory;
use crate::tools::request_user_input::RequestUserInputTool;
use crate::tools::tool_intent::builtin_tool_behavior;
use crate::tools::web_fetch::{WEB_FETCH_DESCRIPTION, WebFetchTool};
use crate::tools::web_search::{WEB_SEARCH_DESCRIPTION, WebSearchTool};
use serde_json::json;
use vtcode_utility_tool_specs::{
    agent_parameters, apply_patch_parameters, cron_parameters, list_files_parameters,
    read_file_parameters, unified_exec_parameters, unified_file_parameters,
    unified_search_parameters,
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
        "Access VT Code persistent memory files under /memories. Use action=view to list available notes before reading or updating; writes are limited to preferences.md, repository-facts.md, and notes/**. Returns file listing or file content.",
    )
    .with_parameter_schema(native_memory::parameter_schema())
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
        "Create, list, or delete session-scoped scheduled prompts. Use action='create' to schedule a prompt via a cron expression, fixed interval, or one-shot fire time (exactly one of cron/delay_minutes/run_at); action='list' shows scheduled prompts with ids and status; action='delete' removes one by id. Do NOT schedule per-minute jobs — they exhaust the per-turn tool budget and will be rate-limited. Scheduled prompts are session-scoped; jobs die when the vtcode process exits.",
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
fn register_agent(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::AGENT,
        CapabilityLevel::Basic,
        false,
        ToolRegistry::agent_executor,
    )
    .with_description(
        "Spawn and steer delegated child agents. Use action='spawn' to delegate a scoped task (the child inherits the current toolset and returns its agent id); action='spawn_subprocess' to launch a managed background subprocess for long-running daemons (file watchers, dev servers) — do NOT use it for one-shot shell commands, use unified_exec instead; action='send_input' to send follow-up input to a running child (requires id); action='resume' to reopen a completed/closed child from saved context (requires id); action='wait' to block the current foreground turn until one or more children reach a terminal state (requires ids array; do NOT pass an empty ids array; default timeout 300s, pass timeout_ms to extend); action='close' to cancel a child's active work and free its tool budget (requires id; do NOT close a child you still need results from — wait first; a closed subtree needs action='resume' to bring back). Children and subprocesses are session-scoped; they die with the vtcode process.",
    )
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
fn register_unified_search(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::UNIFIED_SEARCH,
        CapabilityLevel::CodeSearch,
        false,
        ToolRegistry::unified_search_executor,
    )
    .with_description(
        "Search and discover: grep text, list files, structural-search (ast-grep), list tools, list errors, web search, web fetch, and list skills. Use action=grep for regex across files; action=structural for AST-shaped queries; action=list to enumerate files; action=web with query for search or url to fetch. Do NOT use action=list to read file contents — use unified_file action=read instead. Results are capped by max_results; increase only when genuinely needed.",
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
                "description": "Question or instruction for analyzing the fetched content. Omit for a default summary. Ignored when format='markdown'."
            },
            "format": {
                "type": "string",
                "enum": ["summary", "markdown"],
                "description": "Output format. 'summary' (default) returns an analyzed summary plus a temp_file with full content. 'markdown' returns the page as cleaned markdown via the defuddle.md service — rate-limited to ONE call per session; remote http(s) URLs only."
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
    .with_aliases([
        "fetch_url",
        "web",
        tools::DEFUDDLE_FETCH,
        "defuddle",
        "extract_markdown",
    ])
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

#[distributed_slice(BUILTIN_TOOLS)]
fn register_mcp(_plan_state: Option<&PlanningWorkflowState>) -> ToolRegistration {
    ToolRegistration::new(
        tools::MCP,
        CapabilityLevel::CodeSearch,
        false,
        ToolRegistry::mcp_executor,
    )
    .with_description(
        "Discover and manage Model Context Protocol capabilities. Use action='search_tools' to find MCP tools by natural-language query (progressive detail_level: name, name_description, full); action='get_tool_details' to fetch the full input schema for one MCP tool name; action='list_servers' to list configured servers and their connection state; action='connect' to connect one configured server by name when its tools are referenced but not yet initialized (requires user confirmation via ToolPolicy::Prompt); action='disconnect' to free resources or reset a misbehaving server connection (requires user confirmation; do NOT disconnect mid-task — in-flight calls from that server will fail). Do NOT call list_servers every turn — server state changes rarely.",
    )
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
        "Execute shell commands and code: actions run, write, poll, continue, inspect, list, close, code. Use action=run for one-shot commands; action=write + action=poll for interactive shells. For action=code, write Python/JavaScript that filters, aggregates, or loops over data in-process — connected MCP tools are callable as library functions from the snippet, so prefer computing over fetching and return only small summaries instead of dumping large output into context (spooled tool results are plain files you can read/grep from code). Do NOT use action=write without a follow-up poll/close — the session leaks. Default timeout 180s (max 1800s). All shell calls run through the active sandbox policy. Requires Prompt confirmation.",
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
        "Read, write, edit, patch, delete, move, or copy a single file. Use action=read to load file contents (with optional range); action=edit for surgical text replacements (exact old_str, max 800 chars/40 lines per side); action=patch for larger or multi-hunk changes; action=write for new files or full replacement; action=delete to remove a file; action=move to rename; action=copy to duplicate. Do NOT mix action=edit with action=patch in the same call. Requires Prompt confirmation for write/edit/patch/delete/move/copy.",
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
    .with_description("Write or overwrite a file with new content. Internal — use unified_file action=write instead.")
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
    .with_description("Apply a surgical text replacement in a file. Internal — use unified_file action=edit instead.")
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
    .with_description("Run a one-shot PTY command. Internal — use unified_exec action=run instead.")
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
    .with_description(
        "Send stdin to an active PTY session. Internal — use unified_exec action=write instead.",
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
    .with_description(
        "Read buffered output from a PTY session. Internal — use unified_exec action=poll instead.",
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
    .with_description("Create an interactive PTY session. Internal — managed by unified_exec.")
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
    .with_description("List all active PTY sessions. Internal — managed by unified_exec.")
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
    .with_description("Close a PTY session by ID. Internal — managed by unified_exec.")
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
    .with_description("Retrieve compilation/lint errors from the most recent run. Internal — used by the harness surface.")
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

fn mcp_parameters() -> serde_json::Value {
    json!({
        "type": "object",
        "required": ["action"],
        "properties": {
            "action": {
                "type": "string",
                "enum": ["search_tools", "get_tool_details", "list_servers", "connect", "disconnect"],
                "description": "search_tools: find MCP tools by natural-language query. get_tool_details: fetch the full input schema for one MCP tool name. list_servers: list configured servers and their connection state. connect: connect one configured MCP server by name (requires name; requires Prompt confirmation). disconnect: disconnect one active MCP server by name (requires name; requires Prompt confirmation)."
            },
            "query": {
                "type": "string",
                "description": "search_tools: natural language query describing the MCP capability to find."
            },
            "detail_level": {
                "type": "string",
                "enum": ["name", "name_description", "full"],
                "description": "search_tools: response detail level (names only, names with descriptions, or full schema excerpts)."
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 25,
                "description": "search_tools: maximum number of results to return."
            },
            "name": {
                "type": "string",
                "description": "get_tool_details: exact MCP tool name to inspect. connect/disconnect: configured MCP server name."
            }
        },
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
        // The distributed slice should contain at least 28 tool factories.
        // This catches accidentally missing #[distributed_slice] annotations.
        // (Consolidation via action-param tools — web/cron/agent/mcp merges —
        // intentionally reduces this count; the `llm_visible_builtin_tool_count_stays_bounded`
        // test guards the user-visible tool budget.)
        assert!(
            BUILTIN_TOOLS.len() >= 24,
            "expected at least 24 distributed tool factories, found {}",
            BUILTIN_TOOLS.len()
        );
    }

    /// Cap regression for tool-count creep.
    ///
    /// Every new #[distributed_slice(BUILTIN_TOOLS)] declaration the model can
    /// see costs the model attention at choice time. This asserts the number
    /// of LLM-visible builtin tools stays bounded. If a legitimate feature
    /// needs a new primary tool, consolidate, defer (behind the deferred-load
    /// path), or deliberately raise `MAX_LLM_VISIBLE_BUILTIN_TOOLS` in review.
    ///
    /// Hidden/internal tools (e.g. `read_file`, PTY session tools) set
    /// `expose_in_llm(false)` and are intentionally excluded from this count.
    #[test]
    fn llm_visible_builtin_tool_count_stays_bounded() {
        const MAX_LLM_VISIBLE_BUILTIN_TOOLS: usize = 15;

        let registrations = builtin_tool_registrations(None);
        let visible = registrations
            .iter()
            .filter(|registration| registration.expose_in_llm())
            .count();

        assert!(
            visible <= MAX_LLM_VISIBLE_BUILTIN_TOOLS,
            "LLM-visible builtin tool count is {visible}, exceeding the cap of {MAX_LLM_VISIBLE_BUILTIN_TOOLS}. \
             Consolidate, defer, or deliberately raise the cap in review."
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

        let agent = registrations
            .iter()
            .find(|registration| registration.name() == tools::AGENT)
            .expect("agent registration should exist");
        let agent_description = agent.metadata().description().expect("agent description");
        assert!(agent_description.contains("inherits the current toolset"));
        assert!(agent_description.contains("follow-up input"));
        assert!(agent_description.contains("long-running daemons"));
        assert!(agent_description.contains("terminal state"));
        let agent_aliases = agent.metadata().aliases();
        for legacy in [
            tools::SPAWN_AGENT,
            tools::SPAWN_BACKGROUND_SUBPROCESS,
            tools::SEND_INPUT,
            tools::RESUME_AGENT,
            tools::WAIT_AGENT,
            tools::CLOSE_AGENT,
        ] {
            assert!(
                agent_aliases.iter().any(|alias| alias == legacy),
                "agent should keep legacy alias {legacy}"
            );
        }

        // wait_agent / close_agent no longer register standalone tools; they
        // are folded into `agent` (action='wait' / action='close') and kept
        // only as aliases.
        assert!(
            registrations
                .iter()
                .all(|registration| registration.name() != tools::WAIT_AGENT
                    && registration.name() != tools::CLOSE_AGENT)
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

    #[test]
    fn default_config_exposed_tool_count_within_cap() {
        // Regression guard for the tool-consolidation work: the number of
        // LLM-exposed built-in tools must stay bounded so the model does not
        // waste attention choosing between near-duplicates. Any new
        // registration must either consolidate an existing tool, be deferred
        // behind the deferred-loading path, or deliberately raise this cap in
        // review.
        let registrations = builtin_tool_registrations(None);
        let exposed: usize = registrations
            .iter()
            .filter(|registration| registration.expose_in_llm())
            .count();
        assert!(
            exposed <= 14,
            "exposed built-in tool count is {exposed}; expected <= 14. \
             Consolidate, defer, or raise the cap in review."
        );
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
        use crate::tools::handlers::{
            SessionSurface, SessionToolCatalog, SessionToolsConfig, ToolModelCapabilities,
        };

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
        use crate::tools::handlers::{
            SessionSurface, SessionToolCatalog, SessionToolsConfig, ToolModelCapabilities,
        };
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
                serde_json::to_string(&estimate)
                    .map(|s| s.len() / 4)
                    .unwrap_or(0)
            })
            .sum();

        assert!(
            total_tokens <= 3_000,
            "emitted model tool schema tokens in Progressive mode is {total_tokens}; expected <= 3_000"
        );
    }
}
