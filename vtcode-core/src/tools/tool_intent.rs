use serde_json::Value;

use crate::config::constants::tools;
use crate::tools::command_args::{interactive_input_text, is_readonly_command_string};
use crate::tools::names::canonical_tool_name;

/// Valid list modes accepted by `FileOpsTool`.  Used by the `format`→`mode`
/// cross-mapping in `normalize_unified_search_args` to decide whether a
/// `format` value should be mapped to `mode`.  This must stay in sync with
/// the `mode` enum in `list_files_parameters()` (vtcode-utility-tool-specs)
/// and `FileOpsTool::normalize_list_mode` (file_ops/tool.rs).
const VALID_LIST_MODES: &[&str] = &[
    "list",
    "recursive",
    "tree",
    "find_name",
    "find_content",
    "largest",
    "file",
    "files",
];

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
    /// Whether the tool is a read-only unified action (e.g. `unified_file` read).
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
        tools::UNIFIED_SEARCH => Some(ToolBehavior::function(
            ToolMutationModel::ReadOnly,
            true,
            false,
        )),
        tools::UNIFIED_EXEC => Some(ToolBehavior::function(
            ToolMutationModel::ByArgs(unified_exec_intent),
            false,
            true,
        )),
        tools::UNIFIED_FILE => Some(ToolBehavior::function(
            ToolMutationModel::ByArgs(unified_file_intent),
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
        tools::UNIFIED_FILE => unified_file_action(args)
            .map(is_edited_file_conflict_guarded_unified_file_action)
            .unwrap_or(false),
        _ => false,
    }
}

fn is_edited_file_conflict_guarded_unified_file_action(action: &str) -> bool {
    action_matches_any(Some(action), &["write", "create", "edit", "patch"])
}

pub fn canonical_unified_exec_tool_name(tool_name: &str) -> Option<&'static str> {
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

    if tool_name.is_some_and(|name| canonical_unified_exec_tool_name(name).is_some()) {
        return true;
    }

    if tool_name.is_some_and(|name| canonical_tool_name(name) == tools::UNIFIED_SEARCH) {
        return true;
    }

    if looks_like_unified_search_output(obj) {
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

fn looks_like_unified_search_output(obj: &serde_json::Map<String, Value>) -> bool {
    ["matches", "results", "files", "entries"]
        .iter()
        .any(|key| obj.contains_key(*key))
        || obj
            .get("tool")
            .or_else(|| obj.get("tool_name"))
            .or_else(|| obj.get("name"))
            .and_then(Value::as_str)
            .is_some_and(|name| canonical_tool_name(name) == tools::UNIFIED_SEARCH)
}

/// Returns `true` if `tool_name` refers to a command/PTY execution tool.
///
/// This includes PTY session tools and all unified exec aliases.
#[must_use]
pub fn is_command_tool(tool_name: &str) -> bool {
    tool_name == tools::CREATE_PTY_SESSION
        || tool_name == tools::SEND_PTY_INPUT
        || canonical_unified_exec_tool_name(tool_name).is_some()
}

pub fn is_command_run_tool_call(tool_name: &str, args: &Value) -> bool {
    match tool_name {
        tools::RUN_PTY_CMD | tools::CREATE_PTY_SESSION | tools::SHELL | "bash" => true,
        tools::UNIFIED_EXEC
        | tools::EXEC_PTY_CMD
        | tools::EXEC_COMMAND
        | "exec"
        | "container.exec" => unified_exec_action_is(args, "run"),
        _ => false,
    }
}

pub fn remap_unified_file_command_args_to_unified_exec(args: &Value) -> Option<Value> {
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

fn unified_file_intent(args: &Value) -> ToolIntent {
    if unified_file_action_is(args, "read") {
        ToolIntent::read_only_unified_action()
    } else {
        ToolIntent::mutating()
    }
}

fn unified_exec_intent(args: &Value) -> ToolIntent {
    let has_exec_input = unified_exec_has_input(args);
    let readonly_unified_action = if unified_exec_action_is(args, "run") {
        is_readonly_unified_exec_command(args)
    } else {
        unified_exec_action_in(args, &["poll", "list", "inspect"])
            || (unified_exec_action_is(args, "continue") && !has_exec_input)
    };

    if readonly_unified_action {
        ToolIntent::read_only_unified_action()
    } else {
        ToolIntent::mutating()
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
/// `unified_exec`. Any command that could write, move, or delete must be
/// rejected so it is not cached or parallelized as read-only.
const READONLY_UNIFIED_EXEC_COMMANDS: &[&str] = &[
    "rg", "ls", "cat", "diff", "find", "wc", "grep", "egrep", "fgrep", "head", "tail", "sort",
    "uniq", "awk", "sed", "cut", "tr", "ast-grep", "sg",
];

fn is_readonly_base_command(command: &str) -> bool {
    READONLY_UNIFIED_EXEC_COMMANDS.contains(&command)
}

fn is_readonly_unified_exec_command(args: &Value) -> bool {
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

/// Determine the action for unified_file tool based on args.
/// Returns the action string or a default if inference is possible.
pub fn unified_file_action(args: &Value) -> Option<&str> {
    use crate::tools::editing::looks_like_vte_patch;

    args.get("action").and_then(|v| v.as_str()).or_else(|| {
        let has_read_path = args.get("path").is_some()
            || args.get("file_path").is_some()
            || args.get("filepath").is_some()
            || args.get("target_path").is_some()
            || args.get("file").is_some()
            || args.get("p").is_some();
        let patch_in_input = args
            .get("input")
            .and_then(|v| v.as_str())
            .is_some_and(looks_like_vte_patch);
        let raw_patch = args.as_str().is_some_and(looks_like_vte_patch);

        if args.get("old_str").is_some() {
            Some("edit")
        } else if args.get("patch").is_some() || patch_in_input || raw_patch {
            Some("patch")
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

/// Determine the action for unified_exec tool based on args.
/// Returns the action string or None if no inference is possible.
pub fn unified_exec_action(args: &Value) -> Option<&str> {
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

pub fn unified_file_action_is(args: &Value, expected: &str) -> bool {
    action_matches(unified_file_action(args), expected)
}

pub fn unified_file_action_in(args: &Value, expected: &[&str]) -> bool {
    action_matches_any(unified_file_action(args), expected)
}

pub fn unified_exec_action_is(args: &Value, expected: &str) -> bool {
    action_matches(unified_exec_action(args), expected)
}

pub fn unified_exec_action_in(args: &Value, expected: &[&str]) -> bool {
    action_matches_any(unified_exec_action(args), expected)
}

pub fn unified_search_action_is(args: &Value, expected: &str) -> bool {
    action_matches(unified_search_action(args), expected)
}

pub fn unified_search_action_in(args: &Value, expected: &[&str]) -> bool {
    action_matches_any(unified_search_action(args), expected)
}

fn unified_exec_has_input(args: &Value) -> bool {
    interactive_input_text(args).is_some()
}

fn get_field_case_insensitive<'a>(
    args: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Option<&'a Value> {
    args.get(key).or_else(|| {
        args.iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value)
    })
}

fn has_meaningful_search_field(args: &serde_json::Map<String, Value>, key: &str) -> bool {
    match get_field_case_insensitive(args, key) {
        Some(Value::Null) | None => false,
        Some(Value::String(text)) => !text.trim().is_empty(),
        Some(Value::Array(values)) => !values.is_empty(),
        Some(_) => true,
    }
}

fn looks_like_list_glob_pattern(args: &serde_json::Map<String, Value>) -> bool {
    let pattern = get_field_case_insensitive(args, "pattern")
        .or_else(|| get_field_case_insensitive(args, "query"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty());
    let Some(pattern) = pattern else {
        return false;
    };

    let has_glob_wildcards =
        pattern.contains('*') || pattern.contains('?') || pattern.contains('[');
    if !has_glob_wildcards {
        return false;
    }

    pattern.contains('/')
        || pattern.contains('\\')
        || pattern.starts_with("*.")
        || pattern.contains("*.")
}

fn unified_search_action_from_object(args: &serde_json::Map<String, Value>) -> Option<&str> {
    get_field_case_insensitive(args, "action")
        .and_then(|value| value.as_str())
        .or_else(|| {
            // Smart action inference based on parameters
            let has_structural_workflow = get_field_case_insensitive(args, "workflow")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(|workflow| !workflow.is_empty());
            let has_pattern = has_meaningful_search_field(args, "pattern")
                || has_meaningful_search_field(args, "query");
            let has_structural_hint = has_structural_workflow
                || has_meaningful_search_field(args, "lang")
                || has_meaningful_search_field(args, "selector")
                || has_meaningful_search_field(args, "strictness")
                || has_meaningful_search_field(args, "debug_query")
                || has_meaningful_search_field(args, "globs")
                || has_meaningful_search_field(args, "config_path")
                || has_meaningful_search_field(args, "filter")
                || has_meaningful_search_field(args, "skip_snapshot_tests");
            let has_path = has_meaningful_search_field(args, "path");

            if has_structural_workflow || (has_pattern && has_structural_hint) {
                Some("structural")
            } else if has_pattern && has_path && looks_like_list_glob_pattern(args) {
                Some("list")
            } else if has_pattern {
                Some("grep")
            } else if get_field_case_insensitive(args, "keyword").is_some() {
                Some("tools")
            } else if get_field_case_insensitive(args, "url").is_some() {
                Some("web")
            } else if get_field_case_insensitive(args, "sub_action").is_some()
                || get_field_case_insensitive(args, "name").is_some()
            {
                Some("skill")
            } else if get_field_case_insensitive(args, "scope").is_some() {
                Some("errors")
            } else if get_field_case_insensitive(args, "path").is_some() {
                Some("list")
            } else {
                None
            }
        })
}

fn is_unified_search_arg_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "action"
            | "pattern"
            | "query"
            | "path"
            | "lang"
            | "selector"
            | "strictness"
            | "debug_query"
            | "workflow"
            | "config_path"
            | "filter"
            | "globs"
            | "context_lines"
            | "max_results"
            | "skip_snapshot_tests"
            | "keyword"
            | "url"
            | "scope"
            | "sub_action"
            | "name"
    )
}

fn object_has_unified_search_signal(args: &serde_json::Map<String, Value>) -> bool {
    args.keys().any(|key| is_unified_search_arg_key(key))
}

fn parse_object_json_string(payload: &str) -> Option<serde_json::Map<String, Value>> {
    let parsed = serde_json::from_str::<Value>(payload).ok()?;
    parsed.as_object().cloned()
}

fn extract_unified_search_args_object(args: &Value) -> Option<serde_json::Map<String, Value>> {
    match args {
        Value::Object(args_obj) => {
            if object_has_unified_search_signal(args_obj) {
                return Some(args_obj.clone());
            }

            for wrapper in ["arguments", "args"] {
                let Some(candidate) = get_field_case_insensitive(args_obj, wrapper) else {
                    continue;
                };
                match candidate {
                    Value::Object(inner_obj) => return Some(inner_obj.clone()),
                    Value::String(inner_str) => {
                        if let Some(parsed_obj) = parse_object_json_string(inner_str) {
                            return Some(parsed_obj);
                        }
                    }
                    _ => {}
                }
            }

            Some(args_obj.clone())
        }
        Value::String(raw) => parse_object_json_string(raw),
        _ => None,
    }
}

/// Determine the action for unified_search tool based on args.
/// Returns the action string or None if no inference is possible.
pub fn unified_search_action(args: &Value) -> Option<&str> {
    let args_obj = args.as_object()?;
    unified_search_action_from_object(args_obj)
}

/// Cross-map `unified_search`-specific fields to `ListInput`-compatible
/// names for `action=list`.  The schema defines `max_results`, `globs`, and
/// `format`, but `ListInput` only knows `max_items`/`per_page`/`glob_pattern`/
/// `mode`.  Without this mapping these fields are silently dropped by serde.
///
/// Mappings (all use `or_insert` — explicit caller-provided values are
/// preserved):
/// - `globs` (string or array first element) → `pattern` (serde alias for
///   `glob_pattern`)
/// - `max_results` → `per_page` + `max_items` (both, so one page holds all
///   requested results)
/// - `format` → `mode` when the value is a valid list mode (see
///   `VALID_LIST_MODES`).  When `format` is present but NOT a valid list
///   mode (e.g. `"github"`), a `_format_ignored` warning is injected so the
///   agent gets feedback instead of a silent drop.
fn apply_list_cross_mappings(normalized: &mut serde_json::Map<String, Value>) {
    // globs → pattern
    let globs_pattern = if !normalized.contains_key("pattern") {
        normalized.get("globs").and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .next()
                .map(str::to_string),
            _ => None,
        })
    } else {
        None
    };
    if let Some(glob_str) = globs_pattern {
        normalized
            .entry("pattern".to_string())
            .or_insert(Value::String(glob_str));
    }

    // max_results → per_page + max_items
    if let Some(max_results) = normalized.get("max_results").cloned() {
        normalized
            .entry("per_page".to_string())
            .or_insert(max_results.clone());
        normalized
            .entry("max_items".to_string())
            .or_insert(max_results);
    }

    // format → mode (only when the value is a valid list mode)
    if !normalized.contains_key("mode") {
        if let Some(format_val) = normalized.get("format").and_then(Value::as_str) {
            let format_owned = format_val.to_string();
            let is_valid_mode = VALID_LIST_MODES
                .iter()
                .any(|m| m.eq_ignore_ascii_case(&format_owned));
            if is_valid_mode {
                normalized
                    .entry("mode".to_string())
                    .or_insert(Value::String(format_owned));
            } else {
                // format was provided but is not a valid list mode (e.g.
                // "github", "sarif").  Inject a warning so the agent gets
                // feedback instead of a silent drop.
                let warning = format!(
                    "Parameter `format` value \"{format_owned}\" is not a valid list mode \
                     (valid: {VALID_LIST_MODES:?}). It was ignored. Use `mode` directly \
                     to control listing mode (list, recursive, tree, find_name, \
                     find_content, largest)."
                );
                normalized
                    .entry("_format_ignored".to_string())
                    .or_insert(Value::String(warning));
            }
        }
    }
}

/// Normalize unified_search args so case/shape variants still pass schema checks.
pub fn normalize_unified_search_args(args: &Value) -> Value {
    let Some(args_obj) = extract_unified_search_args_object(args) else {
        return args.clone();
    };

    let mut normalized = serde_json::Map::with_capacity(args_obj.len() + 1);
    for (key, value) in &args_obj {
        let canonical = if key.eq_ignore_ascii_case("action") {
            "action"
        } else if key.eq_ignore_ascii_case("pattern") {
            "pattern"
        } else if key.eq_ignore_ascii_case("query") {
            "query"
        } else if key.eq_ignore_ascii_case("path") {
            "path"
        } else if key.eq_ignore_ascii_case("lang") {
            "lang"
        } else if key.eq_ignore_ascii_case("selector") {
            "selector"
        } else if key.eq_ignore_ascii_case("strictness") {
            "strictness"
        } else if key.eq_ignore_ascii_case("debug_query") || key.eq_ignore_ascii_case("debug-query")
        {
            "debug_query"
        } else if key.eq_ignore_ascii_case("workflow") {
            "workflow"
        } else if key.eq_ignore_ascii_case("config_path") || key.eq_ignore_ascii_case("config-path")
        {
            "config_path"
        } else if key.eq_ignore_ascii_case("filter") {
            "filter"
        } else if key.eq_ignore_ascii_case("globs") {
            "globs"
        } else if key.eq_ignore_ascii_case("exclude") {
            "exclude"
        } else if key.eq_ignore_ascii_case("context_lines")
            || key.eq_ignore_ascii_case("context-lines")
        {
            "context_lines"
        } else if key.eq_ignore_ascii_case("max_results") || key.eq_ignore_ascii_case("max-results")
        {
            "max_results"
        } else if key.eq_ignore_ascii_case("skip_snapshot_tests")
            || key.eq_ignore_ascii_case("skip-snapshot-tests")
        {
            "skip_snapshot_tests"
        } else if key.eq_ignore_ascii_case("keyword") {
            "keyword"
        } else if key.eq_ignore_ascii_case("url") {
            "url"
        } else if key.eq_ignore_ascii_case("scope") {
            "scope"
        } else if key.eq_ignore_ascii_case("sub_action") {
            "sub_action"
        } else if key.eq_ignore_ascii_case("name") {
            "name"
        } else if key.eq_ignore_ascii_case("name_pattern")
            || key.eq_ignore_ascii_case("name-pattern")
        {
            "name_pattern"
        } else {
            key
        };
        normalized
            .entry(canonical.to_string())
            .or_insert_with(|| value.clone());
    }

    let inferred_action = unified_search_action_from_object(&normalized).map(|a| a.to_string());
    if let Some(action) = inferred_action {
        normalized
            .entry("action".to_string())
            .or_insert_with(|| Value::String(action));
    }

    let action = normalized
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let keyword_alias = normalized
        .get("keyword")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let pattern_alias = normalized
        .get("pattern")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            normalized
                .get("query")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| keyword_alias.clone());

    if action.eq_ignore_ascii_case("grep") {
        // Map `match` -> `pattern` for grep action.  The `match` field is
        // only valid for `outline` action, but the model sometimes uses it
        // with grep (checkpoint turn_635: agent passed `match: "doctor"`
        // instead of `pattern`).
        if !normalized.contains_key("pattern") {
            let match_value = normalized
                .get("match")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string());
            if let Some(match_val) = match_value {
                normalized
                    .entry("pattern".to_string())
                    .or_insert_with(|| Value::String(match_val));
                normalized.remove("match");
            }
        }

        if let Some(pattern) = pattern_alias.clone() {
            normalized
                .entry("pattern".to_string())
                .or_insert_with(|| Value::String(pattern));
        }
    }

    if action.eq_ignore_ascii_case("list")
        && !normalized.contains_key("pattern")
        && !normalized.contains_key("name_pattern")
    {
        if let Some(keyword) = keyword_alias {
            normalized
                .entry("name_pattern".to_string())
                .or_insert_with(|| Value::String(keyword));
        } else if let Some(ref pattern) = pattern_alias {
            normalized
                .entry("pattern".to_string())
                .or_insert_with(|| Value::String(pattern.clone()));
        }
    }

    // For outline action: map pattern -> path.  The agent often passes
    // `pattern` (used by grep/list/structural) instead of `path` (used by
    // outline).  Without this mapping, outline defaults to `path: "."`
    // (workspace root) and the agent gets the wrong outline entirely
    // (checkpoint turn_597: agent passed `pattern:
    // "vtcode-core/src/tools/registry.rs"` instead of `path`).
    if action.eq_ignore_ascii_case("outline") && !normalized.contains_key("path") {
        if let Some(pattern) = pattern_alias.clone() {
            normalized
                .entry("path".to_string())
                .or_insert_with(|| Value::String(pattern));
        }
    }

    // Cross-map unified_search-specific fields to ListInput-compatible
    // field names for action=list.  The unified_search schema defines
    // `max_results`, `globs`, and `format`, but `ListInput` only knows
    // `max_items`/`per_page`/`glob_pattern`/`mode`.  Without this
    // mapping these fields are silently dropped by serde, the listing
    // is capped at the default 20 items, and the agent loops trying
    // `max_results: 1000` / `globs: "**/*"` / `format: "tree"` with no
    // effect (checkpoint turn_606: 37-message loop on "what's in this
    // repo?").
    if action.eq_ignore_ascii_case("list") {
        apply_list_cross_mappings(&mut normalized);
    }

    // For grep action: the `format` field is only valid for structural
    // workflow (workflow="scan").  When the model passes `format` with
    // action=grep, it's often an invalid value like "content" that
    // causes schema validation to fail.  Remove it to prevent blocked
    // tool call loops (checkpoint turn_635: 8 consecutive blocked calls).
    if action.eq_ignore_ascii_case("grep") {
        if let Some(format_val) = normalized.get("format").and_then(Value::as_str) {
            let valid_grep_formats = ["files_with_matches", "count"];
            if !valid_grep_formats
                .iter()
                .any(|f| f.eq_ignore_ascii_case(format_val))
            {
                normalized.remove("format");
            }
        }
    }

    Value::Object(normalized)
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_unified_exec_tool_name, classify_tool_intent, is_command_run_tool_call,
        is_edited_file_conflict_guarded_call, is_parallel_safe_call, normalize_unified_search_args,
        remap_unified_file_command_args_to_unified_exec, should_use_spool_reference_only,
        unified_file_action,
    };
    use crate::config::constants::tools;
    use serde_json::json;

    #[test]
    fn unified_file_read_is_retry_safe() {
        let intent = classify_tool_intent(
            tools::UNIFIED_FILE,
            &json!({"action": "read", "path": "README.md"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn unified_exec_poll_is_retry_safe() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "poll", "session_id": 1}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn unified_exec_inspect_is_retry_safe() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "inspect", "spool_path": ".vtcode/context/tool_outputs/run-1.txt"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn unified_exec_continue_without_input_is_retry_safe() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "continue", "session_id": "run-1"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn unified_exec_continue_with_input_is_mutating_and_destructive() {
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
    fn unified_exec_continue_with_empty_input_stays_retry_safe() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "continue", "session_id": "run-1", "input": ""}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn unified_exec_run_is_mutating_and_destructive() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "echo hi"}),
        );
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.retry_safe);
    }

    #[test]
    fn unified_exec_run_diff_is_read_only() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "diff a.rs b.rs"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn unified_exec_run_find_is_read_only() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "find . -type f -name '*.rs' -not -path '*/target/*'"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn unified_exec_run_grep_wc_head_are_read_only() {
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
    fn unified_exec_run_with_redirection_is_mutating() {
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
    fn unified_exec_run_find_with_destructive_flags_is_mutating() {
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
    fn unified_exec_run_pipelines_with_unsafe_segments_are_mutating() {
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
    fn unified_exec_run_allowlisted_is_read_only() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "rg planning_active src"}),
        );
        assert!(!intent.mutating);
        assert!(intent.readonly_unified_action);
        assert!(intent.retry_safe);
    }

    #[test]
    fn unified_exec_run_dry_run_is_read_only() {
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
    fn unified_exec_cmd_alias_infers_run() {
        let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"cmd": "echo hi"}));
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.readonly_unified_action);
    }

    #[test]
    fn unified_exec_chars_alias_infers_write() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"session_id": "abc123", "chars": "status\n"}),
        );
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.readonly_unified_action);
    }

    #[test]
    fn unified_exec_text_alias_infers_write() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"session_id": "abc123", "text": "status\n"}),
        );
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.readonly_unified_action);
    }

    #[test]
    fn unified_exec_spool_path_alias_infers_inspect() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"spool_path": ".vtcode/context/tool_outputs/run-1.txt"}),
        );
        assert!(!intent.mutating);
        assert!(!intent.destructive);
        assert!(intent.readonly_unified_action);
    }

    #[test]
    fn unified_exec_compact_session_alias_infers_poll() {
        let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"s": "run-1"}));
        assert!(!intent.mutating);
        assert!(!intent.destructive);
        assert!(intent.readonly_unified_action);
    }

    #[test]
    fn unified_file_input_patch_infers_patch() {
        let args = json!({
            "input": "*** Begin Patch\n*** End Patch\n"
        });
        let action = unified_file_action(&args);
        assert_eq!(action, Some("patch"));
    }

    #[test]
    fn unified_file_raw_patch_infers_patch() {
        let args = json!("*** Begin Patch\n*** Update File: src/main.rs\n*** End Patch\n");
        let action = unified_file_action(&args);
        assert_eq!(action, Some("patch"));
    }

    #[test]
    fn unified_file_unknown_args_require_action() {
        let args = json!({
            "unexpected": true
        });
        let action = unified_file_action(&args);
        assert_eq!(action, None);
    }

    #[test]
    fn unified_file_compact_path_alias_infers_read() {
        let args = json!({
            "p": "README.md"
        });
        let action = unified_file_action(&args);
        assert_eq!(action, Some("read"));
    }

    #[test]
    fn remap_unified_file_command_args_maps_command_payload_to_unified_exec() {
        let remapped = remap_unified_file_command_args_to_unified_exec(&json!({
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
    fn remap_unified_file_command_args_accepts_exec_action_aliases() {
        let remapped = remap_unified_file_command_args_to_unified_exec(&json!({
            "action": "shell",
            "cmd": "echo ok"
        }))
        .expect("shell action alias should remap");

        assert_eq!(remapped["action"], "run");
        assert_eq!(remapped["command"], "echo ok");
    }

    #[test]
    fn remap_unified_file_command_args_rejects_non_command_actions() {
        let remapped = remap_unified_file_command_args_to_unified_exec(&json!({
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
        assert!(is_edited_file_conflict_guarded_call(
            tools::UNIFIED_FILE,
            &json!({"patch": "*** Begin Patch\n*** End Patch\n"})
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
    fn normalize_unified_search_args_canonicalizes_case_and_infers_action() {
        let normalized = normalize_unified_search_args(&json!({
            "Pattern": "needle",
            "Path": "."
        }));

        assert_eq!(normalized["pattern"], "needle");
        assert_eq!(normalized["path"], ".");
        assert_eq!(normalized["action"], "grep");
    }

    #[test]
    fn normalize_unified_search_args_infers_list_for_glob_patterns() {
        let normalized = normalize_unified_search_args(&json!({
            "Pattern": "**/*.rs",
            "Path": "src"
        }));

        assert_eq!(normalized["pattern"], "**/*.rs");
        assert_eq!(normalized["path"], "src");
        assert_eq!(normalized["action"], "list");
    }

    #[test]
    fn normalize_unified_search_args_unwraps_arguments_object() {
        let normalized = normalize_unified_search_args(&json!({
            "arguments": {
                "Pattern": "needle",
                "Path": "."
            }
        }));

        assert_eq!(normalized["pattern"], "needle");
        assert_eq!(normalized["path"], ".");
        assert_eq!(normalized["action"], "grep");
    }

    #[test]
    fn normalize_unified_search_args_parses_arguments_json_string() {
        let normalized = normalize_unified_search_args(&json!({
            "args": "{\"Pattern\":\"needle\",\"Path\":\".\"}"
        }));

        assert_eq!(normalized["pattern"], "needle");
        assert_eq!(normalized["path"], ".");
        assert_eq!(normalized["action"], "grep");
    }

    #[test]
    fn normalize_unified_search_args_keeps_structural_action_explicit() {
        let normalized = normalize_unified_search_args(&json!({
            "Action": "structural",
            "Pattern": "fn $NAME() {}",
            "Lang": "rust",
            "Debug-Query": "ast",
            "Max-Results": 5
        }));

        assert_eq!(normalized["action"], "structural");
        assert_eq!(normalized["pattern"], "fn $NAME() {}");
        assert_eq!(normalized["lang"], "rust");
        assert_eq!(normalized["debug_query"], "ast");
        assert_eq!(normalized["max_results"], 5);
    }

    #[test]
    fn normalize_unified_search_args_infers_structural_from_pattern_and_lang() {
        let normalized = normalize_unified_search_args(&json!({
            "Pattern": "fn $NAME($$$ARGS) { $$$BODY }",
            "Lang": "rust",
            "Path": "."
        }));

        assert_eq!(normalized["action"], "structural");
        assert_eq!(normalized["lang"], "rust");
        assert_eq!(normalized["path"], ".");
    }

    #[test]
    fn normalize_unified_search_args_canonicalizes_structural_workflow_fields() {
        let normalized = normalize_unified_search_args(&json!({
            "Workflow": "scan",
            "Config-Path": "config/sgconfig.yml",
            "Filter": "rust/no-iterator-for-each",
            "Skip-Snapshot-Tests": true
        }));

        assert_eq!(normalized["workflow"], "scan");
        assert_eq!(normalized["config_path"], "config/sgconfig.yml");
        assert_eq!(normalized["filter"], "rust/no-iterator-for-each");
        assert_eq!(normalized["skip_snapshot_tests"], true);
        assert_eq!(normalized["action"], "structural");
    }

    #[test]
    fn normalize_unified_search_args_maps_keyword_to_pattern_for_grep() {
        let normalized = normalize_unified_search_args(&json!({
            "action": "grep",
            "keyword": "system prompt",
            "path": "src"
        }));

        assert_eq!(normalized["action"], "grep");
        assert_eq!(normalized["pattern"], "system prompt");
        assert_eq!(normalized["path"], "src");
    }

    #[test]
    fn normalize_unified_search_args_maps_keyword_to_name_pattern_for_list() {
        let normalized = normalize_unified_search_args(&json!({
            "action": "list",
            "keyword": "agent",
            "path": "vtcode-core/src",
            "mode": "file"
        }));

        assert_eq!(normalized["action"], "list");
        assert_eq!(normalized["name_pattern"], "agent");
        assert_eq!(normalized["path"], "vtcode-core/src");
        assert_eq!(normalized["mode"], "file");
    }

    #[test]
    fn normalize_unified_search_args_maps_query_to_pattern_for_grep() {
        let normalized = normalize_unified_search_args(&json!({
            "action": "grep",
            "query": "Result<",
            "path": "vtcode-core/src"
        }));

        assert_eq!(normalized["action"], "grep");
        assert_eq!(normalized["pattern"], "Result<");
        assert_eq!(normalized["path"], "vtcode-core/src");
    }

    #[test]
    fn normalize_unified_search_args_removes_invalid_format_for_grep() {
        // "content" is not a valid format value for grep action.
        // Valid values are: "files_with_matches", "count".
        // The normalization should remove invalid values to prevent
        // schema validation failures and blocked tool call loops.
        let normalized = normalize_unified_search_args(&json!({
            "action": "grep",
            "pattern": "doctor",
            "path": ".",
            "format": "content"
        }));

        assert_eq!(normalized["action"], "grep");
        assert_eq!(normalized["pattern"], "doctor");
        assert!(!normalized.as_object().unwrap().contains_key("format"));
    }

    #[test]
    fn normalize_unified_search_args_keeps_valid_format_for_grep() {
        // "files_with_matches" is a valid format value for grep action.
        let normalized = normalize_unified_search_args(&json!({
            "action": "grep",
            "pattern": "doctor",
            "path": ".",
            "format": "files_with_matches"
        }));

        assert_eq!(normalized["action"], "grep");
        assert_eq!(normalized["format"], "files_with_matches");
    }

    #[test]
    fn normalize_unified_search_args_maps_match_to_pattern_for_grep() {
        // "match" is only valid for outline action, not grep.
        // The normalization should map it to "pattern" for grep.
        let normalized = normalize_unified_search_args(&json!({
            "action": "grep",
            "match": "doctor",
            "path": "."
        }));

        assert_eq!(normalized["action"], "grep");
        assert_eq!(normalized["pattern"], "doctor");
        assert!(!normalized.as_object().unwrap().contains_key("match"));
    }

    #[test]
    fn legacy_search_aliases_are_readonly() {
        let grep_intent =
            classify_tool_intent(tools::GREP_FILE, &json!({"pattern": "needle", "path": "."}));
        assert!(!grep_intent.mutating);
        assert!(!grep_intent.destructive);

        let list_intent = classify_tool_intent(tools::LIST_FILES, &json!({"path": "."}));
        assert!(!list_intent.mutating);
        assert!(!list_intent.destructive);
    }

    #[test]
    fn canonical_unified_exec_tool_name_normalizes_exec_aliases() {
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
                canonical_unified_exec_tool_name(alias),
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
    fn spool_reference_only_detects_unified_search_payloads() {
        assert!(should_use_spool_reference_only(
            Some(tools::UNIFIED_SEARCH),
            &json!({
                "spool_path": ".vtcode/context/tool_outputs/unified_search_1.txt",
                "matches": []
            })
        ));
    }

    #[test]
    fn spool_reference_only_detects_unified_search_payload_without_tool_name() {
        assert!(should_use_spool_reference_only(
            None,
            &json!({
                "spool_path": ".vtcode/context/tool_outputs/unified_search_1.txt",
                "matches": []
            })
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

    // ── normalize_unified_search_args: list-action cross-mapping ──────────
    //
    // The unified_search schema defines `max_results`, `globs`, and `format`,
    // but ListInput only knows `max_items`/`per_page`/`glob_pattern`/`mode`.
    // Without cross-mapping these fields are silently dropped and the agent
    // loops (checkpoint turn_606: 37-message loop on "what's in this repo?").

    #[test]
    fn normalize_list_maps_max_results_to_per_page_and_max_items() {
        let result = normalize_unified_search_args(&json!({
            "action": "list",
            "path": ".",
            "max_results": 1000
        }));
        assert_eq!(
            result["per_page"], 1000,
            "max_results should map to per_page"
        );
        assert_eq!(
            result["max_items"], 1000,
            "max_results should map to max_items"
        );
    }

    #[test]
    fn normalize_list_preserves_explicit_per_page_over_max_results() {
        let result = normalize_unified_search_args(&json!({
            "action": "list",
            "path": ".",
            "max_results": 1000,
            "per_page": 50
        }));
        assert_eq!(
            result["per_page"], 50,
            "explicit per_page should not be overridden by max_results"
        );
        assert_eq!(
            result["max_items"], 1000,
            "max_items should still be set from max_results"
        );
    }

    #[test]
    fn normalize_list_maps_globs_string_to_pattern() {
        let result = normalize_unified_search_args(&json!({
            "action": "list",
            "path": ".",
            "globs": "**/*.rs"
        }));
        assert_eq!(
            result["pattern"], "**/*.rs",
            "globs string should map to pattern (serde alias for glob_pattern)"
        );
    }

    #[test]
    fn normalize_list_maps_globs_array_first_element_to_pattern() {
        let result = normalize_unified_search_args(&json!({
            "action": "list",
            "path": ".",
            "globs": ["**/*.rs", "**/*.toml"]
        }));
        assert_eq!(
            result["pattern"], "**/*.rs",
            "globs array first element should map to pattern"
        );
    }

    #[test]
    fn normalize_list_preserves_explicit_pattern_over_globs() {
        let result = normalize_unified_search_args(&json!({
            "action": "list",
            "path": ".",
            "pattern": "src/**/*.rs",
            "globs": "**/*.toml"
        }));
        assert_eq!(
            result["pattern"], "src/**/*.rs",
            "explicit pattern should not be overridden by globs"
        );
    }

    #[test]
    fn normalize_list_maps_format_tree_to_mode() {
        let result = normalize_unified_search_args(&json!({
            "action": "list",
            "path": ".",
            "format": "tree"
        }));
        assert_eq!(
            result["mode"], "tree",
            "format:tree should map to mode:tree (valid list mode)"
        );
    }

    #[test]
    fn normalize_list_maps_format_recursive_to_mode() {
        let result = normalize_unified_search_args(&json!({
            "action": "list",
            "path": ".",
            "format": "recursive"
        }));
        assert_eq!(
            result["mode"], "recursive",
            "format:recursive should map to mode:recursive"
        );
    }

    #[test]
    fn normalize_list_does_not_map_format_github_to_mode() {
        let result = normalize_unified_search_args(&json!({
            "action": "list",
            "path": ".",
            "format": "github"
        }));
        assert!(
            result.get("mode").is_none(),
            "format:github is a scan-only enum value, not a list mode"
        );
    }

    #[test]
    fn normalize_list_preserves_explicit_mode_over_format() {
        let result = normalize_unified_search_args(&json!({
            "action": "list",
            "path": ".",
            "mode": "largest",
            "format": "tree"
        }));
        assert_eq!(
            result["mode"], "largest",
            "explicit mode should not be overridden by format"
        );
    }

    #[test]
    fn normalize_list_full_cross_mapping() {
        // Reproduces the exact args pattern from checkpoint turn_606 that
        // caused the 37-message loop: all three dropped fields in one call.
        let result = normalize_unified_search_args(&json!({
            "action": "list",
            "path": "/Users/example/vtcode",
            "globs": "**/*",
            "max_results": 1000,
            "format": "tree"
        }));
        assert_eq!(result["pattern"], "**/*");
        assert_eq!(result["per_page"], 1000);
        assert_eq!(result["max_items"], 1000);
        assert_eq!(result["mode"], "tree");
    }

    #[test]
    fn normalize_non_list_action_does_not_cross_map() {
        let result = normalize_unified_search_args(&json!({
            "action": "grep",
            "pattern": "TODO",
            "max_results": 100
        }));
        assert!(
            result.get("per_page").is_none(),
            "max_results should not map to per_page for non-list actions"
        );
        assert!(
            result.get("max_items").is_none(),
            "max_results should not map to max_items for non-list actions"
        );
    }

    // ── normalize_unified_search_args: outline pattern→path mapping ──────

    #[test]
    fn normalize_outline_maps_pattern_to_path() {
        let result = normalize_unified_search_args(&json!({
            "action": "outline",
            "pattern": "vtcode-core/src/tools/registry.rs"
        }));
        assert_eq!(
            result["path"], "vtcode-core/src/tools/registry.rs",
            "outline should map pattern to path when path is absent"
        );
    }

    #[test]
    fn normalize_outline_preserves_explicit_path_over_pattern() {
        let result = normalize_unified_search_args(&json!({
            "action": "outline",
            "path": "src/main.rs",
            "pattern": "wrong/path.rs"
        }));
        assert_eq!(
            result["path"], "src/main.rs",
            "explicit path should not be overridden by pattern"
        );
    }

    #[test]
    fn normalize_outline_without_pattern_or_path_defaults() {
        // No pattern, no path → outline will default to "." in from_args
        let result = normalize_unified_search_args(&json!({
            "action": "outline",
            "view": "names"
        }));
        assert!(
            result.get("path").is_none(),
            "no path should be injected when neither pattern nor path is present"
        );
    }
}
