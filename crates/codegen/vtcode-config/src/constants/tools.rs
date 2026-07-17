// ============================================================
// MODEL-FACING TOOLS
// ============================================================
/// Canonical shell execution tool exposed to the model.
pub const EXEC_COMMAND: &str = "exec_command";
/// Canonical stdin writer for active execution sessions.
pub const WRITE_STDIN: &str = "write_stdin";
/// Canonical patch application tool exposed to the model.
pub const APPLY_PATCH: &str = "apply_patch";
/// Advanced bounded syntactic code search tool for VTCode-specific profiles.
pub const CODE_SEARCH: &str = "code_search";

// ============================================================
// LEGACY INTERNAL DISPATCHERS
// ============================================================
/// Internal search and discovery dispatcher retained behind public tools.
pub const UNIFIED_SEARCH: &str = "search_dispatch_internal";
/// Internal shell execution and code execution dispatcher retained behind public tools.
pub const UNIFIED_EXEC: &str = "command_session_internal";
/// Internal file operations dispatcher retained behind public tools.
pub const UNIFIED_FILE: &str = "file_operation_internal";

// ============================================================
// TOOL IDS
// ============================================================
pub const THINK: &str = "think";
pub const SEARCH_TOOLS: &str = "search_tools";
/// Unified MCP tool (action: search_tools | get_tool_details | list_servers |
/// connect | disconnect). `connect`/`disconnect` are config-gated lifecycle
/// ops evaluated under the action-qualified policy keys `mcp:connect` /
/// `mcp:disconnect` (default Prompt) so they keep their human-in-the-loop
/// confirmation even though the base `mcp` tool is `ToolPolicy::Allow`.
pub const MCP: &str = "mcp";
pub const MCP_SEARCH_TOOLS: &str = "mcp_search_tools";
pub const MCP_GET_TOOL_DETAILS: &str = "mcp_get_tool_details";
pub const MCP_LIST_SERVERS: &str = "mcp_list_servers";
pub const MCP_CONNECT_SERVER: &str = "mcp_connect_server";
pub const MCP_DISCONNECT_SERVER: &str = "mcp_disconnect_server";
pub const WEB_SEARCH: &str = "web_search";
pub const WEB_FETCH: &str = "web_fetch";
pub const FETCH_URL: &str = "fetch_url";
/// Defuddle-backed markdown extraction for a single URL. The hosted
/// `defuddle.md/{link}` service is rate-limited, so this tool is hard-capped
/// at one call per tool instance (treat one tool instance as one session).
pub const DEFUDDLE_FETCH: &str = "defuddle_fetch";
pub const LIST: &str = "list";
pub const GREP: &str = "grep";
pub const FETCH: &str = "fetch";
pub const EXEC_PTY_CMD: &str = "exec_pty_cmd";
pub const SHELL: &str = "shell";
pub const GREP_FILE: &str = "grep_file";
pub const LIST_FILES: &str = "list_files";

// ============================================================
// SKILL MANAGEMENT TOOLS (Progressive Disclosure)
// ============================================================
/// List all available skills (local and dormant system utilities)
pub const LIST_SKILLS: &str = "list_skills";
/// Load a skill's instructions and activate its tools
pub const LOAD_SKILL: &str = "load_skill";
/// Load resources from a skill (scripts, templates, docs)
pub const LOAD_SKILL_RESOURCE: &str = "load_skill_resource";

// ============================================================
// INTERNAL EXECUTION HELPERS
// ============================================================
pub const RUN_PTY_CMD: &str = "run_pty_cmd";
pub const CREATE_PTY_SESSION: &str = "create_pty_session";
pub const LIST_PTY_SESSIONS: &str = "list_pty_sessions";
pub const CLOSE_PTY_SESSION: &str = "close_pty_session";
pub const SEND_PTY_INPUT: &str = "send_pty_input";
pub const READ_PTY_SESSION: &str = "read_pty_session";
pub const RESIZE_PTY_SESSION: &str = "resize_pty_session";
pub const EXECUTE_CODE: &str = "execute_code";

// ============================================================
// INTERNAL FILE OPERATION HELPERS
// ============================================================
pub const READ_FILE: &str = "read_file";
pub const WRITE_FILE: &str = "write_file";
pub const EDIT_FILE: &str = "edit_file";
pub const DELETE_FILE: &str = "delete_file";
pub const CREATE_FILE: &str = "create_file";
pub const SEARCH_REPLACE: &str = "search_replace";
pub const FILE_OP: &str = "file_op";
pub const MOVE_FILE: &str = "move_file";
pub const COPY_FILE: &str = "copy_file";

// ============================================================
// ERROR & DIAGNOSTICS
// ============================================================
pub const GET_ERRORS: &str = "get_errors";

// ============================================================
// HUMAN-IN-THE-LOOP (HITL)
// ============================================================
/// Canonical HITL tool name for structured user input.
pub const REQUEST_USER_INPUT: &str = "request_user_input";
/// Canonical memory tool name for Anthropic native memory sessions.
pub const MEMORY: &str = "memory";
/// Legacy alias routed to `request_user_input`.
pub const ASK_QUESTIONS: &str = "ask_questions";
/// Legacy alias routed to `request_user_input` (deprecated tabbed shape).
pub const ASK_USER_QUESTION: &str = "ask_user_question";
/// Unified subagent lifecycle tool (action: spawn | spawn_subprocess |
/// send_input | resume | wait | close). `wait_agent`/`close_agent` are now
/// aliases routed to `agent` (action='wait'/'close'); `LIFECYCLE_CLEANUP_TOOLS`
/// still lists their names so the policy layer keeps treating them as
/// always-allowed cleanup calls regardless of the active primary agent's
/// restricted tool policy.
pub const AGENT: &str = "agent";
/// Unified scheduled-prompt tool (action: create | list | delete).
pub const CRON: &str = "cron";
/// Legacy alias for `cron` action=create.
pub const CRON_CREATE: &str = "cron_create";
/// List session-scoped scheduled tasks.
pub const CRON_LIST: &str = "cron_list";
/// Delete a session-scoped scheduled task by id.
pub const CRON_DELETE: &str = "cron_delete";

// ============================================================
// PLANNING WORKFLOW
// ============================================================
/// Start planning - enables read-only tools and planning workflow.
pub const START_PLANNING: &str = "start_planning";
/// Finish planning - triggers confirmation modal before execution.
pub const FINISH_PLANNING: &str = "finish_planning";
/// Task tracker / plan manager - tracks checklist progress during complex tasks.
pub const TASK_TRACKER: &str = "task_tracker";

// ============================================================
// SUBAGENT COLLABORATION
// ============================================================
/// Spawn a delegated child agent.
pub const SPAWN_AGENT: &str = "spawn_agent";
/// Launch a managed background subprocess that hosts a background-enabled subagent.
pub const SPAWN_BACKGROUND_SUBPROCESS: &str = "spawn_background_subprocess";
/// Send follow-up input to a delegated child agent.
pub const SEND_INPUT: &str = "send_input";
/// Wait for one or more delegated child agents to finish.
pub const WAIT_AGENT: &str = "wait_agent";
/// Resume a delegated child agent without sending a message.
pub const RESUME_AGENT: &str = "resume_agent";
/// Close a delegated child agent.
pub const CLOSE_AGENT: &str = "close_agent";

/// Cleanup-only child-agent tools that must remain available regardless of the
/// active primary agent's tool policy, so restricted primaries can still join or
/// close already-running child work. Add new lifecycle-only cleanup tools here.
pub const LIFECYCLE_CLEANUP_TOOLS: &[&str] = &[WAIT_AGENT, CLOSE_AGENT];

// Special wildcard for full access
pub const WILDCARD_ALL: &str = "*";

// ===========================================================================
// Compile-time validation
//
// All tool name constants are validated at compile time. These assertions
// ensure every name is non-empty, uses only [a-z0-9_], and contains no
// leading/trailing underscores (which would signal a naming convention
// violation). The wildcard "*" is exempt from character checks.
// ===========================================================================

const _: () = {
    /// Validate a tool name at compile time. Panics with a clear message
    /// if the name is empty, contains invalid characters, or has leading/
    /// trailing underscores (except for the wildcard "*").
    const fn validate_tool_name(name: &str) {
        assert!(!name.is_empty(), "tool name must not be empty");
        let bytes = name.as_bytes();
        // Wildcard "*" is the only name allowed to bypass character checks
        if bytes.len() == 1 && bytes[0] == b'*' {
            return;
        }
        assert!(
            bytes[0] != b'_' && bytes[bytes.len() - 1] != b'_',
            "tool name must not have leading/trailing underscores"
        );
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            assert!(
                b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_',
                "tool name must contain only [a-z0-9_]"
            );
            i += 1;
        }
    }

    // Model-facing tools
    validate_tool_name(EXEC_COMMAND);
    validate_tool_name(WRITE_STDIN);
    validate_tool_name(APPLY_PATCH);
    validate_tool_name(CODE_SEARCH);

    // Internal unified tools
    validate_tool_name(UNIFIED_SEARCH);
    validate_tool_name(UNIFIED_EXEC);
    validate_tool_name(UNIFIED_FILE);

    // Tool IDs
    validate_tool_name(THINK);
    validate_tool_name(SEARCH_TOOLS);
    validate_tool_name(MCP);
    validate_tool_name(MCP_SEARCH_TOOLS);
    validate_tool_name(MCP_GET_TOOL_DETAILS);
    validate_tool_name(MCP_LIST_SERVERS);
    validate_tool_name(MCP_CONNECT_SERVER);
    validate_tool_name(MCP_DISCONNECT_SERVER);
    validate_tool_name(WEB_SEARCH);
    validate_tool_name(WEB_FETCH);
    validate_tool_name(FETCH_URL);
    validate_tool_name(DEFUDDLE_FETCH);
    validate_tool_name(LIST);
    validate_tool_name(GREP);
    validate_tool_name(FETCH);
    validate_tool_name(EXEC_PTY_CMD);
    validate_tool_name(SHELL);
    validate_tool_name(GREP_FILE);
    validate_tool_name(LIST_FILES);

    // Skill management
    validate_tool_name(LIST_SKILLS);
    validate_tool_name(LOAD_SKILL);
    validate_tool_name(LOAD_SKILL_RESOURCE);

    // Internal execution helpers
    validate_tool_name(RUN_PTY_CMD);
    validate_tool_name(CREATE_PTY_SESSION);
    validate_tool_name(LIST_PTY_SESSIONS);
    validate_tool_name(CLOSE_PTY_SESSION);
    validate_tool_name(SEND_PTY_INPUT);
    validate_tool_name(READ_PTY_SESSION);
    validate_tool_name(RESIZE_PTY_SESSION);
    validate_tool_name(EXECUTE_CODE);

    // Internal file operation helpers
    validate_tool_name(READ_FILE);
    validate_tool_name(WRITE_FILE);
    validate_tool_name(EDIT_FILE);
    validate_tool_name(DELETE_FILE);
    validate_tool_name(CREATE_FILE);
    validate_tool_name(SEARCH_REPLACE);
    validate_tool_name(FILE_OP);
    validate_tool_name(MOVE_FILE);
    validate_tool_name(COPY_FILE);

    // Error & diagnostics
    validate_tool_name(GET_ERRORS);

    // HITL
    validate_tool_name(REQUEST_USER_INPUT);
    validate_tool_name(MEMORY);
    validate_tool_name(ASK_QUESTIONS);
    validate_tool_name(ASK_USER_QUESTION);
    validate_tool_name(AGENT);
    validate_tool_name(CRON);
    validate_tool_name(CRON_CREATE);
    validate_tool_name(CRON_LIST);
    validate_tool_name(CRON_DELETE);

    // Planning workflow
    validate_tool_name(START_PLANNING);
    validate_tool_name(FINISH_PLANNING);
    validate_tool_name(TASK_TRACKER);

    // Subagent collaboration
    validate_tool_name(SPAWN_AGENT);
    validate_tool_name(SPAWN_BACKGROUND_SUBPROCESS);
    validate_tool_name(SEND_INPUT);
    validate_tool_name(WAIT_AGENT);
    validate_tool_name(RESUME_AGENT);
    validate_tool_name(CLOSE_AGENT);

    // Wildcard
    validate_tool_name(WILDCARD_ALL);
};
