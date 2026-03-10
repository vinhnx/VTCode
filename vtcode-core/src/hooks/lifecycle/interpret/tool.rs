use crate::config::HookCommandConfig;
use serde_json::Value;

use crate::hooks::lifecycle::types::{
    HookMessage, PostToolHookOutcome, PreToolHookDecision, PreToolHookOutcome,
};

use super::common::{
    HookCommandResult, extract_common_fields, handle_non_zero_exit, handle_timeout,
    matches_hook_event, parse_json_output, select_message,
};

pub(crate) fn interpret_pre_tool(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    outcome: &mut PreToolHookOutcome,
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
            outcome.decision = PreToolHookDecision::Deny;
            let reason =
                select_message(result.stderr.trim(), "Tool call blocked by lifecycle hook.");
            outcome.messages.push(HookMessage::error(reason));
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

        if !common.suppress_stdout && !result.stdout.trim().is_empty() {
            outcome
                .messages
                .push(HookMessage::info(result.stdout.trim().to_owned()));
        }
    } else if !result.stdout.trim().is_empty() {
        outcome
            .messages
            .push(HookMessage::info(result.stdout.trim().to_owned()));
    }
}

pub(crate) fn interpret_post_tool(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    outcome: &mut PostToolHookOutcome,
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
            outcome.block_reason = common
                .decision_reason
                .clone()
                .or_else(|| Some("Tool execution requires attention.".to_owned()));
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
        outcome
            .messages
            .push(HookMessage::info(result.stdout.trim().to_owned()));
    }
}
