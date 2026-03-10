use crate::config::HookCommandConfig;
use serde_json::Value;

use crate::hooks::lifecycle::types::HookMessage;

use super::common::{
    HookCommandResult, extract_common_fields, handle_non_zero_exit, handle_timeout,
    matches_hook_event, parse_json_output,
};

pub(crate) fn interpret_session_start(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    messages: &mut Vec<HookMessage>,
    additional_context: &mut Vec<String>,
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
            && let Some(additional) = spec
                .get("additionalContext")
                .and_then(|value| value.as_str())
            && !additional.trim().is_empty()
        {
            additional_context.push(additional.trim().to_owned());
        }

        if !common.suppress_stdout
            && let Some(text) = json
                .get("additional_context")
                .and_then(|value| value.as_str())
            && !text.trim().is_empty()
        {
            additional_context.push(text.trim().to_owned());
        }
    } else if !result.stdout.trim().is_empty() {
        additional_context.push(result.stdout.trim().to_owned());
    }
}

pub(crate) fn interpret_session_end(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    messages: &mut Vec<HookMessage>,
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
    } else if !result.stdout.trim().is_empty() {
        messages.push(HookMessage::info(result.stdout.trim().to_owned()));
    }
}
