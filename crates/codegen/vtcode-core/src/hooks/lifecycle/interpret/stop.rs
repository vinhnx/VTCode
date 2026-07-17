use serde_json::Value;

use crate::config::HookCommandConfig;
use crate::hooks::lifecycle::types::{HookMessage, StopHookOutcome};

use super::common::{
    HookCommandResult, allow_plain_success_stdout, extract_common_fields, handle_non_zero_exit,
    handle_timeout, looks_like_json, matches_hook_event, parse_json_output, trimmed_non_empty,
};

pub(crate) fn interpret_stop(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    outcome: &mut StopHookOutcome,
    quiet_success_output: bool,
) {
    handle_timeout(command, result, &mut outcome.messages);
    if result.timed_out {
        return;
    }

    if let Some(code) = result.exit_code
        && code != 0
    {
        handle_non_zero_exit(command, result, code, &mut outcome.messages, true);
    }

    if let Some(stderr) = trimmed_non_empty(&result.stderr) {
        outcome.messages.push(HookMessage::warning(format!(
            "Stop hook `{}` stderr: {}",
            command.command, stderr
        )));
    }

    let Some(json) = parse_json_output(&result.stdout) else {
        if !result.stdout.trim().is_empty() {
            if looks_like_json(&result.stdout) {
                outcome.messages.push(HookMessage::error(format!(
                    "Stop hook `{}` returned invalid JSON output",
                    command.command
                )));
            } else if allow_plain_success_stdout(result, quiet_success_output) {
                outcome
                    .messages
                    .push(HookMessage::info(result.stdout.trim().to_owned()));
            }
        }
        return;
    };

    let common = extract_common_fields(&json, &mut outcome.messages);
    if let Some(decision) = common.decision.as_deref()
        && decision.eq_ignore_ascii_case("block")
    {
        outcome.block_reason = common
            .decision_reason
            .and_then(|reason| trimmed_non_empty(&reason));
        return;
    }

    if let Some(false) = common.continue_decision {
        outcome.block_reason = common
            .stop_reason
            .and_then(|reason| trimmed_non_empty(&reason));
        return;
    }

    if let Some(Value::Object(spec)) = common.hook_specific
        && matches_hook_event(&spec, "Stop")
    {
        if let Some(decision) = spec.get("decision").and_then(Value::as_str)
            && decision.eq_ignore_ascii_case("block")
        {
            outcome.block_reason = spec
                .get("reason")
                .and_then(Value::as_str)
                .and_then(trimmed_non_empty);
        } else if spec.get("continue").and_then(Value::as_bool) == Some(false) {
            outcome.block_reason = spec
                .get("stopReason")
                .and_then(Value::as_str)
                .and_then(trimmed_non_empty);
        }
    }
}
