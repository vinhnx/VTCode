use super::recovery_guidance::{
    empty_response_notice, empty_response_recovery_mode, empty_response_recovery_reason,
    planning_empty_response_synthesis_directive, recovery_empty_fallback_safety_message,
    recovery_empty_response_fallback_message,
};
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
use crate::agent::runloop::unified::stop_requests::{await_with_stop, stop_outcome};
use crate::agent::runloop::unified::turn::context::{
    PreparedAssistantToolCall, TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext, TurnProcessingResult,
};
use crate::agent::runloop::unified::turn::guards::handle_turn_balancer;
use crate::agent::runloop::unified::turn::tool_outcomes::{ToolOutcomeContext, handle_tool_calls, helpers};
use crate::agent::runloop::unified::turn::turn_loop::{
    PENDING_VERIFICATION_BLOCK_REASON, RECOVERY_CONTRACT_VIOLATION_REASON, budget_recovery_final_response,
};

/// Result of processing a single turn.
pub(crate) struct HandleTurnProcessingResultParams<'a> {
    pub ctx: &'a mut TurnProcessingContext<'a>,
    pub processing_result: TurnProcessingResult,
    pub response_streamed: bool,
    pub step_count: usize,
    pub repeated_tool_attempts: &'a mut helpers::LoopTracker,
    pub turn_modified_files: &'a mut BTreeSet<PathBuf>,
    /// Pre-computed max tool loops limit for efficiency.
    pub max_tool_loops: usize,
    /// Pre-computed tool repeat limit for efficiency.
    pub tool_repeat_limit: usize,
}

/// Withheld completion, owned only while checks run; never projected into history.
struct CompletionDraft {
    text: String,
    reasoning: Vec<vtcode_core::llm::providers::ReasoningSegment>,
    reasoning_details: Option<Vec<String>>,
    response_streamed: bool,
    mutation_generation: u64,
    user_messages: usize,
}

impl CompletionDraft {
    fn is_current(&self, ctx: &TurnProcessingContext<'_>) -> bool {
        !ctx.runtime_steering.has_pending_follow_up_inputs()
            && self.mutation_generation == ctx.tool_registry.completion_mutation_generation()
            && self.user_messages == user_message_count(ctx.working_history)
    }
}

fn user_message_count(history: &[uni::Message]) -> usize {
    history.iter().filter(|message| message.role == uni::MessageRole::User).count()
}

fn should_suppress_pre_tool_result_claim(assistant_text: &str, tool_calls: &[PreparedAssistantToolCall]) -> bool {
    if assistant_text.trim().is_empty() {
        return false;
    }
    if !tool_calls.iter().any(PreparedAssistantToolCall::is_command_execution) {
        return false;
    }

    let lower = assistant_text.to_ascii_lowercase();
    [
        "found ",
        "warning",
        "warnings",
        "error",
        "errors",
        "passed",
        "failed",
        "no issues",
        "completed successfully",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}
fn record_assistant_tool_calls(
    history: &mut Vec<uni::Message>,
    tool_calls: &[PreparedAssistantToolCall],
    history_len_before_assistant: usize,
) {
    if tool_calls.is_empty() {
        return;
    }

    let raw_tool_calls = tool_calls
        .iter()
        .map(|tool_call| tool_call.raw_call().clone())
        .collect::<Vec<_>>();

    let appended_assistant_message = history.len() > history_len_before_assistant
        && history
            .last()
            .is_some_and(|message| message.role == uni::MessageRole::Assistant && message.tool_calls.is_none());

    if appended_assistant_message {
        if let Some(last) = history.last_mut() {
            last.tool_calls = Some(raw_tool_calls);
            last.phase = Some(uni::AssistantPhase::Commentary);
        }
        return;
    }

    // Preserve call/output pairing even when the assistant text was merged into
    // a prior message or omitted; OpenAI-compatible providers require tool call IDs.
    history.push(
        uni::Message::assistant_with_tools(String::new(), raw_tool_calls)
            .with_phase(Some(uni::AssistantPhase::Commentary)),
    );
}

/// Outcome of accounting for one text response while verification is pending.
pub(crate) enum PendingVerificationTextOutcome {
    /// End the turn as verification-blocked.
    Block { reason: String },
    /// Execute the missing completion check through the normal tool pipeline.
    AutoVerify { command: String },
}

impl TurnProcessingContext<'_> {
    /// Account for a text response while verification is pending.
    ///
    /// The answer is withheld until missing checks finish. The registry owns
    /// freshness; failure ends this attempt and the session owns one correction.
    /// Exhausted execution budgets block without reminder rounds.
    ///
    /// Tool-free recovery synthesis bypasses this accounting entirely (see the
    /// caller): with tools disabled no text could verify, so recovery budgets
    /// govern those responses instead.
    pub(crate) fn handle_pending_verification_text_response(
        &mut self,
        repeated_tool_attempts: &mut helpers::LoopTracker,
        _assistant_text: &str,
    ) -> Result<PendingVerificationTextOutcome> {
        repeated_tool_attempts.mark_verification_pending();
        if let Some(reason) = self.verification_execution_blocker(repeated_tool_attempts) {
            return Ok(PendingVerificationTextOutcome::Block { reason });
        }
        // A tool-free answer is a completion attempt. Editing and inspection
        // never schedule checks; run only the registry's missing verifier here.
        if let Some(command) = self.try_harness_auto_verify(repeated_tool_attempts) {
            return Ok(PendingVerificationTextOutcome::AutoVerify { command });
        }
        Ok(PendingVerificationTextOutcome::Block {
            reason: self.tool_registry.unverified_completion_reason(),
        })
    }

    fn verification_execution_blocker(&self, tracker: &helpers::LoopTracker) -> Option<String> {
        if self.harness_state.recovery_is_tool_free() {
            return Some(
                "Verification is still pending: tools are disabled for recovery synthesis. No verifier was scheduled."
                    .to_string(),
            );
        }
        let budget_notice = self
            .harness_state
            .wall_clock_budget_exhaustion()
            .map(|budget| budget.synthesis_directive_message())
            .or_else(|| {
                tracker
                    .pending_verifier_session_id
                    .is_none()
                    .then(|| self.harness_state.tool_budget_exhaustion())
                    .flatten()
                    .map(|budget| budget.synthesis_directive_message())
            })?;
        let cause = crate::agent::runloop::unified::run_loop_context::budget_exhaustion_cause(&budget_notice)?;
        Some(format!(
            "{cause}. Changes remain unverified; no additional verifier was scheduled. Resume with fresh execution budget and retain existing check evidence."
        ))
    }

    /// Shared gate for harness-executed verification: kill-switch,
    /// never-passing escalation budget, per-turn one-shot, and a resolvable
    /// command must ALL pass, else `None` (caller falls back to cap
    /// accounting or the manual handoff). Pure decision — no side effects —
    /// so both the cap-exhaustion path and the completion-claim fast path
    /// share one fail-closed predicate.
    fn try_harness_auto_verify(&mut self, repeated_tool_attempts: &mut helpers::LoopTracker) -> Option<String> {
        let max_failures = helpers::verification_max_consecutive_failures(self.vt_cfg);
        if helpers::verification_auto_execute_enabled(self.vt_cfg)
            && self.verification_execution_blocker(repeated_tool_attempts).is_none()
            && self.session_stats.verification_consecutive_failures() < max_failures
            && repeated_tool_attempts.should_auto_execute_verifier()
        {
            let command =
                helpers::resolve_pending_verifier_command(self.vt_cfg, self.tool_registry, self.working_history);
            return command.or_else(|| {
                repeated_tool_attempts
                    .pending_verifier_session_id
                    .as_ref()
                    .map(|_| "pending verifier".to_string())
            });
        }
        None
    }
}

/// Find the latest tool response for `call_id` within `history[window_start..]`,
/// if any. The window must start where the current execution's assistant
/// message was appended: the harness reuses one fixed call id across turns,
/// so an unbounded scan could attribute a previous turn's response to this
/// execution and inflate the never-passing escalation counter for work that
/// never ran. Out-of-range starts yield `None` (fail-safe: a missed count,
/// never a phantom one).
/// Used to attribute the harness auto-verification outcome (and bound its
/// failure excerpt) without threading pipeline internals through the caller.
fn last_tool_response_text(history: &[uni::Message], call_id: &str, window_start: usize) -> Option<String> {
    history.get(window_start..)?.iter().rev().find_map(|message| {
        (message.role == uni::MessageRole::Tool && message.tool_call_id.as_deref() == Some(call_id))
            .then(|| message.content.as_text().to_string())
    })
}

fn verifier_response_has_nonzero_exit(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| value.get("exit_code").and_then(serde_json::Value::as_i64))
        .is_some_and(|code| code != 0)
}

/// Only a fresh yielded result for this owned session permits another wait.
/// A retained handle alone cannot prove that the latest admission succeeded.
fn verifier_response_is_running_session(text: &str, session_id: &str) -> bool {
    use vtcode_core::tools::command_args::session_id_text;
    let Ok(output) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    let returned_session = session_id_text(&output)
        .or_else(|| output.get("next_wait_args").and_then(session_id_text))
        .or_else(|| output.get("next_continue_args").and_then(session_id_text));
    returned_session == Some(session_id)
        && output.get("exit_code").and_then(serde_json::Value::as_i64).is_none()
        && output.get("error").is_none()
        && output.get("success").and_then(serde_json::Value::as_bool) != Some(false)
        && ![
            "cancelled",
            "blocked",
            "not_executed",
            "termination_requested",
            "reused_recent_result",
        ]
        .iter()
        .any(|key| output.get(*key).and_then(serde_json::Value::as_bool) == Some(true))
}

/// Execute a missing check on the harness's behalf at attempted completion.
///
/// The synthesized call flows through the normal [`handle_tool_calls`]
/// pipeline — mutation guard, validation, permissions, budget, repetition
/// tracker — so harness execution can neither bypass policy nor corrupt gate
/// accounting: a model-issued verifier and this call converge on identical
/// `LoopTracker`/`SessionStats` transitions. Outcomes:
///
/// - Gate cleared → record success (drops the consecutive-failure count),
///   note it in history, and continue the turn.
/// - Gate still pending → preserve bounded failure evidence and end this
///   completion attempt. The session owns the request's corrective pass.
/// - Break outcome from the pipeline (exit, cancel, budget synthesis) →
///   bookkeep the same way, then honor it.
/// - No tool response landed (pre-flight rejection, guard block) → do not
///   count it as a verifier failure; fall through to the turn balancer so a
///   denial surfaces through the established recovery paths instead of
///   polluting the escalation counter.
pub(crate) async fn execute_harness_auto_verification(
    ctx: &mut TurnProcessingContext<'_>,
    repeated_tool_attempts: &mut helpers::LoopTracker,
    turn_modified_files: &mut BTreeSet<PathBuf>,
    step_count: usize,
    max_tool_loops: usize,
    tool_repeat_limit: usize,
    command: String,
) -> Result<TurnHandlerOutcome> {
    use vtcode_core::config::constants::tools as tool_names;

    // A fresh internal turn may still own the previous turn's verifier. Reuse
    // a current-generation execution covering this check, never archived handles.
    let previous_verifier_session = repeated_tool_attempts.pending_verifier_session_id.clone();
    repeated_tool_attempts.pending_verifier_session_id =
        ctx.tool_registry.pending_completion_verifier_session(&command);
    if repeated_tool_attempts.pending_verifier_session_id != previous_verifier_session {
        // A different (or finished) verifier starts a fresh wait window.
        repeated_tool_attempts.reset_verifier_wait_polls();
    }

    if let Some(reason) = ctx.verification_execution_blocker(repeated_tool_attempts) {
        return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }));
    }
    let pending_session = repeated_tool_attempts.pending_verifier_session_id.clone();
    let (tool_name, args) = match pending_session {
        Some(session_id) => {
            // The wait returns as soon as the verifier produces output, so a
            // growing window cuts redundant polls without delaying detection.
            let wait_timeout_seconds = repeated_tool_attempts.next_verifier_wait_timeout_secs();
            (
                tool_names::WRITE_STDIN,
                serde_json::json!({"session_id": session_id, "action": "wait", "wait_timeout_seconds": wait_timeout_seconds, "max_output_tokens": 4000}),
            )
        }
        None => (
            tool_names::EXEC_COMMAND,
            serde_json::json!({"cmd": command, "yield_time_ms": 1000, "max_output_tokens": 4000}),
        ),
    };

    let required_before = ctx.tool_registry.next_required_completion_verifier();
    let announce = !repeated_tool_attempts.auto_verification_executed;
    repeated_tool_attempts.record_auto_verification_executed();
    let action = if tool_name == tool_names::WRITE_STDIN {
        "waiting for the existing verifier"
    } else {
        "running the project verifier"
    };
    if announce {
        ctx.renderer
            .line(MessageStyle::Info, &format!("Harness auto-verification: {action}: `{command}`"))
            .unwrap_or(());
        ctx.working_history.push(uni::Message::system(format!(
            "Harness auto-verification: {action}: `{command}` via {tool_name} (output capped). \
            This call is harness-issued autonomous recovery, not a model action; its result carries the same weight as a model-run verifier."
        )));
    }

    let raw_call = uni::ToolCall::function(
        helpers::HARNESS_AUTO_VERIFY_CALL_ID.to_string(),
        tool_name.to_string(),
        args.to_string(),
    );
    let synthetic = PreparedAssistantToolCall::new(raw_call);
    if synthetic.args().is_none() {
        // We built the JSON above; absence means a serialization regression.
        // Fail closed to the manual handoff rather than executing blind.
        return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
            reason: Some(PENDING_VERIFICATION_BLOCK_REASON.to_string()),
        }));
    }
    let history_len_before_assistant = ctx.working_history.len();
    record_assistant_tool_calls(ctx.working_history, std::slice::from_ref(&synthetic), history_len_before_assistant);

    let break_outcome = {
        let mut t_ctx_inner = ToolOutcomeContext {
            ctx: &mut *ctx,
            repeated_tool_attempts: &mut *repeated_tool_attempts,
            turn_modified_files: &mut *turn_modified_files,
        };
        let outcome = handle_tool_calls(&mut t_ctx_inner, std::slice::from_ref(&synthetic)).await?;
        let passed = t_ctx_inner.ctx.tool_registry.completion_verifier_passed(&command);
        let pending = t_ctx_inner.ctx.tool_registry.completion_verification_pending();
        if passed && !pending {
            t_ctx_inner.repeated_tool_attempts.mark_verification_complete();
        }
        if t_ctx_inner.repeated_tool_attempts.pending_verifier_session_id.is_some() {
            // A running session has no verdict and must not consume failure budget.
        } else if passed && pending {
            t_ctx_inner.repeated_tool_attempts.mark_verification_pending();
            if required_before.as_deref() == Some(command.as_str())
                && t_ctx_inner.ctx.tool_registry.next_required_completion_verifier() != required_before
            {
                // Advancing a mandatory check permits execution of the next
                // one, without replenishing reminder or cross-turn budgets.
                t_ctx_inner.repeated_tool_attempts.auto_verification_executed = false;
            }
            t_ctx_inner
                .ctx
                .working_history
                .push(uni::Message::system(t_ctx_inner.ctx.tool_registry.unverified_completion_reason()));
        } else if t_ctx_inner.repeated_tool_attempts.verification_is_pending() {
            // Count only an observed non-zero exit. Policy/preflight rejections
            // and missing results are not executed verification failures.
            if let Some(response_text) = last_tool_response_text(
                t_ctx_inner.ctx.working_history,
                helpers::HARNESS_AUTO_VERIFY_CALL_ID,
                history_len_before_assistant,
            )
            .filter(|text| verifier_response_has_nonzero_exit(text))
            {
                let failures = t_ctx_inner
                    .ctx
                    .session_stats
                    .record_verification_auto_failure(command.clone(), &response_text);
                t_ctx_inner.ctx.working_history.push(uni::Message::system(format!(
                    "Harness auto-verification `{command}` did not clear the gate (consecutive failure {failures} this episode). \
                    Repair the reported failure before attempting completion again."
                )));
            }
        } else if t_ctx_inner.ctx.tool_registry.completion_verification_pending() {
            // A required check remains, or this verifier predates a later edit.
            t_ctx_inner.repeated_tool_attempts.mark_verification_pending();
            t_ctx_inner
                .ctx
                .working_history
                .push(uni::Message::system(t_ctx_inner.ctx.tool_registry.unverified_completion_reason()));
        } else {
            t_ctx_inner.ctx.session_stats.record_verification_auto_success();
            t_ctx_inner.ctx.working_history.push(uni::Message::system(format!(
                "Harness auto-verification `{command}` exited 0; the verification gate is cleared. Publish the retained completion once."
            )));
        }
        t_ctx_inner
            .ctx
            .session_stats
            .set_verification_snapshot(t_ctx_inner.repeated_tool_attempts.verification_snapshot());
        outcome
    };

    if let Some(res) = break_outcome {
        return Ok(res);
    }

    if let Some(session_id) = repeated_tool_attempts.pending_verifier_session_id.as_deref() {
        let response = last_tool_response_text(
            ctx.working_history,
            helpers::HARNESS_AUTO_VERIFY_CALL_ID,
            history_len_before_assistant,
        );
        if !response
            .as_deref()
            .is_some_and(|text| verifier_response_is_running_session(text, session_id))
        {
            let evidence = response.as_deref().unwrap_or("No current verifier wait result was observed.");
            let evidence = vtcode_commons::formatting::truncate_byte_budget(evidence, 1024, " [result truncated]");
            return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
                reason: Some(format!(
                    "Verification is still pending: verifier admission or wait did not produce a fresh running result. {evidence}"
                )),
            }));
        }
    }

    if ctx.tool_registry.completion_verification_pending()
        && repeated_tool_attempts.pending_verifier_session_id.is_none()
        && repeated_tool_attempts.auto_verification_executed
    {
        return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
            reason: Some(ctx.tool_registry.unverified_completion_reason()),
        }));
    }

    // Mirror the ToolCalls branch: run the balancer before continuing so
    // navigation churn converging during verification still converges.
    Ok(handle_turn_balancer(ctx, step_count, repeated_tool_attempts, max_tool_loops, tool_repeat_limit).await)
}

/// Match the headless verifier's per-command ceiling and owned-session cleanup.
/// Explicit waits do not spend the turn budgets, so they cannot supply this bound.
async fn block_verification_execution_limit(
    ctx: &mut TurnProcessingContext<'_>,
    tracker: &mut helpers::LoopTracker,
    command: &str,
) -> TurnHandlerOutcome {
    let mut reason = format!(
        "Verification is still pending: execution limit exhausted for `{command}`. The completion draft was withheld."
    );
    if let Some(session_id) = ctx.tool_registry.pending_completion_verifier_session(command) {
        if let Err(error) = ctx.tool_registry.terminate_harness_exec_session(&session_id).await {
            reason.push_str(&format!(" Verifier cleanup: {error:#}"));
        }
        if let Err(error) = ctx.tool_registry.close_harness_exec_session(&session_id).await {
            reason.push_str(&format!(" Verifier release: {error:#}"));
        }
    }
    tracker.pending_verifier_session_id = ctx.tool_registry.pending_completion_verifier_session(command);
    ctx.session_stats.set_verification_snapshot(tracker.verification_snapshot());
    if let Some(stop) = stop_outcome(ctx.ctrl_c_state) {
        return TurnHandlerOutcome::Break(stop);
    }
    TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
        reason: Some(vtcode_commons::formatting::truncate_byte_budget(&reason, 2048, " [cleanup details truncated]")),
    })
}

/// Dispatch the appropriate response handler based on the processing result.
pub(crate) async fn handle_turn_processing_result<'a>(
    params: HandleTurnProcessingResultParams<'a>,
) -> Result<TurnHandlerOutcome> {
    match params.processing_result {
        TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text,
            reasoning,
            reasoning_details,
        } => {
            if params.ctx.is_recovery_active() && params.ctx.recovery_pass_used() && params.ctx.recovery_is_tool_free()
            {
                // Preserve any accompanying prose so an exhausted-retries
                // fallback can salvage it instead of discarding the turn.
                if params.ctx.is_planning_active() {
                    if params.ctx.try_planning_violation_repair(&assistant_text)? {
                        return Ok(TurnHandlerOutcome::Continue);
                    }
                    return params
                        .ctx
                        .break_planning_recovery_with_handoff(
                            "the synthesis response attempted a tool call while tools were disabled",
                            (!assistant_text.trim().is_empty()).then_some(assistant_text.as_str()),
                        )
                        .await;
                }
                params
                    .ctx
                    .harness_state
                    .record_recovery_rejected_synthesis(assistant_text.trim().to_string());
                return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
                    reason: Some(RECOVERY_CONTRACT_VIOLATION_REASON.to_string()),
                }));
            }

            let assistant_text = if should_suppress_pre_tool_result_claim(&assistant_text, &tool_calls) {
                String::new()
            } else {
                assistant_text
            };
            let assistant_text_len = assistant_text.len();
            let reasoning_segments = reasoning.len();
            let reasoning_details_count = reasoning_details.as_ref().map_or(0, Vec::len);
            let history_len_before_assistant = params.ctx.working_history.len();
            params.ctx.handle_assistant_response(
                assistant_text,
                reasoning,
                reasoning_details,
                params.response_streamed,
                Some(uni::AssistantPhase::Commentary),
            )?;
            record_assistant_tool_calls(params.ctx.working_history, &tool_calls, history_len_before_assistant);
            tracing::info!(
                target: "vtcode.turn.metrics",
                metric = "tool_call_turn_start",
                run_id = %params.ctx.harness_state.run_id.0,
                turn_id = %params.ctx.harness_state.turn_id.0,
                tool_calls = tool_calls.len(),
                assistant_text_len,
                reasoning_segments,
                reasoning_details = reasoning_details_count,
                history_len = params.ctx.working_history.len(),
                "turn metric"
            );

            let outcome = {
                let mut t_ctx_inner = ToolOutcomeContext {
                    ctx: &mut *params.ctx,
                    repeated_tool_attempts: &mut *params.repeated_tool_attempts,
                    turn_modified_files: &mut *params.turn_modified_files,
                };

                handle_tool_calls(&mut t_ctx_inner, &tool_calls).await?
            };

            if let Some(res) = outcome {
                tracing::info!(
                    target: "vtcode.turn.metrics",
                    metric = "tool_call_turn_outcome",
                    run_id = %params.ctx.harness_state.run_id.0,
                    turn_id = %params.ctx.harness_state.turn_id.0,
                    outcome = "direct_break",
                    "turn metric"
                );
                return Ok(res);
            }

            let balancer_outcome = handle_turn_balancer(
                &mut *params.ctx,
                params.step_count,
                &mut *params.repeated_tool_attempts,
                params.max_tool_loops,
                params.tool_repeat_limit,
            )
            .await;
            tracing::info!(
                target: "vtcode.turn.metrics",
                metric = "tool_call_turn_outcome",
                run_id = %params.ctx.harness_state.run_id.0,
                turn_id = %params.ctx.harness_state.turn_id.0,
                outcome = match &balancer_outcome {
                    TurnHandlerOutcome::Continue => "continue",
                    TurnHandlerOutcome::Break(_) => "break",
                    TurnHandlerOutcome::SwitchPrimaryAgent(_) => "switch_primary_agent",
                    TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy { .. } => "switch_primary_agent",
                    TurnHandlerOutcome::BreakWithPolicy { .. } => "break",
                },
                "turn metric"
            );
            Ok(balancer_outcome)
        }
        TurnProcessingResult::TextResponse { text, reasoning, reasoning_details, proposed_plan } => {
            // Planning synthesis makes no workspace mutation, so a verification
            // checkpoint carried from an earlier build turn must not block the
            // `<proposed_plan>`. Without this, plan mode deadlocks: research
            // completes, but the plan draft counts towards the unverified-text
            // cap and the turn blocks every time.
            let is_planning_synthesis = proposed_plan.is_some() || params.ctx.is_planning_active();
            // Tool-free recovery synthesis cannot verify (tools are disabled
            // at the API level), so its texts bypass verification accounting
            // entirely: counting them toward the verification cap would punish
            // the model for obeying the recovery contract. Recovery budgets
            // bound this path instead, and the generic cap still refuses
            // unverified completion.
            let tool_free_synthesis = params.ctx.in_tool_free_recovery_synthesis();
            if tool_free_synthesis && params.max_tool_loops > 0 && params.step_count >= params.max_tool_loops {
                params.ctx.session_stats.navigation_recovery.consume();
            }
            if !is_planning_synthesis && params.ctx.tool_registry.completion_verification_pending() {
                params.repeated_tool_attempts.mark_verification_pending();
            } else if params.ctx.tool_registry.has_current_completion_verification()
                || params.ctx.tool_registry.next_required_completion_verifier().is_none()
            {
                // Optional checker diagnostics cannot create new obligations;
                // retain explicit pending state when required checks remain.
                params.repeated_tool_attempts.mark_verification_complete();
            }
            if params.ctx.tool_registry.completion_verification_pending()
                && !is_planning_synthesis
                && !tool_free_synthesis
            {
                match params
                    .ctx
                    .handle_pending_verification_text_response(params.repeated_tool_attempts, &text)?
                {
                    PendingVerificationTextOutcome::Block { reason } => {
                        return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }));
                    }
                    PendingVerificationTextOutcome::AutoVerify { command } => {
                        let mut draft = CompletionDraft {
                            text,
                            reasoning,
                            reasoning_details,
                            response_streamed: params.response_streamed,
                            mutation_generation: params.ctx.tool_registry.completion_mutation_generation(),
                            user_messages: user_message_count(params.ctx.working_history),
                        };
                        let mut command = command;
                        let mut results = Vec::new();
                        let execution_ceiling = params
                            .ctx
                            .tool_registry
                            .timeout_policy()
                            .ceiling_for(vtcode_core::tools::registry::ToolTimeoutCategory::LongRunningCommand)
                            .unwrap_or(Duration::from_secs(3600));
                        let mut verifier_started_at: Option<Instant> = None;
                        loop {
                            let control = await_with_stop(
                                params.ctx.ctrl_c_state,
                                params.ctx.ctrl_c_notify,
                                params.ctx.runtime_steering.poll_tool_control(),
                            )
                            .await;
                            if let Some(stop) = stop_outcome(params.ctx.ctrl_c_state) {
                                return Ok(TurnHandlerOutcome::Break(stop));
                            }
                            if control.is_none()
                                || control == Some(vtcode_core::core::agent::runtime::RuntimeControl::StopRequested)
                            {
                                return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled));
                            }
                            if !draft.is_current(params.ctx) {
                                // New steering or code invalidates the answer, not the owned check evidence.
                                return Ok(TurnHandlerOutcome::Continue);
                            }
                            if verifier_started_at.is_some_and(|started| started.elapsed() >= execution_ceiling) {
                                return Ok(block_verification_execution_limit(
                                    params.ctx,
                                    params.repeated_tool_attempts,
                                    &command,
                                )
                                .await);
                            }
                            if !params.ctx.tool_registry.completion_verification_pending() {
                                if !results.is_empty() {
                                    draft.text.push_str("\n\nHarness verification: ");
                                    draft.text.push_str(&results.join("; "));
                                    draft.text.push('.');
                                }
                                return params
                                    .ctx
                                    .handle_text_response(
                                        draft.text,
                                        draft.reasoning,
                                        draft.reasoning_details,
                                        None,
                                        draft.response_streamed,
                                    )
                                    .await;
                            }
                            let started = *verifier_started_at.get_or_insert_with(Instant::now);
                            let outcome = execute_harness_auto_verification(
                                &mut *params.ctx,
                                &mut *params.repeated_tool_attempts,
                                &mut *params.turn_modified_files,
                                params.step_count,
                                params.max_tool_loops,
                                params.tool_repeat_limit,
                                command.clone(),
                            )
                            .await?;
                            if !matches!(outcome, TurnHandlerOutcome::Continue) {
                                return Ok(outcome);
                            }
                            if started.elapsed() >= execution_ceiling {
                                return Ok(block_verification_execution_limit(
                                    params.ctx,
                                    params.repeated_tool_attempts,
                                    &command,
                                )
                                .await);
                            }
                            if params.ctx.tool_registry.completion_verifier_passed(&command) {
                                results.push(format!("`{command}` exited 0"));
                                if let Some(next) = params.ctx.tool_registry.next_required_completion_verifier() {
                                    command = next;
                                    verifier_started_at = None;
                                }
                            }
                            // Running checks are resumed through write_stdin on the next iteration.
                            // Terminal failures and denied/lost results returned a blocked outcome above.
                        }
                    }
                }
            }

            if params.ctx.is_recovery_active()
                && params.ctx.recovery_pass_used()
                && params.ctx.recovery_is_tool_free()
                && (crate::agent::runloop::text_tools::detect_textual_tool_call(&text).is_some()
                    || crate::agent::runloop::text_tools::contains_pseudo_tool_call_markers(&text))
            {
                // A complete, parseable textual tool call is an action attempt,
                // not a synthesis. Publishing the stripped preamble ("Applying
                // the fix now.") as the final answer would fabricate completion
                // while doing no work, so complete calls skip the prose-salvage
                // attempts below and take the contract-violation path: the
                // bounded retry asks for a plain-text summary, and the salvaged
                // prose feeds the labeled fallback instead of the canned answer.
                let attempted_complete_tool_call =
                    crate::agent::runloop::text_tools::detect_textual_tool_call(&text).is_some();
                if !attempted_complete_tool_call {
                    let cleaned = crate::agent::runloop::text_tools::strip_dsml_markup(&text).trim().to_string();
                    // If DSML stripping produced a clean, markup-free text, use it.
                    // Otherwise try stripping the entire detected non-DSML tool-call
                    // region while preserving surrounding prose.
                    if !cleaned.is_empty()
                        && crate::agent::runloop::text_tools::detect_textual_tool_call(&cleaned).is_none()
                        && !crate::agent::runloop::text_tools::contains_pseudo_tool_call_markers(&cleaned)
                    {
                        let _ = params
                            .ctx
                            .renderer
                            .line(MessageStyle::Info, "[i] Cleaned recovery response (removed tool-call markup).");
                        return params
                            .ctx
                            .handle_text_response(
                                cleaned,
                                reasoning,
                                reasoning_details,
                                proposed_plan,
                                params.response_streamed,
                            )
                            .await;
                    }
                    let cleaned = crate::agent::runloop::text_tools::strip_textual_tool_call_regions(&text)
                        .trim()
                        .to_string();
                    if !cleaned.is_empty()
                        && crate::agent::runloop::text_tools::detect_textual_tool_call(&cleaned).is_none()
                        && !crate::agent::runloop::text_tools::contains_pseudo_tool_call_markers(&cleaned)
                    {
                        let _ = params
                            .ctx
                            .renderer
                            .line(MessageStyle::Info, "[i] Cleaned recovery response (removed tool-call markup).");
                        return params
                            .ctx
                            .handle_text_response(
                                cleaned,
                                reasoning,
                                reasoning_details,
                                proposed_plan,
                                params.response_streamed,
                            )
                            .await;
                    }
                }
                // Both cleanup attempts failed (or the response attempted a
                // complete tool call, which skips the attempts above). Salvage
                // the best-effort stripped prose so an exhausted-retries
                // fallback can use it instead of the canned answer.
                let salvage = crate::agent::runloop::text_tools::strip_textual_tool_call_regions(
                    &crate::agent::runloop::text_tools::strip_dsml_markup(&text),
                )
                .trim()
                .to_string();
                if params.ctx.is_planning_active() {
                    if params.ctx.try_planning_violation_repair(&salvage)? {
                        return Ok(TurnHandlerOutcome::Continue);
                    }
                    return params
                        .ctx
                        .break_planning_recovery_with_handoff(
                            "the synthesis response attempted tool-call markup",
                            (!salvage.is_empty()).then_some(salvage.as_str()),
                        )
                        .await;
                }
                params.ctx.harness_state.record_recovery_rejected_synthesis(salvage);
                return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
                    reason: Some(RECOVERY_CONTRACT_VIOLATION_REASON.to_string()),
                }));
            }

            params
                .ctx
                .handle_text_response(text, reasoning, reasoning_details, proposed_plan, params.response_streamed)
                .await
        }
        TurnProcessingResult::Refusal { reason } => {
            tracing::warn!(reason = %reason, "Provider refused the turn; ending it without recovery retries.");
            params.ctx.harness_state.mark_turn_refused();
            Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }))
        }
        TurnProcessingResult::Empty { facts } => {
            if params.ctx.is_recovery_active() && params.ctx.recovery_pass_used() {
                let recovery_mode = if params.ctx.recovery_is_tool_free() {
                    RecoveryMode::ToolFreeSynthesis
                } else {
                    RecoveryMode::ToolEnabledRetry
                };

                // Planning gets one deterministic tool-free synthesis after
                // the second empty response. Do not spend another retry here:
                // an empty synthesis must become a resumable blocked handoff,
                // not another request cycle.
                if params.ctx.is_planning_active() && matches!(recovery_mode, RecoveryMode::ToolEnabledRetry) {
                    params.ctx.finish_recovery_pass();
                    params.ctx.switch_to_tool_free_recovery();
                    let directive = planning_empty_response_synthesis_directive(
                        params.ctx.working_history,
                        params.ctx.tool_registry.workspace_root().as_path(),
                    );
                    params.ctx.push_system_message(directive);
                    params
                        .ctx
                        .renderer
                        .line(
                            MessageStyle::Info,
                            "[!] Two empty planning responses detected; scheduling one tool-free plan synthesis pass.",
                        )
                        .unwrap_or(());
                    tracing::warn!(
                        "Two empty planning responses received; scheduling bounded tool-free plan synthesis."
                    );
                    return Ok(TurnHandlerOutcome::Continue);
                }

                if params.ctx.is_planning_active() && matches!(recovery_mode, RecoveryMode::ToolFreeSynthesis) {
                    // Every response failure uses the same persisted admission
                    // allowance; private reasoning never becomes a draft.
                    if params.ctx.try_planning_empty_output_salvage(&facts)? {
                        return Ok(TurnHandlerOutcome::Continue);
                    }
                    // Keep the detail honest: only a response with neither
                    // tokens nor tool calls may claim plain emptiness.
                    // Structured classification retains the actual failure shape.
                    let detail = if facts.dropped_tool_calls {
                        "the synthesis response attempted native tool calls on a tool-free pass".to_string()
                    } else if facts.reasoning_only {
                        "the synthesis returned private reasoning without final content".to_string()
                    } else if facts.has_completion_tokens() {
                        crate::agent::runloop::unified::turn::context::planning_tokens_without_content_handoff_detail(
                            facts.completion_tokens,
                        )
                    } else {
                        crate::agent::runloop::unified::turn::context::PLANNING_RECOVERY_EMPTY_SYNTHESIS_DETAIL
                            .to_string()
                    };
                    return params.ctx.break_planning_recovery_with_handoff(&detail, None).await;
                }
                let recovery_reason = if params.ctx.recovery_is_tool_free() {
                    "Recovery mode requested a final synthesis pass, but the model returned no answer."
                } else {
                    "Recovery retry requested another autonomous pass, but the model still returned no answer."
                };
                let fallback_message = recovery_empty_response_fallback_message(recovery_mode);

                let final_fallback = if fallback_message.trim().is_empty() {
                    recovery_empty_fallback_safety_message(recovery_mode)
                } else {
                    fallback_message
                };
                let final_fallback =
                    budget_recovery_final_response(params.ctx.is_planning_active(), params.ctx.harness_state)
                        .unwrap_or(final_fallback);
                params.ctx.harness_state.mark_final_response_fallback();
                params.ctx.handle_assistant_response(
                    final_fallback.clone(),
                    Vec::new(),
                    None,
                    false,
                    Some(uni::AssistantPhase::FinalAnswer),
                )?;
                params.ctx.finish_recovery_pass();
                tracing::warn!(
                    mode = ?recovery_mode,
                    reason = recovery_reason,
                    fallback_chars = final_fallback.len(),
                    "Recovery pass returned no content; emitted deterministic fallback answer."
                );
                return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed {
                    plan_approved_execution_pending: false,
                }));
            }

            let recovery_mode = empty_response_recovery_mode(
                params.ctx.working_history,
                params.ctx.is_planning_active(),
                params.ctx.is_approved_plan_execution(),
            );
            let recovery_reason = empty_response_recovery_reason(recovery_mode).to_string();
            params.ctx.activate_recovery_with_mode(recovery_reason.clone(), recovery_mode);
            params
                .ctx
                .renderer
                .line(MessageStyle::Info, empty_response_notice(recovery_mode))
                .unwrap_or(());
            params.ctx.working_history.push(uni::Message::system(recovery_reason));

            Ok(TurnHandlerOutcome::Continue)
        }
    }
}

#[cfg(test)]
mod tests;
