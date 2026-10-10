use super::recovery_guidance::{
    empty_response_notice, empty_response_recovery_mode, empty_response_recovery_reason,
    planning_empty_response_synthesis_directive, recovery_empty_fallback_safety_message,
    recovery_empty_response_fallback_message,
};
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
use crate::agent::runloop::unified::turn::context::{
    PreparedAssistantToolCall, TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext, TurnProcessingResult,
};
use crate::agent::runloop::unified::turn::guards::handle_turn_balancer;
use crate::agent::runloop::unified::turn::tool_outcomes::{ToolOutcomeContext, handle_tool_calls, helpers};
use crate::agent::runloop::unified::turn::turn_loop::{
    MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN, PENDING_VERIFICATION_BLOCK_REASON, RECOVERY_CONTRACT_VIOLATION_REASON,
    budget_recovery_final_response,
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
    /// Keep the turn alive (under the text cap, or a directive retry grant).
    Continue,
    /// End the turn as verification-blocked.
    Block { reason: String },
    /// The text cap was reached: the harness should execute `command`
    /// itself through the normal tool pipeline instead of blocking.
    AutoVerify { command: String },
}

/// Whether a pending-gate text response claims the work is done. A completion
/// claim without verification is the highest-risk moment in the gate's
/// lifecycle — the model is asserting success with no evidence — so claims
/// jump straight to harness verification instead of spending directive
/// rounds. Deliberately recall-biased: a false positive costs one safe,
/// bounded verifier execution, while a false negative just falls back to the
/// ordinary cap accounting. Mirrors the result-claim marker philosophy of
/// `should_suppress_pre_tool_result_claim` (same vocabulary family).
fn is_completion_claim_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "is complete",
        "is done",
        "is finished",
        "are complete",
        "are done",
        "completed",
        "all done",
        "good to go",
        "ready for review",
        "task complete",
        "work complete",
        "implementation complete",
        "successfully",
        "verified",
        "tests pass",
        "test passes",
        "build passes",
        "no errors",
        "working correctly",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

impl TurnProcessingContext<'_> {
    /// Account for a text response while verification is pending.
    ///
    /// The model response is intentionally not stored or rendered. Under the
    /// shared per-turn cap the turn continues, then the harness runs the
    /// verifier once or waits for an existing verifier. Directive retries are
    /// the fallback when autonomous execution is disabled or unavailable.
    /// Exhausted execution budgets block immediately without reminder rounds.
    ///
    /// Tool-free recovery synthesis bypasses this accounting entirely (see the
    /// caller): with tools disabled no text could verify, so recovery budgets
    /// govern those responses instead.
    pub(crate) fn handle_pending_verification_text_response(
        &mut self,
        repeated_tool_attempts: &mut helpers::LoopTracker,
        assistant_text: &str,
    ) -> Result<PendingVerificationTextOutcome> {
        repeated_tool_attempts.mark_verification_pending();
        if let Some(reason) = self.verification_execution_blocker(repeated_tool_attempts) {
            return Ok(PendingVerificationTextOutcome::Block { reason });
        }
        if !repeated_tool_attempts.verification_warning_emitted {
            // When the failed-verifier fix window is active the verifier already
            // ran and failed; the generic "run verification" notice would be
            // circular (it demanded verification the model already attempted
            // while its fix attempts got no feedback). Say the verifier failed
            // and bounded fix edits are granted instead.
            let (warning, directive) = if repeated_tool_attempts.fix_edits_remaining > 0 {
                (helpers::FAILED_VERIFICATION_FIX_WARNING, helpers::FAILED_VERIFICATION_FIX_DIRECTIVE)
            } else {
                (helpers::ANTI_BLIND_EDITING_WARNING, helpers::ANTI_BLIND_EDITING_DIRECTIVE)
            };
            self.renderer.line(MessageStyle::Warning, warning).unwrap_or(());
            self.working_history.push(uni::Message::system(directive.to_string()));
            repeated_tool_attempts.verification_warning_emitted = true;
        }

        // Completion claims skip the explanatory budget: asserting "done"
        // without verification is the exact moment that needs evidence, not
        // another directive round. Falls through to ordinary cap accounting
        // when auto-verification is unavailable.
        if is_completion_claim_text(assistant_text)
            && let Some(command) = self.try_harness_auto_verify(repeated_tool_attempts)
        {
            return Ok(PendingVerificationTextOutcome::AutoVerify { command });
        }

        let response_count = self.harness_state.record_assistant_text_response();
        if response_count < MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN {
            return Ok(PendingVerificationTextOutcome::Continue);
        }

        // Prefer one real verifier outcome to repeated model-only reminders.
        if let Some(command) = self.try_harness_auto_verify(repeated_tool_attempts) {
            return Ok(PendingVerificationTextOutcome::AutoVerify { command });
        }

        // In-turn autonomous recovery: name the exact project verifier and
        // grant a fresh text budget once more, instead of blocking immediately.
        // The streak reset mirrors the failed-verifier path (a verifier outcome
        // that grants fix-ups also resets the streak so the model can diagnose
        // before the cap re-applies): here the "outcome" is the harness
        // deciding the turn is still salvageable without user intervention.
        let max_attempts = helpers::verification_in_turn_attempts(self.vt_cfg);
        if repeated_tool_attempts.record_verification_auto_recovery_with_limit(max_attempts) {
            let attempt = repeated_tool_attempts.verification_auto_recovery_attempts();
            let workspace_root = self.tool_registry.workspace_root();
            let default_verifier =
                helpers::resolve_harness_verifier_command(self.vt_cfg, workspace_root.as_path(), self.working_history);
            let directive = vtcode_core::tools::tool_intent::verification_recovery_directive(
                default_verifier.as_deref(),
                attempt,
                max_attempts,
            );
            self.renderer
                .line(MessageStyle::Info, helpers::VERIFICATION_AUTO_RECOVERY_WARNING)
                .unwrap_or(());
            self.working_history.push(uni::Message::system(directive));
            self.harness_state.reset_assistant_text_response_streak();
            self.session_stats
                .set_verification_snapshot(repeated_tool_attempts.verification_snapshot());
            return Ok(PendingVerificationTextOutcome::Continue);
        }

        Ok(PendingVerificationTextOutcome::Block {
            reason: PENDING_VERIFICATION_BLOCK_REASON.to_string(),
        })
    }

    fn verification_execution_blocker(&self, tracker: &helpers::LoopTracker) -> Option<String> {
        let blocker = if self.harness_state.recovery_is_tool_free() {
            Some("tools are disabled for recovery synthesis")
        } else if self.harness_state.wall_clock_exhausted() {
            Some("the turn wall-clock budget is exhausted")
        } else if self.harness_state.tool_budget_exhausted() && tracker.pending_verifier_session_id.is_none() {
            Some("the turn tool-call budget is exhausted")
        } else {
            None
        };
        blocker.map(|reason| format!("Verification is still pending: {reason}. No verifier was scheduled; resume with fresh execution budget."))
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
            let command = helpers::resolve_harness_verifier_command(
                self.vt_cfg,
                self.tool_registry.workspace_root().as_path(),
                self.working_history,
            );
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

/// Execute the project verifier on the harness's behalf after the model
/// exhausted its directive retries without verifying.
///
/// The synthesized call flows through the normal [`handle_tool_calls`]
/// pipeline — mutation guard, validation, permissions, budget, repetition
/// tracker — so harness execution can neither bypass policy nor corrupt gate
/// accounting: a model-issued verifier and this call converge on identical
/// `LoopTracker`/`SessionStats` transitions. Outcomes:
///
/// - Gate cleared → record success (drops the consecutive-failure count),
///   note it in history, and continue the turn.
/// - Gate still pending → record the failure with a bounded output excerpt
///   for the escalated handoff, note the active fix window, and continue so
///   the model can repair from real evidence.
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
    // only an exact command match from this runtime, never archived handles.
    if repeated_tool_attempts.pending_verifier_session_id.is_none() {
        let sessions = ctx.tool_registry.in_progress_exec_sessions(32).await;
        if let Some(session) = sessions.into_iter().find(|session| {
            session.exit_code.is_none()
                && session.lifecycle_state == Some(vtcode_core::tools::types::VTCodeSessionLifecycleState::Running)
                && (session.command_label() == command
                    || (std::path::Path::new(&session.command)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| matches!(name, "sh" | "bash" | "zsh" | "fish"))
                        && session
                            .args
                            .windows(2)
                            .any(|pair| matches!(pair[0].as_str(), "-c" | "-lc") && pair[1] == command)))
        }) {
            repeated_tool_attempts.pending_verifier_session_id = Some(session.id.to_string());
        }
    }

    if let Some(reason) = ctx.verification_execution_blocker(repeated_tool_attempts) {
        return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }));
    }
    let pending_session = repeated_tool_attempts.pending_verifier_session_id.clone();
    let (tool_name, args) = match pending_session {
        Some(session_id) => (
            tool_names::WRITE_STDIN,
            serde_json::json!({"session_id": session_id, "action": "wait", "wait_timeout_seconds": 600, "max_output_tokens": 4000}),
        ),
        None => (tool_names::EXEC_COMMAND, serde_json::json!({"cmd": command, "max_output_tokens": 4000})),
    };

    repeated_tool_attempts.record_auto_verification_executed();
    let action = if tool_name == tool_names::WRITE_STDIN {
        "waiting for the existing verifier"
    } else {
        "running the project verifier"
    };
    ctx.renderer
        .line(MessageStyle::Info, &format!("Harness auto-verification: {action}: `{command}`"))
        .unwrap_or(());
    ctx.working_history.push(uni::Message::system(format!(
        "Harness auto-verification: {action}: `{command}` via {tool_name} (output capped). \
        This call is harness-issued autonomous recovery, not a model action; its result carries the same weight as a model-run verifier."
    )));

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
        if t_ctx_inner.repeated_tool_attempts.pending_verifier_session_id.is_some() {
            // A running session has no verdict and must not consume failure budget.
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
                    A bounded fix window is active: repair the reported failure, then re-run the standalone verifier."
                )));
            }
        } else {
            t_ctx_inner.ctx.session_stats.record_verification_auto_success();
            t_ctx_inner.ctx.working_history.push(uni::Message::system(format!(
                "Harness auto-verification `{command}` exited 0; the verification gate is cleared. Resume the request."
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

    // Mirror the ToolCalls branch: run the balancer before continuing so
    // navigation churn converging during verification still converges.
    Ok(handle_turn_balancer(ctx, step_count, repeated_tool_attempts, max_tool_loops, tool_repeat_limit).await)
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
                    return params.ctx.break_planning_recovery_with_handoff(
                        "the synthesis response attempted a tool call while tools were disabled",
                        (!assistant_text.trim().is_empty()).then_some(assistant_text.as_str()),
                    );
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
            if params.repeated_tool_attempts.verification_is_pending() && !is_planning_synthesis && !tool_free_synthesis
            {
                match params
                    .ctx
                    .handle_pending_verification_text_response(params.repeated_tool_attempts, &text)?
                {
                    PendingVerificationTextOutcome::Continue => return Ok(TurnHandlerOutcome::Continue),
                    PendingVerificationTextOutcome::Block { reason } => {
                        return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }));
                    }
                    PendingVerificationTextOutcome::AutoVerify { command } => {
                        return execute_harness_auto_verification(
                            &mut *params.ctx,
                            &mut *params.repeated_tool_attempts,
                            &mut *params.turn_modified_files,
                            params.step_count,
                            params.max_tool_loops,
                            params.tool_repeat_limit,
                            command,
                        )
                        .await;
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
                    return params.ctx.break_planning_recovery_with_handoff(
                        "the synthesis response attempted tool-call markup",
                        (!salvage.is_empty()).then_some(salvage.as_str()),
                    );
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
        TurnProcessingResult::Empty => {
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
                    return params
                        .ctx
                        .break_planning_recovery_with_handoff("the model returned an empty synthesis response", None);
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
