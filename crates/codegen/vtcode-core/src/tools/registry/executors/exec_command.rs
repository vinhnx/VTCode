//! Command construction for the exec tool family.
//!
//! Turns a tool payload into the concrete argv (or shell invocation) that gets
//! executed: command/args parsing, env override extraction, shell preference
//! resolution, and session-id generation. Split out of `exec_support.rs` so the
//! "how to run it" concerns live apart from response building and session book-
//! keeping.

use super::exec_support::validate_exec_session_id;
use crate::config::PtyConfig;
use crate::exec::code_executor::Language;
use crate::tools::shell::resolve_fallback_shell;
use anyhow::{Context, Result, anyhow};
use hashbrown::HashMap;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) struct PreparedExecCommand {
    pub(super) command: Vec<String>,
    pub(super) requested_command: Vec<String>,
    pub(super) display_command: String,
    pub(super) requested_command_display: String,
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
    let payload = normalized_payload.as_ref().and_then(Value::as_object).unwrap_or(payload);

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
        _ => match crate::tools::command_args::parse_indexed_command_parts(payload).map_err(|error| anyhow!(error))? {
            Some(indexed_parts) => (indexed_parts, None),
            None => return Err(anyhow!("{missing_error}")),
        },
    };

    if let Some(args_value) = payload.get("args") {
        let args_array = args_value.as_array().ok_or_else(|| anyhow!("args must be an array"))?;
        for value in args_array {
            let part = value.as_str().ok_or_else(|| anyhow!("args array must contain only strings"))?;
            parts.push(part.to_string());
        }
    }

    let collision_command = raw_command.as_deref().or_else(|| parts.first().map(String::as_str));
    if collision_command.is_some_and(crate::tools::names::is_apply_patch_shell_collision_command) {
        return Err(anyhow!(
            "apply_patch is a tool, not a shell executable. {}",
            crate::tools::apply_patch::APPLY_PATCH_ARGUMENT_CORRECTION
        ));
    }

    if parts.is_empty() {
        return Err(anyhow!("{empty_error}"));
    }

    Ok((parts, raw_command))
}

pub(super) fn parse_exec_env_overrides(payload: &serde_json::Map<String, Value>) -> Result<HashMap<String, String>> {
    let Some(env_value) = payload.get("env") else {
        return Ok(HashMap::new());
    };

    match env_value {
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| {
                let value = value.as_str().ok_or_else(|| anyhow!("env values must be strings"))?;
                Ok((key.clone(), value.to_string()))
            })
            .collect(),
        Value::Array(entries) => {
            let mut env = HashMap::new();
            for entry in entries {
                let object = entry.as_object().ok_or_else(|| anyhow!("env entries must be objects"))?;
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
        _ => Err(anyhow!("env must be an object or array of {{name, value}} entries")),
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

pub(super) fn resolve_shell_preference(pref: Option<&str>, config: &PtyConfig) -> String {
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

pub(super) fn resolve_shell_preference_with_zsh_fork(pref: Option<&str>, config: &PtyConfig) -> Result<String> {
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
        format!("{raw} {suffix}")
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
    let existing_shell = command.first().map(|existing| normalized_shell_name(existing));

    if existing_shell != Some(normalized_shell.clone()) {
        let raw_command = payload
            .get("raw_command")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
            .or(auto_raw_command);

        let command_string = build_shell_command_string(raw_command.as_deref(), &command, shell_program);

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
            let n = v.as_u64().ok_or_else(|| anyhow!("{name} must be a number"))?;
            Ok(n as u16)
        }
        None => Ok(default),
    }
}

fn generate_session_id(prefix: &str) -> String {
    format!("{}-{}", prefix, &uuid::Uuid::new_v4().to_string()[..8])
}

pub(super) fn resolve_exec_run_session_id(payload: &serde_json::Map<String, Value>) -> Result<String> {
    crate::tools::command_args::session_id_text_from_payload(payload)
        .map(|session_id| validate_exec_session_id(session_id, "exec_command run"))
        .transpose()?
        .map(str::to_string)
        .map_or_else(|| Ok(generate_session_id("run")), Ok)
}

#[cfg(test)]
mod patch_shell_rejection_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn patch_shell_parser_rejects_direct_and_quoted_names_before_launch() {
        for command in [
            "apply_patch <<'PATCH'\n*** Begin Patch\n*** End Patch\nPATCH",
            "'applypatch' foo",
            "/tmp/apply_patch foo",
        ] {
            let args = json!({"command":command});
            let error =
                parse_command_parts(args.as_object().unwrap(), "missing", "empty").expect_err("reject before launch");
            assert!(error.to_string().contains("apply_patch is a tool"));
        }
        assert!(
            parse_command_parts(json!({"command":"printf '%s' apply_patch"}).as_object().unwrap(), "missing", "empty")
                .is_ok()
        );
    }
}
