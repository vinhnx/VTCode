use vtcode_core::config::HookCommandConfig;

use crate::hooks::lifecycle::interpret::{HookCommandResult, handle_non_zero_exit, handle_timeout};
use crate::hooks::lifecycle::types::HookMessage;

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

#[allow(dead_code)]
pub(super) fn interpret_teammate_idle(
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
            "TeammateIdle hook `{}` stderr: {}",
            command.command,
            result.stderr.trim()
        )));
    }

    if !result.stdout.trim().is_empty() {
        messages.push(HookMessage::info(result.stdout.trim().to_owned()));
    }
}
