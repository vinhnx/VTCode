use serde_json::Value;

use crate::config::constants::tools;
use crate::tools::names::canonical_tool_name;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToolIntent {
    pub mutating: bool,
    pub destructive: bool,
    pub readonly_unified_action: bool,
    pub retry_safe: bool,
}

pub fn classify_tool_intent(tool_name: &str, args: &Value) -> ToolIntent {
    let canonical = canonical_tool_name(tool_name);
    let tool = canonical.as_ref();

    let readonly_unified_action = match tool {
        tools::UNIFIED_FILE => unified_file_action(args)
            .map(|action| action.eq_ignore_ascii_case("read"))
            .unwrap_or(false),
        tools::UNIFIED_EXEC => unified_exec_action(args)
            .map(|action| {
                action.eq_ignore_ascii_case("poll") || action.eq_ignore_ascii_case("list")
            })
            .unwrap_or(false),
        tools::UNIFIED_SEARCH => true,
        _ => false,
    };

    let mutating = if readonly_unified_action {
        false
    } else {
        match tool {
            tools::READ_FILE
            | tools::LIST_FILES
            | tools::GREP_FILE
            | tools::UNIFIED_SEARCH
            | tools::AGENT_INFO
            | tools::ENTER_PLAN_MODE
            | tools::EXIT_PLAN_MODE
            | tools::REQUEST_USER_INPUT
            | tools::LIST_SKILLS
            | tools::LOAD_SKILL
            | tools::LOAD_SKILL_RESOURCE
            | tools::SPAWN_SUBAGENT
            | tools::TASK_TRACKER
            | tools::PLAN_TASK_TRACKER
            | "get_errors"
            | "search_tools"
            | "think" => false,
            tools::UNIFIED_FILE | tools::UNIFIED_EXEC => true,
            _ => true,
        }
    };

    let destructive = match tool {
        tools::DELETE_FILE
        | tools::WRITE_FILE
        | tools::EDIT_FILE
        | tools::APPLY_PATCH
        | tools::RUN_PTY_CMD
        | tools::SHELL
        | tools::SEND_PTY_INPUT
        | tools::CREATE_PTY_SESSION
        | tools::EXECUTE_CODE => true,
        tools::UNIFIED_FILE => !readonly_unified_action,
        tools::UNIFIED_EXEC => unified_exec_action(args)
            .map(|action| {
                action.eq_ignore_ascii_case("run")
                    || action.eq_ignore_ascii_case("write")
                    || action.eq_ignore_ascii_case("code")
            })
            .unwrap_or(mutating),
        _ => mutating,
    };

    let retry_safe = !mutating || readonly_unified_action;

    ToolIntent {
        mutating,
        destructive,
        readonly_unified_action,
        retry_safe,
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
        if args.get("command").is_some()
            || args.get("cmd").is_some()
            || args.get("raw_command").is_some()
        {
            Some("run")
        } else if args.get("code").is_some() {
            Some("code")
        } else if args.get("input").is_some()
            || args.get("chars").is_some()
            || args.get("text").is_some()
        {
            Some("write")
        } else if args.get("session_id").is_some() {
            Some("poll")
        } else {
            None
        }
    })
}

/// Determine the action for unified_search tool based on args.
/// Returns the action string or None if no inference is possible.
pub fn unified_search_action(args: &Value) -> Option<&str> {
    args.get("action").and_then(|v| v.as_str()).or_else(|| {
        // Smart action inference based on parameters
        if args.get("pattern").is_some() || args.get("query").is_some() {
            Some("grep")
        } else if args.get("keyword").is_some() {
            Some("tools")
        } else if args.get("operation").is_some() {
            Some("intelligence")
        } else if args.get("url").is_some() {
            Some("web")
        } else if args.get("sub_action").is_some() || args.get("name").is_some() {
            Some("skill")
        } else if args.get("scope").is_some() {
            Some("errors")
        } else if args.get("path").is_some() {
            Some("list")
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{classify_tool_intent, unified_file_action};
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
}
