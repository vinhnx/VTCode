use crate::config::HookCommandConfig;
use serde_json::Value;

use crate::hooks::lifecycle::types::{HookMessage, UserPromptHookOutcome};

use super::common::{
    HookCommandResult, allow_plain_success_stdout, extract_common_fields, handle_non_zero_exit, looks_like_json,
    matches_hook_event, parse_json_output, report_capture_failure, trimmed_non_empty,
};

pub(crate) fn interpret_user_prompt(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    outcome: &mut UserPromptHookOutcome,
    quiet_success_output: bool,
) {
    let capture_failed = report_capture_failure(command, result, &mut outcome.messages);

    let mut blocked_by_exit_code_two = false;
    if let Some(code) = result.exit_code {
        if code == 2 {
            if let Some(reason) = trimmed_non_empty(&result.stderr) {
                outcome.allow_prompt = false;
                outcome.block_reason = Some(reason.clone());
                outcome.messages.push(HookMessage::error(reason));
                blocked_by_exit_code_two = true;
            } else {
                outcome.messages.push(HookMessage::error(format!(
                    "UserPromptSubmit hook `{}` exited with code 2 without stderr feedback",
                    command.command
                )));
                return;
            }
        } else if code != 0 {
            handle_non_zero_exit(command, result, code, &mut outcome.messages, true);
        }
    }

    if !result.stderr.trim().is_empty() && !blocked_by_exit_code_two {
        outcome.messages.push(HookMessage::warning(format!(
            "UserPromptSubmit hook `{}` stderr: {}",
            command.command,
            result.stderr.trim()
        )));
    }

    if capture_failed {
        return;
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
            if let Some(reason) = common.decision_reason.clone().and_then(|reason| trimmed_non_empty(&reason)) {
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
            && let Some(additional) = spec.get("additionalContext").and_then(|value| value.as_str())
            && !additional.trim().is_empty()
        {
            outcome.additional_context.push(additional.trim().to_owned());
        }

        if !common.suppress_stdout
            && let Some(text) = json.get("additional_context").and_then(|value| value.as_str())
            && !text.trim().is_empty()
        {
            outcome.additional_context.push(text.trim().to_owned());
        }

        if !outcome.allow_prompt
            && !blocked_by_exit_code_two
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
            outcome.additional_context.push(result.stdout.trim().to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::lifecycle::interpret::{HookCaptureFailure, interpret_pre_tool};
    use crate::hooks::lifecycle::types::{PreToolHookDecision, PreToolHookOutcome};

    #[test]
    fn incomplete_stdout_does_not_override_exit_two_and_stderr() {
        let command = HookCommandConfig {
            kind: Default::default(),
            command: "fixture".into(),
            timeout_seconds: None,
        };
        let result = HookCommandResult {
            exit_code: Some(2),
            capture_failure: Some(HookCaptureFailure::StdoutTimeout),
            stdout: "incomplete".into(),
            stderr: "Denied by the hook".into(),
        };
        let mut prompt = UserPromptHookOutcome::default();
        interpret_user_prompt(&command, &result, &mut prompt, false);
        assert!(!prompt.allow_prompt);
        assert_eq!(prompt.block_reason.as_deref(), Some("Denied by the hook"));

        let mut pre_tool = PreToolHookOutcome {
            decision: PreToolHookDecision::Ask,
            ..Default::default()
        };
        interpret_pre_tool(&command, &result, &mut pre_tool, false);
        assert!(matches!(pre_tool.decision, PreToolHookDecision::Deny));
        assert!(pre_tool.messages.iter().any(|message| message.text == "Denied by the hook"));
    }
}
