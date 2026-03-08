use serde_json::Value;

use crate::config::constants::tools;
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

    pub fn classify(self, args: &Value) -> ToolIntent {
        self.mutation_model.classify(args)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToolIntent {
    pub mutating: bool,
    pub destructive: bool,
    pub readonly_unified_action: bool,
    pub retry_safe: bool,
}

impl ToolIntent {
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

pub fn builtin_tool_behavior(tool_name: &str) -> Option<ToolBehavior> {
    let canonical = canonical_tool_name(tool_name);
    builtin_tool_behavior_canonical(canonical.as_ref())
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
        | tools::ENTER_PLAN_MODE
        | tools::EXIT_PLAN_MODE
        | tools::LIST_SKILLS
        | tools::LOAD_SKILL
        | tools::LOAD_SKILL_RESOURCE
        | tools::TASK_TRACKER
        | tools::PLAN_TASK_TRACKER
        | tools::GET_ERRORS
        | tools::SEARCH_TOOLS
        | tools::THINK => Some(ToolBehavior::function(
            ToolMutationModel::ReadOnly,
            false,
            false,
        )),
        tools::READ_FILE | tools::GREP_FILE | tools::LIST_FILES => Some(ToolBehavior::function(
            ToolMutationModel::ReadOnly,
            true,
            false,
        )),
        tools::WRITE_FILE | tools::EDIT_FILE | tools::DELETE_FILE | tools::CREATE_FILE => Some(
            ToolBehavior::function(ToolMutationModel::Mutating, false, true),
        ),
        tools::RUN_PTY_CMD
        | tools::SEND_PTY_INPUT
        | tools::CREATE_PTY_SESSION
        | tools::READ_PTY_SESSION
        | tools::LIST_PTY_SESSIONS
        | tools::CLOSE_PTY_SESSION
        | tools::EXECUTE_CODE
        | "shell" => Some(ToolBehavior::function(
            ToolMutationModel::Mutating,
            false,
            true,
        )),
        _ => None,
    }
}

pub fn is_parallel_safe_call(tool_name: &str, args: &Value) -> bool {
    let canonical = canonical_tool_name(tool_name);
    if let Some(behavior) = builtin_tool_behavior_canonical(canonical.as_ref()) {
        return behavior.supports_parallel_calls && !behavior.classify(args).mutating;
    }

    !classify_tool_intent(canonical.as_ref(), args).mutating
}

pub fn classify_tool_intent(tool_name: &str, args: &Value) -> ToolIntent {
    let canonical = canonical_tool_name(tool_name);
    builtin_tool_behavior_canonical(canonical.as_ref())
        .map(|behavior| behavior.classify(args))
        .unwrap_or_else(ToolIntent::mutating)
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

pub fn is_command_run_tool_call(tool_name: &str, args: &Value) -> bool {
    match tool_name {
        tools::RUN_PTY_CMD | tools::CREATE_PTY_SESSION | tools::SHELL | "bash" => true,
        tools::UNIFIED_EXEC
        | tools::EXEC_PTY_CMD
        | tools::EXEC_COMMAND
        | "exec"
        | "container.exec" => unified_exec_action(args)
            .map(|action| action.eq_ignore_ascii_case("run"))
            .unwrap_or(false),
        _ => false,
    }
}

fn unified_file_intent(args: &Value) -> ToolIntent {
    let readonly_unified_action = unified_file_action(args)
        .map(|action| action.eq_ignore_ascii_case("read"))
        .unwrap_or(false);

    if readonly_unified_action {
        ToolIntent::read_only_unified_action()
    } else {
        ToolIntent::mutating()
    }
}

fn unified_exec_intent(args: &Value) -> ToolIntent {
    let has_exec_input = unified_exec_has_input(args);
    let readonly_unified_action = unified_exec_action(args)
        .map(|action| {
            action.eq_ignore_ascii_case("poll")
                || action.eq_ignore_ascii_case("list")
                || action.eq_ignore_ascii_case("inspect")
                || (action.eq_ignore_ascii_case("continue") && !has_exec_input)
        })
        .unwrap_or(false);

    if readonly_unified_action {
        ToolIntent::read_only_unified_action()
    } else {
        ToolIntent::mutating()
    }
}

/// Determine the action for unified_file tool based on args.
/// Returns the action string or a default if inference is possible.
pub fn unified_file_action(args: &Value) -> Option<&str> {
    fn looks_like_patch_text(text: &str) -> bool {
        let trimmed = text.trim_start();
        trimmed.starts_with("*** Begin Patch")
            || trimmed.starts_with("*** Update File:")
            || trimmed.starts_with("*** Add File:")
            || trimmed.starts_with("*** Delete File:")
    }

    args.get("action").and_then(|v| v.as_str()).or_else(|| {
        let has_read_path = args.get("path").is_some()
            || args.get("file_path").is_some()
            || args.get("filepath").is_some()
            || args.get("target_path").is_some();
        let patch_in_input = args
            .get("input")
            .and_then(|v| v.as_str())
            .is_some_and(looks_like_patch_text);
        let raw_patch = args.as_str().is_some_and(looks_like_patch_text);

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
        } else if args.get("session_id").is_some() {
            Some("poll")
        } else {
            None
        }
    })
}

fn unified_exec_has_input(args: &Value) -> bool {
    args.get("input").is_some() || args.get("chars").is_some() || args.get("text").is_some()
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

fn unified_search_action_from_object(args: &serde_json::Map<String, Value>) -> Option<&str> {
    get_field_case_insensitive(args, "action")
        .and_then(|value| value.as_str())
        .or_else(|| {
            // Smart action inference based on parameters
            if get_field_case_insensitive(args, "pattern").is_some()
                || get_field_case_insensitive(args, "query").is_some()
            {
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
        } else {
            key
        };
        normalized
            .entry(canonical.to_string())
            .or_insert_with(|| value.clone());
    }

    if !normalized.contains_key("action")
        && let Some(inferred_action) = unified_search_action_from_object(&normalized)
    {
        normalized.insert(
            "action".to_string(),
            Value::String(inferred_action.to_string()),
        );
    }

    Value::Object(normalized)
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_unified_exec_tool_name, classify_tool_intent, is_command_run_tool_call,
        is_parallel_safe_call, normalize_unified_search_args, unified_file_action,
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
    fn unified_exec_run_is_mutating_and_destructive() {
        let intent = classify_tool_intent(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "ls"}),
        );
        assert!(intent.mutating);
        assert!(intent.destructive);
        assert!(!intent.retry_safe);
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
        let intent = classify_tool_intent(tools::UNIFIED_EXEC, &json!({"cmd": "ls -la"}));
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
