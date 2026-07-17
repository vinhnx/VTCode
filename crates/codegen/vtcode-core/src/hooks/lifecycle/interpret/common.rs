use serde_json::Value;

use crate::config::HookCommandConfig;

use crate::hooks::lifecycle::types::{HookMessage, HookMessageLevel};

pub(crate) struct HookCommandResult {
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) timed_out: bool,
    pub(crate) timeout_seconds: u64,
}

pub(crate) fn parse_json_output(stdout: &str) -> Option<Value> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }

    serde_json::from_str(trimmed).ok()
}

pub(crate) fn looks_like_json(stdout: &str) -> bool {
    let trimmed = stdout.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

pub(crate) fn trimmed_non_empty(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

pub(crate) fn allow_plain_success_stdout(
    result: &HookCommandResult,
    quiet_success_output: bool,
) -> bool {
    if !quiet_success_output {
        return true;
    }

    if result.timed_out {
        return true;
    }

    !matches!(result.exit_code, None | Some(0))
}

pub(crate) struct CommonJsonFields {
    pub(super) continue_decision: Option<bool>,
    pub(super) stop_reason: Option<String>,
    pub(super) suppress_stdout: bool,
    pub(super) decision: Option<String>,
    pub(super) decision_reason: Option<String>,
    pub(super) hook_specific: Option<Value>,
}

pub(crate) fn extract_common_fields(
    json: &Value,
    messages: &mut Vec<HookMessage>,
) -> CommonJsonFields {
    if let Some(system_message) = json
        .get("systemMessage")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        messages.push(HookMessage::info(system_message.to_owned()));
    }

    CommonJsonFields {
        continue_decision: json.get("continue").and_then(|value| value.as_bool()),
        stop_reason: json
            .get("stopReason")
            .and_then(|value| value.as_str())
            .map(|value| value.to_owned()),
        suppress_stdout: json
            .get("suppressOutput")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        decision: json
            .get("decision")
            .and_then(|value| value.as_str())
            .map(|value| value.to_owned()),
        decision_reason: json
            .get("reason")
            .and_then(|value| value.as_str())
            .map(|value| value.to_owned()),
        hook_specific: json.get("hookSpecificOutput").cloned(),
    }
}

pub(crate) fn matches_hook_event(spec: &serde_json::Map<String, Value>, event_name: &str) -> bool {
    match spec.get("hookEventName").and_then(|value| value.as_str()) {
        Some(name) => name.eq_ignore_ascii_case(event_name),
        None => true,
    }
}

pub(crate) fn handle_timeout(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    messages: &mut Vec<HookMessage>,
) {
    if result.timed_out {
        messages.push(HookMessage::error(format!(
            "Hook `{}` timed out after {}s",
            command.command, result.timeout_seconds
        )));
    }
}

pub(crate) fn handle_non_zero_exit(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    code: i32,
    messages: &mut Vec<HookMessage>,
    warn: bool,
) {
    let level = if warn {
        HookMessageLevel::Warning
    } else {
        HookMessageLevel::Error
    };

    let text = if result.stderr.trim().is_empty() {
        format!("Hook `{}` exited with status {code}", command.command)
    } else {
        format!(
            "Hook `{}` exited with status {code}: {}",
            command.command,
            result.stderr.trim()
        )
    };

    messages.push(HookMessage { level, text });
}
