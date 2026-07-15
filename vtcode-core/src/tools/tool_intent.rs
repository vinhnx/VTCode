use serde_json::Value;

use crate::config::constants::tools;
use crate::tools::command_args::{interactive_input_text, is_readonly_command_string};
use crate::tools::names::canonical_tool_name;

pub type ToolIntentClassifier = fn(&Value) -> ToolIntent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSurfaceKind {
    Function,
    ApplyPatch,
}

#[derive(Debug, Clone, Copy)]
pub enum ToolMutationModel {
    ReadOnly,
    Mutating,
    ByArgs(ToolIntentClassifier),
}

impl ToolMutationModel {
    pub fn classify(self, args: &Value) -> ToolIntent {
        match self {
            Self::ReadOnly => ToolIntent::read_only(),
            Self::Mutating => ToolIntent::mutating(),
            Self::ByArgs(classifier) => classifier(args),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ToolBehavior {
    pub surface_kind: ToolSurfaceKind,
    pub mutation_model: ToolMutationModel,
    pub supports_parallel_calls: bool,
    pub safe_mode_prompt: bool,
}

impl ToolBehavior {
    pub const fn function(
        mutation_model: ToolMutationModel,
        supports_parallel_calls: bool,
        safe_mode_prompt: bool,
    ) -> Self {
        Self {
            surface_kind: ToolSurfaceKind::Function,
            mutation_model,
            supports_parallel_calls,
            safe_mode_prompt,
        }
    }

    pub const fn apply_patch(
        mutation_model: ToolMutationModel,
        supports_parallel_calls: bool,
        safe_mode_prompt: bool,
    ) -> Self {
        Self {
            surface_kind: ToolSurfaceKind::ApplyPatch,
            mutation_model,
            supports_parallel_calls,
            safe_mode_prompt,
        }
    }

    /// Classifies the tool's intent for the given arguments by delegating to the mutation model.
    pub fn classify(self, args: &Value) -> ToolIntent {
        self.mutation_model.classify(args)
    }
}

/// Describes whether a tool invocation is mutating, destructive, or safe to retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToolIntent {
    /// Whether the tool modifies state or files.
    pub mutating: bool,
    /// Whether the tool performs potentially destructive operations.
    pub destructive: bool,
    /// Whether the tool is a read-only unified action (e.g. `file_operation` read).
    pub readonly_unified_action: bool,
    /// Whether the tool call is safe to retry on failure.
    pub retry_safe: bool,
}

impl ToolIntent {
    /// Returns a read-only, non-destructive, retry-safe intent.
    pub const fn read_only() -> Self {
        Self {
            mutating: false,
            destructive: false,
            readonly_unified_action: false,
            retry_safe: true,
        }
    }

    pub const fn read_only_unified_action() -> Self {
        Self {
            mutating: false,
            destructive: false,
            readonly_unified_action: true,
            retry_safe: true,
        }
    }

    pub const fn mutating() -> Self {
        Self {
            mutating: true,
            destructive: true,
            readonly_unified_action: false,
            retry_safe: false,
        }
    }
}

/// Returns the subset of actions that are allowed for a multi-action tool
/// when planning mode is active. Returns `None` for tools that are not
/// multi-action or have no action-level restrictions.
pub fn planning_allowed_actions(tool_name: &str) -> Option<&'static [&'static str]> {
    let canonical = canonical_tool_name(tool_name);
    match canonical {
        tools::UNIFIED_FILE => Some(&["read"]),
        tools::UNIFIED_EXEC => Some(&["run", "poll", "list", "inspect", "continue"]),
        // `code`, `write`, and `close` are always mutating and excluded.
        _ => None,
    }
}

pub fn builtin_tool_behavior(tool_name: &str) -> Option<ToolBehavior> {
    let canonical = canonical_tool_name(tool_name);
    builtin_tool_behavior_canonical(canonical)
}

fn builtin_tool_behavior_canonical(tool: &str) -> Option<ToolBehavior> {
    match tool {
        tools::CODE_SEARCH => Some(ToolBehavior::function(
            ToolMutationModel::ReadOnly,
            true,
            false,
        )),
        tools::UNIFIED_EXEC => Some(ToolBehavior::function(
            ToolMutationModel::ByArgs(command_session_intent),
            false,
            true,
        )),
        tools::EXEC_COMMAND => Some(ToolBehavior::function(
            ToolMutationModel::ByArgs(exec_command_intent),
            false,
            true,
        )),
        tools::WRITE_STDIN => Some(ToolBehavior::function(
            ToolMutationModel::ByArgs(write_stdin_intent),
            false,
            true,
        )),
        tools::UNIFIED_FILE => Some(ToolBehavior::function(
            ToolMutationModel::ByArgs(file_operation_intent),
            false,
            false,
        )),
        tools::APPLY_PATCH => Some(ToolBehavior::apply_patch(
            ToolMutationModel::Mutating,
            false,
            true,
        )),
        tools::REQUEST_USER_INPUT
        | tools::MEMORY
        | tools::START_PLANNING
        | tools::FINISH_PLANNING
        | tools::LIST_SKILLS
        | tools::LOAD_SKILL
        | tools::LOAD_SKILL_RESOURCE
        | tools::TASK_TRACKER
        | tools::GET_ERRORS
        | tools::SEARCH_TOOLS
        | tools::MCP_SEARCH_TOOLS
        | tools::MCP_GET_TOOL_DETAILS
        | tools::MCP_LIST_SERVERS
        | tools::THINK => Some(ToolBehavior::function(
            if tool == tools::MEMORY {
                ToolMutationModel::ByArgs(memory_tool_intent)
            } else {
                ToolMutationModel::ReadOnly
            },
            false,
            false,
        )),
        tools::READ_FILE | tools::GREP_FILE | tools::LIST_FILES => Some(ToolBehavior::function(
            ToolMutationModel::ReadOnly,
            true,
            false,
        )),
        tools::WEB_FETCH | tools::FETCH_URL | tools::WEB_SEARCH | tools::DEFUDDLE_FETCH => Some(
            ToolBehavior::function(ToolMutationModel::ReadOnly, false, false),
        ),
        tools::WRITE_FILE | tools::EDIT_FILE | tools::DELETE_FILE | tools::CREATE_FILE => Some(
            ToolBehavior::function(ToolMutationModel::Mutating, false, true),
        ),
        tools::MCP_CONNECT_SERVER | tools::MCP_DISCONNECT_SERVER => Some(ToolBehavior::function(
            ToolMutationModel::Mutating,
            false,
            false,
        )),
        tools::RUN_PTY_CMD
        | tools::SEND_PTY_INPUT
        | tools::CREATE_PTY_SESSION
        | tools::READ_PTY_SESSION
        | tools::LIST_PTY_SESSIONS
        | tools::CLOSE_PTY_SESSION
        | tools::EXECUTE_CODE
        | tools::SHELL => Some(ToolBehavior::function(
            ToolMutationModel::Mutating,
            false,
            true,
        )),
        _ => None,
    }
}

pub fn is_parallel_safe_call(tool_name: &str, args: &Value) -> bool {
    let canonical = canonical_tool_name(tool_name);
    if let Some(behavior) = builtin_tool_behavior_canonical(canonical) {
        return behavior.supports_parallel_calls && !behavior.classify(args).mutating;
    }

    !classify_tool_intent(canonical, args).mutating
}

pub fn classify_tool_intent(tool_name: &str, args: &Value) -> ToolIntent {
    let canonical = canonical_tool_name(tool_name);
    builtin_tool_behavior_canonical(canonical)
        .map(|behavior| behavior.classify(args))
        .unwrap_or_else(ToolIntent::mutating)
}

pub fn is_edited_file_conflict_guarded_call(tool_name: &str, args: &Value) -> bool {
    let canonical = canonical_tool_name(tool_name);
    match canonical {
        tools::WRITE_FILE | tools::CREATE_FILE | tools::EDIT_FILE | tools::APPLY_PATCH => true,
        tools::UNIFIED_FILE => file_operation_action(args)
            .map(is_edited_file_conflict_guarded_file_operation_action)
            .unwrap_or(false),
        _ => false,
    }
}

fn is_edited_file_conflict_guarded_file_operation_action(action: &str) -> bool {
    action_matches_any(Some(action), &["write", "create", "edit", "patch"])
}

pub fn canonical_command_session_tool_name(tool_name: &str) -> Option<&'static str> {
    match tool_name {
        tools::UNIFIED_EXEC
        | tools::RUN_PTY_CMD
        | tools::SEND_PTY_INPUT
        | tools::CREATE_PTY_SESSION
        | tools::READ_PTY_SESSION
        | tools::LIST_PTY_SESSIONS
        | tools::CLOSE_PTY_SESSION
        | tools::EXECUTE_CODE
        | tools::EXEC_PTY_CMD
        | tools::EXEC_COMMAND
        | tools::WRITE_STDIN
        | tools::SHELL
        | "bash"
        | "exec"
        | "container.exec" => Some(tools::UNIFIED_EXEC),
        _ => None,
    }
}

pub fn should_use_spool_reference_only(tool_name: Option<&str>, output: &Value) -> bool {
    let Some(obj) = output.as_object() else {
        return false;
    };

    let has_spool_path = obj
        .get("spool_path")
        .and_then(Value::as_str)
        .is_some_and(|path| !path.trim().is_empty());
    if !has_spool_path {
        return false;
    }

    if obj.get("loop_detected").and_then(Value::as_bool) == Some(true) {
        return false;
    }

    if tool_name.is_some_and(|name| canonical_command_session_tool_name(name).is_some()) {
        return true;
    }

    if obj
        .get("content_type")
        .and_then(Value::as_str)
        .is_some_and(|content_type| content_type == "exec_inspect")
    {
        return true;
    }

    [
        "command",
        "id",
        "session_id",
        "process_id",
        "is_exited",
        "exit_code",
    ]
    .iter()
    .any(|key| obj.contains_key(*key))
}

/// Returns `true` if `tool_name` refers to a command/PTY execution tool.
///
/// This includes PTY session tools and all unified exec aliases.
#[must_use]
pub fn is_command_tool(tool_name: &str) -> bool {
    tool_name == tools::CREATE_PTY_SESSION
        || tool_name == tools::SEND_PTY_INPUT
        || canonical_command_session_tool_name(tool_name).is_some()
}

pub fn is_command_run_tool_call(tool_name: &str, args: &Value) -> bool {
    match tool_name {
        tools::RUN_PTY_CMD | tools::CREATE_PTY_SESSION | tools::SHELL | "bash" => true,
        tools::EXEC_COMMAND => crate::tools::command_args::command_text(args)
            .ok()
            .flatten()
            .is_some(),
        tools::UNIFIED_EXEC | tools::EXEC_PTY_CMD | "exec" | "container.exec" => {
            command_session_action_is(args, "run")
        }
        _ => false,
    }
}

pub fn remap_file_operation_command_args_to_command_session(args: &Value) -> Option<Value> {
    let obj = args.as_object()?;
    let command = obj
        .get("command")
        .or_else(|| obj.get("cmd"))
        .or_else(|| obj.get("raw_command"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let action = obj.get("action").and_then(Value::as_str).map(str::trim);
    if let Some(action) = action
        && !action.is_empty()
        && !action_matches_any(Some(action), &["run", "exec", "execute", "shell"])
    {
        return None;
    }

    let mut mapped = serde_json::Map::new();
    mapped.insert("action".to_string(), Value::String("run".to_string()));
    mapped.insert("command".to_string(), Value::String(command.to_string()));

    for key in [
        "args",
        "cwd",
        "workdir",
        "env",
        "timeout_ms",
        "yield_time_ms",
        "login",
        "shell",
        "tty",
        "sandbox_permissions",
        "justification",
        "prefix_rule",
    ] {
        if let Some(value) = obj.get(key) {
            mapped.insert(key.to_string(), value.clone());
        }
    }

    Some(Value::Object(mapped))
}

fn file_operation_intent(args: &Value) -> ToolIntent {
    if file_operation_action_is(args, "read") {
        ToolIntent::read_only_unified_action()
    } else {
        ToolIntent::mutating()
    }
}

fn command_session_intent(args: &Value) -> ToolIntent {
    let has_exec_input = command_session_has_input(args);
    let readonly_unified_action = if command_session_action_is(args, "run") {
        is_readonly_command_session_command(args)
    } else {
        command_session_action_in(args, &["poll", "list", "inspect"])
            || (command_session_action_is(args, "continue") && !has_exec_input)
    };

    if readonly_unified_action {
        ToolIntent::read_only_unified_action()
    } else {
        ToolIntent::mutating()
    }
}

fn exec_command_intent(args: &Value) -> ToolIntent {
    if is_readonly_command_session_command(args) {
        ToolIntent::read_only_unified_action()
    } else {
        ToolIntent::mutating()
    }
}

fn write_stdin_intent(args: &Value) -> ToolIntent {
    match crate::tools::command_args::write_stdin_dispatch(args) {
        Ok(crate::tools::command_args::WriteStdinDispatch::Poll) => ToolIntent::read_only(),
        Ok(crate::tools::command_args::WriteStdinDispatch::Write) | Err(_) => {
            ToolIntent::mutating()
        }
    }
}

fn memory_tool_intent(args: &Value) -> ToolIntent {
    let command = args
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if command.eq_ignore_ascii_case("view") {
        ToolIntent::read_only()
    } else {
        ToolIntent::mutating()
    }
}

/// Conservative allow-list of read-only inspection commands used by
/// `command_session`. Any command that could write, move, or delete must be
/// rejected so it is not cached or parallelized as read-only.
const READONLY_UNIFIED_EXEC_COMMANDS: &[&str] = &[
    "rg", "ls", "cat", "diff", "find", "wc", "grep", "egrep", "fgrep", "head", "tail", "sort",
    "uniq", "awk", "sed", "cut", "tr", "ast-grep", "sg",
];

fn is_readonly_base_command(command: &str) -> bool {
    READONLY_UNIFIED_EXEC_COMMANDS.contains(&command)
}

fn is_readonly_command_session_command(args: &Value) -> bool {
    let Ok(Some(parts)) = crate::tools::command_args::command_words(args) else {
        return false;
    };

    if parts.iter().any(|part| part == "--dry-run") {
        return true;
    }

    let Some(command) = parts.first().map(String::as_str) else {
        return false;
    };

    if is_readonly_base_command(command) {
        // Verify the raw command has no redirections, command substitutions, or
        // destructive subcommands (e.g. `find -delete`, `-exec rm`).
        if !is_readonly_command_string(args) {
            return false;
        }
        // For pipelines, every segment must start with an allow-listed command.
        return is_readonly_pipeline_segments(args);
    }

    match command {
        "git" => matches!(parts.get(1).map(String::as_str), Some("status")),
        "cargo" => matches!(parts.get(1).map(String::as_str), Some("check" | "test")),
        "npm" | "pnpm" | "yarn" => match parts.get(1).map(String::as_str) {
            Some("test") => true,
            Some("run") => matches!(parts.get(2).map(String::as_str), Some("test")),
            _ => false,
        },
        _ => false,
    }
}

/// For pipelined commands, ensure every segment begins with an allow-listed
/// read-only command. This prevents read-only caching of constructs like
/// `cat a.txt | tee b.txt` or `grep x | rm`.
fn is_readonly_pipeline_segments(args: &Value) -> bool {
    let Some(raw) = crate::tools::command_args::raw_command_text(args) else {
        return false;
    };

    let segments: Vec<&str> = raw.split('|').map(str::trim).collect();
    if segments.len() <= 1 {
        return true;
    }

    for segment in segments {
        if segment.is_empty() {
            return false;
        }
        let first_command = segment
            .split_whitespace()
            .find(|token| !token.starts_with('-') && !token.contains('='))
            .map(|token| token.to_ascii_lowercase());
        let Some(first_command) = first_command else {
            return false;
        };
        if !is_readonly_base_command(&first_command) {
            return false;
        }
    }

    true
}

/// Determine the action for file_operation tool based on args.
/// Returns the action string or a default if inference is possible.
pub fn file_operation_action(args: &Value) -> Option<&str> {
    args.get("action").and_then(|v| v.as_str()).or_else(|| {
        let has_read_path = args.get("path").is_some()
            || args.get("file_path").is_some()
            || args.get("filepath").is_some()
            || args.get("target_path").is_some()
            || args.get("file").is_some()
            || args.get("p").is_some();

        if args.get("old_str").is_some() {
            Some("edit")
        } else if args.get("content").is_some() {
            Some("write")
        } else if args.get("destination").is_some() {
            Some("move")
        } else if has_read_path {
            Some("read")
        } else {
            None
        }
    })
}

/// Determine the action for command_session tool based on args.
/// Returns the action string or None if no inference is possible.
pub fn command_session_action(args: &Value) -> Option<&str> {
    args.get("action").and_then(|v| v.as_str()).or_else(|| {
        // Check for standard command fields
        if args.get("command").is_some()
            || args.get("cmd").is_some()
            || args.get("raw_command").is_some()
            || crate::tools::command_args::has_indexed_command_parts(args)
        {
            Some("run")
        } else if args.get("code").is_some() {
            Some("code")
        } else if args.get("input").is_some()
            || args.get("chars").is_some()
            || args.get("text").is_some()
        {
            Some("write")
        } else if args.get("spool_path").is_some()
            || args.get("query").is_some()
            || args.get("head_lines").is_some()
            || args.get("tail_lines").is_some()
            || args.get("max_matches").is_some()
            || args.get("literal").is_some()
        {
            Some("inspect")
        } else if args.get("session_id").is_some() || args.get("s").is_some() {
            Some("poll")
        } else {
            None
        }
    })
}

fn action_matches(action: Option<&str>, expected: &str) -> bool {
    action.is_some_and(|candidate| candidate.eq_ignore_ascii_case(expected))
}

fn action_matches_any(action: Option<&str>, expected: &[&str]) -> bool {
    action.is_some_and(|candidate| {
        expected
            .iter()
            .any(|expected_action| candidate.eq_ignore_ascii_case(expected_action))
    })
}

pub fn file_operation_action_is(args: &Value, expected: &str) -> bool {
    action_matches(file_operation_action(args), expected)
}

pub fn file_operation_action_in(args: &Value, expected: &[&str]) -> bool {
    action_matches_any(file_operation_action(args), expected)
}

pub fn command_session_action_is(args: &Value, expected: &str) -> bool {
    action_matches(command_session_action(args), expected)
}

pub fn command_session_action_in(args: &Value, expected: &[&str]) -> bool {
    action_matches_any(command_session_action(args), expected)
}

/// Return the explicit action requested through the consolidated MCP tool.
pub fn mcp_action(args: &Value) -> Option<&str> {
    args.get("action").and_then(Value::as_str)
}

pub fn mcp_action_is(args: &Value, expected: &str) -> bool {
    action_matches(mcp_action(args), expected)
}

/// Return the policy identity for actions whose risk differs from their
/// otherwise low-risk parent tool.
pub fn action_qualified_policy_name(tool_name: &str, args: Option<&Value>) -> Option<&'static str> {
    if tool_name == tools::MCP_CONNECT_SERVER
        || (tool_name == tools::MCP && args.is_some_and(|args| mcp_action_is(args, "connect")))
    {
        return Some("mcp:connect");
    }

    if tool_name == tools::MCP_DISCONNECT_SERVER
        || (tool_name == tools::MCP && args.is_some_and(|args| mcp_action_is(args, "disconnect")))
    {
        return Some("mcp:disconnect");
    }

    None
}

fn command_session_has_input(args: &Value) -> bool {
    interactive_input_text(args).is_some()
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_command_session_tool_name, classify_tool_intent, file_operation_action,
        is_command_run_tool_call, is_edited_file_conflict_guarded_call, is_parallel_safe_call,
        remap_file_operation_command_args_to_command_session, should_use_spool_reference_only,
    };
    use crate::config::constants::tools;
    use serde_json::json;

    #[test]
    fn file_operation_read_is_retry_safe() {
        let intent = classify_tool_intent(
            tools::UNIFIED_FILE,
            &json!({"action": "read", "path": "README.md"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_poll_is_retry_safe() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "poll", "session_id": 1}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_inspect_is_retry_safe() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "inspect", "spool_path": ".vtcode/context/tool_outputs/run-1.txt"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_continue_without_input_is_retry_safe() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "continue", "session_id": "run-1"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_continue_with_input_is_mutating_and_destructive() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "continue", "session_id": "run-1", "input": "q"}),
        );
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.readonly_unified_action);
        assert!(!intent.retry_safe);
    }

    #[test]
    fn command_session_continue_with_empty_input_stays_retry_safe() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "continue", "session_id": "run-1", "input": ""}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_run_is_mutating_and_destructive() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "echo hi"}),
        );
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.retry_safe);
    }

    #[test]
    fn command_session_run_diff_is_read_only() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "diff a.rs b.rs"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_run_find_is_read_only() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "find . -type f -name '*.rs' -not -path '*/target/*'"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_run_grep_wc_head_are_read_only() {
        for cmd in [
            "grep -rn 'todo' src",
            "wc -l src/main.rs",
            "head -50 src/lib.rs",
            "tail -20 src/lib.rs",
            "sort src/words.txt | uniq",
            "ast-grep -p 'foo($A)' -l rs",
        ] {
            let intent = classify_tool_intent(
                tools::UNIFIED_EXEC,
                &json!({"action": "run", "command": cmd}),
            );
            assert!(!intent.mutating, "expected '{cmd}' to be read-only");
            assert!(
                intent.readonly_unified_action,
                "expected '{cmd}' to be readonly_unified_action"
            );
        }
    }

    #[test]
    fn command_session_run_with_redirection_is_mutating() {
        for cmd in [
            "cat a.txt > b.txt",
            "grep x src > out.txt",
            "diff a b | wc -l > count.txt",
            "echo $(date) > log.txt",
        ] {
            let intent = classify_tool_intent(
                tools::UNIFIED_EXEC,
                &json!({"action": "run", "command": cmd}),
            );
            assert!(
                intent.mutating,
                "expected '{cmd}' to be mutating because it contains redirection/substitution"
            );
        }
    }

    #[test]
    fn command_session_run_find_with_destructive_flags_is_mutating() {
        for cmd in [
            "find . -type f -delete",
            "find . -name '*.tmp' -exec rm {} \\;",
            "find . -name '*.tmp' -exec chmod 600 {} \\;",
        ] {
            let intent = classify_tool_intent(
                tools::UNIFIED_EXEC,
                &json!({"action": "run", "command": cmd}),
            );
            assert!(
                intent.mutating,
                "expected '{cmd}' to be mutating because it has destructive find flags"
            );
        }
    }

    #[test]
    fn command_session_run_pipelines_with_unsafe_segments_are_mutating() {
        for cmd in [
            "cat a.txt | tee b.txt",
            "echo hi | cat",
            "grep x src | rm -rf",
        ] {
            let intent = classify_tool_intent(
                tools::UNIFIED_EXEC,
                &json!({"action": "run", "command": cmd}),
            );
            assert!(
                intent.mutating,
                "expected '{cmd}' to be mutating because a pipeline segment is unsafe"
            );
        }
    }

    #[test]
    fn command_session_run_allowlisted_is_read_only() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "rg planning_active src"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn command_session_run_dry_run_is_read_only() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "npm install --dry-run"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn parallel_safe_calls_reject_control_and_exec_paths() {
        assert!(is_parallel_safe_call(
            tools::READ_FILE,
            &json!({"path": "README.md"})
        ));
        assert!(!is_parallel_safe_call(tools::LIST_PTY_SESSIONS, &json!({})));
        assert!(!is_parallel_safe_call(
            tools::REQUEST_USER_INPUT,
            &json!({"questions": []})
        ));
        assert!(!is_parallel_safe_call(
            tools::UNIFIED_EXEC,
            &json!({"action": "inspect", "session_id": "run-1"})
        ));
    }

    #[test]
    fn command_session_cmd_alias_infers_run() {
        let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"cmd": "echo hi"}));
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.readonly_unified_action);
    }

    #[test]
    fn command_session_chars_alias_infers_write() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"session_id": "abc123", "chars": "status\n"}),
        );
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.readonly_unified_action);
    }

    #[test]
    fn write_stdin_empty_chars_is_read_only_and_retry_safe() {
        let intent = classify_tool_intent(
            tools::WRITE_STDIN,
            &json!({"session_id": "abc123", "chars": ""}),
        );

        assert!(!intent.mutating);
        assert!(!intent.destructive);
        assert!(!intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn write_stdin_non_empty_chars_is_mutating() {
        let intent = classify_tool_intent(
            tools::WRITE_STDIN,
            &json!({"session_id": "abc123", "chars": "  status\n"}),
        );

        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.readonly_unified_action);
        assert!(!intent.retry_safe);
    }

    #[test]
    fn command_session_text_alias_infers_write() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"session_id": "abc123", "text": "status\n"}),
        );
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.readonly_unified_action);
    }

    #[test]
    fn command_session_spool_path_alias_infers_inspect() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"spool_path": ".vtcode/context/tool_outputs/run-1.txt"}),
        );
        assert!(!intent.mutating);
        assert!(!intent.destructive);
        assert!(intent.readonly_unified_action);
    }

    #[test]
    fn command_session_compact_session_alias_infers_poll() {
        let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"s": "run-1"}));
        assert!(!intent.mutating);
        assert!(!intent.destructive);
        assert!(intent.readonly_unified_action);
    }

    #[test]
    fn file_operation_unknown_args_require_action() {
        let args = json!({
            "unexpected": true
        });
        let action = file_operation_action(&args);
        assert_eq!(action, None);
    }

    #[test]
    fn file_operation_compact_path_alias_infers_read() {
        let args = json!({
            "p": "README.md"
        });
        let action = file_operation_action(&args);
        assert_eq!(action, Some("read"));
    }

    #[test]
    fn remap_file_operation_command_args_maps_command_payload_to_command_session() {
        let remapped = remap_file_operation_command_args_to_command_session(&json!({
            "command": "cargo check",
            "cwd": ".",
            "timeout_ms": 1000
        }))
        .expect("command payload should remap");

        assert_eq!(remapped["action"], "run");
        assert_eq!(remapped["command"], "cargo check");
        assert_eq!(remapped["cwd"], ".");
        assert_eq!(remapped["timeout_ms"], 1000);
    }

    #[test]
    fn remap_file_operation_command_args_accepts_exec_action_aliases() {
        let remapped = remap_file_operation_command_args_to_command_session(&json!({
            "action": "shell",
            "cmd": "echo ok"
        }))
        .expect("shell action alias should remap");

        assert_eq!(remapped["action"], "run");
        assert_eq!(remapped["command"], "echo ok");
    }

    #[test]
    fn remap_file_operation_command_args_rejects_non_command_actions() {
        let remapped = remap_file_operation_command_args_to_command_session(&json!({
            "action": "read",
            "command": "echo ok"
        }));

        assert_eq!(remapped, None);
    }

    #[test]
    fn edited_file_conflict_guard_accepts_supported_mutations() {
        assert!(is_edited_file_conflict_guarded_call(
            tools::WRITE_FILE,
            &json!({"path": "README.md", "content": "agent"})
        ));
        assert!(is_edited_file_conflict_guarded_call(
            tools::CREATE_FILE,
            &json!({"path": "README.md", "content": "agent"})
        ));
        assert!(is_edited_file_conflict_guarded_call(
            tools::EDIT_FILE,
            &json!({"path": "README.md", "old_str": "a", "new_str": "b"})
        ));
        assert!(is_edited_file_conflict_guarded_call(
            tools::APPLY_PATCH,
            &json!({"patch": "*** Begin Patch\n*** End Patch\n"})
        ));
        assert!(is_edited_file_conflict_guarded_call(
            tools::UNIFIED_FILE,
            &json!({"action": "write", "path": "README.md", "content": "agent"})
        ));
        assert!(is_edited_file_conflict_guarded_call(
            tools::UNIFIED_FILE,
            &json!({"action": "create", "path": "README.md", "content": "agent"})
        ));
    }

    #[test]
    fn edited_file_conflict_guard_rejects_non_guarded_calls() {
        assert!(!is_edited_file_conflict_guarded_call(
            tools::READ_FILE,
            &json!({"path": "README.md"})
        ));
        assert!(!is_edited_file_conflict_guarded_call(
            tools::GREP_FILE,
            &json!({"pattern": "needle", "path": "."})
        ));
        assert!(!is_edited_file_conflict_guarded_call(
            tools::LIST_FILES,
            &json!({"path": "."})
        ));
        assert!(!is_edited_file_conflict_guarded_call(
            tools::UNIFIED_FILE,
            &json!({"action": "read", "path": "README.md"})
        ));
        assert!(!is_edited_file_conflict_guarded_call(
            tools::UNIFIED_FILE,
            &json!({"action": "delete", "path": "README.md"})
        ));
        assert!(!is_edited_file_conflict_guarded_call(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "git status"})
        ));
    }

    #[test]
    fn canonical_command_session_tool_name_normalizes_exec_aliases() {
        for alias in [
            tools::UNIFIED_EXEC,
            tools::RUN_PTY_CMD,
            tools::SEND_PTY_INPUT,
            tools::READ_PTY_SESSION,
            tools::LIST_PTY_SESSIONS,
            tools::CLOSE_PTY_SESSION,
            tools::EXECUTE_CODE,
            tools::EXEC_PTY_CMD,
            tools::EXEC_COMMAND,
            tools::WRITE_STDIN,
            tools::SHELL,
            "bash",
            "exec",
            "container.exec",
        ] {
            assert_eq!(
                canonical_command_session_tool_name(alias),
                Some(tools::UNIFIED_EXEC)
            );
        }
    }

    #[test]
    fn spool_reference_only_detects_exec_aliases() {
        assert!(should_use_spool_reference_only(
            Some(tools::RUN_PTY_CMD),
            &json!({"spool_path": ".vtcode/context/tool_outputs/run-1.txt"})
        ));
    }

    #[test]
    fn spool_reference_only_detects_exec_payload_without_tool_name() {
        assert!(should_use_spool_reference_only(
            None,
            &json!({
                "command": "cargo check",
                "spool_path": ".vtcode/context/tool_outputs/run-1.txt",
                "exit_code": 0
            })
        ));
    }

    #[test]
    fn spool_reference_only_skips_loop_recovery_payloads() {
        assert!(!should_use_spool_reference_only(
            Some(tools::UNIFIED_EXEC),
            &json!({
                "spool_path": ".vtcode/context/tool_outputs/run-1.txt",
                "exit_code": 0,
                "loop_detected": true
            })
        ));
    }

    #[test]
    fn is_command_run_tool_call_only_accepts_run_actions() {
        assert!(is_command_run_tool_call(
            tools::RUN_PTY_CMD,
            &json!({"command": "cargo check"})
        ));
        assert!(is_command_run_tool_call(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "cargo check"})
        ));
        assert!(is_command_run_tool_call(
            tools::EXEC_COMMAND,
            &json!({"cmd": "cargo check"})
        ));
        assert!(!is_command_run_tool_call(
            tools::UNIFIED_EXEC,
            &json!({"action": "poll", "session_id": "run-1"})
        ));
        assert!(!is_command_run_tool_call(
            tools::WRITE_STDIN,
            &json!({"session_id": "run-1", "chars": "q"})
        ));
    }
}
