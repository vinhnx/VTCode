// ============================================================
// UNIFIED TOOLS (Primary Interface)
// ============================================================
/// Unified search & discovery tool (aliases: grep_file, list_files, etc.)
pub const UNIFIED_SEARCH: &str = "unified_search";
/// Unified shell execution & code execution tool (aliases: run_pty_cmd, execute_code, etc.)
pub const UNIFIED_EXEC: &str = "unified_exec";
/// Unified file operations tool (aliases: read_file, write_file, edit_file, etc.)
pub const UNIFIED_FILE: &str = "unified_file";

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
// AGENT CONTROL TOOLS (Delegation)
// ============================================================
/// Spawn a subagent for specialized tasks (explore, plan, general, etc.)
pub const SPAWN_SUBAGENT: &str = "spawn_subagent";

// ============================================================
// LEGACY SEARCH ALIASES (use unified_search instead)
// ============================================================
pub const GREP_FILE: &str = "grep_file";
pub const LIST_FILES: &str = "list_files";
pub const SEARCH_TOOLS: &str = "search_tools";
pub const SKILL: &str = "skill";
pub const AGENT_INFO: &str = "agent_info";
pub const WEB_FETCH: &str = "web_fetch";
pub const SEARCH: &str = "search";
pub const FIND: &str = "find";

// ============================================================
// LEGACY EXECUTION ALIASES (use unified_exec instead)
// ============================================================
pub const RUN_PTY_CMD: &str = "run_pty_cmd";
pub const CREATE_PTY_SESSION: &str = "create_pty_session";
pub const LIST_PTY_SESSIONS: &str = "list_pty_sessions";
pub const CLOSE_PTY_SESSION: &str = "close_pty_session";
pub const SEND_PTY_INPUT: &str = "send_pty_input";
pub const READ_PTY_SESSION: &str = "read_pty_session";
pub const RESIZE_PTY_SESSION: &str = "resize_pty_session";
pub const EXECUTE_CODE: &str = "execute_code";
/// Legacy provider-emitted alias for execute_code.
pub const EXEC_CODE: &str = "exec_code";
pub const EXEC_PTY_CMD: &str = "exec_pty_cmd";
pub const EXEC: &str = "exec";
pub const SHELL: &str = "shell";

// ============================================================
// LEGACY FILE OPERATION ALIASES (use unified_file instead)
// ============================================================
pub const READ_FILE: &str = "read_file";
pub const WRITE_FILE: &str = "write_file";
pub const EDIT_FILE: &str = "edit_file";
pub const DELETE_FILE: &str = "delete_file";
pub const CREATE_FILE: &str = "create_file";
pub const APPLY_PATCH: &str = "apply_patch";
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
/// Legacy alias routed to `request_user_input`.
pub const ASK_QUESTIONS: &str = "ask_questions";
/// Legacy alias routed to `request_user_input` (deprecated tabbed shape).
pub const ASK_USER_QUESTION: &str = "ask_user_question";

// ============================================================
// PLAN MODE
// ============================================================
/// Enter plan mode - enables read-only tools and planning workflow.
pub const ENTER_PLAN_MODE: &str = "enter_plan_mode";
/// Exit plan mode - triggers confirmation modal before execution.
pub const EXIT_PLAN_MODE: &str = "exit_plan_mode";
/// Task tracker / plan manager - tracks checklist progress during complex tasks.
pub const TASK_TRACKER: &str = "task_tracker";
/// Plan-mode scoped task tracker persisted under `.vtcode/plans/`.
pub const PLAN_TASK_TRACKER: &str = "plan_task_tracker";

// Special wildcard for full access
pub const WILDCARD_ALL: &str = "*";
