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
mod tests {
    use std::collections::BTreeSet;

    use super::{
        HandleTurnProcessingResultParams, handle_turn_processing_result, record_assistant_tool_calls,
        should_suppress_pre_tool_result_claim,
    };
    use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
    use crate::agent::runloop::unified::turn::context::{
        PreparedAssistantToolCall, TurnHandlerOutcome, TurnLoopResult, TurnProcessingResult,
    };
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker;
    use crate::agent::runloop::unified::turn::turn_loop::RECOVERY_CONTRACT_VIOLATION_REASON;
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
    use vtcode_core::llm::provider as uni;

    fn prepared_command_tool_call() -> PreparedAssistantToolCall {
        PreparedAssistantToolCall::new(uni::ToolCall::function(
            "call_1".to_string(),
            "exec_command".to_string(),
            r#"{"action":"run","command":"cargo clippy"}"#.to_string(),
        ))
    }

    #[test]
    fn suppresses_result_claims_before_run_tool_output() {
        let tool_calls = vec![prepared_command_tool_call()];
        assert!(should_suppress_pre_tool_result_claim("Found 3 clippy warnings. Let me fix them.", &tool_calls));
    }

    #[test]
    fn keeps_non_result_preamble_for_run_tools() {
        let tool_calls = vec![prepared_command_tool_call()];
        assert!(!should_suppress_pre_tool_result_claim("Running cargo clippy now.", &tool_calls));
    }

    #[test]
    fn records_tool_calls_on_newly_added_assistant_message() {
        let mut history = vec![uni::Message::user("u".to_string())];
        let tool_calls = vec![PreparedAssistantToolCall::new(uni::ToolCall::function(
            "call_1".to_string(),
            "code_search".to_string(),
            r#"{"query":"foo"}"#.to_string(),
        ))];

        let len_before_assistant = history.len();
        history.push(uni::Message::assistant("Searching now.".to_string()));

        record_assistant_tool_calls(&mut history, &tool_calls, len_before_assistant);

        assert_eq!(history.len(), 2);
        let last = history.last().expect("assistant message");
        assert_eq!(last.role, uni::MessageRole::Assistant);
        assert_eq!(last.phase, Some(uni::AssistantPhase::Commentary));
        assert_eq!(last.tool_calls.as_ref().map(|calls| calls[0].id.clone()).as_deref(), Some("call_1"));
    }

    #[test]
    fn appends_tool_call_message_when_no_assistant_message_was_added() {
        let mut history = vec![uni::Message::user("u".to_string())];
        let tool_calls = vec![PreparedAssistantToolCall::new(uni::ToolCall::function(
            "call_1".to_string(),
            "code_search".to_string(),
            r#"{"query":"foo"}"#.to_string(),
        ))];

        let len_before_assistant = history.len();
        record_assistant_tool_calls(&mut history, &tool_calls, len_before_assistant);

        assert_eq!(history.len(), 2);
        let last = history.last().expect("synthetic assistant tool call message");
        assert_eq!(last.role, uni::MessageRole::Assistant);
        assert_eq!(last.content.as_text(), "");
        assert_eq!(last.phase, Some(uni::AssistantPhase::Commentary));
        assert_eq!(last.tool_calls.as_ref().map(|calls| calls[0].id.clone()).as_deref(), Some("call_1"));
    }

    #[tokio::test]
    async fn recovery_tool_calls_break_turn_as_blocked() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let tool_calls = vec![PreparedAssistantToolCall::new(uni::ToolCall::function(
            "call_1".to_string(),
            "code_search".to_string(),
            r#"{"query":"loop"}"#.to_string(),
        ))];
        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::ToolCalls {
                tool_calls,
                assistant_text: String::new(),
                reasoning: Vec::new(),
                reasoning_details: None,
            },
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("recovery tool calls should be handled");

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) })
            if reason == RECOVERY_CONTRACT_VIOLATION_REASON
        ));
    }

    #[tokio::test]
    async fn anti_blind_guard_does_not_allow_final_response_before_verification() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut repeated_tool_attempts = LoopTracker::new();
        repeated_tool_attempts.consecutive_mutations =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
        let mut turn_modified_files = BTreeSet::new();

        let outcome = {
            let mut ctx = backing.turn_processing_context();
            handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::TextResponse {
                    text: "The README update is complete.".to_string(),
                    reasoning: Vec::new(),
                    reasoning_details: None,
                    proposed_plan: None,
                },
                response_streamed: false,
                step_count: 1,
                repeated_tool_attempts: &mut repeated_tool_attempts,
                turn_modified_files: &mut turn_modified_files,
                max_tool_loops: 4,
                tool_repeat_limit: 4,
            })
            .await
            .expect("anti-blind guard should handle the unverified response")
        };

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(backing.last_history_message_contains(
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::ANTI_BLIND_EDITING_DIRECTIVE
        ));
    }

    #[tokio::test]
    async fn anti_blind_guard_blocks_repeated_unverified_text_responses() {
        use crate::agent::runloop::unified::turn::tool_outcomes::helpers::MAX_VERIFICATION_AUTO_RECOVERY_ATTEMPTS;

        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut repeated_tool_attempts = LoopTracker::new();
        repeated_tool_attempts.consecutive_mutations =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
        let mut turn_modified_files = BTreeSet::new();

        // The per-turn text cap is 2, but each cap-hit now consumes one
        // bounded autonomous recovery attempt (fresh streak + project-aware
        // directive) instead of blocking immediately. With
        // MAX_VERIFICATION_AUTO_RECOVERY_ATTEMPTS grants, the turn blocks only
        // after 2 * (1 + MAX) text responses.
        let expected_texts = 2 * (1 + u32::from(MAX_VERIFICATION_AUTO_RECOVERY_ATTEMPTS));
        for step in 1..=expected_texts {
            let outcome = {
                let mut ctx = backing.turn_processing_context();
                handle_turn_processing_result(HandleTurnProcessingResultParams {
                    ctx: &mut ctx,
                    processing_result: TurnProcessingResult::TextResponse {
                        text: "The change is complete.".to_string(),
                        reasoning: Vec::new(),
                        reasoning_details: None,
                        proposed_plan: None,
                    },
                    response_streamed: false,
                    step_count: step as usize,
                    repeated_tool_attempts: &mut repeated_tool_attempts,
                    turn_modified_files: &mut turn_modified_files,
                    max_tool_loops: 4,
                    tool_repeat_limit: 4,
                })
                .await
                .expect("anti-blind response should be handled")
            };
            if step < expected_texts {
                assert!(matches!(outcome, TurnHandlerOutcome::Continue), "step {step} should continue");
            } else {
                assert!(
                    matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(_) })),
                    "step {step} should block after auto-recovery exhaustion"
                );
            }
        }

        assert!(!backing.last_history_message_contains("The change is complete."));
        assert_eq!(
            repeated_tool_attempts.verification_auto_recovery_attempts(),
            MAX_VERIFICATION_AUTO_RECOVERY_ATTEMPTS
        );
    }

    #[tokio::test]
    async fn anti_blind_auto_recovery_names_project_verifier() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.set_verification_override_for_test("rustc --version");
        let mut repeated_tool_attempts = LoopTracker::new();
        repeated_tool_attempts.consecutive_mutations =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
        let mut turn_modified_files = BTreeSet::new();

        for step in 1..=2 {
            let mut ctx = backing.turn_processing_context();
            let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::TextResponse {
                    text: "Still working.".to_string(),
                    reasoning: Vec::new(),
                    reasoning_details: None,
                    proposed_plan: None,
                },
                response_streamed: false,
                step_count: step,
                repeated_tool_attempts: &mut repeated_tool_attempts,
                turn_modified_files: &mut turn_modified_files,
                max_tool_loops: 4,
                tool_repeat_limit: 4,
            })
            .await
            .expect("auto-recovery response should be handled");
            assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        }

        assert_eq!(
            repeated_tool_attempts.verification_auto_recovery_attempts(),
            0,
            "second text response should run the verifier without reminder rounds"
        );
        assert!(
            !repeated_tool_attempts.verification_is_pending(),
            "the configured verifier must clear the gate at the text cap"
        );
        assert!(
            backing.last_history_message_contains("gate is cleared"),
            "recovery must report the observed verifier outcome"
        );
    }

    /// Drive `step` text responses through the handler, returning the last outcome.
    async fn drive_pending_texts(
        backing: &mut TestTurnProcessingBacking,
        repeated_tool_attempts: &mut LoopTracker,
        turn_modified_files: &mut BTreeSet<std::path::PathBuf>,
        steps: u32,
    ) -> TurnHandlerOutcome {
        let mut outcome = TurnHandlerOutcome::Continue;
        for step in 1..=steps {
            let mut ctx = backing.turn_processing_context();
            outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::TextResponse {
                    text: "Still working.".to_string(),
                    reasoning: Vec::new(),
                    reasoning_details: None,
                    proposed_plan: None,
                },
                response_streamed: false,
                step_count: step as usize,
                repeated_tool_attempts: &mut *repeated_tool_attempts,
                turn_modified_files: &mut *turn_modified_files,
                max_tool_loops: 8,
                tool_repeat_limit: 4,
            })
            .await
            .expect("pending-verification text should be handled");
            if !matches!(outcome, TurnHandlerOutcome::Continue)
                || repeated_tool_attempts.auto_verification_executed
                || !repeated_tool_attempts.verification_is_pending()
            {
                break;
            }
        }
        outcome
    }

    #[tokio::test]
    async fn harness_auto_verification_success_clears_gate_and_continues() {
        // `rustc --version` exits 0 without touching workspace files: a
        // hermetic passing verifier for the harness-executed path.
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.set_verification_override_for_test("rustc --version");
        let mut repeated_tool_attempts = LoopTracker::new();
        repeated_tool_attempts.consecutive_mutations =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
        let mut turn_modified_files = BTreeSet::new();

        // The text cap fires one harness execution before directive retries.
        let outcome = drive_pending_texts(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files, 6).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(
            !repeated_tool_attempts.verification_is_pending(),
            "harness-executed `rustc --version` must clear the gate"
        );
        assert_eq!(repeated_tool_attempts.consecutive_mutations, 0);
        assert!(
            !repeated_tool_attempts.should_auto_execute_verifier(),
            "a cleared gate must not re-arm harness execution"
        );
        assert!(
            backing.last_history_message_contains("gate is cleared"),
            "success must leave an explicit gate-cleared note"
        );
    }

    #[tokio::test]
    async fn exhausted_budget_blocks_verification_without_reminder_rounds() {
        let mut backing = TestTurnProcessingBacking::new(1).await;
        backing.set_verification_override_for_test("rustc --version");
        let mut tracker = LoopTracker::new();
        tracker.mark_verification_pending();
        let mut ctx = backing.turn_processing_context();
        ctx.harness_state.tool_calls = 1;
        let outcome = ctx
            .handle_pending_verification_text_response(&mut tracker, "The change is complete.")
            .unwrap();
        let super::PendingVerificationTextOutcome::Block { reason } = outcome else {
            panic!("exhausted execution budget must produce a verification block");
        };
        assert!(reason.contains("tool-call budget"));
        let final_text = crate::agent::runloop::unified::turn::turn_loop::format_blocked_turn_final_response(&reason);
        assert!(super::helpers::is_pending_verification_block(&reason));
        assert!(super::helpers::verification_block_allows_auto_recovery(&reason, &final_text));
        assert_eq!(tracker.verification_auto_recovery_attempts(), 0);
        assert!(!tracker.auto_verification_executed);
        assert_eq!(ctx.session_stats.verification_consecutive_failures(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn harness_waits_for_a_running_verifier_instead_of_starting_another() {
        use std::os::unix::fs::PermissionsExt;
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.set_verification_override_for_test("./rustc --version");
        {
            let ctx = backing.turn_processing_context();
            let script = ctx.tool_registry.workspace_root().join("rustc");
            std::fs::write(&script, "#!/bin/sh\nsleep 2\nprintf 'verifier-finished\\n'\n").unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
            ctx.tool_registry.allow_all_tools().await.unwrap();
            let mut commands = ctx.tool_registry.commands_config();
            commands.allow_list.push("./rustc".to_string());
            ctx.tool_registry.apply_commands_config(&commands);
        }
        let mut tracker = LoopTracker::new();
        tracker.mark_verification_pending();
        let mut modified = BTreeSet::new();
        drive_model_tool_call(
            &mut backing,
            &mut tracker,
            &mut modified,
            "exec_command",
            r#"{"cmd":"./rustc --version","background":true,"yield_time_ms":250}"#,
        )
        .await;
        assert!(tracker.verification_is_pending());
        assert!(
            tracker.pending_verifier_session_id.is_some(),
            "{:?}",
            backing.turn_processing_context().working_history
        );
        // Recreate the turn-local tracker to exercise live-session rediscovery.
        tracker = LoopTracker::new();
        tracker.mark_verification_pending();
        let mut ctx = backing.turn_processing_context();
        super::execute_harness_auto_verification(
            &mut ctx,
            &mut tracker,
            &mut modified,
            2,
            8,
            4,
            "./rustc --version".to_string(),
        )
        .await
        .unwrap();
        assert!(!tracker.verification_is_pending());
        assert_eq!(ctx.session_stats.verification_consecutive_failures(), 0);
        let calls: Vec<_> = ctx
            .working_history
            .iter()
            .filter_map(|message| message.tool_calls.as_ref())
            .flatten()
            .filter(|call| call.id == "harness-auto-verify")
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "write_stdin");
    }

    #[test]
    fn verification_failure_attribution_requires_an_observed_exit() {
        assert!(super::verifier_response_has_nonzero_exit(r#"{"exit_code":1}"#));
        for response in [
            r#"{"exit_code":0}"#,
            r#"{"session_id":"run-live","lifecycle_state":"running"}"#,
            r#"{"error":"budget exhausted","failure_kind":"policy"}"#,
        ] {
            assert!(!super::verifier_response_has_nonzero_exit(response));
        }
    }

    #[tokio::test]
    async fn harness_auto_verification_failure_grants_fix_window_and_records_episode() {
        // Unrecognized rustc flag: fast deterministic non-zero exit, no files.
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.set_verification_override_for_test("rustc --invalid-flag-xyz");
        let mut repeated_tool_attempts = LoopTracker::new();
        repeated_tool_attempts.consecutive_mutations =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
        let mut turn_modified_files = BTreeSet::new();

        let outcome = drive_pending_texts(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files, 6).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(repeated_tool_attempts.verification_is_pending(), "a failed verifier must keep the gate pending");
        assert_eq!(
            repeated_tool_attempts.fix_edits_remaining,
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::FAILED_VERIFICATION_FIX_ALLOWANCE,
            "harness-executed failure must grant the same fix window as a model-run verifier"
        );
        assert!(
            repeated_tool_attempts.auto_verification_executed,
            "the one-shot flag must stick while the gate stays pending"
        );
        assert!(
            backing.last_history_message_contains("did not clear the gate"),
            "failure must leave an explicit fix-window note"
        );
        let ctx = backing.turn_processing_context();
        assert_eq!(ctx.session_stats.verification_consecutive_failures(), 1);
    }

    /// Drive one model-issued tool call through the normal dispatch pipeline,
    /// returning nothing; assertions read the tracker and history afterwards.
    async fn drive_model_tool_call(
        backing: &mut TestTurnProcessingBacking,
        repeated_tool_attempts: &mut LoopTracker,
        turn_modified_files: &mut BTreeSet<std::path::PathBuf>,
        tool_name: &str,
        args_json: &str,
    ) {
        use crate::agent::runloop::unified::turn::tool_outcomes::ToolOutcomeContext;
        use crate::agent::runloop::unified::turn::tool_outcomes::handle_tool_calls;

        let mut ctx = backing.turn_processing_context();
        let call = PreparedAssistantToolCall::new(uni::ToolCall::function(
            "call-rewrite-e2e".to_string(),
            tool_name.to_string(),
            args_json.to_string(),
        ));
        let mut t_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut *repeated_tool_attempts,
            turn_modified_files: &mut *turn_modified_files,
        };
        handle_tool_calls(&mut t_ctx, std::slice::from_ref(&call))
            .await
            .expect("model tool call should dispatch");
    }

    #[tokio::test]
    async fn piped_verifier_executes_standalone_with_truthful_status() {
        // End-to-end proof of the root fix: the model types a piped verifier,
        // the kernel elides the truncator, and the gate follows the VERIFIER's
        // real exit status — not the tail's. `rustc --version | head -c 5`
        // would report exit 0 with 5 chars of output under pipeline semantics;
        // rewritten, the full version surfaces and the gate clears.
        let mut backing = TestTurnProcessingBacking::new(8).await;
        let mut repeated_tool_attempts = LoopTracker::new();
        repeated_tool_attempts.consecutive_mutations =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
        assert!(repeated_tool_attempts.verification_is_pending());
        let mut turn_modified_files = BTreeSet::new();

        drive_model_tool_call(
            &mut backing,
            &mut repeated_tool_attempts,
            &mut turn_modified_files,
            "exec_command",
            r#"{"cmd": "rustc --version | head -c 5"}"#,
        )
        .await;

        assert!(
            !repeated_tool_attempts.verification_is_pending(),
            "truthful exit 0 from the elided verifier must clear the gate"
        );
        assert_eq!(repeated_tool_attempts.consecutive_mutations, 0);
    }

    #[tokio::test]
    async fn piped_verifier_failure_surfaces_instead_of_tail_success() {
        // Converse proof: `rustc --invalid-flag-xyz | tail -5` exits 0 as a
        // pipeline (tail's status) while the verifier fails. Rewritten, the
        // failure surfaces: gate stays pending with the fix window granted.
        let mut backing = TestTurnProcessingBacking::new(8).await;
        let mut repeated_tool_attempts = LoopTracker::new();
        repeated_tool_attempts.consecutive_mutations =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
        let mut turn_modified_files = BTreeSet::new();

        drive_model_tool_call(
            &mut backing,
            &mut repeated_tool_attempts,
            &mut turn_modified_files,
            "exec_command",
            r#"{"cmd": "rustc --invalid-flag-xyz | tail -5"}"#,
        )
        .await;

        assert!(
            repeated_tool_attempts.verification_is_pending(),
            "truthful verifier failure must keep the gate pending"
        );
        assert_eq!(
            repeated_tool_attempts.fix_edits_remaining,
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::FAILED_VERIFICATION_FIX_ALLOWANCE,
            "truthful failure must grant the fix window"
        );
    }

    #[test]
    fn auto_verification_attribution_ignores_prior_turn_responses() {
        use super::last_tool_response_text;

        let history = vec![
            uni::Message::user("do the work".to_string()),
            uni::Message::tool_response("harness-auto-verify".to_string(), "stale failure".to_string()),
            uni::Message::assistant("commentary".to_string()),
        ];
        // Window starting after the stale response: nothing attributable, so
        // a dispatch that broke before executing records no phantom failure.
        assert_eq!(last_tool_response_text(&history, "harness-auto-verify", 3), None);
        // Out-of-range starts are fail-safe, never panics.
        assert_eq!(last_tool_response_text(&history, "harness-auto-verify", 99), None);
        // Window covering it: attributed normally.
        assert_eq!(last_tool_response_text(&history, "harness-auto-verify", 0).as_deref(), Some("stale failure"));
        // Wrong call id: never attributed.
        assert_eq!(last_tool_response_text(&history, "call-other", 0), None);
    }

    #[test]
    fn completion_claim_detector_targets_done_assertions_not_progress_chatter() {
        use super::is_completion_claim_text;

        for claim in [
            "The change is complete.",
            "All done — implementation complete.",
            "Done. All tests pass.",
            "The build passes with no errors.",
            "Fixed and verified.",
            "READY FOR REVIEW",
        ] {
            assert!(is_completion_claim_text(claim), "should detect claim: {claim}");
        }
        for chatter in [
            "Still working on the refactor.",
            "Let me check the remaining files.",
            "Running the next batch of edits now.",
            "",
            "ok",
        ] {
            assert!(!is_completion_claim_text(chatter), "must not fire on chatter: {chatter}");
        }
    }

    #[tokio::test]
    async fn completion_claim_jumps_straight_to_harness_verification() {
        // A "done" assertion on the FIRST pending text must not spend
        // directive rounds: the harness verifies immediately.
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.set_verification_override_for_test("rustc --version");
        let mut repeated_tool_attempts = LoopTracker::new();
        repeated_tool_attempts.consecutive_mutations =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
        let mut turn_modified_files = BTreeSet::new();

        let mut ctx = backing.turn_processing_context();
        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::TextResponse {
                text: "The change is complete.".to_string(),
                reasoning: Vec::new(),
                reasoning_details: None,
                proposed_plan: None,
            },
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 8,
            tool_repeat_limit: 4,
        })
        .await
        .expect("completion claim should be handled");

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!repeated_tool_attempts.verification_is_pending(), "executed `rustc --version` must clear the gate");
        assert_eq!(
            repeated_tool_attempts.verification_auto_recovery_attempts(),
            0,
            "fast path must not consume directive budget"
        );
        assert!(!repeated_tool_attempts.auto_verification_executed, "success clears the one-shot flag with the gate");
    }

    #[tokio::test]
    async fn harness_auto_execute_disabled_falls_back_to_block() {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        let mut vt_cfg = vtcode_core::config::loader::VTCodeConfig::default();
        vt_cfg.agent.harness.verification.auto_execute = false;
        vt_cfg.agent.harness.verification.default_verifier_override = Some("rustc --version".to_string());
        backing.set_vt_cfg_for_test(vt_cfg);
        let mut repeated_tool_attempts = LoopTracker::new();
        repeated_tool_attempts.consecutive_mutations =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
        let mut turn_modified_files = BTreeSet::new();

        let outcome = drive_pending_texts(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files, 6).await;
        assert!(
            matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })),
            "disabled auto-execute must preserve the manual blocked handoff"
        );
        assert!(!repeated_tool_attempts.auto_verification_executed);
        assert!(repeated_tool_attempts.verification_is_pending());
    }

    #[tokio::test]
    async fn harness_auto_verification_escalated_suite_blocks_without_executing() {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.set_verification_override_for_test("rustc --version");
        {
            let ctx = backing.turn_processing_context();
            for _ in 0..3 {
                ctx.session_stats
                    .record_verification_auto_failure("rustc --version".to_string(), "boom");
            }
            assert_eq!(ctx.session_stats.verification_consecutive_failures(), 3);
        }
        let mut repeated_tool_attempts = LoopTracker::new();
        repeated_tool_attempts.consecutive_mutations =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
        let mut turn_modified_files = BTreeSet::new();

        let outcome = drive_pending_texts(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files, 6).await;
        assert!(
            matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })),
            "an escalated never-passing suite must block instead of executing again"
        );
        assert!(!repeated_tool_attempts.auto_verification_executed);
    }

    #[tokio::test]
    async fn tool_free_recovery_texts_bypass_verification_accounting() {
        // Regression: with tools disabled at the API level no text could
        // verify, so counting recovery synthesis toward the verification cap
        // punished the model for obeying the recovery contract. Recovery
        // budgets govern this path instead; the generic cap still refuses
        // unverified completion (cap_ends_completed requires a clear gate).
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.set_verification_override_for_test("rustc --version");
        let mut repeated_tool_attempts = LoopTracker::new();
        repeated_tool_attempts.consecutive_mutations =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
        let mut turn_modified_files = BTreeSet::new();

        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("post-tool follow-up failure");
        assert!(ctx.consume_recovery_pass());
        assert!(ctx.in_tool_free_recovery_synthesis());
        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::TextResponse {
                text: "Synthesizing the gathered evidence into a final answer.".to_string(),
                reasoning: Vec::new(),
                reasoning_details: None,
                proposed_plan: None,
            },
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 8,
            tool_repeat_limit: 4,
        })
        .await
        .expect("recovery synthesis should be handled");

        // Clean recovery prose completes via the recovery path — never via
        // the verification-blocked handoff, and without consuming any
        // verification budget or firing the harness executor.
        assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
        assert!(repeated_tool_attempts.verification_is_pending());
        assert_eq!(repeated_tool_attempts.verification_auto_recovery_attempts(), 0);
        assert!(!repeated_tool_attempts.auto_verification_executed);
    }

    #[tokio::test]
    async fn pending_verification_notice_reports_failed_verifier_during_fix_window() {
        use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
            ANTI_BLIND_EDITING_DIRECTIVE, FAILED_VERIFICATION_FIX_ALLOWANCE,
        };

        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut repeated_tool_attempts =
            LoopTracker::with_verification_snapshot((true, FAILED_VERIFICATION_FIX_ALLOWANCE));
        let mut turn_modified_files = BTreeSet::new();

        let outcome = {
            let mut ctx = backing.turn_processing_context();
            handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::TextResponse {
                    text: "The build failure is in the parser module.".to_string(),
                    reasoning: Vec::new(),
                    reasoning_details: None,
                    proposed_plan: None,
                },
                response_streamed: false,
                step_count: 1,
                repeated_tool_attempts: &mut repeated_tool_attempts,
                turn_modified_files: &mut turn_modified_files,
                max_tool_loops: 4,
                tool_repeat_limit: 4,
            })
            .await
            .expect("fix-window notice should be handled")
        };

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        // The active fix window means the verifier already ran and failed: the
        // notice must say so instead of implying verification was never run.
        assert!(backing.last_history_message_contains("verification command ran and failed"));
        assert!(
            !backing.last_history_message_contains(ANTI_BLIND_EDITING_DIRECTIVE),
            "generic never-ran directive must not be used while fix edits are granted"
        );
    }

    #[tokio::test]
    async fn refusal_blocks_turn_with_reason_even_during_recovery() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let reason = "The model declined this request (category: cyber).".to_string();
        let blocked_with_reason = {
            let mut ctx = backing.turn_processing_context();
            ctx.activate_recovery("loop detector");
            assert!(ctx.consume_recovery_pass());

            let mut repeated_tool_attempts = LoopTracker::new();
            let mut turn_modified_files = BTreeSet::new();

            let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::Refusal { reason: reason.clone() },
                response_streamed: true,
                step_count: 1,
                repeated_tool_attempts: &mut repeated_tool_attempts,
                turn_modified_files: &mut turn_modified_files,
                max_tool_loops: 4,
                tool_repeat_limit: 4,
            })
            .await
            .expect("refusal should be handled");

            matches!(
                outcome,
                TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(ref blocked) }) if *blocked == reason
            )
        };

        assert!(blocked_with_reason);
        assert!(backing.turn_refused(), "refusal must mark the turn for history rollback");
    }

    #[tokio::test]
    async fn recovery_empty_response_emits_fallback_and_completes_turn() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::Empty,
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("recovery empty response should be handled");

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: _ })
        ));
        assert!(backing.last_history_message_contains(
            "I couldn't produce a final synthesis because the model returned no answer on the recovery pass."
        ));
    }

    #[tokio::test]
    async fn recovery_empty_response_fallback_stays_concise_without_evidence_dump() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.push(uni::Message::user("tell me more".to_string()));
        ctx.working_history
            .push(uni::Message::tool_response("call_1".to_string(), "first tool output".to_string()));
        ctx.working_history
            .push(uni::Message::tool_response("call_2".to_string(), "second tool output".to_string()));
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::Empty,
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("recovery empty response should be handled");

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: _ })
        ));
        // Spec tui-diagnostics-cleanup S2D: user-facing fallback stays concise;
        // evidence dumps stay in tool history, not the assistant answer.
        // `activate_recovery` defaults to ToolFreeSynthesis.
        assert!(backing.last_history_message_contains(
            "I couldn't produce a final synthesis because the model returned no answer on the recovery pass."
        ));
        assert!(!backing.last_history_message_contains("Latest user request:"));
        assert!(!backing.last_history_message_contains("Tool output 1:"));
    }

    #[tokio::test]
    async fn recovery_empty_response_fallback_omits_spool_excerpt() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();

        ctx.working_history
            .push(uni::Message::user("summarize the failed read".to_string()));
        ctx.working_history.push(uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "path": "src/main.rs",
                "spool_path": ".vtcode/context/tool_outputs/read_1.txt",
                "preview": "fallback-line-1\nfallback-line-2"
            })
            .to_string(),
        ));
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::Empty,
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("recovery empty response should be handled");

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: _ })
        ));
        assert!(backing.last_history_message_contains(
            "I couldn't produce a final synthesis because the model returned no answer on the recovery pass."
        ));
        assert!(!backing.last_history_message_contains("Spool excerpt:"));
        assert!(!backing.last_history_message_contains("fallback-line-1"));
    }

    #[tokio::test]
    async fn recovery_retry_empty_response_emits_fallback_and_completes_turn() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.push_system_message("prior context");
        ctx.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);
        assert!(ctx.consume_recovery_pass());

        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::Empty,
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("recovery retry empty response should be handled");

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: _ })
        ));
        assert!(
            backing.last_history_message_contains(
                "I couldn't continue because the model returned no answer twice in a row."
            )
        );
    }

    #[tokio::test]
    async fn recovery_textual_tool_markup_breaks_turn_as_blocked() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::TextResponse {
                text: r#"
<minimax:tool_call>
<invoke name="apply_patch">
<parameter name="action">read</parameter>
<parameter name="path">crates/codegen/vtcode-core/src/core/agent/runtime/mod.rs</parameter>
</invoke>
</minimax:tool_call>
"#
                .to_string(),
                reasoning: Vec::new(),
                reasoning_details: None,
                proposed_plan: None,
            },
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("recovery textual tool markup should be handled");

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) })
            if reason == RECOVERY_CONTRACT_VIOLATION_REASON
        ));
    }

    /// A complete, parseable tool call takes the contract-violation path even
    /// when the surrounding prose honestly reports the disabled state: the
    /// disclosure is preserved via the recorded salvage (which feeds the
    /// labeled fallback), but the turn must not complete with an action
    /// attempt pending. No disclosure exception exists because the retry
    /// directive itself contains "tools are disabled", so any exception would
    /// be echoable from history (see
    /// `recovery_complete_tool_call_with_appended_disclosure_still_breaks`).
    #[tokio::test]
    async fn recovery_complete_tool_call_with_disclosure_breaks_as_violation() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::TextResponse {
                text: r#"The requested change was not applied because tools were disabled.
<minimax:tool_call>
<invoke name="apply_patch">
<parameter name="action">write</parameter>
<parameter name="path">README.md</parameter>
</invoke>
</minimax:tool_call>
Please re-run with tools enabled."#
                    .to_string(),
                reasoning: Vec::new(),
                reasoning_details: None,
                proposed_plan: None,
            },
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("complete tool-call markup should take the violation path");

        assert!(
            matches!(
                outcome,
                TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) })
                if reason == RECOVERY_CONTRACT_VIOLATION_REASON
            ),
            "a complete textual tool call must break with a contract violation even with honest disclosure prose"
        );
        let salvaged = backing
            .take_recovery_rejected_synthesis_for_test()
            .expect("violation should record salvaged prose");
        assert!(
            salvaged.contains("The requested change was not applied because tools were disabled."),
            "honest disclosure prose must be preserved for the fallback, got: {salvaged}"
        );
        assert!(
            salvaged.contains("Please re-run with tools enabled."),
            "trailing guidance must be preserved for the fallback, got: {salvaged}"
        );
        assert!(!backing.last_history_message_contains("<invoke"));
    }

    #[tokio::test]
    async fn empty_response_schedules_tool_enabled_retry_without_prior_tool_activity() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = {
            let mut ctx = backing.turn_processing_context();
            handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::Empty,
                response_streamed: true,
                step_count: 1,
                repeated_tool_attempts: &mut repeated_tool_attempts,
                turn_modified_files: &mut turn_modified_files,
                max_tool_loops: 4,
                tool_repeat_limit: 4,
            })
            .await
            .expect("empty response should schedule recovery")
        };

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!backing.recovery_is_tool_free());
        assert!(backing.last_history_message_contains("Tools remain available"));
    }

    #[tokio::test]
    async fn empty_response_after_tool_activity_schedules_tool_free_recovery() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history
                .push(uni::Message::assistant("Running cargo fmt now.".to_string()).with_tool_calls(vec![
                    uni::ToolCall::function(
                        "call_1".to_string(),
                        "exec_command".to_string(),
                        r#"{"action":"run","command":"cargo fmt"}"#.to_string(),
                    ),
                ]));
            ctx.working_history
                .push(uni::Message::tool_response("call_1".to_string(), "formatted".to_string()));

            handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::Empty,
                response_streamed: true,
                step_count: 1,
                repeated_tool_attempts: &mut repeated_tool_attempts,
                turn_modified_files: &mut turn_modified_files,
                max_tool_loops: 4,
                tool_repeat_limit: 4,
            })
            .await
            .expect("empty response after tool activity should schedule synthesis recovery")
        };

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(backing.recovery_is_tool_free());
    }

    #[tokio::test]
    async fn planning_two_empty_responses_schedule_one_tool_free_synthesis() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.activate_planning_for_test();
        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let first = {
            let mut ctx = backing.turn_processing_context();
            handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::Empty,
                response_streamed: false,
                step_count: 1,
                repeated_tool_attempts: &mut repeated_tool_attempts,
                turn_modified_files: &mut turn_modified_files,
                max_tool_loops: 4,
                tool_repeat_limit: 4,
            })
            .await
            .expect("first empty response should arm recovery")
        };
        assert!(matches!(first, TurnHandlerOutcome::Continue));
        assert!(!backing.recovery_is_tool_free());
        {
            let mut ctx = backing.turn_processing_context();
            assert!(ctx.consume_recovery_pass());
        }

        let second = {
            let mut ctx = backing.turn_processing_context();
            handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::Empty,
                response_streamed: false,
                step_count: 2,
                repeated_tool_attempts: &mut repeated_tool_attempts,
                turn_modified_files: &mut turn_modified_files,
                max_tool_loops: 4,
                tool_repeat_limit: 4,
            })
            .await
            .expect("second empty response should schedule synthesis")
        };
        assert!(matches!(second, TurnHandlerOutcome::Continue));
        assert!(backing.recovery_is_tool_free());
        assert!(backing.last_history_message_contains("exactly one completed `<proposed_plan>` block"));
        {
            let mut ctx = backing.turn_processing_context();
            assert!(ctx.consume_recovery_pass());
        }

        let third = {
            let mut ctx = backing.turn_processing_context();
            handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::Empty,
                response_streamed: false,
                step_count: 3,
                repeated_tool_attempts: &mut repeated_tool_attempts,
                turn_modified_files: &mut turn_modified_files,
                max_tool_loops: 4,
                tool_repeat_limit: 4,
            })
            .await
            .expect("failed synthesis should produce a blocked handoff")
        };
        assert!(matches!(third, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })));
        assert!(backing.last_history_message_contains("Planning remains active"));
    }

    /// Regression test for TD-015: text containing malformed `<tool_call>` tags
    /// that `detect_textual_tool_call` cannot parse must still be caught by the
    /// recovery guard via `contains_pseudo_tool_call_markers`.
    #[tokio::test]
    async fn recovery_non_parseable_tool_call_marker_breaks_turn() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        // Malformed markup: <tool_call> wrapping <function=...> with <parameter=>
        // tags that `detect_textual_tool_call` cannot fully parse, but which
        // clearly represent tool-call intent.
        let malformed = "Since tools are disabled in this recovery pass, here is what I would \
                         apply:\n\
                         <tool_call>\n\
                         <function=apply_patch>\n\
                         <parameter=patch>--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-old\n+new\n</parameter=patch>\n\
                         </function=apply_patch>\n\
                         </tool_call>";

        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::TextResponse {
                text: malformed.to_string(),
                reasoning: Vec::new(),
                reasoning_details: None,
                proposed_plan: None,
            },
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("malformed tool-call markup should not panic");

        // The guard must fire (Break) even though detect_textual_tool_call
        // cannot parse the payload. After stripping, either clean prose is
        // returned (Completed) or the text was only markup (Blocked) — either
        // way it must not fall through to showing raw markup as a final answer.
        assert!(
            matches!(outcome, TurnHandlerOutcome::Break(_)),
            "recovery with non-parseable tool_call tag should break, not continue"
        );
    }

    /// Regression test for the approved-plan "no file changes" failure: a
    /// tool-free recovery response that bundles a prose preamble with a
    /// COMPLETE, parseable `<tool_call>` block must take the
    /// contract-violation path (salvage + Blocked) instead of publishing the
    /// stripped preamble ("Applying the section rewrite now.") as a final
    /// answer that fabricates completion.
    #[tokio::test]
    async fn recovery_parseable_tool_call_with_preamble_breaks_turn() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let text = "•   I have sufficient evidence from prior reads. Applying the section rewrite now.\
                    <tool_call>exec_command<arg_key>cmd\n\
                    </arg_key><arg_value>grep -n \"## Why VT Code\" README.md</arg_value></tool_call>";

        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::TextResponse {
                text: text.to_string(),
                reasoning: Vec::new(),
                reasoning_details: None,
                proposed_plan: None,
            },
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("parseable tool-call markup should not panic");

        assert!(
            matches!(
                outcome,
                TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) })
                if reason == RECOVERY_CONTRACT_VIOLATION_REASON
            ),
            "recovery with a complete textual tool call must break with a contract violation, not complete with the preamble"
        );
        assert!(
            !backing.last_history_message_contains("Applying the section rewrite"),
            "stripped preamble must not be published as the final answer"
        );
        let salvaged = backing
            .take_recovery_rejected_synthesis_for_test()
            .expect("violation should record salvaged prose");
        assert!(
            salvaged.contains("Applying the section rewrite"),
            "preamble must be preserved for the labeled fallback, got: {salvaged}"
        );
    }

    /// Anti-gameability pin: appending a disabled-tools disclosure to an action
    /// preamble must not launder a complete tool call into a completion. The
    /// retry directive itself contains "tools are disabled", so a disclosure
    /// exception would be echoable from history on the very next pass.
    #[tokio::test]
    async fn recovery_complete_tool_call_with_appended_disclosure_still_breaks() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let text = "Applying the section rewrite now.\
                    <tool_call>exec_command<arg_key>cmd</arg_key><arg_value>grep -n x README.md</arg_value></tool_call> \
                    (tools were disabled, so this could not run)";

        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::TextResponse {
                text: text.to_string(),
                reasoning: Vec::new(),
                reasoning_details: None,
                proposed_plan: None,
            },
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("appended disclosure must not bypass the violation path");

        assert!(
            matches!(
                outcome,
                TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) })
                if reason == RECOVERY_CONTRACT_VIOLATION_REASON
            ),
            "a complete textual tool call with appended disclosure must still break with a contract violation"
        );
    }

    /// Planning-mode complete calls take the planning violation tail (bounded
    /// repair or resumable handoff), never a normal text completion.
    #[tokio::test]
    async fn recovery_complete_tool_call_in_planning_takes_handoff() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.activate_planning_for_test();
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let text = "•   I have sufficient evidence from prior reads. Applying the section rewrite now.\
                    <tool_call>exec_command<arg_key>cmd</arg_key><arg_value>grep -n x README.md</arg_value></tool_call>";

        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::TextResponse {
                text: text.to_string(),
                reasoning: Vec::new(),
                reasoning_details: None,
                proposed_plan: None,
            },
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("planning complete tool-call markup should take the handoff path");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })),
            "planning complete tool-call markup must break with a resumable handoff, not complete"
        );
        assert!(backing.last_history_message_contains("Planning remains active"));
    }

    /// Regression test: recovery text that contains only prose (no markers)
    /// must pass through unmodified — the guard must not fire on clean text.
    #[tokio::test]
    async fn recovery_clean_prose_is_not_intercepted_by_marker_guard() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::TextResponse {
                text: "The search found 3 matches. Next step: update the failing test.".to_string(),
                reasoning: Vec::new(),
                reasoning_details: None,
                proposed_plan: None,
            },
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("clean recovery prose should be handled");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })),
            "clean prose in recovery should complete the turn normally"
        );
        assert!(backing.last_history_message_contains("3 matches"));
    }

    #[tokio::test]
    async fn denied_interview_retries_plain_text_once_before_approval() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.activate_planning_for_test();
        backing.mark_interview_denied_for_test();

        let mut ctx = backing.turn_processing_context();
        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();
        let response = TurnProcessingResult::TextResponse {
            text: "Next open decision: none from available evidence. Type yes to start this plan.".to_string(),
            reasoning: Vec::new(),
            reasoning_details: None,
            proposed_plan: None,
        };

        let first = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: response,
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("denied interview response should schedule synthesis");
        assert!(matches!(first, TurnHandlerOutcome::Continue));
    }
}
