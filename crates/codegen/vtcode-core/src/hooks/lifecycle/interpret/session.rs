use crate::config::HookCommandConfig;
use serde_json::Value;

use crate::hooks::lifecycle::types::HookMessage;

use super::common::{
    HookCommandResult, allow_plain_success_stdout, extract_common_fields, handle_non_zero_exit,
    handle_timeout, looks_like_json, matches_hook_event, parse_json_output,
};

fn push_additional_context_value(additional_context: &mut Vec<String>, value: &Value) {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                additional_context.push(trimmed.to_owned());
            }
        }
        Value::Array(values) => {
            for value in values {
                push_additional_context_value(additional_context, value);
            }
        }
        _ => {}
    }
}

pub(crate) fn interpret_session_start(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    messages: &mut Vec<HookMessage>,
    additional_context: &mut Vec<String>,
    quiet_success_output: bool,
) {
    handle_timeout(command, result, messages);
    if result.timed_out {
        return;
    }

    if let Some(code) = result.exit_code
        && code != 0
    {
        handle_non_zero_exit(command, result, code, messages, false);
    }

    if !result.stderr.trim().is_empty() {
        messages.push(HookMessage::error(format!(
            "SessionStart hook `{}` stderr: {}",
            command.command,
            result.stderr.trim()
        )));
    }

    if let Some(json) = parse_json_output(&result.stdout) {
        let common = extract_common_fields(&json, messages);
        if let Some(Value::Object(spec)) = common.hook_specific
            && matches_hook_event(&spec, "SessionStart")
            && let Some(additional) = spec.get("additionalContext")
        {
            push_additional_context_value(additional_context, additional);
        }

        if !common.suppress_stdout
            && let Some(additional) = json.get("additional_context")
        {
            push_additional_context_value(additional_context, additional);
        }
    } else if !result.stdout.trim().is_empty() {
        if looks_like_json(&result.stdout) {
            messages.push(HookMessage::error(format!(
                "SessionStart hook `{}` returned invalid JSON output",
                command.command
            )));
        } else if allow_plain_success_stdout(result, quiet_success_output) {
            additional_context.push(result.stdout.trim().to_owned());
        }
    }
}

pub(crate) fn interpret_session_end(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    messages: &mut Vec<HookMessage>,
    quiet_success_output: bool,
) {
    handle_timeout(command, result, messages);
    if result.timed_out {
        return;
    }

    if let Some(code) = result.exit_code
        && code != 0
    {
        handle_non_zero_exit(command, result, code, messages, false);
    }

    if !result.stderr.trim().is_empty() {
        messages.push(HookMessage::error(format!(
            "SessionEnd hook `{}` stderr: {}",
            command.command,
            result.stderr.trim()
        )));
    }

    if let Some(json) = parse_json_output(&result.stdout) {
        let _ = extract_common_fields(&json, messages);
    } else if allow_plain_success_stdout(result, quiet_success_output)
        && !result.stdout.trim().is_empty()
    {
        messages.push(HookMessage::info(result.stdout.trim().to_owned()));
    }
}
