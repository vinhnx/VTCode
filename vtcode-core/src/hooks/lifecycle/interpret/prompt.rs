use crate::config::HookCommandConfig;
use serde_json::Value;

use crate::hooks::lifecycle::types::{HookMessage, UserPromptHookOutcome};

use super::common::{
    HookCommandResult, allow_plain_success_stdout, extract_common_fields, handle_non_zero_exit,
    handle_timeout, looks_like_json, matches_hook_event, parse_json_output, trimmed_non_empty,
};

pub(crate) fn interpret_user_prompt(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    outcome: &mut UserPromptHookOutcome,
    quiet_success_output: bool,
) {
    handle_timeout(command, result, &mut outcome.messages);
    if result.timed_out {
        return;
    }

    if let Some(code) = result.exit_code {
        if code == 2 {
            if let Some(reason) = trimmed_non_empty(&result.stderr) {
                outcome.allow_prompt = false;
                outcome.block_reason = Some(reason.clone());
                outcome.messages.push(HookMessage::error(reason));
            } else {
                outcome.messages.push(HookMessage::error(format!(
                    "UserPromptSubmit hook `{}` exited with code 2 without stderr feedback",
                    command.command
                )));
            }
            return;
        } else if code != 0 {
            handle_non_zero_exit(command, result, code, &mut outcome.messages, true);
        }
    }

    if !result.stderr.trim().is_empty() {
        outcome.messages.push(HookMessage::warning(format!(
            "UserPromptSubmit hook `{}` stderr: {}",
            command.command,
            result.stderr.trim()
        )));
    }

    if let Some(json) = parse_json_output(&result.stdout) {
        let common = extract_common_fields(&json, &mut outcome.messages);
        if let Some(false) = common.continue_decision {
            if let Some(reason) = common
                .stop_reason
                .clone()
                .or(common.decision_reason.clone())
                .and_then(|reason| trimmed_non_empty(&reason))
            {
                outcome.allow_prompt = false;
                outcome.block_reason = Some(reason);
            } else {
                outcome.messages.push(HookMessage::error(format!(
                    "UserPromptSubmit hook `{}` returned continue=false without a non-empty reason",
                    command.command
                )));
            }
        }

        if let Some(decision) = common.decision.as_deref()
            && decision.eq_ignore_ascii_case("block")
        {
            if let Some(reason) = common
                .decision_reason
                .clone()
                .and_then(|reason| trimmed_non_empty(&reason))
            {
                outcome.allow_prompt = false;
                outcome.block_reason = Some(reason);
            } else {
                outcome.messages.push(HookMessage::error(format!(
                    "UserPromptSubmit hook `{}` returned decision=block without a non-empty reason",
                    command.command
                )));
            }
        }

        if let Some(Value::Object(spec)) = common.hook_specific
            && matches_hook_event(&spec, "UserPromptSubmit")
            && let Some(additional) = spec
                .get("additionalContext")
                .and_then(|value| value.as_str())
            && !additional.trim().is_empty()
        {
            outcome
                .additional_context
                .push(additional.trim().to_owned());
        }

        if !common.suppress_stdout
            && let Some(text) = json
                .get("additional_context")
                .and_then(|value| value.as_str())
            && !text.trim().is_empty()
        {
            outcome.additional_context.push(text.trim().to_owned());
        }

        if !outcome.allow_prompt
            && let Some(reason) = outcome.block_reason.clone()
        {
            outcome.messages.push(HookMessage::error(reason));
        }
    } else if !result.stdout.trim().is_empty() {
        if looks_like_json(&result.stdout) {
            outcome.messages.push(HookMessage::error(format!(
                "UserPromptSubmit hook `{}` returned invalid JSON output",
                command.command
            )));
        } else if allow_plain_success_stdout(result, quiet_success_output) {
            outcome
                .additional_context
                .push(result.stdout.trim().to_owned());
        }
    }
}
