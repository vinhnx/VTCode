//! Shared helpers for command-style tool arguments.

use serde_json::Value;

const INDEXED_COMMAND_TYPE_ERROR: &str = "command array must contain only strings";
const COMMAND_VALUE_TYPE_ERROR: &str = "command must be a string or array of strings";

fn collect_indexed_command_parts(
    payload: &serde_json::Map<String, Value>,
    start_index: usize,
) -> Result<Vec<String>, &'static str> {
    let mut parts = Vec::new();
    let mut index = start_index;
    while let Some(value) = payload.get(&format!("command.{}", index)) {
        let Some(part) = value.as_str() else {
            return Err(INDEXED_COMMAND_TYPE_ERROR);
        };
        parts.push(part.to_string());
        index += 1;
    }
    Ok(parts)
}

pub fn has_indexed_command_parts(args: &Value) -> bool {
    let Some(payload) = args.as_object() else {
        return false;
    };

    payload.contains_key("command.0") || payload.contains_key("command.1")
}

pub fn parse_indexed_command_parts(
    payload: &serde_json::Map<String, Value>,
) -> Result<Option<Vec<String>>, &'static str> {
    let zero_based = collect_indexed_command_parts(payload, 0)?;
    if !zero_based.is_empty() {
        return Ok(Some(zero_based));
    }

    let one_based = collect_indexed_command_parts(payload, 1)?;
    if one_based.is_empty() {
        Ok(None)
    } else {
        Ok(Some(one_based))
    }
}

pub fn normalize_indexed_command_args(args: &Value) -> Result<Option<Value>, &'static str> {
    let Some(payload) = args.as_object() else {
        return Ok(None);
    };
    if payload.get("command").is_some() {
        return Ok(None);
    }

    let Some(parts) = parse_indexed_command_parts(payload)? else {
        return Ok(None);
    };

    let mut normalized = payload.clone();
    normalized.insert(
        "command".to_string(),
        Value::String(shell_words::join(parts.iter().map(String::as_str))),
    );
    Ok(Some(Value::Object(normalized)))
}

pub fn normalized_command_value(args: &Value) -> Result<Option<Value>, &'static str> {
    if let Some(command) = args
        .get("command")
        .or_else(|| args.get("cmd"))
        .or_else(|| args.get("raw_command"))
    {
        return Ok(Some(command.clone()));
    }

    Ok(normalize_indexed_command_args(args)?
        .and_then(|normalized| normalized.get("command").cloned()))
}

pub fn command_words(args: &Value) -> Result<Option<Vec<String>>, &'static str> {
    let Some(command) = normalized_command_value(args)? else {
        return Ok(None);
    };

    let mut parts = match command {
        Value::String(command) => {
            shell_words::split(&command).map_err(|_| COMMAND_VALUE_TYPE_ERROR)?
        }
        Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .ok_or(COMMAND_VALUE_TYPE_ERROR)
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(COMMAND_VALUE_TYPE_ERROR),
    };

    if let Some(extra_args) = args.get("args").and_then(Value::as_array) {
        for value in extra_args {
            let Some(part) = value.as_str() else {
                return Err(COMMAND_VALUE_TYPE_ERROR);
            };
            parts.push(part.to_string());
        }
    }

    if parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(parts))
    }
}

pub fn command_text(args: &Value) -> Result<Option<String>, &'static str> {
    let Some(parts) = command_words(args)? else {
        return Ok(None);
    };
    Ok(Some(shell_words::join(parts.iter().map(String::as_str))))
}

fn has_nonempty_string_field(args: &Value, key: &str) -> bool {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

pub fn interactive_input_text(args: &Value) -> Option<&str> {
    args.get("input")
        .and_then(Value::as_str)
        .or_else(|| args.get("chars").and_then(Value::as_str))
        .or_else(|| args.get("text").and_then(Value::as_str))
        .filter(|value| !value.is_empty())
}

pub fn session_id_text(args: &Value) -> Option<&str> {
    args.get("session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub fn unified_exec_missing_required_args(args: &Value) -> Vec<&'static str> {
    let Some(action) = crate::tools::tool_intent::unified_exec_action(args) else {
        return Vec::new();
    };

    let mut missing = Vec::new();
    match action {
        action if action.eq_ignore_ascii_case("run") => {
            if command_text(args).ok().flatten().is_none() {
                missing.push("command");
            }
        }
        action if action.eq_ignore_ascii_case("write") => {
            if session_id_text(args).is_none() {
                missing.push("session_id");
            }
            if interactive_input_text(args).is_none() {
                missing.push("input or chars or text");
            }
        }
        action
            if action.eq_ignore_ascii_case("poll")
                || action.eq_ignore_ascii_case("continue")
                || action.eq_ignore_ascii_case("close") =>
        {
            if session_id_text(args).is_none() {
                missing.push("session_id");
            }
        }
        action if action.eq_ignore_ascii_case("inspect") => {
            let has_session_id = session_id_text(args).is_some();
            let has_spool_path = has_nonempty_string_field(args, "spool_path");
            if !has_session_id && !has_spool_path {
                missing.push("session_id or spool_path");
            }
        }
        action if action.eq_ignore_ascii_case("code") => {
            let has_code = has_nonempty_string_field(args, "code")
                || has_nonempty_string_field(args, "command");
            if !has_code {
                missing.push("code or command");
            }
        }
        action if action.eq_ignore_ascii_case("list") => {}
        _ => {}
    }

    missing
}

pub fn unified_exec_requires_command_safety(args: &Value) -> bool {
    crate::tools::tool_intent::unified_exec_action(args)
        .map(|action| action.eq_ignore_ascii_case("run"))
        .unwrap_or(false)
}

pub fn working_dir_text_from_payload(payload: &serde_json::Map<String, Value>) -> Option<&str> {
    payload
        .get("working_dir")
        .or_else(|| payload.get("cwd"))
        .or_else(|| payload.get("workdir"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub fn working_dir_text(args: &Value) -> Option<&str> {
    args.as_object().and_then(working_dir_text_from_payload)
}

pub fn normalize_shell_args(args: &Value) -> Result<Value, &'static str> {
    let mut normalized = match normalize_indexed_command_args(args)? {
        Some(value) => value,
        None => args.clone(),
    };

    let Some(payload) = normalized.as_object_mut() else {
        return Ok(normalized);
    };

    if payload.get("command").is_none() {
        if let Some(command) = payload.get("cmd").cloned() {
            payload.insert("command".to_string(), command);
        } else if let Some(command) = payload.get("raw_command").cloned() {
            payload.insert("command".to_string(), command);
        }
    }

    if payload.get("input").is_none() {
        if let Some(input) = payload.get("chars").cloned() {
            payload.insert("input".to_string(), input);
        } else if let Some(input) = payload.get("text").cloned() {
            payload.insert("input".to_string(), input);
        }
    }

    if payload.get("max_tokens").is_none()
        && let Some(max_output_tokens) = payload.get("max_output_tokens").cloned()
    {
        payload.insert("max_tokens".to_string(), max_output_tokens);
    }

    if payload.get("max_output_tokens").is_none()
        && let Some(max_tokens) = payload.get("max_tokens").cloned()
    {
        payload.insert("max_output_tokens".to_string(), max_tokens);
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::{
        command_text, command_words, has_indexed_command_parts, interactive_input_text,
        normalize_indexed_command_args, normalize_shell_args, normalized_command_value,
        parse_indexed_command_parts, session_id_text, unified_exec_missing_required_args,
        unified_exec_requires_command_safety, working_dir_text, working_dir_text_from_payload,
    };
    use serde_json::{Value, json};

    #[test]
    fn detects_indexed_command_keys() {
        assert!(has_indexed_command_parts(&json!({"command.0": "ls"})));
        assert!(has_indexed_command_parts(&json!({"command.1": "ls"})));
        assert!(!has_indexed_command_parts(&json!({"command.2": "ls"})));
    }

    #[test]
    fn parses_zero_based_indexed_command_parts() {
        let parts = parse_indexed_command_parts(
            json!({
                "command.0": "ls",
                "command.1": "-a"
            })
            .as_object()
            .expect("object"),
        )
        .expect("valid indexed args");

        assert_eq!(parts, Some(vec!["ls".to_string(), "-a".to_string()]));
    }

    #[test]
    fn parses_one_based_indexed_command_parts() {
        let parts = parse_indexed_command_parts(
            json!({
                "command.1": "ls",
                "command.2": "-a"
            })
            .as_object()
            .expect("object"),
        )
        .expect("valid indexed args");

        assert_eq!(parts, Some(vec!["ls".to_string(), "-a".to_string()]));
    }

    #[test]
    fn rejects_non_string_indexed_command_parts() {
        let error = parse_indexed_command_parts(
            json!({
                "command.0": 42
            })
            .as_object()
            .expect("object"),
        )
        .expect_err("non-string segment should fail");

        assert_eq!(error, "command array must contain only strings");
    }

    #[test]
    fn normalizes_indexed_command_args_into_command_string() {
        let normalized = normalize_indexed_command_args(&json!({
            "command.1": "ls",
            "command.2": "-a",
            "working_dir": "."
        }))
        .expect("valid indexed args")
        .expect("normalized payload");

        assert_eq!(
            normalized.get("command").and_then(Value::as_str),
            Some("ls -a")
        );
        assert_eq!(
            normalized.get("working_dir").and_then(Value::as_str),
            Some(".")
        );
    }

    #[test]
    fn normalized_command_value_prefers_cmd_aliases() {
        let normalized = normalized_command_value(&json!({"cmd": "ls -a"}))
            .expect("valid command alias")
            .expect("command value");

        assert_eq!(normalized.as_str(), Some("ls -a"));
    }

    #[test]
    fn command_text_joins_command_arrays() {
        let command = command_text(&json!({"command": ["git", "status", "--short"]}))
            .expect("valid command")
            .expect("command text");

        assert_eq!(command, "git status --short");
    }

    #[test]
    fn command_words_append_extra_args() {
        let words = command_words(&json!({
            "command": "cargo test",
            "args": ["-p", "vtcode-core"]
        }))
        .expect("valid command")
        .expect("command words");

        assert_eq!(words, vec!["cargo", "test", "-p", "vtcode-core"]);
    }

    #[test]
    fn interactive_input_text_preserves_whitespace() {
        assert_eq!(
            interactive_input_text(&json!({"chars": "  echo hi\n"})),
            Some("  echo hi\n")
        );
    }

    #[test]
    fn session_id_text_trims_whitespace() {
        assert_eq!(
            session_id_text(&json!({"session_id": " run-1 "})),
            Some("run-1")
        );
    }

    #[test]
    fn working_dir_text_accepts_aliases() {
        assert_eq!(working_dir_text(&json!({"workdir": " src "})), Some("src"));
        assert_eq!(working_dir_text(&json!({"cwd": "."})), Some("."));
    }

    #[test]
    fn working_dir_text_from_payload_accepts_aliases() {
        let value = json!({"workdir": " src "});
        let payload = value.as_object().expect("object");
        assert_eq!(working_dir_text_from_payload(payload), Some("src"));
    }

    #[test]
    fn normalize_shell_args_maps_codex_fields() {
        let normalized = normalize_shell_args(&json!({
            "cmd": "echo hi",
            "chars": "status\n"
        }))
        .expect("valid shell args");

        assert_eq!(
            normalized.get("command").and_then(Value::as_str),
            Some("echo hi")
        );
        assert_eq!(
            normalized.get("input").and_then(Value::as_str),
            Some("status\n")
        );
    }

    #[test]
    fn normalize_shell_args_copies_max_output_tokens_to_max_tokens() {
        let normalized = normalize_shell_args(&json!({
            "command": "echo hi",
            "max_output_tokens": 42
        }))
        .expect("valid shell args");

        assert_eq!(
            normalized.get("max_output_tokens").and_then(Value::as_u64),
            Some(42)
        );
        assert_eq!(
            normalized.get("max_tokens").and_then(Value::as_u64),
            Some(42)
        );
    }

    #[test]
    fn normalize_shell_args_copies_max_tokens_to_max_output_tokens() {
        let normalized = normalize_shell_args(&json!({
            "command": "echo hi",
            "max_tokens": 42
        }))
        .expect("valid shell args");

        assert_eq!(
            normalized.get("max_tokens").and_then(Value::as_u64),
            Some(42)
        );
        assert_eq!(
            normalized.get("max_output_tokens").and_then(Value::as_u64),
            Some(42)
        );
    }

    #[test]
    fn unified_exec_missing_required_args_is_action_aware() {
        assert_eq!(
            unified_exec_missing_required_args(&json!({"action": "run"})),
            vec!["command"]
        );
        assert_eq!(
            unified_exec_missing_required_args(&json!({"action": "write", "session_id": "run-1"})),
            vec!["input or chars or text"]
        );
        assert_eq!(
            unified_exec_missing_required_args(&json!({"action": "inspect"})),
            vec!["session_id or spool_path"]
        );
        assert!(unified_exec_missing_required_args(&json!({"action": "list"})).is_empty());
    }

    #[test]
    fn unified_exec_requires_command_safety_only_for_run() {
        assert!(unified_exec_requires_command_safety(&json!({
            "action": "run",
            "command": "cargo check"
        })));
        assert!(!unified_exec_requires_command_safety(&json!({
            "action": "poll",
            "session_id": "run-1"
        })));
    }
}
