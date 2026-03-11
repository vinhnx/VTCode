use anyhow::Error;
use serde_json::{Value, json};
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::registry::ToolPreflightOutcome;
use vtcode_core::tools::tool_intent;

use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

pub(super) fn recovery_fallback_for_tool(tool_name: &str, args: &Value) -> Option<(String, Value)> {
    match tool_name {
        tool_names::UNIFIED_SEARCH | "list files" | "search text" => Some((
            tool_names::UNIFIED_SEARCH.to_string(),
            json!({
                "action": "list",
                "path": args.get("path").and_then(|v| v.as_str()).unwrap_or(".")
            }),
        )),
        tool_names::READ_FILE | "read file" | "repo_browser.read_file" => {
            let parent_path = args
                .get("path")
                .and_then(|v| v.as_str())
                .and_then(|path| std::path::Path::new(path).parent())
                .and_then(|path| path.to_str())
                .unwrap_or(".");
            Some((
                tool_names::UNIFIED_SEARCH.to_string(),
                json!({
                    "action": "list",
                    "path": parent_path
                }),
            ))
        }
        _ => Some((
            tool_names::UNIFIED_SEARCH.to_string(),
            json!({
                "action": "list",
                "path": "."
            }),
        )),
    }
}

pub(super) fn build_validation_error_content_with_fallback(
    error: String,
    validation_stage: &'static str,
    fallback_tool: Option<String>,
    fallback_tool_args: Option<Value>,
) -> String {
    let is_recoverable = fallback_tool.is_some();
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
    payload.to_string()
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
        let normalized = request_user_input_preflight_fallback_args(args_val)?;
        if normalized == *args_val {
            return None;
        }
        return Some((tool_names::REQUEST_USER_INPUT.to_string(), normalized));
    }

    let is_unified_search = tool_name == tool_names::UNIFIED_SEARCH
        || error_text.contains("tool 'unified_search'")
        || error_text.contains("for 'unified_search'");
    if is_unified_search {
        let mut normalized = tool_intent::normalize_unified_search_args(args_val);
        let inferred_pattern = normalized
            .get("pattern")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                normalized
                    .get("keyword")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .or_else(|| {
                normalized
                    .get("query")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });

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
            return Some((tool_names::UNIFIED_SEARCH.to_string(), normalized));
        }
    }

    let is_unified_file = tool_name == tool_names::UNIFIED_FILE
        || error_text.contains("tool 'unified_file'")
        || error_text.contains("for 'unified_file'");
    if !is_unified_file {
        return None;
    }

    remap_unified_file_command_args_to_unified_exec(args_val)
        .map(|args| (tool_names::UNIFIED_EXEC.to_string(), args))
}

pub(super) fn remap_unified_file_command_args_to_unified_exec(args: &Value) -> Option<Value> {
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
        && !action.eq_ignore_ascii_case("run")
        && !action.eq_ignore_ascii_case("exec")
        && !action.eq_ignore_ascii_case("execute")
        && !action.eq_ignore_ascii_case("shell")
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

fn find_ci_field<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a Value> {
    obj.get(key).or_else(|| {
        obj.iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value)
    })
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn normalize_fallback_question_id(raw: Option<&str>, index: usize) -> String {
    let source = raw.unwrap_or_default();
    let mut out = String::new();
    let mut last_was_underscore = false;
    for ch in source.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_underscore = false;
        } else if !last_was_underscore {
            out.push('_');
            last_was_underscore = true;
        }
    }
    while out.starts_with('_') {
        out.remove(0);
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        return format!("question_{}", index + 1);
    }
    if !out
        .chars()
        .next()
        .map(|ch| ch.is_ascii_lowercase())
        .unwrap_or(false)
    {
        out.insert(0, 'q');
    }
    out
}

fn normalize_fallback_header(raw: Option<&str>, fallback: &str) -> String {
    let candidate = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    truncate_chars(candidate, 12)
}

fn normalize_fallback_option(value: &Value) -> Option<Value> {
    match value {
        Value::String(label) => {
            let label = label.trim();
            if label.is_empty() {
                return None;
            }
            Some(json!({
                "label": label,
                "description": "Select this option."
            }))
        }
        Value::Object(obj) => {
            let label = ["label", "title", "id"]
                .iter()
                .find_map(|key| find_ci_field(obj, key).and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())?;
            let description = ["description", "subtitle", "details"]
                .iter()
                .find_map(|key| find_ci_field(obj, key).and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("Select this option.");
            Some(json!({
                "label": label,
                "description": description
            }))
        }
        _ => None,
    }
}

fn normalize_fallback_options(value: &Value) -> Option<Vec<Value>> {
    let Value::Array(raw_options) = value else {
        return None;
    };
    let mut normalized = Vec::new();
    let mut seen_labels = std::collections::BTreeSet::new();
    for option in raw_options {
        let Some(normalized_option) = normalize_fallback_option(option) else {
            continue;
        };
        let label = normalized_option
            .get("label")
            .and_then(Value::as_str)
            .map(|value| value.to_ascii_lowercase());
        if let Some(label) = label
            && !seen_labels.insert(label)
        {
            continue;
        }
        normalized.push(normalized_option);
        if normalized.len() == 3 {
            break;
        }
    }
    if normalized.len() >= 2 {
        Some(normalized)
    } else {
        None
    }
}

fn normalize_request_user_input_question(
    obj: &serde_json::Map<String, Value>,
    index: usize,
) -> Option<serde_json::Map<String, Value>> {
    let question_text = ["question", "prompt", "text"]
        .iter()
        .find_map(|key| find_ci_field(obj, key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    let mut question = serde_json::Map::new();
    let question_id = ["id", "question_id", "name"]
        .iter()
        .find_map(|key| find_ci_field(obj, key).and_then(Value::as_str));
    question.insert(
        "id".to_string(),
        Value::String(normalize_fallback_question_id(question_id, index)),
    );
    let header_source = ["header", "title"]
        .iter()
        .find_map(|key| find_ci_field(obj, key).and_then(Value::as_str));
    question.insert(
        "header".to_string(),
        Value::String(normalize_fallback_header(header_source, "Question")),
    );
    question.insert(
        "question".to_string(),
        Value::String(question_text.to_string()),
    );

    if let Some(options_value) =
        find_ci_field(obj, "options").or_else(|| find_ci_field(obj, "items"))
        && let Some(options) = normalize_fallback_options(options_value)
    {
        question.insert("options".to_string(), Value::Array(options));
    }

    Some(question)
}

fn request_user_input_preflight_fallback_args(args_val: &Value) -> Option<Value> {
    let single_text_question = args_val
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if let Some(question) = single_text_question {
        return Some(json!({
            "questions": [{
                "id": "question_1",
                "header": "Question",
                "question": question
            }]
        }));
    }

    let args_obj = args_val.as_object()?;

    if let Some(questions_value) = find_ci_field(args_obj, "questions") {
        let mut normalized_questions = Vec::new();
        match questions_value {
            Value::Array(entries) => {
                for (index, entry) in entries.iter().enumerate() {
                    if let Some(obj) = entry.as_object()
                        && let Some(question) = normalize_request_user_input_question(obj, index)
                    {
                        normalized_questions.push(Value::Object(question));
                    }
                }
            }
            Value::Object(obj) => {
                if let Some(question) = normalize_request_user_input_question(obj, 0) {
                    normalized_questions.push(Value::Object(question));
                }
            }
            _ => {}
        }
        if !normalized_questions.is_empty() {
            return Some(json!({ "questions": normalized_questions }));
        }
    }

    if let Some(tabs_value) = find_ci_field(args_obj, "tabs")
        && let Some(first_tab) = tabs_value.as_array().and_then(|tabs| tabs.first())
        && let Some(tab_obj) = first_tab.as_object()
    {
        let question_text = find_ci_field(args_obj, "question")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                find_ci_field(tab_obj, "question")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or("What should we prioritize?");
        let question_id = find_ci_field(tab_obj, "id")
            .and_then(Value::as_str)
            .or_else(|| find_ci_field(args_obj, "id").and_then(Value::as_str));
        let header_source = find_ci_field(tab_obj, "title")
            .and_then(Value::as_str)
            .or_else(|| find_ci_field(args_obj, "header").and_then(Value::as_str));

        let mut question = serde_json::Map::new();
        question.insert(
            "id".to_string(),
            Value::String(normalize_fallback_question_id(question_id, 0)),
        );
        question.insert(
            "header".to_string(),
            Value::String(normalize_fallback_header(header_source, "Question")),
        );
        question.insert(
            "question".to_string(),
            Value::String(question_text.to_string()),
        );
        if let Some(items) = find_ci_field(tab_obj, "items")
            && let Some(options) = normalize_fallback_options(items)
        {
            question.insert("options".to_string(), Value::Array(options));
        }

        return Some(json!({ "questions": [Value::Object(question)] }));
    }

    if let Some(question_text) = find_ci_field(args_obj, "question")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let mut question = serde_json::Map::new();
        let question_id = find_ci_field(args_obj, "id").and_then(Value::as_str);
        let header_source = find_ci_field(args_obj, "header").and_then(Value::as_str);
        question.insert(
            "id".to_string(),
            Value::String(normalize_fallback_question_id(question_id, 0)),
        );
        question.insert(
            "header".to_string(),
            Value::String(normalize_fallback_header(header_source, "Question")),
        );
        question.insert(
            "question".to_string(),
            Value::String(question_text.to_string()),
        );
        if let Some(options_value) = find_ci_field(args_obj, "options")
            && let Some(options) = normalize_fallback_options(options_value)
        {
            question.insert("options".to_string(), Value::Array(options));
        }

        return Some(json!({ "questions": [Value::Object(question)] }));
    }

    None
}

pub(super) fn try_recover_preflight_with_fallback(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    args_val: &Value,
    error: &Error,
) -> Option<(String, ToolPreflightOutcome, Value)> {
    let (recovered_tool_name, recovered_args) =
        preflight_validation_fallback(tool_name, args_val, error)?;
    let preflight_result = ctx
        .tool_registry
        .preflight_validate_call(&recovered_tool_name, &recovered_args);
    match preflight_result {
        Ok(preflight) => Some((recovered_tool_name, preflight, recovered_args)),
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
