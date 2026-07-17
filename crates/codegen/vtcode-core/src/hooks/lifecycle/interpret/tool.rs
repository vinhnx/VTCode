use crate::config::HookCommandConfig;
use serde_json::Value;

use crate::hooks::lifecycle::types::{
    HookMessage, PostToolHookOutcome, PreToolHookDecision, PreToolHookOutcome,
};

use super::common::{
    HookCommandResult, allow_plain_success_stdout, extract_common_fields, handle_non_zero_exit,
    handle_timeout, looks_like_json, matches_hook_event, parse_json_output, trimmed_non_empty,
};

pub(crate) fn interpret_pre_tool(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    outcome: &mut PreToolHookOutcome,
    quiet_success_output: bool,
) {
    handle_timeout(command, result, &mut outcome.messages);
    if result.timed_out {
        if matches!(outcome.decision, PreToolHookDecision::Continue) {
            outcome.decision = PreToolHookDecision::Deny;
            outcome.messages.push(HookMessage::error(format!(
                "Tool call blocked because hook `{}` timed out",
                command.command
            )));
        }
        return;
    }

    if let Some(code) = result.exit_code {
        if code == 2 {
            if let Some(reason) = trimmed_non_empty(&result.stderr) {
                outcome.decision = PreToolHookDecision::Deny;
                outcome.messages.push(HookMessage::error(reason));
            } else {
                outcome.messages.push(HookMessage::error(format!(
                    "PreToolUse hook `{}` exited with code 2 without stderr feedback",
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
            "PreToolUse hook `{}` stderr: {}",
            command.command,
            result.stderr.trim()
        )));
    }

    if let Some(json) = parse_json_output(&result.stdout) {
        let common = extract_common_fields(&json, &mut outcome.messages);
        if let Some(false) = common.continue_decision {
            outcome.decision = PreToolHookDecision::Deny;
            if let Some(reason) = common.stop_reason.or(common.decision_reason) {
                outcome.messages.push(HookMessage::error(reason));
            }
            return;
        }

        if let Some(Value::Object(spec)) = common.hook_specific
            && matches_hook_event(&spec, "PreToolUse")
        {
            if let Some(decision) = spec
                .get("permissionDecision")
                .and_then(|value| value.as_str())
            {
                match decision {
                    "allow" => outcome.decision = PreToolHookDecision::Allow,
                    "deny" => outcome.decision = PreToolHookDecision::Deny,
                    "ask" => {
                        if matches!(outcome.decision, PreToolHookDecision::Continue) {
                            outcome.decision = PreToolHookDecision::Ask;
                        }
                    }
                    _ => {}
                }
            }

            if let Some(reason) = spec
                .get("permissionDecisionReason")
                .and_then(|value| value.as_str())
                && !reason.trim().is_empty()
            {
                outcome
                    .messages
                    .push(HookMessage::info(reason.trim().to_owned()));
            }
        }

        if !common.suppress_stdout
            && allow_plain_success_stdout(result, quiet_success_output)
            && !result.stdout.trim().is_empty()
        {
            outcome
                .messages
                .push(HookMessage::info(result.stdout.trim().to_owned()));
        }
    } else if !result.stdout.trim().is_empty() {
        if looks_like_json(&result.stdout) {
            outcome.messages.push(HookMessage::error(format!(
                "PreToolUse hook `{}` returned invalid JSON output",
                command.command
            )));
        } else if allow_plain_success_stdout(result, quiet_success_output) {
            outcome
                .messages
                .push(HookMessage::info(result.stdout.trim().to_owned()));
        }
    }
}

pub(crate) fn interpret_post_tool(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    outcome: &mut PostToolHookOutcome,
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

    if !result.stderr.trim().is_empty() {
        outcome.messages.push(HookMessage::warning(format!(
            "PostToolUse hook `{}` stderr: {}",
            command.command,
            result.stderr.trim()
        )));
    }

    if let Some(json) = parse_json_output(&result.stdout) {
        let common = extract_common_fields(&json, &mut outcome.messages);
        if let Some(decision) = common.decision.as_deref()
            && decision.eq_ignore_ascii_case("block")
        {
            if let Some(reason) = common
                .decision_reason
                .clone()
                .and_then(|reason| trimmed_non_empty(&reason))
            {
                outcome.block_reason = Some(reason);
            } else {
                outcome.messages.push(HookMessage::error(format!(
                    "PostToolUse hook `{}` returned decision=block without a non-empty reason",
                    command.command
                )));
            }
        }

        if let Some(Value::Object(spec)) = common.hook_specific
            && matches_hook_event(&spec, "PostToolUse")
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
    } else if !result.stdout.trim().is_empty() {
        if looks_like_json(&result.stdout) {
            outcome.messages.push(HookMessage::error(format!(
                "PostToolUse hook `{}` returned invalid JSON output",
                command.command
            )));
        } else if allow_plain_success_stdout(result, quiet_success_output) {
            outcome
                .messages
                .push(HookMessage::info(result.stdout.trim().to_owned()));
        }
    }
}
