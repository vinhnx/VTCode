use super::cargo_failure_diagnostics::{
    attach_exec_recovery_guidance, attach_failure_diagnostics_metadata,
    cargo_test_failure_diagnostics,
};
use crate::exec::code_executor::Language;
use crate::tools::continuation::PtyContinuationArgs;
use crate::tools::shell::resolve_fallback_shell;
use crate::tools::types::VTCodeExecSession;
use anyhow::{Context, Result, anyhow};
use hashbrown::HashMap;
use regex::Regex;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use vtcode_commons::preview::excerpt_text_lines;

pub(super) const DEFAULT_INSPECT_HEAD_LINES: usize = 30;
pub(super) const DEFAULT_INSPECT_TAIL_LINES: usize = 30;
pub(super) const DEFAULT_INSPECT_MAX_MATCHES: usize = 200;
pub(super) const MIN_EXEC_YIELD_MS: u64 = 250;
pub(super) const MAX_EXEC_YIELD_MS: u64 = 30_000;
pub(super) const EXEC_OUTPUT_TRUNCATED_SENTINEL: &str = "\n[Output truncated]";

// Conservative PTY command policy inspired by bash allow/deny defaults.
const PTY_DENY_PREFIXES: &[&str] = &[
    "bash -i",
    "sh -i",
    "zsh -i",
    "fish -i",
    "python -i",
    "python3 -i",
    "ipython",
    "nano",
    "vim",
    "vi",
    "emacs",
    "top",
    "htop",
    "less",
    "more",
    "screen",
    "tmux",
];

const PTY_DENY_STANDALONE: &[&str] = &["python", "python3", "bash", "sh", "zsh", "fish"];

#[allow(dead_code)]
const PTY_ALLOW_PREFIXES: &[&str] = &[
    "pwd",
    "whoami",
    "ls",
    "git status",
    "git diff",
    "git log",
    "stat",
    "which",
    "echo",
    "cat",
];

pub(super) struct ExecOutputPreview {
    pub(super) raw_output: String,
    pub(super) output: String,
    pub(super) truncated: bool,
}

pub(super) struct ExecRunOutputConfig {
    pub(super) max_tokens: usize,
    pub(super) inspect_query: Option<String>,
    pub(super) inspect_literal: bool,
    pub(super) inspect_max_matches: usize,
}

pub(super) struct PreparedExecCommand {
    pub(super) command: Vec<String>,
    pub(super) requested_command: Vec<String>,
    pub(super) display_command: String,
    pub(super) requested_command_display: String,
}

pub(super) struct PtyEphemeralCapture {
    pub(super) output: String,
    pub(super) exit_code: Option<i32>,
    pub(super) duration: Duration,
}

pub(super) fn summarized_arg_keys(args: &Value) -> String {
    match args.as_object() {
        Some(map) => {
            if map.is_empty() {
                return "<none>".to_string();
            }
            let mut keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
            keys.sort_unstable();
            let mut preview = keys.into_iter().take(10).collect::<Vec<_>>().join(", ");
            if map.len() > 10 {
                preview.push_str(", ...");
            }
            preview
        }
        None => match args {
            Value::Null => "<null>".to_string(),
            Value::Array(_) => "<array>".to_string(),
            Value::String(_) => "<string>".to_string(),
            Value::Bool(_) => "<bool>".to_string(),
            Value::Number(_) => "<number>".to_string(),
            Value::Object(_) => "<object>".to_string(),
        },
    }
}

pub(super) fn serialized_payload_size_bytes(args: &Value) -> usize {
    serde_json::to_vec(args)
        .map(|bytes| bytes.len())
        .unwrap_or_else(|_| args.to_string().len())
}

pub(super) fn missing_unified_exec_action_error(args: &Value) -> anyhow::Error {
    anyhow!(
        "Missing unified_exec action. Use `action` or fields: \
         `command|cmd|raw_command` (run), `session_id`+`input|chars|text` (write), \
         `session_id` (poll), `action:\"continue\"` with `session_id` and optional `input|chars|text`, \
         `spool_path|query|head_lines|tail_lines|max_matches|literal` (inspect), \
         or `action:\"list\"|\"close\"`. Keys: {}",
        summarized_arg_keys(args)
    )
}

pub(super) fn missing_unified_file_action_error(args: &Value) -> anyhow::Error {
    anyhow!(
        "Missing action in unified_file. Provide `action` or file-operation fields such as \
         `path`, `content`, `old_str`, `patch`, or `destination`. Received keys: {}",
        summarized_arg_keys(args)
    )
}

pub(super) fn missing_unified_search_action_error(args: &Value) -> anyhow::Error {
    anyhow!(
        "Missing unified_search action. Use `action` or fields: \
         `pattern|query` (grep), `action:\"structural\"` with `pattern` (structural search), `path` (list), `keyword` (tools), \
         `scope` (errors), `url` (web), `sub_action|name` (skill). Keys: {}",
        summarized_arg_keys(args)
    )
}

pub(super) fn is_valid_pty_session_id(session_id: &str) -> bool {
    !session_id.trim().is_empty()
        && session_id.len() <= 128
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub(super) fn validate_exec_session_id<'a>(
    raw_session_id: &'a str,
    context: &str,
) -> Result<&'a str> {
    let session_id = raw_session_id.trim();
    if is_valid_pty_session_id(session_id) {
        Ok(session_id)
    } else {
        Err(anyhow!(
            "Invalid session_id for {}: '{}'. Expected an ASCII token (letters, digits, '-', '_').",
            context,
            raw_session_id
        ))
    }
}

fn build_session_command_display_parts(command: &str, args: &[String]) -> String {
    if let Some(flag_index) = args
        .iter()
        .position(|arg| matches!(arg.as_str(), "-c" | "/C" | "-Command"))
        && let Some(command) = args.get(flag_index + 1)
        && !command.trim().is_empty()
    {
        return command.clone();
    }

    let mut parts = Vec::with_capacity(1 + args.len());
    if !command.trim().is_empty() {
        parts.push(command);
    }
    for arg in args {
        if !arg.trim().is_empty() {
            parts.push(arg.as_str());
        }
    }

    if parts.is_empty() {
        "unknown".to_string()
    } else {
        shell_words::join(parts)
    }
}

pub(super) fn build_exec_session_command_display(session: &VTCodeExecSession) -> String {
    build_session_command_display_parts(&session.command, &session.args)
}

pub(super) fn is_pty_exec_session(session: &VTCodeExecSession) -> bool {
    session.backend == "pty"
}

pub(super) fn attach_exec_response_context(
    response: &mut Value,
    session: &VTCodeExecSession,
    command: &str,
    is_exited: bool,
) {
    response["session_id"] = json!(session.id.as_str());
    response["command"] = json!(command);
    if let Some(value) = session.working_dir.as_deref() {
        response["working_directory"] = json!(value);
    }
    response["backend"] = json!(session.backend);
    if let Some(rows) = session.rows {
        response["rows"] = json!(rows);
    }
    if let Some(cols) = session.cols {
        response["cols"] = json!(cols);
    }
    response["is_exited"] = json!(is_exited);
}

pub(super) fn extract_run_session_id_from_tool_output_path(path: &str) -> Option<String> {
    let file_name = Path::new(path).file_name()?.to_str()?;
    let session_id = file_name.strip_suffix(".txt")?;
    if session_id.starts_with("run-") && is_valid_pty_session_id(session_id) {
        Some(session_id.to_string())
    } else {
        None
    }
}

pub(super) fn extract_run_session_id_from_read_file_error(error_message: &str) -> Option<String> {
    let marker = "session_id=\"";
    let start = error_message.find(marker)? + marker.len();
    let rest = &error_message[start..];
    let end = rest.find('"')?;
    let session_id = &rest[..end];
    if session_id.starts_with("run-") && is_valid_pty_session_id(session_id) {
        Some(session_id.to_string())
    } else {
        None
    }
}

pub(super) fn build_read_pty_fallback_args(args: &Value, error_message: &str) -> Option<Value> {
    let session_id = args
        .get("path")
        .or_else(|| args.get("file_path"))
        .or_else(|| args.get("filepath"))
        .or_else(|| args.get("target_path"))
        .and_then(Value::as_str)
        .and_then(extract_run_session_id_from_tool_output_path)
        .or_else(|| extract_run_session_id_from_read_file_error(error_message))?;

    let mut payload = serde_json::Map::new();
    payload.insert("session_id".to_string(), json!(session_id));

    if let Some(yield_time_ms) = args.get("yield_time_ms").cloned() {
        payload.insert("yield_time_ms".to_string(), yield_time_ms);
    }

    Some(Value::Object(payload))
}

pub(super) fn attach_pty_continuation(response: &mut Value, session_id: &str) {
    response["next_continue_args"] = PtyContinuationArgs::new(session_id).to_value();
}

pub(super) fn clamp_exec_yield_ms(value: Option<u64>, default: u64) -> u64 {
    value
        .unwrap_or(default)
        .clamp(MIN_EXEC_YIELD_MS, MAX_EXEC_YIELD_MS)
}

pub(super) fn clamp_peek_yield_ms(value: Option<u64>) -> u64 {
    value.unwrap_or(0).min(MAX_EXEC_YIELD_MS)
}

pub(super) fn max_output_tokens_from_payload(
    payload: &serde_json::Map<String, Value>,
) -> Option<usize> {
    payload
        .get("max_output_tokens")
        .or_else(|| payload.get("max_tokens"))
        .and_then(Value::as_u64)
        .map(|value| value as usize)
}

fn floor_exec_char_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }

    let mut boundary = index;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

pub(super) fn build_exec_output_preview(
    raw_output: String,
    max_tokens: usize,
) -> ExecOutputPreview {
    let max_output_len = max_tokens.saturating_mul(4);
    if max_tokens == 0 || raw_output.len() <= max_output_len {
        return ExecOutputPreview {
            output: raw_output.clone(),
            raw_output,
            truncated: false,
        };
    }

    let preview_end = floor_exec_char_boundary(&raw_output, max_output_len);
    let mut output = raw_output[..preview_end].to_string();
    output.push_str(EXEC_OUTPUT_TRUNCATED_SENTINEL);

    ExecOutputPreview {
        raw_output,
        output,
        truncated: true,
    }
}

pub(super) fn build_exec_response(
    session: &VTCodeExecSession,
    command: &str,
    capture: &PtyEphemeralCapture,
    output_preview: ExecOutputPreview,
    matched_count: Option<usize>,
    query_truncated: bool,
    running_process_id: Option<&str>,
) -> Value {
    let ExecOutputPreview {
        raw_output,
        output,
        truncated,
    } = output_preview;
    let cargo_test_diagnostics =
        cargo_test_failure_diagnostics(command, &raw_output, capture.exit_code);
    let mut response = json!({
        "success": true,
        "output": output,
        "raw_output": raw_output,
        "wall_time": capture.duration.as_secs_f64(),
    });
    if let Some(count) = matched_count {
        response["matched_count"] = json!(count);
        response["query_truncated"] = json!(query_truncated);
    }

    attach_exec_response_context(&mut response, session, command, capture.exit_code.is_some());

    if let Some(code) = capture.exit_code {
        response["exit_code"] = json!(code);
    } else if let Some(process_id) = running_process_id {
        response["process_id"] = json!(process_id);
    }

    if truncated {
        response["truncated"] = json!(true);
    }
    if capture.exit_code.is_none() {
        attach_pty_continuation(&mut response, session.id.as_str());
    }

    attach_exec_recovery_guidance(&mut response, command, capture.exit_code);
    if let Some(diagnostics) = cargo_test_diagnostics {
        attach_failure_diagnostics_metadata(&mut response, &diagnostics);
    }
    response
}

pub(super) fn exec_run_output_config(
    payload: &serde_json::Map<String, Value>,
    display_command: &str,
) -> ExecRunOutputConfig {
    ExecRunOutputConfig {
        max_tokens: max_output_tokens_from_payload(payload)
            .or_else(|| suggest_max_tokens_for_command(display_command))
            .unwrap_or(crate::config::constants::defaults::DEFAULT_PTY_OUTPUT_MAX_TOKENS),
        inspect_query: payload
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        inspect_literal: payload
            .get("literal")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        inspect_max_matches: clamp_max_matches(payload.get("max_matches").and_then(Value::as_u64)),
    }
}

pub(super) fn build_exec_filtered_response(
    session_metadata: &VTCodeExecSession,
    command_display: &str,
    capture: &PtyEphemeralCapture,
    output_config: &ExecRunOutputConfig,
    running_process_id: Option<&str>,
) -> Result<Value> {
    let raw_output = filter_pty_output(&strip_ansi(&capture.output));
    let mut matched_count = None;
    let mut query_truncated = false;
    let filtered_output = if let Some(query) = output_config.inspect_query.as_deref() {
        let (filtered, count, truncated_matches) = filter_lines(
            &raw_output,
            query,
            output_config.inspect_literal,
            output_config.inspect_max_matches,
        )?;
        matched_count = Some(count);
        query_truncated = truncated_matches;
        filtered
    } else {
        raw_output.clone()
    };
    let preview = build_exec_output_preview(filtered_output, output_config.max_tokens);

    Ok(build_exec_response(
        session_metadata,
        command_display,
        capture,
        ExecOutputPreview {
            raw_output,
            output: preview.output,
            truncated: preview.truncated,
        },
        matched_count,
        query_truncated,
        running_process_id,
    ))
}

pub(super) fn build_exec_passthrough_response(
    session_metadata: &VTCodeExecSession,
    command_display: &str,
    capture: &PtyEphemeralCapture,
    max_tokens: Option<usize>,
) -> Value {
    let raw_output = filter_pty_output(&strip_ansi(&capture.output));
    let output_preview = if let Some(limit) = max_tokens {
        let preview = build_exec_output_preview(raw_output.clone(), limit);
        ExecOutputPreview {
            raw_output,
            output: preview.output,
            truncated: preview.truncated,
        }
    } else {
        ExecOutputPreview {
            raw_output: raw_output.clone(),
            output: raw_output,
            truncated: false,
        }
    };

    build_exec_response(
        session_metadata,
        command_display,
        capture,
        output_preview,
        None,
        false,
        None,
    )
}

pub(super) fn clamp_inspect_lines(value: Option<u64>, default: usize) -> usize {
    value.map(|v| v as usize).unwrap_or(default).min(5_000)
}

pub(super) fn clamp_max_matches(value: Option<u64>) -> usize {
    value
        .map(|v| v as usize)
        .unwrap_or(DEFAULT_INSPECT_MAX_MATCHES)
        .clamp(1, 10_000)
}

pub(super) fn build_head_tail_preview(
    content: &str,
    head_lines: usize,
    tail_lines: usize,
) -> (String, bool) {
    let preview = excerpt_text_lines(content, head_lines.max(1), tail_lines.max(1));
    if preview.total == 0 {
        return (String::new(), false);
    }

    if preview.hidden_count == 0 {
        return (preview.head.join("\n"), false);
    }

    let mut lines = Vec::with_capacity(preview.head.len() + preview.tail.len() + 1);
    lines.extend(preview.head.into_iter().map(String::from));
    lines.push(format!("[... omitted {} lines ...]", preview.hidden_count));
    lines.extend(preview.tail.into_iter().map(String::from));
    (lines.join("\n"), true)
}

pub(super) fn filter_lines(
    content: &str,
    query: &str,
    literal: bool,
    max_matches: usize,
) -> Result<(String, usize, bool)> {
    let matcher = if literal {
        None
    } else {
        Some(Regex::new(query).with_context(|| format!("Invalid regex query: {}", query))?)
    };

    let mut matches = Vec::new();
    let mut total_matches = 0usize;

    for (idx, line) in content.lines().enumerate() {
        let is_match = if literal {
            line.contains(query)
        } else {
            matcher
                .as_ref()
                .map(|regex| regex.is_match(line))
                .unwrap_or(false)
        };
        if !is_match {
            continue;
        }

        total_matches = total_matches.saturating_add(1);
        if matches.len() < max_matches {
            matches.push(format!("{}: {}", idx + 1, line));
        }
    }

    let truncated = total_matches > max_matches;
    Ok((matches.join("\n"), total_matches, truncated))
}

pub(super) fn resolve_workspace_scoped_path(
    workspace_root: &Path,
    raw_path: &str,
) -> Result<PathBuf> {
    let path = Path::new(raw_path.trim());
    if path.as_os_str().is_empty() {
        return Err(anyhow!("spool_path cannot be empty"));
    }

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    };
    let normalized = crate::utils::path::normalize_path(&absolute);
    let normalized_workspace = crate::utils::path::normalize_path(workspace_root);
    if !normalized.starts_with(&normalized_workspace) {
        return Err(anyhow!(
            "spool_path must stay within workspace: {}",
            raw_path
        ));
    }

    Ok(normalized)
}

fn path_is_tool_accessible_from_workspace(workspace_root: &Path, raw_path: &str) -> bool {
    let path = Path::new(raw_path.trim());
    if path.as_os_str().is_empty() {
        return false;
    }

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    };
    let normalized = crate::utils::path::normalize_path(&absolute);
    let normalized_workspace = crate::utils::path::normalize_path(workspace_root);
    normalized.starts_with(&normalized_workspace)
}

pub(super) fn sanitize_subagent_tool_output_paths(workspace_root: &Path, value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    if let Some(raw_path) = object.get("transcript_path").and_then(Value::as_str)
        && !path_is_tool_accessible_from_workspace(workspace_root, raw_path)
    {
        object.remove("transcript_path");
    }

    if let Some(entry) = object.get_mut("entry") {
        sanitize_subagent_tool_output_paths(workspace_root, entry);
    }
}

pub(super) fn parse_command_parts(
    payload: &serde_json::Map<String, Value>,
    missing_error: &str,
    empty_error: &str,
) -> Result<(Vec<String>, Option<String>)> {
    let normalized_payload = (!payload.contains_key("command")
        && (payload.contains_key("cmd")
            || payload.contains_key("raw_command")
            || payload.contains_key("command.0")
            || payload.contains_key("command.1")))
    .then(|| {
        crate::tools::command_args::normalize_shell_args(&Value::Object(payload.clone()))
            .map_err(|error| anyhow!(error))
    })
    .transpose()?;
    let payload = normalized_payload
        .as_ref()
        .and_then(Value::as_object)
        .unwrap_or(payload);

    let (mut parts, raw_command) = match payload.get("command") {
        Some(Value::String(command)) => {
            let parts = shell_words::split(command).context("Failed to parse command string")?;
            (parts, Some(command.to_string()))
        }
        Some(Value::Array(values)) => {
            let parts = values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(|part| part.to_string())
                        .ok_or_else(|| anyhow!("command array must contain only strings"))
                })
                .collect::<Result<Vec<_>>>()?;
            (parts, None)
        }
        _ => match crate::tools::command_args::parse_indexed_command_parts(payload)
            .map_err(|error| anyhow!(error))?
        {
            Some(indexed_parts) => (indexed_parts, None),
            None => return Err(anyhow!("{}", missing_error)),
        },
    };

    if let Some(args_value) = payload.get("args")
        && let Some(args_array) = args_value.as_array()
    {
        for value in args_array {
            if let Some(part) = value.as_str() {
                parts.push(part.to_string());
            }
        }
    }

    if parts.is_empty() {
        return Err(anyhow!("{}", empty_error));
    }

    Ok((parts, raw_command))
}

pub(super) fn parse_exec_env_overrides(
    payload: &serde_json::Map<String, Value>,
) -> Result<HashMap<String, String>> {
    let Some(env_value) = payload.get("env") else {
        return Ok(HashMap::new());
    };

    match env_value {
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| {
                let value = value
                    .as_str()
                    .ok_or_else(|| anyhow!("env values must be strings"))?;
                Ok((key.clone(), value.to_string()))
            })
            .collect(),
        Value::Array(entries) => {
            let mut env = HashMap::new();
            for entry in entries {
                let object = entry
                    .as_object()
                    .ok_or_else(|| anyhow!("env entries must be objects"))?;
                let name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| anyhow!("env entries must include a non-empty name"))?;
                let value = object
                    .get("value")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("env entries must include a string value"))?;
                env.insert(name.to_string(), value.to_string());
            }
            Ok(env)
        }
        _ => Err(anyhow!(
            "env must be an object or array of {{name, value}} entries"
        )),
    }
}

pub(super) fn is_git_diff_command(parts: &[String]) -> bool {
    let Some(first) = parts.first() else {
        return false;
    };
    let basename = Path::new(first)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(first.as_str())
        .to_ascii_lowercase();
    if basename != "git" && basename != "git.exe" {
        return false;
    }
    parts.iter().skip(1).any(|part| part == "diff")
}

pub(super) fn resolve_shell_preference(
    pref: Option<&str>,
    config: &crate::config::PtyConfig,
) -> String {
    pref.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            config
                .preferred_shell
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(resolve_fallback_shell)
}

pub(super) fn resolve_shell_preference_with_zsh_fork(
    pref: Option<&str>,
    config: &crate::config::PtyConfig,
) -> Result<String> {
    if let Some(zsh_path) = config.zsh_fork_shell_path()? {
        return Ok(zsh_path.to_string());
    }

    Ok(resolve_shell_preference(pref, config))
}

fn normalized_shell_name(shell: &str) -> String {
    PathBuf::from(shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(shell)
        .to_lowercase()
}

fn build_shell_command_string(raw: Option<&str>, parts: &[String], _shell: &str) -> String {
    let fallback = || shell_words::join(parts.iter().map(|s| s.as_str()));

    let Some(raw) = raw else {
        return fallback();
    };

    let Ok(raw_parts) = shell_words::split(raw) else {
        return fallback();
    };

    if parts.len() <= raw_parts.len() || !parts.starts_with(&raw_parts) {
        return raw.to_string();
    }

    let suffix = shell_words::join(parts[raw_parts.len()..].iter().map(|s| s.as_str()));
    if suffix.is_empty() {
        raw.to_string()
    } else {
        format!("{} {}", raw, suffix)
    }
}

fn shell_command_flag(shell_program: &str, normalized_shell: &str) -> String {
    if should_use_windows_command_tokenizer(Some(shell_program)) {
        match normalized_shell {
            "cmd" | "cmd.exe" => "/C".to_string(),
            "powershell" | "powershell.exe" | "pwsh" => "-Command".to_string(),
            _ => "-c".to_string(),
        }
    } else {
        "-c".to_string()
    }
}

fn command_display_for_shell(parts: &[String], shell_program: &str) -> String {
    if should_use_windows_command_tokenizer(Some(shell_program)) {
        join_windows_command(parts)
    } else {
        shell_words::join(parts.iter().map(|part| part.as_str()))
    }
}

pub(super) fn prepare_exec_command(
    payload: &serde_json::Map<String, Value>,
    shell_program: &str,
    login_shell: bool,
    mut command: Vec<String>,
    auto_raw_command: Option<String>,
) -> PreparedExecCommand {
    let requested_command = command.clone();

    let normalized_shell = normalized_shell_name(shell_program);
    let existing_shell = command
        .first()
        .map(|existing| normalized_shell_name(existing));

    if existing_shell != Some(normalized_shell.clone()) {
        let raw_command = payload
            .get("raw_command")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
            .or(auto_raw_command);

        let command_string =
            build_shell_command_string(raw_command.as_deref(), &command, shell_program);

        let mut shell_invocation = Vec::with_capacity(4);
        shell_invocation.push(shell_program.to_string());

        if login_shell && !should_use_windows_command_tokenizer(Some(shell_program)) {
            shell_invocation.push("-l".to_string());
        }

        shell_invocation.push(shell_command_flag(shell_program, &normalized_shell));
        shell_invocation.push(command_string);
        command = shell_invocation;
    }

    PreparedExecCommand {
        display_command: command_display_for_shell(&command, shell_program),
        requested_command_display: command_display_for_shell(&requested_command, shell_program),
        command,
        requested_command,
    }
}

pub(super) fn code_language_from_args(args: &Value) -> Language {
    let language_str = args
        .get("language")
        .or_else(|| args.get("lang"))
        .and_then(|v| v.as_str())
        .unwrap_or("python3");

    match language_str {
        "python3" | "python" => Language::Python3,
        "javascript" | "js" => Language::JavaScript,
        _ => Language::Python3,
    }
}

/// Check if a command is a file display command that should have limited output.
/// Returns suggested max_tokens if the command is a file display command without explicit limits.
pub fn suggest_max_tokens_for_command(cmd: &str) -> Option<usize> {
    let trimmed = cmd.trim().to_lowercase();

    if trimmed.contains("head") || trimmed.contains("tail") || trimmed.contains("| ") {
        return None;
    }

    let file_display_cmds = ["cat ", "bat ", "type "];

    for prefix in &file_display_cmds {
        if trimmed.starts_with(prefix) {
            return Some(250);
        }
    }

    None
}

fn should_use_windows_command_tokenizer(shell: Option<&str>) -> bool {
    if cfg!(windows) {
        if let Some(s) = shell {
            let lower = s.to_lowercase();
            return lower.contains("cmd") || lower.contains("powershell") || lower.contains("pwsh");
        }
        return true;
    }
    false
}

fn join_windows_command(parts: &[String]) -> String {
    parts.join(" ")
}

pub(super) fn parse_pty_dimension(name: &str, value: Option<&Value>, default: u16) -> Result<u16> {
    match value {
        Some(v) => {
            let n = v
                .as_u64()
                .ok_or_else(|| anyhow!("{} must be a number", name))?;
            Ok(n as u16)
        }
        None => Ok(default),
    }
}

fn generate_session_id(prefix: &str) -> String {
    format!("{}-{}", prefix, &uuid::Uuid::new_v4().to_string()[..8])
}

pub(super) fn resolve_exec_run_session_id(
    payload: &serde_json::Map<String, Value>,
) -> Result<String> {
    crate::tools::command_args::session_id_text_from_payload(payload)
        .map(|session_id| validate_exec_session_id(session_id, "unified_exec run"))
        .transpose()?
        .map(str::to_string)
        .map_or_else(|| Ok(generate_session_id("run")), Ok)
}

pub(super) fn strip_ansi(text: &str) -> String {
    crate::utils::ansi_parser::strip_ansi(text)
}

pub(super) fn filter_pty_output(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub(super) fn enforce_pty_command_policy(display_command: &str, confirm: bool) -> Result<()> {
    let lower = display_command.to_ascii_lowercase();
    let trimmed = lower.trim();
    let is_standalone = trimmed.split_whitespace().count() == 1;

    let deny_match = PTY_DENY_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix));
    let standalone_denied = is_standalone && PTY_DENY_STANDALONE.contains(&trimmed);

    if deny_match || standalone_denied {
        if confirm {
            return Ok(());
        }
        return Err(anyhow!(
            "Command '{}' is blocked by PTY safety policy. Set confirm=true to force execution.",
            display_command
        ));
    }

    Ok(())
}
