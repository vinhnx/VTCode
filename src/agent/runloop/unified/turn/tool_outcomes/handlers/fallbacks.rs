use anyhow::Error;
use serde_json::{Value, json};
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::tool_intent;

use crate::agent::runloop::unified::request_user_input::normalize_request_user_input_fallback_args;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use crate::agent::runloop::unified::turn::tool_outcomes::execution_result::compact_model_tool_payload;
use crate::agent::runloop::unified::turn::tool_outcomes::handlers::PreparedToolCall;

fn trimmed_non_empty_string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn public_exec_fallback(command: String) -> (String, Value) {
    (
        tool_names::EXEC_COMMAND.to_string(),
        json!({
            "cmd": command
        }),
    )
}

fn public_list_fallback(path: &str) -> (String, Value) {
    public_exec_fallback(format!(
        "find {} -maxdepth 1 -mindepth 1 -print",
        shell_single_quote(path)
    ))
}

fn public_grep_fallback(args: &Value) -> Option<(String, Value)> {
    let pattern = trimmed_non_empty_string_field(args, "pattern")
        .or_else(|| trimmed_non_empty_string_field(args, "keyword"))
        .or_else(|| trimmed_non_empty_string_field(args, "query"))?;
    let path = trimmed_non_empty_string_field(args, "path").unwrap_or_else(|| ".".to_string());
    Some(public_exec_fallback(format!(
        "rg --line-number --column --color=never {} {}",
        shell_single_quote(&pattern),
        shell_single_quote(&path)
    )))
}

fn public_search_fallback_for_args(args: Value) -> Option<(String, Value)> {
    match tool_intent::search_dispatch_action(&args).unwrap_or("grep") {
        action if action.eq_ignore_ascii_case("list") => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
            Some(public_list_fallback(path))
        }
        action if action.eq_ignore_ascii_case("grep") => public_grep_fallback(&args),
        action
            if action.eq_ignore_ascii_case("structural")
                || action.eq_ignore_ascii_case("outline") =>
        {
            Some((tool_names::CODE_SEARCH.to_string(), args))
        }
        _ => None,
    }
}

pub(super) fn recovery_fallback_for_tool(tool_name: &str, args: &Value) -> Option<(String, Value)> {
    match tool_name {
        tool_names::UNIFIED_SEARCH => {
            let normalized = tool_intent::normalize_search_dispatch_args(args);
            let action = tool_intent::search_dispatch_action(&normalized).unwrap_or("grep");
            if action.eq_ignore_ascii_case("list") {
                let path = normalized
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                Some(public_list_fallback(path))
            } else if action.eq_ignore_ascii_case("structural")
                || action.eq_ignore_ascii_case("outline")
            {
                Some((tool_names::CODE_SEARCH.to_string(), normalized))
            } else {
                None
            }
        }
        "list files" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            Some(public_list_fallback(path))
        }
        "search text" => None,
        tool_names::READ_FILE | "read file" | "repo_browser.read_file" => {
            let parent_path = args
                .get("path")
                .and_then(|v| v.as_str())
                .and_then(|path| std::path::Path::new(path).parent())
                .and_then(|path| path.to_str())
                .unwrap_or(".");
            Some(public_list_fallback(parent_path))
        }
        tool_names::UNIFIED_FILE => {
            let action = tool_intent::file_operation_action(args).unwrap_or("read");
            if action.eq_ignore_ascii_case("write") {
                file_operation_write_fallback(args)
            } else {
                let path = trimmed_non_empty_string_field(args, "path")
                    .or_else(|| trimmed_non_empty_string_field(args, "file_path"))
                    .or_else(|| trimmed_non_empty_string_field(args, "filepath"))
                    .or_else(|| trimmed_non_empty_string_field(args, "target_path"))
                    .or_else(|| trimmed_non_empty_string_field(args, "file"))
                    .or_else(|| trimmed_non_empty_string_field(args, "p"))
                    .unwrap_or_else(|| ".".to_string());
                Some(public_list_fallback(&path))
            }
        }
        _ => Some(public_list_fallback(".")),
    }
}

/// Build an `exec_command` fallback for a `file_operation` write that failed
/// (e.g. content exceeded the safe write limit or JSON arguments were corrupted).
///
/// Uses `cat` with a heredoc to write the file content via shell.
fn file_operation_write_fallback(args: &Value) -> Option<(String, Value)> {
    let path = args
        .get("path")
        .or_else(|| args.get("file_path"))
        .or_else(|| args.get("filepath"))
        .or_else(|| args.get("target_path"))
        .or_else(|| args.get("file"))
        .or_else(|| args.get("p"))
        .and_then(Value::as_str)?;

    let content = args.get("content").and_then(Value::as_str)?;

    let delimiter = unique_heredoc_delimiter(content)?;
    let command = format!(
        "cat > {} << '{delimiter}'\n{content}\n{delimiter}",
        shell_single_quote(path)
    );
    Some(public_exec_fallback(command))
}

/// Pick a heredoc delimiter that does not appear as a standalone line in `content`.
/// Returns `None` if all candidates collide (extremely unlikely).
fn unique_heredoc_delimiter(content: &str) -> Option<&'static str> {
    const CANDIDATES: &[&str] = &[
        "__VT_WRITE_EOF__",
        "__VT_WRITE_EOF_2__",
        "__VT_WRITE_EOF_3__",
    ];
    CANDIDATES
        .iter()
        .find(|d| !content.lines().any(|line| line == **d))
        .copied()
}

pub(super) fn build_validation_error_content_with_fallback(
    error: String,
    validation_stage: &'static str,
    fallback_tool: Option<String>,
    fallback_tool_args: Option<Value>,
) -> String {
    let is_recoverable = fallback_tool.is_some();
    let loop_detected = validation_stage == "loop_detection";
    let next_action = if is_recoverable {
        "Retry with fallback_tool_args."
    } else {
        "Fix tool arguments to match the schema."
    };
    let mut payload = serde_json::json!({
        "error": error,
        "failure_kind": "validation",
        "error_class": "invalid_arguments",
        "validation_stage": validation_stage,
        "retryable": false,
        "is_recoverable": is_recoverable,
        "loop_detected": loop_detected,
        "next_action": next_action,
    });
    if let Some(obj) = payload.as_object_mut() {
        if let Some(tool) = fallback_tool {
            obj.insert("fallback_tool".to_string(), Value::String(tool));
        }
        if let Some(args) = fallback_tool_args {
            obj.insert("fallback_tool_args".to_string(), args);
        }
    }
    compact_model_tool_payload(payload).to_string()
}

pub(super) fn preflight_validation_fallback(
    tool_name: &str,
    args_val: &Value,
    error: &Error,
) -> Option<(String, Value)> {
    let error_text = error.to_string();
    let is_request_user_input = tool_name == tool_names::REQUEST_USER_INPUT
        || error_text.contains("tool 'request_user_input'")
        || error_text.contains("for 'request_user_input'");
    if is_request_user_input {
        let normalized = normalize_request_user_input_fallback_args(args_val)?;
        if normalized == *args_val {
            return None;
        }
        return Some((tool_names::REQUEST_USER_INPUT.to_string(), normalized));
    }

    let is_search_dispatch = tool_name == tool_names::UNIFIED_SEARCH
        || error_text.contains("tool 'search_dispatch'")
        || error_text.contains("for 'search_dispatch'");
    if is_search_dispatch {
        let mut normalized = tool_intent::normalize_search_dispatch_args(args_val);
        let inferred_pattern = trimmed_non_empty_string_field(&normalized, "pattern")
            .or_else(|| trimmed_non_empty_string_field(&normalized, "keyword"))
            .or_else(|| trimmed_non_empty_string_field(&normalized, "query"));

        if let Some(obj) = normalized.as_object_mut() {
            let action = obj
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if action.eq_ignore_ascii_case("read") {
                if inferred_pattern.is_some() {
                    obj.insert("action".to_string(), Value::String("grep".to_string()));
                } else {
                    obj.insert("action".to_string(), Value::String("list".to_string()));
                }
            }

            if obj.get("action").and_then(Value::as_str) == Some("grep")
                && obj.get("pattern").is_none()
                && let Some(pattern) = inferred_pattern
            {
                obj.insert("pattern".to_string(), Value::String(pattern));
            }
        }

        if normalized != *args_val && normalized.get("action").is_some() {
            return public_search_fallback_for_args(normalized);
        }
    }

    let is_file_operation = tool_name == tool_names::UNIFIED_FILE
        || error_text.contains("tool 'file_operation'")
        || error_text.contains("for 'file_operation'");
    if !is_file_operation {
        return None;
    }

    if tool_intent::file_operation_action_is(args_val, "list") {
        let path =
            trimmed_non_empty_string_field(args_val, "path").unwrap_or_else(|| ".".to_string());
        return Some(public_list_fallback(&path));
    }

    tool_intent::remap_file_operation_command_args_to_command_session(args_val).and_then(|args| {
        let command = args
            .get("command")
            .and_then(Value::as_str)
            .map(str::to_string)?;
        let mut fallback_args = json!({ "cmd": command });
        if let Some(cwd) = args.get("cwd").and_then(Value::as_str)
            && let Some(obj) = fallback_args.as_object_mut()
        {
            obj.insert("workdir".to_string(), Value::String(cwd.to_string()));
        }
        Some((tool_names::EXEC_COMMAND.to_string(), fallback_args))
    })
}

pub(super) fn try_recover_preflight_with_fallback(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    args_val: &Value,
    error: &Error,
) -> Option<PreparedToolCall> {
    let (recovered_tool_name, recovered_args) =
        preflight_validation_fallback(tool_name, args_val, error)?;
    match ctx
        .tool_registry
        .admit_public_tool_call(&recovered_tool_name, &recovered_args)
    {
        Ok(prepared) => Some(prepared),
        Err(recovery_err) => {
            tracing::debug!(
                tool = tool_name,
                original_error = %error,
                recovered_tool = %recovered_tool_name,
                recovery_error = %recovery_err,
                "Preflight recovery fallback failed"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{preflight_validation_fallback, trimmed_non_empty_string_field};
    use anyhow::anyhow;
    use serde_json::json;
    use vtcode_core::config::constants::tools as tool_names;

    #[test]
    fn trimmed_non_empty_string_field_trims_and_rejects_blank() {
        let value = json!({
            "pattern": "  todo  ",
            "blank": "   "
        });

        assert_eq!(
            trimmed_non_empty_string_field(&value, "pattern"),
            Some("todo".to_string())
        );
        assert_eq!(trimmed_non_empty_string_field(&value, "blank"), None);
        assert_eq!(trimmed_non_empty_string_field(&value, "missing"), None);
    }

    #[test]
    fn preflight_validation_fallback_promotes_search_dispatch_keyword_to_pattern() {
        let args = json!({
            "action": "read",
            "path": "src",
            "keyword": "  todo  "
        });
        let error = anyhow!("tool 'search_dispatch' validation failed");

        let (tool_name, recovered_args) =
            preflight_validation_fallback(tool_names::UNIFIED_SEARCH, &args, &error)
                .expect("search_dispatch fallback should recover");

        assert_eq!(tool_name, tool_names::EXEC_COMMAND);
        assert!(
            recovered_args["cmd"]
                .as_str()
                .is_some_and(|cmd| cmd.contains("todo"))
        );
    }

    #[test]
    fn recovery_fallback_file_operation_write_returns_exec_command() {
        let args = json!({
            "action": "write",
            "path": "docs/output.md",
            "content": "# Large Document\n\nSome content here."
        });

        let (tool_name, fallback_args) =
            super::recovery_fallback_for_tool(tool_names::UNIFIED_FILE, &args)
                .expect("file_operation write should have fallback");

        assert_eq!(tool_name, tool_names::EXEC_COMMAND);
        let command = fallback_args["cmd"]
            .as_str()
            .expect("command should be a string");
        assert!(
            command.contains("'docs/output.md'"),
            "command should quote the path"
        );
        assert!(
            command.contains("# Large Document"),
            "command should include the content"
        );
        assert!(
            command.contains("__VT_WRITE_EOF__"),
            "command should use unique delimiter"
        );
    }

    #[test]
    fn recovery_fallback_file_operation_write_quotes_path_with_spaces() {
        let args = json!({
            "action": "write",
            "path": "my files/output.md",
            "content": "data"
        });

        let (tool_name, fallback_args) =
            super::recovery_fallback_for_tool(tool_names::UNIFIED_FILE, &args)
                .expect("file_operation write should have fallback");

        assert_eq!(tool_name, tool_names::EXEC_COMMAND);
        let command = fallback_args["cmd"].as_str().expect("command");
        assert!(
            command.contains("'my files/output.md'"),
            "path with spaces should be quoted"
        );
    }

    #[test]
    fn recovery_fallback_file_operation_write_infers_action_from_content() {
        let args = json!({
            "path": "output.txt",
            "content": "Hello, world!"
        });

        let (tool_name, fallback_args) =
            super::recovery_fallback_for_tool(tool_names::UNIFIED_FILE, &args)
                .expect("file_operation with content should have write fallback");

        assert_eq!(tool_name, tool_names::EXEC_COMMAND);
        assert!(fallback_args["cmd"].as_str().is_some());
    }

    #[test]
    fn recovery_fallback_file_operation_read_preserves_path() {
        let args = json!({
            "action": "read",
            "path": "src/main.rs"
        });

        let (tool_name, fallback_args) =
            super::recovery_fallback_for_tool(tool_names::UNIFIED_FILE, &args)
                .expect("file_operation read should have fallback");

        assert_eq!(tool_name, tool_names::EXEC_COMMAND);
        assert!(
            fallback_args["cmd"]
                .as_str()
                .is_some_and(|cmd| cmd.contains("'src/main.rs'"))
        );
    }

    #[test]
    fn unique_heredoc_delimiter_avoids_content_collision() {
        let content = "line 1\n__VT_WRITE_EOF__\nline 3";
        let delimiter = super::unique_heredoc_delimiter(content)
            .expect("should find a non-colliding delimiter");
        assert_ne!(delimiter, "__VT_WRITE_EOF__");
        assert!(!content.lines().any(|l| l == delimiter));
    }

    #[test]
    fn unique_heredoc_delimiter_returns_none_when_all_collide() {
        let content = "__VT_WRITE_EOF__\n__VT_WRITE_EOF_2__\n__VT_WRITE_EOF_3__";
        assert!(super::unique_heredoc_delimiter(content).is_none());
    }
}
