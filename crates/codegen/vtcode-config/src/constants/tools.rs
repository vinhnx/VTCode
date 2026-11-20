// Wire aliases emitted by transcript layers. These are not canonical tool
// names, but producers and parsers still reference them by constant.
pub use vtcode_commons::tool_types::tool_names::{
    BASH_ALIAS, CONTAINER_EXEC_ALIAS, EXEC_ALIAS, REPO_BROWSER_LIST_FILES, REPO_BROWSER_READ_FILE,
    REPO_BROWSER_WRITE_FILE,
};

// ============================================================
// MODEL-FACING TOOLS
// ============================================================
/// Canonical patch application tool exposed to the model.
pub use vtcode_commons::tool_types::tool_names::APPLY_PATCH;
/// Advanced bounded syntactic code search tool for VTCode-specific profiles.
pub use vtcode_commons::tool_types::tool_names::CODE_SEARCH;
/// Model-initiated computer-use turn in the isolated browser target.
pub use vtcode_commons::tool_types::tool_names::COMPUTER_USE;
/// Canonical shell execution tool exposed to the model.
pub use vtcode_commons::tool_types::tool_names::EXEC_COMMAND;
/// Optional public rationale recording within the canonical current task.
pub use vtcode_commons::tool_types::tool_names::RECORD_DECISION;
/// Canonical stdin writer for active execution sessions.
pub use vtcode_commons::tool_types::tool_names::WRITE_STDIN;

// ============================================================
// LEGACY INTERNAL DISPATCHERS
// ============================================================
/// Internal shell execution and code execution dispatcher retained behind public tools.
pub use vtcode_commons::tool_types::tool_names::UNIFIED_EXEC;
/// Internal file operations dispatcher retained behind public tools.
pub use vtcode_commons::tool_types::tool_names::UNIFIED_FILE;
/// Internal search and discovery dispatcher retained behind public tools.
pub use vtcode_commons::tool_types::tool_names::UNIFIED_SEARCH;

// ============================================================
// TOOL IDS
// ============================================================
pub use vtcode_commons::tool_types::tool_names::EXEC_PTY_CMD;
pub use vtcode_commons::tool_types::tool_names::FETCH;
pub use vtcode_commons::tool_types::tool_names::FETCH_URL;
pub use vtcode_commons::tool_types::tool_names::GREP;
pub use vtcode_commons::tool_types::tool_names::GREP_FILE;
pub use vtcode_commons::tool_types::tool_names::LIST;
pub use vtcode_commons::tool_types::tool_names::LIST_FILES;
/// Unified MCP tool (action: search_tools | get_tool_details | list_servers |
/// connect | disconnect). `connect`/`disconnect` are config-gated lifecycle
/// ops evaluated under the action-qualified policy keys `mcp:connect` /
/// `mcp:disconnect` (default Prompt) so they keep their human-in-the-loop
/// confirmation even though the base `mcp` tool is `ToolPolicy::Allow`.
pub use vtcode_commons::tool_types::tool_names::MCP;
pub use vtcode_commons::tool_types::tool_names::MCP_CONNECT_SERVER;
pub use vtcode_commons::tool_types::tool_names::MCP_DISCONNECT_SERVER;
pub use vtcode_commons::tool_types::tool_names::MCP_GET_TOOL_DETAILS;
pub use vtcode_commons::tool_types::tool_names::MCP_LIST_SERVERS;
pub use vtcode_commons::tool_types::tool_names::MCP_SEARCH_TOOLS;
pub use vtcode_commons::tool_types::tool_names::SEARCH_TOOLS;
pub use vtcode_commons::tool_types::tool_names::SHELL;
pub use vtcode_commons::tool_types::tool_names::THINK;
pub use vtcode_commons::tool_types::tool_names::WEB_FETCH;
pub use vtcode_commons::tool_types::tool_names::WEB_SEARCH;

// ============================================================
// SKILL MANAGEMENT TOOLS (Progressive Disclosure)
// ============================================================
/// List all available skills (local and dormant system utilities)
pub use vtcode_commons::tool_types::tool_names::LIST_SKILLS;
/// Load a skill's instructions and activate its tools
pub use vtcode_commons::tool_types::tool_names::LOAD_SKILL;
/// Load resources from a skill (scripts, templates, docs)
pub use vtcode_commons::tool_types::tool_names::LOAD_SKILL_RESOURCE;

// ============================================================
// GOAL LOOP TOOLS
// ============================================================
/// Read the current thread goal loop state.
pub use vtcode_commons::tool_types::tool_names::GET_GOAL;
/// Complete, block, or pause the active thread goal loop.
pub use vtcode_commons::tool_types::tool_names::UPDATE_GOAL;

// ============================================================
// INTERNAL EXECUTION HELPERS
// ============================================================
pub use vtcode_commons::tool_types::tool_names::CLOSE_PTY_SESSION;
pub use vtcode_commons::tool_types::tool_names::CREATE_PTY_SESSION;
pub use vtcode_commons::tool_types::tool_names::EXECUTE_CODE;
pub use vtcode_commons::tool_types::tool_names::LIST_PTY_SESSIONS;
pub use vtcode_commons::tool_types::tool_names::READ_PTY_SESSION;
pub use vtcode_commons::tool_types::tool_names::RESIZE_PTY_SESSION;
pub use vtcode_commons::tool_types::tool_names::RUN_PTY_CMD;
pub use vtcode_commons::tool_types::tool_names::SEND_PTY_INPUT;

// ============================================================
// INTERNAL FILE OPERATION HELPERS
// ============================================================
pub use vtcode_commons::tool_types::tool_names::COPY_FILE;
pub use vtcode_commons::tool_types::tool_names::CREATE_FILE;
pub use vtcode_commons::tool_types::tool_names::DELETE_FILE;
pub use vtcode_commons::tool_types::tool_names::EDIT_FILE;
pub use vtcode_commons::tool_types::tool_names::FILE_OP;
pub use vtcode_commons::tool_types::tool_names::MOVE_FILE;
pub use vtcode_commons::tool_types::tool_names::READ_FILE;
pub use vtcode_commons::tool_types::tool_names::SEARCH_REPLACE;
pub use vtcode_commons::tool_types::tool_names::WRITE_FILE;

// ============================================================
// ERROR & DIAGNOSTICS
// ============================================================
pub use vtcode_commons::tool_types::tool_names::GET_ERRORS;

// ============================================================
// HUMAN-IN-THE-LOOP (HITL)
// ============================================================
/// Unified subagent lifecycle tool (action: spawn | spawn_subprocess |
/// send_input | resume | wait | close). `wait_agent`/`close_agent` are now
/// aliases routed to `agent` (action='wait'/'close'); `LIFECYCLE_CLEANUP_TOOLS`
/// still lists their names so the policy layer keeps treating them as
/// always-allowed cleanup calls regardless of the active primary agent's
/// restricted tool policy.
pub use vtcode_commons::tool_types::tool_names::AGENT;
/// Legacy alias routed to `request_user_input`.
pub use vtcode_commons::tool_types::tool_names::ASK_QUESTIONS;
/// Legacy alias routed to `request_user_input` (deprecated tabbed shape).
pub use vtcode_commons::tool_types::tool_names::ASK_USER_QUESTION;
/// Unified scheduled-prompt tool (action: create | list | delete).
pub use vtcode_commons::tool_types::tool_names::CRON;
/// Legacy alias for `cron` action=create.
pub use vtcode_commons::tool_types::tool_names::CRON_CREATE;
/// Delete a session-scoped scheduled task by id.
pub use vtcode_commons::tool_types::tool_names::CRON_DELETE;
/// List session-scoped scheduled tasks.
pub use vtcode_commons::tool_types::tool_names::CRON_LIST;
/// Durable scheduler control for coordinators and assignment-owned worker reports.
pub use vtcode_commons::tool_types::tool_names::MATRIX;
/// Canonical memory tool name for Anthropic native memory sessions.
pub use vtcode_commons::tool_types::tool_names::MEMORY;
/// Canonical HITL tool name for structured user input.
pub use vtcode_commons::tool_types::tool_names::REQUEST_USER_INPUT;

// ============================================================
// PLANNING WORKFLOW
// ============================================================
/// Start planning - enables read-only tools and planning workflow.
pub use vtcode_commons::tool_types::tool_names::START_PLANNING;
/// Task tracker / plan manager - tracks checklist progress during complex tasks.
pub use vtcode_commons::tool_types::tool_names::TASK_TRACKER;

// ============================================================
// SUBAGENT COLLABORATION
// ============================================================
/// Answer a question received from a peer CLI session.
pub use vtcode_commons::tool_types::tool_names::ANSWER_SESSION;
/// Ask a peer CLI session in this workspace a question.
pub use vtcode_commons::tool_types::tool_names::ASK_SESSION;
/// Close a delegated child agent.
pub use vtcode_commons::tool_types::tool_names::CLOSE_AGENT;
/// List live peer CLI sessions in this workspace.
pub use vtcode_commons::tool_types::tool_names::LIST_SESSIONS;
/// Resume a delegated child agent without sending a message.
pub use vtcode_commons::tool_types::tool_names::RESUME_AGENT;
/// Send follow-up input to a delegated child agent.
pub use vtcode_commons::tool_types::tool_names::SEND_INPUT;
/// Spawn a delegated child agent.
pub use vtcode_commons::tool_types::tool_names::SPAWN_AGENT;
/// Launch a managed background subprocess that hosts a background-enabled subagent.
pub use vtcode_commons::tool_types::tool_names::SPAWN_BACKGROUND_SUBPROCESS;
/// Wait for one or more delegated child agents to finish.
pub use vtcode_commons::tool_types::tool_names::WAIT_AGENT;

/// Cleanup-only child-agent tools that must remain available regardless of the
/// active primary agent's tool policy, so restricted primaries can still join or
/// close already-running child work. Add new lifecycle-only cleanup tools here.
pub const LIFECYCLE_CLEANUP_TOOLS: &[&str] = &[WAIT_AGENT, CLOSE_AGENT];

/// Workflow-coordination tools that must remain available regardless of the
/// `[automation.full_auto].allowed_tools` allow-list, so planning, checklist,
/// and interview coordination keep working in Auto mode and other full-auto
/// runs. These are control-plane (not execution blast radius): `task_tracker`
/// owns only its session tracker file, `start_planning` only flips the
/// planning-workflow state, and `request_user_input` still resolves through
/// the interactive-session gate at execution time. Callers must pass the
/// canonical tool name (see `canonical_tool_name`); matching is exact.
pub const ALWAYS_AVAILABLE_WORKFLOW_TOOLS: &[&str] = &[TASK_TRACKER, START_PLANNING, REQUEST_USER_INPUT, MATRIX];

/// Returns `true` for workflow-coordination tools that bypass the full-auto
/// allow-list (see [`ALWAYS_AVAILABLE_WORKFLOW_TOOLS`]).
#[inline]
pub fn is_workflow_coordination_tool(canonical_name: &str) -> bool {
    ALWAYS_AVAILABLE_WORKFLOW_TOOLS.contains(&canonical_name)
}

/// Canonical command-session tool name for a possibly-aliased tool name.
///
/// Maps every exec/PTY alias (plus the `"bash"`, `"exec"`, and
/// `"container.exec"` spellings transcript layers emit) to [`UNIFIED_EXEC`].
/// Pure string match living next to the constants so lower layers (e.g.
/// transcript normalization) can use it without depending on the classifier.
#[inline]
pub fn canonical_command_session_tool_name(tool_name: &str) -> Option<&'static str> {
    match tool_name {
        UNIFIED_EXEC | RUN_PTY_CMD | SEND_PTY_INPUT | CREATE_PTY_SESSION | READ_PTY_SESSION | LIST_PTY_SESSIONS
        | CLOSE_PTY_SESSION | EXECUTE_CODE | EXEC_PTY_CMD | EXEC_COMMAND | WRITE_STDIN | SHELL | BASH_ALIAS
        | EXEC_ALIAS | CONTAINER_EXEC_ALIAS => Some(UNIFIED_EXEC),
        _ => None,
    }
}

/// Returns `true` if `tool_name` is any alias of the unified command-session tool.
#[must_use]
#[inline]
pub fn is_command_session_tool(tool_name: &str) -> bool {
    canonical_command_session_tool_name(tool_name).is_some()
}

// Special wildcard for full access
pub use vtcode_commons::tool_types::tool_names::WILDCARD_ALL;

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
            assert!(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_', "tool name must contain only [a-z0-9_]");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_coordination_tools_bypass_the_full_auto_allow_list() {
        for tool in [TASK_TRACKER, START_PLANNING, REQUEST_USER_INPUT] {
            assert!(is_workflow_coordination_tool(tool), "{tool} must bypass the allow-list");
        }
    }

    #[test]
    fn execution_tools_do_not_bypass_the_full_auto_allow_list() {
        for tool in [
            EXEC_COMMAND,
            WRITE_STDIN,
            APPLY_PATCH,
            CODE_SEARCH,
            WEB_FETCH,
            "not_a_tool",
        ] {
            assert!(!is_workflow_coordination_tool(tool), "{tool} must stay allow-list gated");
        }
    }
}
