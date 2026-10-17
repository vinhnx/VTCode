//! Headless verifier execution, owned-session waits, and bounded cleanup.

use super::AgentRunner;
use super::continuation::VerificationResult;
use super::execute_helpers::summarize_verification_output;
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::runtime::{AgentRuntime, RuntimeControl};
use crate::core::agent::task::TaskOutcome;
use anyhow::Result;
use serde_json::json;

impl AgentRunner {
    pub(super) async fn run_verification_commands(
        &self,
        commands: &[String],
        event_recorder: &mut ExecEventRecorder,
        runtime: &mut AgentRuntime,
    ) -> Result<Vec<VerificationResult>> {
        let wait_args = |session_id: &str| {
            json!({
                "action": "wait", "session_id": session_id,
                "wait_timeout_seconds": 1, "max_output_tokens": 4000,
            })
        };
        let mut results = Vec::with_capacity(commands.len());
        for command in commands {
            if self.tool_registry.completion_verifier_passed(command) {
                results.push(VerificationResult {
                    command: command.clone(),
                    success: true,
                    exit_code: Some(0),
                    output: "Reused fresh terminal verification evidence.".to_string(),
                });
                continue;
            }
            let command_event = event_recorder.command_started(command);
            let mut launched_session = self.tool_registry.pending_completion_verifier_session(command);
            let payload = if let Some(session_id) = launched_session.as_deref() {
                wait_args(session_id)
            } else {
                json!({
                    "action": "run",
                    "command": command,
                    "workdir": self._workspace.display().to_string(),
                    "yield_time_ms": 1000,
                })
            };
            let started = std::time::Instant::now();
            let ceiling = self
                .tool_registry
                .timeout_policy()
                .ceiling_for(crate::tools::registry::ToolTimeoutCategory::LongRunningCommand)
                .unwrap_or(std::time::Duration::from_secs(3600));
            let mut observed = self.tool_registry.execute_harness_command_session(payload).await;
            let mut output = String::new();
            let exit_code = loop {
                let result = match observed {
                    Ok(result) => result,
                    Err(error) => {
                        output.push_str(&format!("\nUnverified: verifier result unavailable: {error:#}"));
                        break None;
                    }
                };
                let chunk = summarize_verification_output(&result);
                // Retain bounded evidence across yielded polls.
                output.push_str(&chunk);
                if output.len() > 16_384 {
                    let mut boundary = output.len() - 16_384;
                    while !output.is_char_boundary(boundary) {
                        boundary += 1;
                    }
                    output.drain(..boundary);
                }
                let session = crate::tools::command_args::session_id_text(&result)
                    .or_else(|| {
                        result
                            .get("next_wait_args")
                            .and_then(crate::tools::command_args::session_id_text)
                    })
                    .or_else(|| {
                        result
                            .get("next_continue_args")
                            .and_then(crate::tools::command_args::session_id_text)
                    })
                    .map(str::to_owned);
                if launched_session.is_none() {
                    launched_session = session.clone();
                }
                if result.get("error").is_some()
                    || result.get("success").and_then(serde_json::Value::as_bool) == Some(false)
                    || ["cancelled", "blocked", "not_executed", "termination_requested"]
                        .iter()
                        .any(|key| result.get(key).and_then(serde_json::Value::as_bool) == Some(true))
                {
                    output.push_str("\nUnverified: verifier was denied, cancelled, or unavailable.");
                    break None;
                }
                if matches!(runtime.poll_turn_control().await, RuntimeControl::StopRequested) {
                    runtime.state.outcome = TaskOutcome::Cancelled;
                    output.push_str("\nUnverified: verification cancelled.");
                    break None;
                }
                if started.elapsed() >= ceiling {
                    output.push_str("\nUnverified: verification execution limit exhausted.");
                    break None;
                }
                if session
                    .as_deref()
                    .zip(launched_session.as_deref())
                    .is_some_and(|(returned, expected)| returned != expected)
                {
                    output.push_str("\nUnverified: verifier session identity changed.");
                    break None;
                }
                if let Some(code) = result.get("exit_code").and_then(serde_json::Value::as_i64) {
                    break i32::try_from(code).ok();
                }
                let Some(session_id) = launched_session.as_deref() else {
                    output.push_str("\nUnverified: no terminal exit or verifier session was returned.");
                    break None;
                };
                observed = self.tool_registry.execute_harness_command_session(wait_args(session_id)).await;
            };
            if exit_code.is_none()
                && let Some(session_id) = launched_session
            {
                if let Err(error) = self.tool_registry.terminate_harness_exec_session(&session_id).await {
                    output.push_str(&format!("\nVerifier cleanup: {error:#}"));
                }
                if let Err(error) = self.tool_registry.close_harness_exec_session(&session_id).await {
                    output.push_str(&format!("\nVerifier release: {error:#}"));
                }
            }
            let success = exit_code == Some(0);
            let output =
                vtcode_commons::formatting::truncate_byte_budget(&output, 16_384, "\n[verification output truncated]");
            event_recorder.command_finished(
                &command_event,
                if success {
                    crate::exec::events::CommandExecutionStatus::Completed
                } else {
                    crate::exec::events::CommandExecutionStatus::Failed
                },
                exit_code,
                &output,
            );
            results.push(VerificationResult {
                command: command.clone(),
                success,
                exit_code,
                output,
            });
            if !success {
                break;
            }
        }
        Ok(results)
    }
}

#[cfg(all(test, unix))]
mod tests;
