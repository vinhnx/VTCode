//! Shared helpers for command-style tool arguments.

use serde_json::Value;

use crate::tools::tool_intent::{
    command_session_action, command_session_action_in, command_session_action_is,
};

const INDEXED_COMMAND_TYPE_ERROR: &str = "command array must contain only strings";
const COMMAND_VALUE_TYPE_ERROR: &str = "command must be a string or array of strings";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteStdinDispatch {
    Write,
    Poll,
}

impl WriteStdinDispatch {
    #[must_use]
    pub(crate) const fn command_session_action(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Poll => "poll",
        }
    }
}

pub(crate) fn write_stdin_dispatch(args: &Value) -> Result<WriteStdinDispatch, &'static str> {
    let payload = args
        .as_object()
        .ok_or("write_stdin requires a JSON object")?;
    let chars = payload
        .get("chars")
        .and_then(Value::as_str)
        .ok_or("write_stdin requires string chars")?;

    if chars.is_empty() {
        Ok(WriteStdinDispatch::Poll)
    } else {
        Ok(WriteStdinDispatch::Write)
    }
}

fn collect_indexed_command_parts(
    payload: &serde_json::Map<String, Value>,
    start_index: usize,
) -> Result<Vec<String>, &'static str> {
    let mut parts = Vec::new();
    let mut index = start_index;
    while let Some(value) = payload.get(&format!("command.{index}")) {
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
            shell_words::split(&command).map_err(|_e| COMMAND_VALUE_TYPE_ERROR)?
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

pub fn session_id_text_from_payload(payload: &serde_json::Map<String, Value>) -> Option<&str> {
    payload
        .get("session_id")
        .or_else(|| payload.get("s"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub fn session_id_text(args: &Value) -> Option<&str> {
    args.as_object().and_then(session_id_text_from_payload)
}

pub fn command_session_missing_required_args(args: &Value) -> Vec<&'static str> {
    if command_session_action(args).is_none() {
        return Vec::new();
    }

    let mut missing = Vec::new();
    if command_session_action_is(args, "run") {
        if command_text(args).ok().flatten().is_none() {
            missing.push("command");
        }
    } else if command_session_action_is(args, "write") {
        if session_id_text(args).is_none() {
            missing.push("session_id");
        }
        if interactive_input_text(args).is_none() {
            missing.push("input or chars or text");
        }
    } else if command_session_action_in(args, &["poll", "continue", "close"]) {
        if session_id_text(args).is_none() {
            missing.push("session_id");
        }
    } else if command_session_action_is(args, "inspect") {
        let has_session_id = session_id_text(args).is_some();
        let has_spool_path = has_nonempty_string_field(args, "spool_path");
        if !has_session_id && !has_spool_path {
            missing.push("session_id or spool_path");
        }
    } else if command_session_action_is(args, "code") {
        let has_code =
            has_nonempty_string_field(args, "code") || has_nonempty_string_field(args, "command");
        if !has_code {
            missing.push("code or command");
        }
    }

    missing
}

pub fn command_session_requires_command_safety(args: &Value) -> bool {
    command_session_action_is(args, "run")
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

/// Extract the raw command string from command_session-style arguments without
/// splitting it into words. This is useful for checking shell syntax that
/// would be lost after `shell_words::split`, such as redirections and pipes.
pub fn raw_command_text(args: &Value) -> Option<String> {
    let payload = args.as_object()?;

    if let Some(command) = payload
        .get("command")
        .or_else(|| payload.get("cmd"))
        .or_else(|| payload.get("raw_command"))
    {
        match command {
            Value::String(text) => return Some(text.clone()),
            Value::Array(values) => {
                let parts: Option<Vec<&str>> = values.iter().map(|v| v.as_str()).collect();
                return parts.map(shell_words::join);
            }
            _ => return None,
        }
    }

    let indexed = parse_indexed_command_parts(payload).ok().flatten()?;
    Some(shell_words::join(indexed.iter().map(String::as_str)))
}

/// Returns true if the raw command string appears to be a safe read-only
/// inspection command. It checks for shell write operators, process
/// substitutions, and common destructive subcommands/flags.
///
/// This is intentionally conservative: a command that does anything suspicious
/// is treated as mutating so it does not get silently cached or parallelized.
pub fn is_readonly_command_string(args: &Value) -> bool {
    let Some(command) = raw_command_text(args) else {
        return false;
    };
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Deny shell operators that produce side effects or hide them.
    // Matches: >, >>, >|, <(, >(, ;, &&, ||, $() backticks handled below.
    // Pipelines are allowed here but validated segment-by-segment by the caller.
    if trimmed.contains('>')
        || trimmed.contains("<(")
        || trimmed.contains(">(")
        || trimmed.contains(';')
        || trimmed.contains("&&")
        || trimmed.contains("||")
    {
        return false;
    }

    // Deny command substitution and process redirection that can run arbitrary code.
    if trimmed.contains("$(") || trimmed.contains('`') || trimmed.contains("$((") {
        return false;
    }

    // Deny common destructive commands outright, regardless of flags.
    let lower = trimmed.to_ascii_lowercase();
    for destructive in [
        " rm ",
        "rm ",
        " shred ",
        "shred ",
        " truncate ",
        "truncate ",
        " tee ",
        "tee ",
        " mv ",
        "mv ",
        " cp ",
        "cp ",
        " install ",
        "install ",
        " chmod ",
        "chmod ",
        " chown ",
        "chown ",
        " chattr ",
        "chattr ",
        " mkfs ",
        "mkfs",
        " dd ",
        "dd ",
        " wipe ",
        "wipe ",
        " srm ",
        "srm ",
        " rm\t",
        "shred\t",
        "truncate\t",
        "tee\t",
        "mv\t",
        "cp\t",
    ] {
        if lower.contains(destructive) {
            return false;
        }
    }
    if lower.starts_with("rm ")
        || lower.starts_with("shred ")
        || lower.starts_with("truncate ")
        || lower.starts_with("tee ")
        || lower.starts_with("mv ")
        || lower.starts_with("cp ")
        || lower.starts_with("install ")
        || lower.starts_with("chmod ")
        || lower.starts_with("chown ")
        || lower.starts_with("chattr ")
        || lower.starts_with("mkfs")
        || lower.starts_with("dd ")
        || lower.starts_with("wipe ")
        || lower.starts_with("srm ")
    {
        return false;
    }

    // Deny in-place editing commands (sed -i, perl -i, ruby -i) which modify files
    // despite being in the read-only allow-list.
    if lower.contains("sed ") && lower.contains(" -i") {
        return false;
    }
    if lower.contains("perl ") && lower.contains(" -i") {
        return false;
    }
    if lower.contains("ruby ") && lower.contains(" -i") {
        return false;
    }

    // Deny destructive `find` flags before we allow `find` as read-only.
    if lower.contains("find ") {
        for destructive_flag in [
            " -delete",
            "-delete ",
            "\t-delete",
            " -exec rm",
            "-exec rm",
            " -exec shred",
            "-exec shred",
            " -exec chmod",
            "-exec chmod",
            " -exec chown",
            "-exec chown",
            " -exec truncate",
            "-exec truncate",
            " -exec tee",
            "-exec tee",
            " -exec mv",
            "-exec mv",
            " -exec cp",
            "-exec cp",
            " -exec install",
            "-exec install",
            " -execdd",
            " -exec bash",
            "-exec bash",
        ] {
            if lower.contains(destructive_flag) {
                return false;
            }
        }
    }

    true
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

    if payload.get("session_id").is_none()
        && let Some(session_id) = payload.get("s").cloned()
    {
        payload.insert("session_id".to_string(), session_id);
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
        WriteStdinDispatch, command_session_missing_required_args,
        command_session_requires_command_safety, command_text, command_words,
        has_indexed_command_parts, interactive_input_text, is_readonly_command_string,
        normalize_indexed_command_args, normalize_shell_args, normalized_command_value,
        parse_indexed_command_parts, raw_command_text, session_id_text,
        session_id_text_from_payload, working_dir_text, working_dir_text_from_payload,
        write_stdin_dispatch,
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
    fn write_stdin_dispatch_distinguishes_write_from_poll() {
        assert_eq!(
            write_stdin_dispatch(&json!({"chars": ""})),
            Ok(WriteStdinDispatch::Poll)
        );
        assert_eq!(
            write_stdin_dispatch(&json!({"chars": "  status\n"})),
            Ok(WriteStdinDispatch::Write)
        );
    }

    #[test]
    fn write_stdin_dispatch_requires_public_chars() {
        assert_eq!(
            write_stdin_dispatch(&json!({"input": "status\n"})),
            Err("write_stdin requires string chars")
        );
        assert_eq!(
            write_stdin_dispatch(&json!({"chars": 1})),
            Err("write_stdin requires string chars")
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
    fn session_id_text_accepts_compact_alias() {
        assert_eq!(session_id_text(&json!({"s": " run-1 "})), Some("run-1"));
    }

    #[test]
    fn session_id_text_from_payload_accepts_aliases() {
        let value = json!({"s": " run-1 "});
        let payload = value.as_object().expect("object");
        assert_eq!(session_id_text_from_payload(payload), Some("run-1"));
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
    fn normalize_shell_args_maps_compact_session_id() {
        let normalized = normalize_shell_args(&json!({
            "s": "run-1"
        }))
        .expect("valid shell args");

        assert_eq!(
            normalized.get("session_id").and_then(Value::as_str),
            Some("run-1")
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
    fn command_session_missing_required_args_is_action_aware() {
        assert_eq!(
            command_session_missing_required_args(&json!({"action": "run"})),
            vec!["command"]
        );
        assert_eq!(
            command_session_missing_required_args(
                &json!({"action": "write", "session_id": "run-1"})
            ),
            vec!["input or chars or text"]
        );
        assert_eq!(
            command_session_missing_required_args(&json!({"action": "inspect"})),
            vec!["session_id or spool_path"]
        );
        assert!(command_session_missing_required_args(&json!({"action": "list"})).is_empty());
    }

    #[test]
    fn command_session_requires_command_safety_only_for_run() {
        assert!(command_session_requires_command_safety(&json!({
            "action": "run",
            "command": "cargo check"
        })));
        assert!(!command_session_requires_command_safety(&json!({
            "action": "poll",
            "session_id": "run-1"
        })));
    }

    #[test]
    fn raw_command_text_extracts_command_string() {
        assert_eq!(
            raw_command_text(&json!({"command": "rg foo"})),
            Some("rg foo".to_string())
        );
        assert_eq!(
            raw_command_text(&json!({"cmd": "ls -la"})),
            Some("ls -la".to_string())
        );
        assert_eq!(
            raw_command_text(&json!({"command.0": "cat", "command.1": "file.txt"})),
            Some("cat file.txt".to_string())
        );
        assert_eq!(
            raw_command_text(&json!({"command": ["wc", "-l"]})),
            Some("wc -l".to_string())
        );
        assert!(raw_command_text(&json!({})).is_none());
    }

    #[test]
    fn is_readonly_command_string_allows_inspection_commands() {
        for cmd in [
            "diff a.rs b.rs",
            "find . -type f -name '*.rs'",
            "wc -l src/main.rs",
            "grep -rn 'todo' src",
            "head -50 src/lib.rs",
            "sort src/words.txt | uniq",
        ] {
            assert!(
                is_readonly_command_string(&json!({"command": cmd})),
                "expected '{cmd}' to be read-only"
            );
        }
    }

    #[test]
    fn is_readonly_command_string_allows_pipelines() {
        // Pipelines are allowed here; the caller is responsible for checking each
        // segment against an allow-list of safe commands.
        assert!(is_readonly_command_string(
            &json!({"command": "diff a b | wc -l"})
        ));
        assert!(is_readonly_command_string(
            &json!({"command": "grep x src | sort | uniq"})
        ));
    }

    #[test]
    fn is_readonly_command_string_rejects_redirections_and_substitutions() {
        for cmd in [
            "cat a.txt > b.txt",
            "grep x src >> out.txt",
            "echo $(date)",
            "echo `date`",
            "cat <(echo hi)",
            "cat >(echo hi)",
            "true && rm a.txt",
        ] {
            assert!(
                !is_readonly_command_string(&json!({"command": cmd})),
                "expected '{cmd}' to be rejected"
            );
        }
    }

    #[test]
    fn is_readonly_command_string_rejects_destructive_commands() {
        for cmd in [
            "rm a.txt",
            "find . -type f -delete",
            "find . -name '*.tmp' -exec rm {} \\;",
            "shred a.txt",
            "mv a.txt b.txt",
        ] {
            assert!(
                !is_readonly_command_string(&json!({"command": cmd})),
                "expected '{cmd}' to be rejected"
            );
        }
    }
}
