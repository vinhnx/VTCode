use serde_json::Value;

use vtcode_core::config::HookCommandConfig;

use crate::hooks::lifecycle::types::{
    HookMessage, HookMessageLevel, PostToolHookOutcome, PreToolHookDecision, PreToolHookOutcome,
    UserPromptHookOutcome,
};

pub(super) struct HookCommandResult {
    pub(super) exit_code: Option<i32>,
    pub(super) stdout: String,
    pub(super) stderr: String,
    pub(super) timed_out: bool,
    pub(super) timeout_seconds: u64,
}

pub(super) fn parse_json_output(stdout: &str) -> Option<Value> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }

    serde_json::from_str(trimmed).ok()
}

pub(super) struct CommonJsonFields {
    pub(super) continue_decision: Option<bool>,
    pub(super) stop_reason: Option<String>,
    pub(super) suppress_stdout: bool,
    pub(super) decision: Option<String>,
    pub(super) decision_reason: Option<String>,
    pub(super) hook_specific: Option<Value>,
}

pub(super) fn extract_common_fields(
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

    let continue_decision = json.get("continue").and_then(|value| value.as_bool());
    let stop_reason = json
        .get("stopReason")
        .and_then(|value| value.as_str())
        .map(|value| value.to_owned());
    let suppress_stdout = json
        .get("suppressOutput")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let decision = json
        .get("decision")
        .and_then(|value| value.as_str())
        .map(|value| value.to_owned());
    let decision_reason = json
        .get("reason")
        .and_then(|value| value.as_str())
        .map(|value| value.to_owned());
    let hook_specific = json.get("hookSpecificOutput").cloned();

    CommonJsonFields {
        continue_decision,
        stop_reason,
        suppress_stdout,
        decision,
        decision_reason,
        hook_specific,
    }
}

pub(super) fn matches_hook_event(spec: &serde_json::Map<String, Value>, event_name: &str) -> bool {
    match spec.get("hookEventName").and_then(|value| value.as_str()) {
        Some(name) => name.eq_ignore_ascii_case(event_name),
        None => true,
    }
}

pub(super) fn handle_timeout(
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

pub(super) fn handle_non_zero_exit(
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

fn select_message<'a>(stderr: &'a str, fallback: &'a str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

pub(super) fn interpret_session_start(
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

pub(super) fn interpret_session_end(
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

pub(super) fn interpret_user_prompt(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    outcome: &mut UserPromptHookOutcome,
) {
    handle_timeout(command, result, &mut outcome.messages);
    if result.timed_out {
        return;
    }

    if let Some(code) = result.exit_code {
        if code == 2 {
            outcome.allow_prompt = false;
            let reason = select_message(result.stderr.trim(), "Prompt blocked by lifecycle hook.");
            outcome.block_reason = Some(reason.clone());
            outcome.messages.push(HookMessage::error(reason));
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
            outcome.allow_prompt = false;
            outcome.block_reason = common
                .stop_reason
                .clone()
                .or(common.decision_reason.clone())
                .or_else(|| Some("Prompt blocked by lifecycle hook.".to_owned()));
        }

        if let Some(decision) = common.decision.as_deref()
            && decision.eq_ignore_ascii_case("block")
        {
            outcome.allow_prompt = false;
            outcome.block_reason = common
                .decision_reason
                .clone()
                .or_else(|| Some("Prompt blocked by lifecycle hook.".to_owned()));
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
        outcome
            .additional_context
            .push(result.stdout.trim().to_owned());
    }
}

pub(super) fn interpret_pre_tool(
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

pub(super) fn interpret_post_tool(
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

#[allow(dead_code)]
pub(super) fn interpret_task_completion(
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
        messages.push(HookMessage::warning(format!(
            "TaskCompletion hook `{}` stderr: {}",
            command.command,
            result.stderr.trim()
        )));
    }

    if !result.stdout.trim().is_empty() {
        messages.push(HookMessage::info(result.stdout.trim().to_owned()));
    }
}
