use super::*;
use crate::agent::runloop::unified::plan_blocks::{
    looks_like_attempted_plan, looks_like_clarifying_question, strip_plan_persistence_policy_line,
};
use crate::agent::runloop::unified::planning_workflow::{
    PlanApprovalRoute, PlanArtifactError, ValidatedPlanArtifact, allocate_plan_file_if_missing,
    build_plan_repair_directive, emit_plan_ready_events, persist_plan_draft, persisted_plan_is_ready,
    plan_approval_route, plan_rejection_history_feedback, plan_repair_directive_for_error, validate_plan_content,
};
use crate::agent::runloop::unified::turn::turn_processing::resolve_effective_request_model;
use crate::agent::runloop::unified::ui_interaction_stream_helpers::render_compact_reasoning_block;
mod text_response;

pub(crate) const DENIED_INTERVIEW_PLAN_SYNTHESIS_RETRY_DIRECTIVE: &str = "Planning recovery: the interactive interview is unavailable, and the previous response did not contain a completed plan. A further question cannot be answered in this mode, and approval is offered only once a plan exists, so emit one compact `<proposed_plan>` from the repository evidence already in this conversation; include Summary, numbered steps in the form `Action -> files: [path] -> verify: [command]`, Validation, and short Assumptions. Each `verify:` must be a concrete command or observable check. Valid examples: `verify: [cargo nextest run -p vtcode]`, `verify: [cargo check --locked]`, `verify: [rg -n 'symbol' src/file.rs]`, `verify: [sed -n '1,40p' docs/file.md]`, and `verify: [grep -n 'symbol' src/file.rs]`. Invalid examples: `verify: [run checks]`, `verify: [check later]`, and `verify: [git diff --check]`; every comma-separated item must independently be concrete. This pass only synthesizes from gathered evidence, so reply with the plan and no tool calls.";

pub(crate) const PLAN_PSEUDO_TOOL_CALL_REPROMPT_DIRECTIVE: &str = "Planning: the previous response contained tool-call markup that was not executed — XML tool-call text is not a tool call. If you need more repository evidence, invoke tools through the tool-call channel. Otherwise present the completed plan as one compact `<proposed_plan>` (Summary, numbered steps in the form `Action -> files: [path] -> verify: [command]`, Validation, short Assumptions). Each `verify:` must be a concrete command or observable check; valid examples: `cargo nextest run -p vtcode`, `cargo check --locked`, `rg -n 'symbol' src/file.rs`, `sed -n '1,40p' docs/file.md`, `grep -n 'symbol' src/file.rs`. Invalid: `run checks`, `check later`, or `git diff --check`. Tool-call markup written as text is never executed.";

const EXECUTION_PLAN_REJECTION_NOTICE: &str = "The proposed plan was rejected and discarded; no continuation turn was scheduled. Adjust the request or revise the plan to continue.";
const PLANNING_PLAN_REJECTION_NOTICE: &str = "The proposed plan was rejected and discarded; no continuation turn was scheduled. Revise the plan or restate the request to continue.";
const TERMINAL_PLAN_REJECTION_REASON: &str = "plan artifact rejected; revise the plan before continuing";
const PLAN_APPROVAL_WAITING_NOTICE: &str = "Plan is awaiting approval. Type `approve`, `implement`, or `yes` to begin execution, or `edit` to revise the plan.";
const PLAN_APPROVAL_DISMISSED_NOTICE: &str = "Plan review ended without starting implementation. The plan remains available for revision or approval in a later turn.";

/// Model-visible text for a terminal plan rejection.
///
/// Keeps the user-facing `notice` as a stable first line (tests, logs) and
/// appends validator-owned feedback so a later user `continue` can repair the
/// draft instead of resubmitting the same invalid shape.
fn terminal_plan_rejection_message(notice: &str, error: &PlanArtifactError) -> String {
    format!("{notice}\n\n{}", plan_rejection_history_feedback(error))
}

/// Detail for a tool-free pass that returned no public final content.
pub(crate) const PLANNING_RECOVERY_EMPTY_SYNTHESIS_DETAIL: &str = "the model returned an empty synthesis response";

/// Handoff detail for a tool-free synthesis that recorded completion tokens
/// without extractable final content. Retains the plain empty-content detail
/// while stating the observed token evidence.
pub(crate) fn planning_tokens_without_content_handoff_detail(completion_tokens: u32) -> String {
    format!(
        "{PLANNING_RECOVERY_EMPTY_SYNTHESIS_DETAIL}; the provider recorded {completion_tokens} completion tokens but no extractable final content"
    )
}

/// Request-only directive for the bounded `NoOutputFacts` salvage: a tool-free
/// planning synthesis recorded completion tokens without extractable final
/// content. Factor-built so the captured token count drives the wording
/// instead of an unverified "the model returned nothing" claim.
pub(crate) fn planning_tokens_without_content_salvage_directive(completion_tokens: u32) -> String {
    format!(
        "Planning salvage: your previous synthesis recorded {completion_tokens} completion tokens but returned no \
         extractable final content, and a tool-free pass must not emit tool calls. Stop analyzing and emit the final \
         content itself now as ONE compact `<proposed_plan>` (Summary; numbered steps in the form `Action -> files: \
         [path] -> verify: [command]`; Validation; short Assumptions). Each `verify:` must be a concrete command or \
         observable check. Reply with only the plan."
    )
}

impl<'a> TurnProcessingContext<'a> {
    /// End a failed plan-mode recovery pass with an explicit resumable
    /// handoff. This path intentionally emits a `Blocked` outcome: no plan
    /// was approved, but the planning session and its bounded evidence remain
    /// available for a later `keep planning`/restatement turn.
    pub(crate) async fn break_planning_recovery_with_handoff(
        &mut self,
        detail: &str,
        rejected_plan: Option<&str>,
    ) -> anyhow::Result<TurnHandlerOutcome> {
        self.recover_planning_failure(detail, rejected_plan).await
    }

    /// Bounded repair for a planning tool-free violation that still carried
    /// salvageable prose (tool calls or markup alongside plan-like text).
    ///
    /// Both violation surfaces in `result_handler.rs` (native tool calls and
    /// textual markup during a tool-free pass) previously ended in an immediate
    /// resumable handoff, bypassing the validation-repair budget that
    /// `reject_plan_artifact` enjoys. When the salvage looks like an attempted
    /// plan and repair budget remains, consume one repair, surface the
    /// canonical format directive, and re-arm a tool-free pass instead.
    /// Returns `true` when the caller should `Continue`; `false` means fall
    /// through to the handoff. Terminal invariant holds: at most
    /// the shared request allowance after research closes.
    pub(crate) fn try_planning_violation_repair(&mut self, salvage: &str) -> anyhow::Result<bool> {
        if !self.is_planning_active() {
            return Ok(false);
        }
        let salvage = salvage.trim();
        if salvage.is_empty() || !looks_like_attempted_plan(salvage) {
            return Ok(false);
        }
        if !self.plan_session.plan_validation_repair_allowed() {
            return Ok(false);
        }
        let validation = validate_plan_content(salvage);
        // A valid plan mixed with violation markup still needs a clean
        // text-only re-emit. Its validator report is ready, so
        // `repair_feedback()` would misleadingly claim "validation issues";
        // use validator-owned re-emit feedback instead so the model sees the
        // real contract violation. Invalid drafts use the report-specific
        // feedback via the shared error→directive mapping.
        let (error, directive) = if validation.is_ready() {
            let reasons = "synthesis mixed a valid plan with a tool-call violation; re-emit text only".to_string();
            let error = PlanArtifactError::Invalid { reasons, report: Box::new(validation) };
            let directive = build_plan_repair_directive(
                "The synthesis mixed a valid plan with a tool-call violation. Re-emit the same plan as text only, without tool calls or tool-call markup.",
            );
            (error, directive)
        } else {
            let reasons = validation.reasons().join("; ");
            let error = PlanArtifactError::Invalid { reasons, report: Box::new(validation) };
            let directive = plan_repair_directive_for_error(&error);
            (error, directive)
        };
        self.plan_session.mark_plan_validation_repair_used();
        tracing::warn!(
            target: "vtcode.planning_workflow",
            error = %error,
            repair_scheduled = true,
            tool_free = true,
            "planning violation carried plan-like salvage; scheduling bounded repair"
        );
        append_rejected_plan_draft_to_last_assistant(self.working_history, salvage);
        self.push_system_message(directive);
        Ok(self.retry_recovery_pass())
    }

    /// A bounded salvage within the shared allowance for a tool-free synthesis that
    /// recorded completion tokens but produced no extractable final content —
    /// typically a reasoning-interlaced model that ended inside a thinking
    /// run, or output the content-extraction pipeline could not surface.
    ///
    /// The captured facts drive the wording: the directive states the recorded
    /// token count instead of asserting an unverified cause, and asks the
    /// model to emit the plan as final content only. Returns `true` when the
    /// caller should `Continue` (retry armed); `false` falls through to the
    /// resumable blocked handoff with the honest detail.
    pub(crate) fn try_planning_empty_output_salvage(&mut self, facts: &NoOutputFacts) -> anyhow::Result<bool> {
        if !self.is_planning_active() || facts.dropped_tool_calls {
            return Ok(false);
        }
        if !self.plan_session.empty_synthesis_salvage_allowed() {
            return Ok(false);
        }
        self.plan_session.mark_empty_synthesis_salvage_used();
        tracing::warn!(
            target: "vtcode.planning_workflow",
            completion_tokens = facts.completion_tokens,
            salvage_scheduled = true,
            tool_free = true,
            "planning synthesis recorded completion tokens without extractable final content; scheduling bounded salvage"
        );
        self.push_system_message(planning_tokens_without_content_salvage_directive(facts.completion_tokens));
        Ok(self.retry_recovery_pass())
    }

    async fn reject_plan_artifact(
        &mut self,
        error: PlanArtifactError,
        plan_text: &str,
        allow_repair: bool,
    ) -> anyhow::Result<TurnHandlerOutcome> {
        use vtcode_core::utils::ansi::MessageStyle;

        // An unapproved execution turn can still propose a plan for review.
        // Give an invalid first draft one tool-free repair pass so it can
        // reach the same approval gate as a valid draft, without allowing
        // edits while the plan is being repaired. Approved-plan revisions
        // retain their terminal rejection behavior.
        if !self.is_planning_active() {
            if allow_repair
                && !self.is_approved_plan_execution()
                && self.plan_session.plan_validation_repair_allowed()
                && self.activate_recovery("invalid execution-mode plan awaiting validation repair")
            {
                self.plan_session.mark_plan_validation_repair_used();
                tracing::warn!(
                    target: "vtcode.planning_workflow",
                    error = %error,
                    repair_scheduled = true,
                    tool_free = true,
                    "execution-mode plan rejected before approval; scheduling bounded repair"
                );
                append_rejected_plan_draft_to_last_assistant(self.working_history, plan_text);
                self.push_system_message(format!(
                    "The agent proposed a plan during execution. Repair it for approval before making edits. {}",
                    plan_repair_directive_for_error(&error)
                ));
                return Ok(TurnHandlerOutcome::Continue);
            }
            tracing::warn!(
                target: "vtcode.planning_workflow",
                error = %error,
                "execution-mode plan revision rejected"
            );
            self.renderer
                .line(MessageStyle::Warning, &format!("Plan revision rejected: {error}"))?;
            if !plan_text.trim().is_empty() {
                self.renderer.line(MessageStyle::Info, "Rejected plan revision:")?;
                self.renderer.line(MessageStyle::Response, plan_text)?;
            }
            // The rejection is a terminal, user-visible outcome. Publish it
            // through the assistant-response path as well as the renderer so
            // finalization does not mistake this blocked control-flow turn
            // for a turn that never produced a final response. History must
            // carry the validator feedback, not only the TUI lines above —
            // otherwise a later `continue` resubmits the same invalid shape.
            self.handle_assistant_response(
                terminal_plan_rejection_message(EXECUTION_PLAN_REJECTION_NOTICE, &error),
                Vec::new(),
                None,
                false,
                Some(uni::AssistantPhase::FinalAnswer),
            )?;
            append_rejected_plan_draft_to_last_assistant(self.working_history, plan_text);
            return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
                reason: Some(TERMINAL_PLAN_REJECTION_REASON.to_string()),
            }));
        }

        if self.recovery_is_tool_free() && self.is_planning_active() {
            // Bounded repair inside tool-free recovery: give one validation
            // failure a single text-only retry via the existing repair budget
            // (`MAX_PLAN_VALIDATION_REPAIR_REPROMPTS`, turn-scoped) and the
            // existing recovery retry (`retry_recovery_pass`, which re-arms a
            // Pending pass without re-enabling tools). Without this, any
            // invalid draft ends Blocked even though the evidence is present.
            // The terminal invariant holds: the persisted shared allowance bounds repairs, then
            // the resumable blocked handoff below.
            if self.plan_session.plan_validation_repair_allowed() {
                self.plan_session.mark_plan_validation_repair_used();
                tracing::warn!(
                    target: "vtcode.planning_workflow",
                    error = %error,
                    repair_scheduled = true,
                    tool_free = true,
                    "plan artifact rejected in tool-free recovery; scheduling bounded repair"
                );
                append_rejected_plan_draft_to_last_assistant(self.working_history, plan_text);
                let directive = plan_repair_directive_for_error(&error);
                self.push_system_message(directive);
                if self.retry_recovery_pass() {
                    return Ok(TurnHandlerOutcome::Continue);
                }
            }
            return self
                .break_planning_recovery_with_handoff(
                    &format!("the synthesized draft failed validation: {error}"),
                    Some(plan_text),
                )
                .await;
        }

        if allow_repair && self.plan_session.plan_validation_repair_allowed() {
            self.plan_session.mark_plan_validation_repair_used();
            // The error→feedback mapping and bounded repair policy live in the
            // planning facade so this initial-plan rejection path and the
            // later-turn approval rejection path share identical guidance.
            tracing::warn!(
                target: "vtcode.planning_workflow",
                error = %error,
                repair_scheduled = true,
                "plan artifact rejected before approval; scheduling bounded repair"
            );
            // Keep the rejected draft in assistant history so the repair
            // request can inspect it without elevating model- or
            // repository-controlled text into a system message.
            append_rejected_plan_draft_to_last_assistant(self.working_history, plan_text);
            let directive = plan_repair_directive_for_error(&error);
            self.push_system_message(directive);
            return Ok(TurnHandlerOutcome::Continue);
        }
        let message = format!("Plan is not ready for approval: {error}");
        self.renderer.line(MessageStyle::Warning, &message)?;
        tracing::warn!(target: "vtcode.planning_workflow", error = %error, "plan artifact rejected before approval");
        // Terminal rejection: the draft is never persisted or shown by the
        // approval flow, so render it here — otherwise the user cannot see
        // what was rejected or revise it manually (checkpoint turn_912).
        if !plan_text.trim().is_empty() {
            self.renderer.line(MessageStyle::Info, "Rejected plan draft:")?;
            self.renderer.line(MessageStyle::Response, plan_text)?;
        }
        self.renderer.line(
            MessageStyle::Warning,
            crate::agent::runloop::unified::planning_workflow_state::PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT,
        )?;
        self.handle_assistant_response(
            terminal_plan_rejection_message(PLANNING_PLAN_REJECTION_NOTICE, &error),
            Vec::new(),
            None,
            false,
            Some(uni::AssistantPhase::FinalAnswer),
        )?;
        append_rejected_plan_draft_to_last_assistant(self.working_history, plan_text);
        Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
            reason: Some(TERMINAL_PLAN_REJECTION_REASON.to_string()),
        }))
    }

    /// Schedule the one bounded plan-only retry allowed after a permanent
    /// interview denial. Keeping the transition here prevents callers from
    /// duplicating the denial/recovery state machine.
    pub(crate) fn retry_denied_interview_plan_synthesis(&mut self) -> bool {
        if !self.is_planning_active() || !self.plan_session.plan_synthesis_retry_allowed() {
            return false;
        }

        self.plan_session.mark_plan_synthesis_retry_used();
        self.push_system_message(DENIED_INTERVIEW_PLAN_SYNTHESIS_RETRY_DIRECTIVE);
        self.harness_state.retry_recovery_pass()
    }

    pub(crate) fn handle_assistant_response(
        &mut self,
        text: String,
        reasoning: Vec<ReasoningSegment>,
        reasoning_details: Option<Vec<String>>,
        response_streamed: bool,
        phase: Option<uni::AssistantPhase>,
    ) -> anyhow::Result<()> {
        let mut text = text;
        let detail_reasoning = reasoning_details
            .as_deref()
            .and_then(vtcode_core::llm::providers::common::extract_reasoning_text_from_serialized_details);
        if should_suppress_redundant_diff_recap(self.working_history, &text) {
            text.clear();
        }
        let has_visible_text = !text.trim().is_empty();
        let final_response_text = matches!(phase, Some(uni::AssistantPhase::FinalAnswer))
            .then(|| text.clone())
            .filter(|text| !text.trim().is_empty());
        if !reasoning.is_empty() || reasoning_details.as_ref().is_some_and(|details| !details.is_empty()) {
            tracing::info!(
                target: "vtcode.turn.metrics",
                metric = "reasoning_observed",
                run_id = %self.harness_state.run_id.0,
                turn_id = %self.harness_state.turn_id.0,
                phase = match phase {
                    Some(uni::AssistantPhase::Commentary) => "commentary",
                    Some(uni::AssistantPhase::FinalAnswer) => "final_answer",
                    None => "unspecified",
                },
                reasoning_segments = reasoning.len(),
                reasoning_details = reasoning_details.as_ref().map_or(0, Vec::len),
                has_detail_reasoning = detail_reasoning.is_some(),
                has_visible_text,
                response_streamed,
                "turn metric"
            );
        }

        if !response_streamed {
            use vtcode_core::utils::ansi::MessageStyle;

            if !text.trim().is_empty() {
                self.renderer.line(MessageStyle::Response, &text)?;
            }
            let mut rendered_reasoning = detail_reasoning.is_some().then(|| Vec::with_capacity(reasoning.len()));

            for segment in &reasoning {
                if let Some(stage) = &segment.stage {
                    self.handle.set_reasoning_stage(Some(stage.clone()));
                }

                let reasoning_text = &segment.text;
                if !reasoning_text.trim().is_empty() {
                    let duplicates_content = has_visible_text && reasoning_duplicates_content(reasoning_text, &text);
                    if !duplicates_content {
                        let compact = vtcode_commons::formatting::compact_reasoning_text(reasoning_text);
                        if compact.trim().is_empty() {
                            continue;
                        }
                        let rendered = render_compact_reasoning_block(self.renderer, reasoning_text)?;
                        if rendered && let Some(rendered_reasoning) = rendered_reasoning.as_mut() {
                            rendered_reasoning.push(compact);
                        }
                    }
                }
            }

            if let Some(detail_text) = detail_reasoning.as_deref() {
                let cleaned_detail = vtcode_commons::formatting::compact_reasoning_text(detail_text);
                let duplicates_content = has_visible_text && reasoning_duplicates_content(&cleaned_detail, &text);
                let duplicates_rendered = rendered_reasoning.as_ref().is_some_and(|rendered_reasoning| {
                    rendered_reasoning.iter().any(|existing: &String| {
                        reasoning_duplicates_content(existing, &cleaned_detail)
                            || reasoning_duplicates_content(&cleaned_detail, existing)
                    })
                });
                if !cleaned_detail.is_empty() && !duplicates_content && !duplicates_rendered {
                    render_compact_reasoning_block(self.renderer, detail_text)?;
                }
            }
            self.handle.set_reasoning_stage(None);
        }

        let combined_reasoning = build_combined_reasoning(&reasoning, detail_reasoning.as_deref());
        let include_reasoning = combined_reasoning
            .as_deref()
            .is_some_and(|combined_reasoning| !reasoning_duplicates_content(combined_reasoning, &text));
        let msg = uni::Message::assistant(text).with_phase(phase);
        let mut msg_with_reasoning = if include_reasoning {
            msg.with_reasoning(combined_reasoning)
        } else {
            msg
        };

        if let Some(details) = reasoning_details.filter(|d| !d.is_empty()) {
            let payload = details
                .into_iter()
                .map(|detail| parse_reasoning_detail_value(&detail))
                .collect::<Vec<_>>();
            msg_with_reasoning = msg_with_reasoning.with_reasoning_details(Some(payload));
        }

        if !msg_with_reasoning.content.as_text().is_empty()
            || msg_with_reasoning.reasoning.is_some()
            || msg_with_reasoning.reasoning_details.is_some()
        {
            push_assistant_message(self.working_history, msg_with_reasoning);
        }

        if let Some(final_response_text) = final_response_text {
            self.harness_state.mark_final_response_rendered();
            if self.harness_emitter.is_none() || self.harness_state.streamed_response_event_emitted() {
                self.harness_state.mark_final_response_event_emitted();
            } else if !self.harness_state.final_response_event_emitted()
                && let Some(emitter) = self.harness_emitter
            {
                match emitter.emit_assistant_message(&self.harness_state.turn_id.0, &final_response_text) {
                    Ok(()) => self.harness_state.mark_final_response_event_emitted(),
                    Err(err) => tracing::warn!(error = %err, "final assistant message harness emission failed"),
                }
            }
        }

        Ok(())
    }
}

/// Pure predicate for the planless-planning-turn hint; kept separate from the
/// async persistence check so the decision is unit-testable.
pub(super) fn should_render_no_ready_plan_hint(denied_interview_hint_shown: bool, final_text: &str) -> bool {
    !denied_interview_hint_shown && !looks_like_clarifying_question(final_text)
}

/// Append the no-approval-ready hint to the stored assistant message from
/// this response, so the guidance survives in history, ATIF, and harness
/// logs. The message was pushed by `handle_assistant_response` before
/// terminality was known; mutating the trailing assistant message keeps the
/// hint attached to the answer it explains. Content is rewritten through
/// `MessageContent::text`, which is lossless here: the message just pushed
/// above is always built from a plain text string.
fn append_no_ready_plan_hint_to_last_assistant(working_history: &mut [uni::Message]) {
    append_to_last_assistant_message(
        working_history,
        crate::agent::runloop::unified::planning_workflow_state::PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT,
    );
}

/// Cap for a rejected plan draft re-attached to the assistant message that
/// produced it. Valid plans are compact by contract (<4KB), so 8KB bounds
/// pathological drafts while never truncating a legitimate one.
const REJECTED_PLAN_DRAFT_HISTORY_BUDGET: usize = 8 * 1024;

/// Re-attach a rejected `<proposed_plan>` draft to the last assistant
/// message. The plan block is extracted from the response text before
/// `handle_assistant_response` stores it, so without this the draft vanishes
/// from history on rejection: the repair retry cannot see what it is fixing,
/// and terminal rejections leave checkpoints/events with no trace of the
/// rejected plan (turn_912/913: the final assistant message degraded to the
/// planning-workflow reminder bullet). Keeping it as assistant content also
/// prevents untrusted draft text from being interpreted as system guidance.
fn append_rejected_plan_draft_to_last_assistant(working_history: &mut Vec<uni::Message>, plan_text: &str) {
    let Some(draft) = bounded_rejected_plan_draft(plan_text) else {
        return;
    };
    if working_history
        .last()
        .is_some_and(|message| message.role == uni::MessageRole::Assistant)
    {
        append_to_last_assistant_message(working_history, &draft);
    } else {
        // A non-streaming provider can return only an extracted plan. In that
        // shape `handle_assistant_response` has no visible text to store, so
        // there is no assistant message to attach the rejected draft to. Keep
        // the draft in the assistant role as bounded context for the repair
        // pass instead of silently dropping the validator's failure input.
        working_history.push(uni::Message::assistant(draft).with_phase(Some(uni::AssistantPhase::Commentary)));
    }
}

fn bounded_rejected_plan_draft(plan_text: &str) -> Option<String> {
    let plan_text = plan_text.trim();
    if plan_text.is_empty() {
        return None;
    }
    let bounded =
        vtcode_commons::formatting::truncate_byte_budget(plan_text, REJECTED_PLAN_DRAFT_HISTORY_BUDGET, "…[truncated]");
    Some(format!("<proposed_plan>\n{bounded}\n</proposed_plan>"))
}

fn append_to_last_assistant_message(working_history: &mut [uni::Message], addition: &str) {
    let Some(last) = working_history
        .iter_mut()
        .rev()
        .find(|message| message.role == uni::MessageRole::Assistant)
    else {
        return;
    };
    let text = last.content.as_text();
    let updated = if text.trim().is_empty() {
        addition.to_string()
    } else {
        format!("{}\n\n{}", text.trim_end(), addition)
    };
    last.content = uni::MessageContent::text(updated);
}

// NOTE: Provider-noise stripping (MiniMax `]<]minimax[>[` and similar) has been
// centralized in `turn::provider_noise`. All call sites — textual tool parsers,
// response handling, and the live stream renderer — delegate to
// `strip_provider_noise` / `sanitize_recovery_answer` there. See that module
// for the canonical noise vocabulary and comprehensive tests.

#[cfg(test)]
mod tests;
