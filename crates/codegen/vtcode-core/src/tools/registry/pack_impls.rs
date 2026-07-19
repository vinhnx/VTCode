//! Builtin tool pack implementations.
//!
//! Each struct below implements [`ToolPack`] for a logical group of related
//! tools. The `register()` method batch-registers its tools into the inventory
//! in a single pass.
//!
//! The `linkme::distributed_slice` macro uses `link_section` internally,
//! which triggers the `unsafe_code` lint. This is inherent to the crate's
//! mechanism and cannot be avoided at the call site.
#![allow(unsafe_code)]

use std::sync::Arc;

use linkme::distributed_slice;

use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::tool_policy::ToolPolicy;
use crate::tools::defuddle::{DEFUDDLE_FETCH_DESCRIPTION, DefuddleTool};
use crate::tools::handlers::{FinishPlanningTool, PlanningWorkflowState, StartPlanningTool, TaskTrackerTool};
use crate::tools::native_memory;
use crate::tools::registry::distributed::tool_config;
use crate::tools::registry::pack::BUILTIN_PACKS;
use crate::tools::registry::pack::{ToolPack, batch_register};
use crate::tools::registry::registration::ToolRegistration;
use crate::tools::registry::{ToolInventory, ToolRegistry, native_cgp_tool_factory};
use crate::tools::request_user_input::RequestUserInputTool;
use crate::tools::web_fetch::{WEB_FETCH_DESCRIPTION, WebFetchTool};
use crate::tools::web_search::{WEB_SEARCH_DESCRIPTION, WebSearchTool};
use serde_json::json;
use vtcode_utility_tool_specs::{
    agent_parameters, apply_patch_parameters, code_search_parameters, cron_parameters, exec_command_parameters,
    list_files_parameters, mcp_parameters, write_stdin_parameters,
};

// ===========================================================================
// HITL Pack
// ===========================================================================

#[derive(Default)]
pub struct HitlPack;

#[async_trait::async_trait]
impl ToolPack for HitlPack {
    fn pack_id(&self) -> &'static str {
        "hitl"
    }

    async fn register(&self, inventory: &ToolInventory, _plan_state: &PlanningWorkflowState) {
        let registrations = vec![
            ToolRegistration::from_tool_instance(
                tools::REQUEST_USER_INPUT,
                CapabilityLevel::Basic,
                RequestUserInputTool,
            )
            .with_native_cgp_factory(native_cgp_tool_factory(|| RequestUserInputTool)),
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
            .with_permission(ToolPolicy::Allow),
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
            ]),
        ];
        batch_register(inventory, registrations);
    }
}

// ===========================================================================
// Planning Pack
// ===========================================================================

#[derive(Default)]
pub struct PlanningPack;

#[async_trait::async_trait]
impl ToolPack for PlanningPack {
    fn pack_id(&self) -> &'static str {
        "planning"
    }

    async fn register(&self, inventory: &ToolInventory, plan_state: &PlanningWorkflowState) {
        let plan_state = Clone::clone(plan_state);
        let factory_state = Arc::new(plan_state.clone());
        let start_factory = Arc::clone(&factory_state);
        let finish_factory = Arc::clone(&factory_state);
        let tracker_factory = Arc::clone(&factory_state);
        let registrations = vec![
            ToolRegistration::from_tool_instance(
                tools::START_PLANNING,
                CapabilityLevel::Basic,
                StartPlanningTool::new(plan_state.clone()),
            )
            .with_native_cgp_factory(native_cgp_tool_factory(move || {
                let state = Arc::clone(&start_factory);
                StartPlanningTool::new(state.as_ref().clone())
            })),
            ToolRegistration::from_tool_instance(
                tools::FINISH_PLANNING,
                CapabilityLevel::Basic,
                FinishPlanningTool::new(plan_state.clone()),
            )
            .with_native_cgp_factory(native_cgp_tool_factory(move || {
                let state = Arc::clone(&finish_factory);
                FinishPlanningTool::new(state.as_ref().clone())
            })),
            ToolRegistration::from_tool_instance(
                tools::TASK_TRACKER,
                CapabilityLevel::Basic,
                TaskTrackerTool::new(
                    plan_state.workspace_root().unwrap_or_default(),
                    plan_state,
                ),
            )
            .with_native_cgp_factory(native_cgp_tool_factory(move || {
                let state = Arc::clone(&tracker_factory);
                TaskTrackerTool::new(
                    state.as_ref().workspace_root().unwrap_or_else(std::path::PathBuf::new),
                    state.as_ref().clone(),
                )
            }))
            .with_description(
                "Track task progress through a single checklist API (action: create | update | list | add). Use task_tracker with action=create at the start of a multi-step plan; use action=update as work progresses; use action=list to review current state. Do NOT call action=create twice — subsequent calls update the existing checklist. Tracker state mirrors between `.vtcode/tasks/current_task.md` and active plan sidecar files when available.",
            )
            .with_aliases(["plan_manager", "track_tasks", "checklist"]),
        ];
        batch_register(inventory, registrations);
    }
}

// ===========================================================================
// Multi-Agent Pack
// ===========================================================================

#[derive(Default)]
pub struct MultiAgentPack;

#[async_trait::async_trait]
impl ToolPack for MultiAgentPack {
    fn pack_id(&self) -> &'static str {
        "multi_agent"
    }

    async fn register(&self, inventory: &ToolInventory, _plan_state: &PlanningWorkflowState) {
        let registrations = vec![ToolRegistration::new(
            tools::AGENT,
            CapabilityLevel::Basic,
            false,
            ToolRegistry::agent_executor,
        )
        .with_description(
            "Spawn and steer delegated child agents. Use action=spawn to delegate a scoped task, action=spawn_subprocess for a managed background process, action=send_input to continue a child, action=resume to reopen a completed child, action=wait for results, or action=close to cancel a child. Use exec_command for one-shot shell commands.",
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
        ])];
        batch_register(inventory, registrations);
    }
}

// ===========================================================================
// Search Pack
// ===========================================================================

#[derive(Default)]
pub struct SearchPack;

#[async_trait::async_trait]
impl ToolPack for SearchPack {
    fn pack_id(&self) -> &'static str {
        "search"
    }

    async fn register(&self, inventory: &ToolInventory, _plan_state: &PlanningWorkflowState) {
        let registrations = vec![
            ToolRegistration::new(
                tools::CODE_SEARCH,
                CapabilityLevel::CodeSearch,
                false,
                ToolRegistry::code_search_executor,
            )
            .with_description(
                "Search workspace code with one literal query. Use optional path, file_types, result_types, and max_results filters to find definitions, syntactic usages, text matches, and matching paths.",
            )
            .with_parameter_schema(code_search_parameters())
            .with_permission(ToolPolicy::Allow),
            ToolRegistration::new(
                tools::MCP,
                CapabilityLevel::CodeSearch,
                false,
                ToolRegistry::mcp_executor,
            )
            .with_description(
                "Discover and manage Model Context Protocol capabilities. Use action=search_tools to find tools, action=get_tool_details to fetch one schema, action=list_servers to inspect configured servers, or action=connect and action=disconnect to manage a named server. Do not disconnect a server while one of its tool calls is active.",
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
            ]),
        ];
        batch_register(inventory, registrations);
    }
}

// ===========================================================================
// Web Pack
// ===========================================================================

#[derive(Default)]
pub struct WebPack;

#[async_trait::async_trait]
impl ToolPack for WebPack {
    fn pack_id(&self) -> &'static str {
        "web"
    }

    async fn register(&self, inventory: &ToolInventory, _plan_state: &PlanningWorkflowState) {
        let web_fetch = tool_config()
            .map(|snapshot| WebFetchTool::from_config(&snapshot.web_fetch))
            .unwrap_or_default();
        let web_fetch_for_factory = web_fetch.clone();
        let web_fetch_factory = native_cgp_tool_factory(move || web_fetch_for_factory.clone());

        let web_search =
            WebSearchTool::with_config(tool_config().map(|snapshot| snapshot.web_search.clone()).unwrap_or_default());
        let web_search_for_factory = web_search.clone();
        let web_search_factory = native_cgp_tool_factory(move || web_search_for_factory.clone());

        let defuddle = DefuddleTool::new();
        let defuddle_for_factory = defuddle.clone();
        let defuddle_factory = native_cgp_tool_factory(move || defuddle_for_factory.clone());

        let registrations = vec![
            ToolRegistration::from_tool_instance(tools::WEB_FETCH, CapabilityLevel::Basic, web_fetch)
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
                .with_aliases(["fetch_url", "web"]),
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
                .with_aliases(["search_web", "websearch"]),
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
                .with_llm_visibility(false),
        ];
        batch_register(inventory, registrations);
    }
}

// ===========================================================================
// Shell Pack
// ===========================================================================

#[derive(Default)]
pub struct ShellPack;

#[async_trait::async_trait]
impl ToolPack for ShellPack {
    fn pack_id(&self) -> &'static str {
        "shell"
    }

    async fn register(&self, inventory: &ToolInventory, _plan_state: &PlanningWorkflowState) {
        let registrations = vec![
            ToolRegistration::new(
                tools::EXEC_COMMAND,
                CapabilityLevel::Bash,
                false,
                ToolRegistry::exec_command_executor,
            )
            .with_description(
                "Use this to execute a shell command through the active sandbox policy and permission checks. Put normal shell tools such as ls, rg, find, cat, sed, awk, build tools, and test tools in cmd. Returns output, exit status, and a reusable session id when the command is still running.",
            )
            .with_parameter_schema(exec_command_parameters())
            .with_permission(ToolPolicy::Allow),
            ToolRegistration::new(
                tools::EXEC_PTY_CMD,
                CapabilityLevel::Bash,
                false,
                ToolRegistry::run_pty_cmd_executor,
            )
            .with_description(
                "Execute a shell command attached to a PTY (pseudo-terminal) so interactive and TTY-aware programs behave as in a real terminal. Use this when the command needs a controlling terminal (e.g. pagers, prompts, curses UIs). Returns output, exit status, and a reusable session id when the command is still running.",
            )
            .with_parameter_schema(exec_command_parameters())
            .with_permission(ToolPolicy::Allow)
            .with_llm_visibility(false),
            ToolRegistration::new(
                tools::WRITE_STDIN,
                CapabilityLevel::Bash,
                false,
                ToolRegistry::write_stdin_executor,
            )
            .with_description("Write characters to an active exec_command session stdin, then poll for fresh output.")
            .with_parameter_schema(write_stdin_parameters())
            .with_permission(ToolPolicy::Allow),
        ];
        batch_register(inventory, registrations);
    }
}

// ===========================================================================
// Internal PTY Pack
// ===========================================================================

#[derive(Default)]
pub struct InternalPtyPack;

#[async_trait::async_trait]
impl ToolPack for InternalPtyPack {
    fn pack_id(&self) -> &'static str {
        "internal_pty"
    }

    async fn register(&self, inventory: &ToolInventory, _plan_state: &PlanningWorkflowState) {
        let registrations = vec![
            ToolRegistration::new(tools::READ_FILE, CapabilityLevel::CodeSearch, false, ToolRegistry::read_file_executor)
                .with_description(
                    "Read file contents with chunked ranges or indentation-aware block selection. Exposed as a first-class browse tool for the harness surface.",
                )
                .with_permission(ToolPolicy::Allow)
                .with_llm_visibility(false),
            ToolRegistration::new(tools::LIST_FILES, CapabilityLevel::CodeSearch, false, ToolRegistry::list_files_executor)
                .with_description(
                    "List files and directories with pagination. Exposed as a first-class browse tool for the harness surface.",
                )
                .with_parameter_schema(list_files_parameters())
                .with_permission(ToolPolicy::Allow)
                .with_llm_visibility(false),
            ToolRegistration::new(tools::WRITE_FILE, CapabilityLevel::Editing, false, ToolRegistry::write_file_executor)
                .with_description("Write or overwrite a file with new content. Internal file helper.")
                .with_llm_visibility(false),
            ToolRegistration::new(tools::EDIT_FILE, CapabilityLevel::Editing, false, ToolRegistry::edit_file_executor)
                .with_description("Apply a surgical text replacement in a file. Internal file helper.")
                .with_llm_visibility(false),
            ToolRegistration::new(tools::RUN_PTY_CMD, CapabilityLevel::Bash, false, ToolRegistry::run_pty_cmd_executor)
                .with_description("Run a one-shot PTY command. Internal execution helper.")
                .with_llm_visibility(false),
            ToolRegistration::new(tools::SEND_PTY_INPUT, CapabilityLevel::Bash, false, ToolRegistry::send_pty_input_executor)
                .with_description("Send stdin to an active PTY session. Internal execution helper.")
                .with_llm_visibility(false),
            ToolRegistration::new(
                tools::READ_PTY_SESSION,
                CapabilityLevel::Bash,
                false,
                ToolRegistry::read_pty_session_executor,
            )
            .with_description("Read buffered output from a PTY session. Internal execution helper.")
            .with_llm_visibility(false),
            ToolRegistration::new(
                tools::CREATE_PTY_SESSION,
                CapabilityLevel::Bash,
                false,
                ToolRegistry::create_pty_session_executor,
            )
            .with_description("Create an interactive PTY session. Internal execution helper.")
            .with_llm_visibility(false),
            ToolRegistration::new(
                tools::LIST_PTY_SESSIONS,
                CapabilityLevel::Bash,
                false,
                ToolRegistry::list_pty_sessions_executor,
            )
            .with_description("List all active PTY sessions. Internal execution helper.")
            .with_llm_visibility(false),
            ToolRegistration::new(
                tools::CLOSE_PTY_SESSION,
                CapabilityLevel::Bash,
                false,
                ToolRegistry::close_pty_session_executor,
            )
            .with_description("Close a PTY session by ID. Internal execution helper.")
            .with_llm_visibility(false),
            ToolRegistration::new(tools::GET_ERRORS, CapabilityLevel::CodeSearch, false, ToolRegistry::get_errors_executor)
                .with_description(
                    "Retrieve compilation/lint errors from the most recent run. Internal — used by the harness surface.",
                )
                .with_llm_visibility(false),
        ];
        batch_register(inventory, registrations);
    }
}

// ===========================================================================
// Editing Pack
// ===========================================================================

#[derive(Default)]
pub struct EditingPack;

#[async_trait::async_trait]
impl ToolPack for EditingPack {
    fn pack_id(&self) -> &'static str {
        "editing"
    }

    async fn register(&self, inventory: &ToolInventory, _plan_state: &PlanningWorkflowState) {
        let registrations = vec![ToolRegistration::new(
            tools::APPLY_PATCH,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::apply_patch_executor,
        )
        .with_description(crate::tools::apply_patch::with_semantic_anchor_guidance(
            "Apply patches to files after permission checks. IMPORTANT: Use VT Code patch format (*** Begin Patch, *** Update File: path, @@ hunks with -/+ lines, *** End Patch), NOT standard unified diff (---/+++ format).",
        ))
        .with_parameter_schema(apply_patch_parameters())
        .with_permission(ToolPolicy::Prompt)];
        batch_register(inventory, registrations);
    }
}

// ===========================================================================
// Pack factory functions (collected via linkme)
// ===========================================================================

#[distributed_slice(BUILTIN_PACKS)]
fn hitl_pack() -> Box<dyn ToolPack> {
    Box::new(HitlPack)
}

#[distributed_slice(BUILTIN_PACKS)]
fn planning_pack() -> Box<dyn ToolPack> {
    Box::new(PlanningPack)
}

#[distributed_slice(BUILTIN_PACKS)]
fn multi_agent_pack() -> Box<dyn ToolPack> {
    Box::new(MultiAgentPack)
}

#[distributed_slice(BUILTIN_PACKS)]
fn search_pack() -> Box<dyn ToolPack> {
    Box::new(SearchPack)
}

#[distributed_slice(BUILTIN_PACKS)]
fn web_pack() -> Box<dyn ToolPack> {
    Box::new(WebPack)
}

#[distributed_slice(BUILTIN_PACKS)]
fn shell_pack() -> Box<dyn ToolPack> {
    Box::new(ShellPack)
}

#[distributed_slice(BUILTIN_PACKS)]
fn internal_pty_pack() -> Box<dyn ToolPack> {
    Box::new(InternalPtyPack)
}

#[distributed_slice(BUILTIN_PACKS)]
fn editing_pack() -> Box<dyn ToolPack> {
    Box::new(EditingPack)
}
