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
            | tools::CODE_INTELLIGENCE
            | tools::UNIFIED_SEARCH
            | tools::AGENT_INFO
            | tools::ENTER_PLAN_MODE
            | tools::EXIT_PLAN_MODE
            | tools::ASK_USER_QUESTION
            | tools::REQUEST_USER_INPUT
            | tools::LIST_SKILLS
            | tools::LOAD_SKILL
            | tools::LOAD_SKILL_RESOURCE
            | tools::SPAWN_SUBAGENT
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
    args.get("action").and_then(|v| v.as_str()).or_else(|| {
        if args.get("old_str").is_some() {
            Some("edit")
        } else if args.get("patch").is_some() {
            Some("patch")
        } else if args.get("content").is_some() {
            Some("write")
        } else if args.get("destination").is_some() {
            Some("move")
        } else {
            Some("read")
        }
    })
}

/// Determine the action for unified_exec tool based on args.
/// Returns the action string or None if no inference is possible.
pub fn unified_exec_action(args: &Value) -> Option<&str> {
    args.get("action").and_then(|v| v.as_str()).or_else(|| {
        if args.get("command").is_some() {
            Some("run")
        } else if args.get("code").is_some() {
            Some("code")
        } else if args.get("input").is_some() {
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
    use super::classify_tool_intent;
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
}
