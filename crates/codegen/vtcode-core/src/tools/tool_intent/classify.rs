use serde_json::Value;

use crate::config::constants::tools;
use crate::tools::command_args::interactive_input_text;
use crate::tools::names::canonical_tool_name;
use crate::tools::tool_intent::actions::{
    action_matches_any, command_session_action_in, command_session_action_is, file_operation_action,
    file_operation_action_is,
};
use crate::tools::tool_intent::readonly::is_readonly_command_session_command;
use crate::tools::tool_intent::types::{ToolBehavior, ToolIntent, ToolMutationModel};

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

    if tool_name.is_some_and(|name| {
        name == tools::CODE_SEARCH
            || name == tools::UNIFIED_SEARCH
            || name == "code_search"
            || name == "search_dispatch_internal"
    }) {
        return true;
    }

    ["command", "id", "session_id", "process_id", "is_exited", "exit_code"]
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

/// Returns `true` if `tool_name` is a direct command-run tool whose output is
/// rendered inline as a terminal panel and whose raw process output (including
/// ANSI) should be preserved rather than stripped/line-styled.
///
/// This is the stable set shared by the TUI display gates (terminal-panel
/// dispatch, ANSI preservation, and command-text summary). New command-run
/// tools MUST be added here so the display stays in sync; the args-aware
/// [`is_command_run_tool_call`] covers call-site routing.
#[must_use]
pub fn is_command_run_tool(tool_name: &str) -> bool {
    matches!(tool_name, tools::RUN_PTY_CMD | tools::EXEC_COMMAND)
}

pub fn is_command_run_tool_call(tool_name: &str, args: &Value) -> bool {
    match tool_name {
        tools::RUN_PTY_CMD | tools::CREATE_PTY_SESSION | tools::SHELL | "bash" => true,
        tools::EXEC_COMMAND => crate::tools::command_args::command_text(args).ok().flatten().is_some(),
        tools::UNIFIED_EXEC | tools::EXEC_PTY_CMD | "exec" | "container.exec" => command_session_action_is(args, "run"),
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
        Ok(crate::tools::command_args::WriteStdinDispatch::Write) | Err(_) => ToolIntent::mutating(),
    }
}

fn memory_tool_intent(args: &Value) -> ToolIntent {
    let command = args.get("command").and_then(Value::as_str).map(str::trim).unwrap_or_default();
    if command.eq_ignore_ascii_case("view") {
        ToolIntent::read_only()
    } else {
        ToolIntent::mutating()
    }
}

fn builtin_tool_behavior_canonical(tool: &str) -> Option<ToolBehavior> {
    match tool {
        tools::CODE_SEARCH => Some(ToolBehavior::function(ToolMutationModel::ReadOnly, true, false)),
        tools::UNIFIED_EXEC => {
            Some(ToolBehavior::function(ToolMutationModel::ByArgs(command_session_intent), false, true))
        }
        tools::EXEC_COMMAND => {
            Some(ToolBehavior::function(ToolMutationModel::ByArgs(exec_command_intent), false, true))
        }
        tools::WRITE_STDIN => Some(ToolBehavior::function(ToolMutationModel::ByArgs(write_stdin_intent), false, true)),
        tools::UNIFIED_FILE => {
            Some(ToolBehavior::function(ToolMutationModel::ByArgs(file_operation_intent), false, false))
        }
        tools::APPLY_PATCH => Some(ToolBehavior::apply_patch(ToolMutationModel::Mutating, false, true)),
        tools::REQUEST_USER_INPUT
        | tools::MEMORY
        | tools::START_PLANNING
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
        tools::READ_FILE | tools::GREP_FILE | tools::LIST_FILES => {
            Some(ToolBehavior::function(ToolMutationModel::ReadOnly, true, false))
        }
        tools::WEB_FETCH | tools::FETCH_URL | tools::WEB_SEARCH | tools::DEFUDDLE_FETCH => {
            Some(ToolBehavior::function(ToolMutationModel::ReadOnly, false, false))
        }
        tools::WRITE_FILE | tools::EDIT_FILE | tools::DELETE_FILE | tools::CREATE_FILE => {
            Some(ToolBehavior::function(ToolMutationModel::Mutating, false, true))
        }
        tools::MCP_CONNECT_SERVER | tools::MCP_DISCONNECT_SERVER => {
            Some(ToolBehavior::function(ToolMutationModel::Mutating, false, false))
        }
        tools::RUN_PTY_CMD
        | tools::SEND_PTY_INPUT
        | tools::CREATE_PTY_SESSION
        | tools::READ_PTY_SESSION
        | tools::LIST_PTY_SESSIONS
        | tools::CLOSE_PTY_SESSION
        | tools::EXECUTE_CODE
        | tools::SHELL => Some(ToolBehavior::function(ToolMutationModel::Mutating, false, true)),
        _ => None,
    }
}

fn command_session_has_input(args: &Value) -> bool {
    interactive_input_text(args).is_some()
}
