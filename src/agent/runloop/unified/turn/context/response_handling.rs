use super::*;
use crate::agent::runloop::unified::plan_blocks::strip_plan_persistence_policy_line;
use crate::agent::runloop::unified::planning_workflow::{
    PlanApprovalRoute, PlanArtifactError, ValidatedPlanArtifact, allocate_plan_file_if_missing,
    build_plan_repair_directive, emit_plan_ready_events, persist_plan_draft, persisted_plan_is_ready,
    plan_approval_route, plan_rejection_history_feedback, plan_repair_directive_for_error, validate_plan_content,
};
use crate::agent::runloop::unified::turn::turn_processing::resolve_effective_request_model;
use crate::agent::runloop::unified::ui_interaction_stream_helpers::render_compact_reasoning_block;

pub(crate) const DENIED_INTERVIEW_PLAN_SYNTHESIS_RETRY_DIRECTIVE: &str = "Planning recovery: the interactive interview is unavailable, and the previous response did not contain a completed plan. A further question cannot be answered in this mode, and approval is offered only once a plan exists, so emit one compact `<proposed_plan>` from the repository evidence already in this conversation; include Summary, numbered steps in the form `Action -> files: [path] -> verify: [command]`, Validation, and short Assumptions. Each `verify:` must be a concrete command or observable check. Valid examples: `verify: [cargo nextest run -p vtcode]`, `verify: [cargo check --locked]`, `verify: [rg -n 'symbol' src/file.rs]`, `verify: [sed -n '1,40p' docs/file.md]`, and `verify: [grep -n 'symbol' src/file.rs]`. Invalid examples: `verify: [run checks]`, `verify: [check later]`, and `verify: [git diff --check]`; every comma-separated item must independently be concrete. This pass only synthesizes from gathered evidence, so reply with the plan and no tool calls.";

pub(crate) const PLAN_PSEUDO_TOOL_CALL_REPROMPT_DIRECTIVE: &str = "Planning: the previous response contained tool-call markup that was not executed — XML tool-call text is not a tool call. If you need more repository evidence, invoke tools through the tool-call channel. Otherwise present the completed plan as one compact `<proposed_plan>` (Summary, numbered steps in the form `Action -> files: [path] -> verify: [command]`, Validation, short Assumptions). Each `verify:` must be a concrete command or observable check; valid examples: `cargo nextest run -p vtcode`, `cargo check --locked`, `rg -n 'symbol' src/file.rs`, `sed -n '1,40p' docs/file.md`, `grep -n 'symbol' src/file.rs`. Invalid: `run checks`, `check later`, or `git diff --check`. Tool-call markup written as text is never executed.";

const EXECUTION_PLAN_REJECTION_NOTICE: &str = "The proposed plan was rejected and discarded; no continuation turn was scheduled. Adjust the request or revise the plan to continue.";
const PLANNING_PLAN_REJECTION_NOTICE: &str = "The proposed plan was rejected and discarded; no continuation turn was scheduled. Revise the plan or restate the request to continue.";
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

/// Detect whether a planning-mode text response is a clarifying question
/// posed to the user rather than a plan or research prose. The deterministic
/// interview-denial recovery must NOT force plan synthesis when the model is
/// legitimately asking the user a question in plain text (the text-mode
/// equivalent of the unavailable `request_user_input` modal). Without this
/// check, the retry directive suppresses the question and the agent proceeds
/// to propose a plan without waiting for the user's answer (checkpoint
/// turn_856).
///
/// Heuristic: the last non-empty line ends with `?`. This is a strong signal
/// that the model is asking a question, and it does not match completed plans
/// (which end with Assumptions/Validation prose) or research dumps.
pub(super) fn looks_like_clarifying_question(text: &str) -> bool {
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .is_some_and(|last_line| last_line.trim().ends_with('?'))
}

/// Detect whether a planning-mode text response is an attempted plan emitted
/// without the required `<proposed_plan>` tags (checkpoint turn_1075: a
/// `## Simple plan` markdown answer with seven numbered steps but no tags,
/// no `files:` targets, and no `verify:` checks). Without this check the
/// turn ends with only the generic "no approval-ready plan" hint and the
/// model never learns the canonical step contract, so it repeats the same
/// tagless shape every turn.
///
/// Heuristic (conservative by design): the text has a markdown heading naming
/// an implementation section (`## Implementation Steps`, or `## Goal`
/// together with `## Plan`) or contains at least two numbered-step lines
/// (`1. ...`, `1) ...`). Single numbered lines in research prose do not
/// trigger; clarifying questions are excluded by the caller, not here, so
/// this stays a pure shape check.
pub(super) fn looks_like_attempted_plan(text: &str) -> bool {
    fn heading_rest(line: &str) -> Option<&str> {
        let trimmed = line.trim().trim_start_matches('>').trim_start();
        let without_hashes = trimmed.trim_start_matches('#');
        if without_hashes.len() == trimmed.len() {
            return None;
        }
        Some(without_hashes.trim_start().trim_start_matches(['*', '_', '`']).trim_start())
    }

    fn is_heading_name(line: &str, name: &str) -> bool {
        let Some(rest) = heading_rest(line) else {
            return false;
        };
        if rest.len() < name.len() || !rest[..name.len()].eq_ignore_ascii_case(name) {
            return false;
        }
        // Word boundary: `## Planning` must not match a `## Plan` heading,
        // and `## Goals` must not match `## Goal`.
        rest[name.len()..]
            .chars()
            .next()
            .is_none_or(|next| !next.is_ascii_alphanumeric())
    }

    let mut has_implementation_heading = false;
    let mut has_plan_heading = false;
    let mut has_goal_heading = false;
    for line in text.lines() {
        if !has_implementation_heading && is_heading_name(line, "implementation steps") {
            has_implementation_heading = true;
        }
        if !has_plan_heading && is_heading_name(line, "plan") {
            has_plan_heading = true;
        }
        if !has_goal_heading && is_heading_name(line, "goal") {
            has_goal_heading = true;
        }
        if has_implementation_heading || (has_plan_heading && has_goal_heading) {
            break;
        }
    }
    if has_implementation_heading {
        return true;
    }
    if has_plan_heading && has_goal_heading {
        return true;
    }

    let mut numbered_steps = 0usize;
    for line in text.lines() {
        let trimmed = line.trim().trim_start_matches('>').trim_start();
        // Tolerate a `Step ` prefix (`Step 1: ...`), mirroring
        // `numbered_line_parts` in planning_workflow/artifacts.rs: only strip
        // when a digit follows so `stepwise` is never mistaken for a step.
        let trimmed = trimmed
            .get(..4)
            .filter(|prefix| prefix.eq_ignore_ascii_case("step"))
            .map(|_| trimmed[4..].trim_start())
            .filter(|rest| rest.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
            .unwrap_or(trimmed);
        let mut digits_len = 0usize;
        for ch in trimmed.chars() {
            if ch.is_ascii_digit() {
                digits_len += ch.len_utf8();
            } else {
                break;
            }
        }
        if digits_len == 0 {
            continue;
        }
        let rest = trimmed[digits_len..].trim_start();
        let mut chars = rest.chars();
        // Accept the same step punctuation as the artifact validator
        // (`numbered_line_parts` in planning_workflow/artifacts.rs).
        match chars.next() {
            Some('.') | Some(')') | Some(':') if chars.next().is_some_and(|next| next.is_whitespace()) => {
                numbered_steps += 1;
                if numbered_steps >= 2 {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

impl<'a> TurnProcessingContext<'a> {
    /// End a failed plan-mode recovery pass with an explicit resumable
    /// handoff. This path intentionally emits a `Blocked` outcome: no plan
    /// was approved, but the planning session and its bounded evidence remain
    /// available for a later `keep planning`/restatement turn.
    pub(crate) fn break_planning_recovery_with_handoff(
        &mut self,
        detail: &str,
        rejected_plan: Option<&str>,
    ) -> anyhow::Result<TurnHandlerOutcome> {
        let detail = detail.trim();
        let detail = if detail.is_empty() {
            "the synthesis pass failed"
        } else {
            detail
        };
        let detail = if detail.len() > 240 {
            let mut end = 240;
            while end > 0 && !detail.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}…", &detail[..end])
        } else {
            detail.to_string()
        };
        let mut message = format!(
            "Planning remains active, but the one tool-free recovery synthesis did not produce an approval-ready plan ({detail}). The latest request and bounded evidence are preserved. The next attempt can reuse the tool outputs above instead of re-reading files, and should emit one complete `<proposed_plan>` with `Action -> files: [path] -> verify: [command]` steps. Each `verify:` must be a concrete command or observable check: valid examples are `verify: [cargo nextest run -p vtcode]`, `verify: [rg -n 'symbol' src/file.rs]`, `verify: [sed -n '1,40p' docs/file.md]`, and `verify: [grep -n 'symbol' src/file.rs]`; invalid examples are `verify: [run checks]`, `verify: [check later]`, and `verify: [git diff --check]`. Re-state the planning request or type `keep planning` to try again; no changes were applied."
        );
        let mut reason =
            "planning recovery did not produce an approval-ready plan; planning remains active".to_string();
        if let Some(cause) = self.harness_state.budget_recovery_reason() {
            message = format!("{cause}. {message}");
            reason = format!("{cause}. {reason}");
        }

        self.harness_state.mark_final_response_fallback();
        self.handle_assistant_response(message, Vec::new(), None, false, Some(uni::AssistantPhase::FinalAnswer))?;
        if let Some(rejected_plan) = rejected_plan {
            append_rejected_plan_draft_to_last_assistant(self.working_history, rejected_plan);
        }
        self.finish_recovery_pass();
        Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }))
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
    /// `MAX_PLAN_VALIDATION_REPAIR_REPROMPTS` repairs per turn.
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
            let reasons = "synthesis mixed a valid plan with tool-call markup; re-emit text only".to_string();
            let error = PlanArtifactError::Invalid { reasons, report: Box::new(validation) };
            let directive = build_plan_repair_directive(
                "The synthesis mixed a valid plan with tool-call markup. Re-emit the same plan as text only, without tool calls or tool-call markup.",
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

    fn reject_plan_artifact(
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
            // finalization does not mistake this completed control-flow turn
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
            return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false }));
        }

        if self.recovery_is_tool_free() && self.is_planning_active() {
            // Bounded repair inside tool-free recovery: give one validation
            // failure a single text-only retry via the existing repair budget
            // (`MAX_PLAN_VALIDATION_REPAIR_REPROMPTS`, turn-scoped) and the
            // existing recovery retry (`retry_recovery_pass`, which re-arms a
            // Pending pass without re-enabling tools). Without this, any
            // invalid draft ends Blocked even though the evidence is present.
            // The terminal invariant holds: at most 2 repairs per turn, then
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
            return self.break_planning_recovery_with_handoff(
                &format!("the synthesized draft failed validation: {error}"),
                Some(plan_text),
            );
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
        Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false }))
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

    pub(crate) async fn handle_text_response(
        &mut self,
        text: String,
        reasoning: Vec<ReasoningSegment>,
        reasoning_details: Option<Vec<String>>,
        proposed_plan: Option<String>,
        response_streamed: bool,
    ) -> anyhow::Result<TurnHandlerOutcome> {
        let recovery_pass_response = self.is_recovery_active() && self.recovery_pass_used();
        let tool_free_recovery_pass = recovery_pass_response && self.recovery_is_tool_free();
        // Tool-free recovery is terminal: the model's text IS the final answer.
        // Some providers (e.g. MiniMax) emit a noise prefix like `]<]minimax[>[`
        // before/instead of real content. When the model has nothing to
        // synthesize, this residue becomes the user-visible final answer — the
        // "agent just stops with garbage" symptom (checkpoints turn_609/613).
        // Strip known noise and, if nothing meaningful remains, substitute a
        // clear fallback so the user gets an actionable message instead of
        // provider noise.
        // Strip provider noise (e.g. MiniMax `]<]minimax[>[`) from ALL assistant
        // text — commentary, normal final answers, and recovery final answers.
        // This prevents noise from leaking into the user-visible output and,
        // more importantly, from being echoed back to the API via
        // `working_history` on follow-up calls (polluted context degrades
        // subsequent responses and contributes to post-tool follow-up
        // failures). For tool-free recovery passes, additionally substitute a
        // fallback when nothing meaningful remains after stripping.
        let text = if tool_free_recovery_pass {
            crate::agent::runloop::unified::turn::provider_noise::sanitize_recovery_answer(text)
        } else {
            crate::agent::runloop::unified::turn::provider_noise::strip_provider_noise(&text)
        };
        let mut proposed_plan = proposed_plan;
        let text = if proposed_plan.is_some() {
            strip_plan_persistence_policy_line(&text)
        } else {
            text
        };
        // Plan-mode salvage: a model with no tool schemas on the wire (or a
        // confused checkpoint) sometimes answers with XML-ish tool-call markup
        // as text. No textual parser could execute it, and in plan mode any
        // text ends the turn, so the raw markup became the user-visible final
        // answer and leaked into history, ATIF, and harness logs
        // (turn_887/turn_888). Strip the markup from the stored/visible text;
        // a bounded re-prompt below gives the model a chance to call tools
        // natively or present the plan instead.
        let pseudo_tool_call_markup_detected = self.is_planning_active()
            && !tool_free_recovery_pass
            && proposed_plan.is_none()
            && crate::agent::runloop::text_tools::contains_pseudo_tool_call_markers(&text);
        let mut text = if pseudo_tool_call_markup_detected {
            crate::agent::runloop::text_tools::strip_textual_tool_call_regions(&text)
                .trim()
                .to_string()
        } else {
            text
        };
        let recovery_plan_was_extracted = proposed_plan.is_some();
        if tool_free_recovery_pass && self.is_planning_active() {
            if proposed_plan.is_none() {
                if text.trim().is_empty() {
                    return self.break_planning_recovery_with_handoff(
                        "the response did not contain exactly one completed <proposed_plan> block",
                        None,
                    );
                }
                proposed_plan = Some(text.clone());
                tracing::info!(
                    target: "vtcode.planning_workflow",
                    "treating unwrapped recovery text as a plan candidate for validation"
                );
            }
            // Prose-tolerant recovery: models often add a one-line intro
            // ("Here is the plan:") around an otherwise valid block. The
            // extractor already split `proposed_plan` from `text`, so drop the
            // surrounding prose and validate the plan instead of ending
            // Blocked. The plan-approval flow renders the plan once; the
            // intro carries no approval state.
            if recovery_plan_was_extracted && !text.trim().is_empty() {
                tracing::info!(
                    target: "vtcode.planning_workflow",
                    prose_len = text.trim().len(),
                    "dropping bounded surrounding prose around valid recovery plan block"
                );
                text.clear();
            }
        }
        let denied_interview_plan_retry = self.is_planning_active()
            && !tool_free_recovery_pass
            && proposed_plan.is_none()
            && !text.trim().is_empty()
            && self.plan_session.plan_synthesis_retry_allowed();
        let denied_interview_recovery_retry = self.is_planning_active()
            && tool_free_recovery_pass
            && proposed_plan.is_none()
            && self.plan_session.plan_synthesis_retry_allowed();
        let denied_interview_without_ready_plan = self.is_planning_active()
            && !tool_free_recovery_pass
            && self.plan_session.is_interview_denied()
            && proposed_plan.is_none()
            && !persisted_plan_is_ready(&self.tool_registry.planning_workflow_state()).await
            && !looks_like_clarifying_question(&text);
        let text = if denied_interview_without_ready_plan {
            crate::agent::runloop::unified::planning_workflow_state::PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT
                .to_string()
        } else {
            text
        };
        // Dead-end guard (checkpoint turn_912): decide whether this planning
        // response qualifies for the no-approval-ready hint, but DEFER the
        // append until terminality is known — the pseudo-tool-call reprompt
        // and stop-hook paths below can still return `Continue`, and storing
        // the hint before a continuation would leave stale "no approval-ready
        // plan" guidance in history for a turn that kept going. Clarifying
        // questions are excluded — the turn ends intentionally for the user's
        // answer — and the denied-interview path above already surfaced the
        // hint as the response text. Responses carrying a `proposed_plan` are
        // also excluded: the plan-validation/persistence branch owns their
        // terminal messaging.
        let defer_no_ready_plan_hint = self.is_planning_active()
            && !tool_free_recovery_pass
            && proposed_plan.is_none()
            && should_render_no_ready_plan_hint(denied_interview_without_ready_plan, &text)
            && !persisted_plan_is_ready(&self.tool_registry.planning_workflow_state()).await;
        let final_text = text.clone();
        let consecutive_relaxed = self.harness_state.consecutive_relaxed_continuations;
        let continuation_decision = if tool_free_recovery_pass {
            // Tool-free recovery is terminal: the text produced during recovery
            // IS the final answer. Allowing continuation here would call
            // `finish_recovery_pass()` (deactivating recovery), re-enable tools
            // on the next iteration, and — if the follow-up fails again —
            // re-trigger recovery, producing an infinite cycle that no existing
            // bound catches (`consecutive_relaxed_continuations` is bypassed by
            // non-relaxed "recent_tool_activity" continuations that reset the
            // counter to 0, and `MAX_RECOVERY_RETRIES` only counts retries
            // within a single pass). Evaluate continuation intent solely to
            // populate diagnostic fields for the tracing log; the decision is
            // always to end the turn.
            let decision = evaluate_interim_text_continuation(
                self.full_auto,
                self.is_planning_active(),
                self.working_history,
                &text,
                consecutive_relaxed,
            );
            InterimTextContinuationDecision {
                should_continue: false,
                reason: "tool_free_recovery_terminal",
                is_interim_progress: decision.is_interim_progress,
                last_user_follow_up: decision.last_user_follow_up,
                recent_tool_activity: decision.recent_tool_activity,
                last_user_requested_progressive_work: decision.last_user_requested_progressive_work,
                is_relaxed_continuation: false,
            }
        } else if proposed_plan.is_some() {
            // A completed plan is terminal in every mode: it must reach the
            // persist/approval handoff below instead of being continued away
            // as interim progress. Evaluate continuation intent solely to
            // populate diagnostic fields for the tracing log.
            let decision = evaluate_interim_text_continuation(
                self.full_auto,
                self.is_planning_active(),
                self.working_history,
                &text,
                consecutive_relaxed,
            );
            InterimTextContinuationDecision {
                should_continue: false,
                reason: "completed_plan_terminal",
                is_interim_progress: decision.is_interim_progress,
                last_user_follow_up: decision.last_user_follow_up,
                recent_tool_activity: decision.recent_tool_activity,
                last_user_requested_progressive_work: decision.last_user_requested_progressive_work,
                is_relaxed_continuation: false,
            }
        } else {
            evaluate_interim_text_continuation(
                self.full_auto,
                self.is_planning_active(),
                self.working_history,
                &text,
                consecutive_relaxed,
            )
        };

        // Tracker-aware override (shipped runloop surface): when task_tracker
        // still has incomplete steps, status recaps must continue instead of
        // ending the turn and nudging the user.
        //
        // Adoption gate: a fresh informational request must not be redirected
        // into unrelated workspace tracker work. Require progressive-work
        // intent, recent tool activity, or a follow-up in this session before
        // the workspace tracker can force continuation. Explicit `--resume`
        // paths bypass this (explicit adoption).
        //
        // Tool-free recovery stays terminal in-turn: finish_recovery_pass()
        // would re-enable tools and can re-enter recovery (see the
        // `tool_free_recovery_terminal` branch above). Outer auto-queue after
        // a Completed recovery end schedules the next tracker turn instead.
        // Completed-plan branches stay terminal in-turn as well.
        // In-turn override is gated on `auto_continue_tracker` only —
        // `cross_turn_turns == 0` disables only the outer auto-queue.
        // Complete probes clear the cache so auto-continue stops when the
        // tracker finishes; Unavailable keeps the last known incomplete set.
        let tracker_adoption_allowed = continuation_decision.last_user_requested_progressive_work
            || continuation_decision.recent_tool_activity
            || continuation_decision.last_user_follow_up;
        let live_probe = if !continuation_decision.should_continue
            && !tool_free_recovery_pass
            && proposed_plan.is_none()
            && !self.is_planning_active()
            && tracker_adoption_allowed
            && crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_auto_continue_enabled(self.vt_cfg)
        {
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::probe_tracker_incomplete(self.tool_registry)
                .await
        } else {
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::TrackerProbeOutcome::Unavailable
        };
        let effective_incomplete = self.harness_state.apply_tracker_probe(live_probe);
        let tracker_incomplete = effective_incomplete.is_some_and(|items| !items.is_empty());
        let continuation_decision = if !continuation_decision.should_continue
            && !tool_free_recovery_pass
            && proposed_plan.is_none()
            && !self.is_planning_active()
            && tracker_adoption_allowed
            && crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_auto_continue_enabled(self.vt_cfg)
            && tracker_incomplete
        {
            apply_tracker_continuation_override(continuation_decision, true, self.is_planning_active(), &text)
        } else {
            continuation_decision
        };

        // Track consecutive relaxed continuations to prevent infinite loops.
        if continuation_decision.should_continue && continuation_decision.is_relaxed_continuation {
            self.harness_state.consecutive_relaxed_continuations += 1;
        } else if continuation_decision.should_continue {
            // Non-relaxed continuation resets the counter
            self.harness_state.consecutive_relaxed_continuations = 0;
        } else {
            // Turn is ending, reset the counter
            self.harness_state.consecutive_relaxed_continuations = 0;
        }

        let assistant_phase = if continuation_decision.should_continue {
            Some(uni::AssistantPhase::Commentary)
        } else {
            Some(uni::AssistantPhase::FinalAnswer)
        };
        self.handle_assistant_response(text, reasoning, reasoning_details, response_streamed, assistant_phase)?;

        // Count this text response so the recovery loop can short-circuit
        // when the model has already produced a final answer but the loop
        // keeps re-prompting. See `MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN`.
        self.harness_state.record_assistant_text_response();

        if recovery_pass_response {
            self.finish_recovery_pass();
        }

        // A tool-free pass is normally terminal, but a permanently denied
        // interview has one additional bounded contract: it must produce a
        // real draft before the user can approve anything. If the provider
        // ignored the recovery directive and returned prose without a plan,
        // retry once while tools remain disabled instead of ending mid-turn
        // with no approval-ready draft.
        //
        // EXCEPTION: if the text is a clarifying question (the text-mode
        // equivalent of the unavailable interview modal), end the turn so the
        // user can answer it. Forcing plan synthesis here would suppress the
        // question and proceed to propose a plan without user input
        // (checkpoint turn_856).
        if denied_interview_recovery_retry {
            if looks_like_clarifying_question(&final_text) {
                tracing::info!(
                    target: "vtcode.planning_workflow",
                    "denied interview recovery produced a clarifying question; ending turn for user input instead of retrying plan synthesis"
                );
                // Fall through to normal turn completion — the question is
                // already in working_history as the assistant's final answer.
            } else if self.retry_denied_interview_plan_synthesis() {
                tracing::info!(
                    target: "vtcode.planning_workflow",
                    "retrying tool-free synthesis after denied interview returned no plan"
                );
                // The text above already incremented the generic text-response
                // streak; queue one bounded allowance so the scheduled retry
                // is not immediately cancelled by the cap on the next loop.
                self.plan_session.queue_bounded_planning_follow_up();
                return Ok(TurnHandlerOutcome::Continue);
            }
        }

        // A permanent interview denial is different from a cancelled
        // interview: the model must still produce a real draft before the
        // user can approve it. The denial diagnostic is advisory, so some
        // models answer only with "type yes" instead of emitting a plan.
        // Give that response one bounded synthesis retry. This keeps the
        // approval path draft-backed without re-enabling the unavailable
        // interview tool or allowing an unbounded continuation loop.
        //
        // EXCEPTION: a clarifying question is the text-mode equivalent of
        // the unavailable interview modal — end the turn for user input
        // instead of suppressing it with a forced synthesis retry.
        if denied_interview_plan_retry && !looks_like_clarifying_question(&final_text) {
            self.plan_session.mark_plan_synthesis_retry_used();
            self.push_system_message(DENIED_INTERVIEW_PLAN_SYNTHESIS_RETRY_DIRECTIVE);
            tracing::info!(
                target: "vtcode.planning_workflow",
                "retrying denied interview response as a bounded plan synthesis"
            );
            // See the tool-free branch above: allow the queued retry through
            // the generic text-response cap.
            self.plan_session.queue_bounded_planning_follow_up();
            return Ok(TurnHandlerOutcome::Continue);
        }

        // Plan-mode pseudo-tool-call reprompt: a model with no tool schemas on
        // the wire (or a confused checkpoint) sometimes emits XML-ish
        // tool-call markup as text. No textual parser could execute it, and in
        // plan mode any text ends the turn, so the raw markup previously
        // became the user-visible final answer and leaked into history, ATIF,
        // and harness logs (turn_887/turn_888). The markup was already stripped
        // from the stored text above; give the model a bounded chance to call
        // tools natively or present the plan instead of ending mid-turn with
        // cleaned-up prose. When the reprompt budget is exhausted, fall through
        // to normal turn completion — the already-stripped text guarantees raw
        // markup never reaches the user.
        if pseudo_tool_call_markup_detected && self.plan_session.plan_pseudo_tool_call_reprompt_allowed() {
            self.plan_session.mark_plan_pseudo_tool_call_reprompt_used();
            self.push_system_message(PLAN_PSEUDO_TOOL_CALL_REPROMPT_DIRECTIVE);
            tracing::info!(
                target: "vtcode.planning_workflow",
                "re-prompting after pseudo-tool-call markup in plan mode"
            );
            // Bounded reprompt with its own budget; queue one cap allowance
            // so the cleanup pass is not blocked by the streak it just grew.
            self.plan_session.queue_bounded_planning_follow_up();
            return Ok(TurnHandlerOutcome::Continue);
        }

        // Untagged plan-like text in normal planning mode (checkpoint
        // turn_1075): the model emitted a markdown plan without
        // `<proposed_plan>` tags, so extraction produced no candidate and the
        // turn would end with only the generic no-ready-plan hint. Route it
        // through the same bounded validation-repair path as tagged drafts
        // so the model learns the canonical step contract instead of
        // repeating the tagless shape every turn. Tool-free recovery keeps
        // its own salvage path above; clarifying questions still end the
        // turn for user input.
        //
        // A tagless draft that already validates promotes to a plan candidate
        // (mirroring the tool-free salvage above) so a complete plan is not
        // discarded with a planless hint just for missing tags. The interim
        // continuation and planless-hint branches below are skipped once a
        // candidate exists.
        if self.is_planning_active()
            && !tool_free_recovery_pass
            && proposed_plan.is_none()
            && !final_text.trim().is_empty()
            && !looks_like_clarifying_question(&final_text)
            && looks_like_attempted_plan(&final_text)
        {
            let validation = validate_plan_content(&final_text);
            if !validation.is_ready() {
                let error = PlanArtifactError::Invalid {
                    reasons: validation.reasons().join("; "),
                    report: Box::new(validation),
                };
                if self.plan_session.plan_validation_repair_allowed() {
                    self.plan_session.mark_plan_validation_repair_used();
                    tracing::warn!(
                        target: "vtcode.planning_workflow",
                        error = %error,
                        repair_scheduled = true,
                        untagged = true,
                        "untagged plan-like text in planning mode; scheduling bounded repair"
                    );
                    append_rejected_plan_draft_to_last_assistant(self.working_history, &final_text);
                    let directive = plan_repair_directive_for_error(&error);
                    self.push_system_message(directive);
                    return Ok(TurnHandlerOutcome::Continue);
                }
                return self.reject_plan_artifact(error, &final_text, false);
            }
            tracing::info!(
                target: "vtcode.planning_workflow",
                untagged = true,
                "untagged plan-like text validated; promoting to plan candidate"
            );
            proposed_plan = Some(final_text.clone());
        }

        tracing::info!(
            target: "vtcode.turn.metrics",
            metric = "text_response_decision",
            run_id = %self.harness_state.run_id.0,
            turn_id = %self.harness_state.turn_id.0,
            should_continue = continuation_decision.should_continue,
            reason = continuation_decision.reason,
            outcome = continuation_telemetry_outcome(self.full_auto, &continuation_decision),
            is_interim_progress = continuation_decision.is_interim_progress,
            last_user_follow_up = continuation_decision.last_user_follow_up,
            recent_tool_activity = continuation_decision.recent_tool_activity,
            last_user_requested_progressive_work =
                continuation_decision.last_user_requested_progressive_work,
            recovery_pass_response,
            tool_free_recovery_pass,
            planning_workflow = self.is_planning_active(),
            full_auto = self.full_auto,
            history_len = self.working_history.len(),
            "turn metric"
        );

        if continuation_decision.should_continue && proposed_plan.is_none() {
            push_system_directive_once(self.working_history, AUTONOMOUS_CONTINUE_DIRECTIVE);
            return Ok(TurnHandlerOutcome::Continue);
        }

        if let Some(hooks) = self.lifecycle_hooks {
            let outcome = hooks.run_stop(&final_text, self.harness_state.stop_hook_active).await?;
            crate::agent::runloop::unified::turn::utils::render_hook_messages(self.renderer, &outcome.messages)?;
            if let Some(reason) = outcome.block_reason {
                push_system_directive_once(self.working_history, &reason);
                self.harness_state.stop_hook_active = true;
                return Ok(TurnHandlerOutcome::Continue);
            }
        }
        self.harness_state.stop_hook_active = false;

        // Terminal planless exit: every continuation path (interim-progress,
        // denied-interview retry, pseudo-tool-call reprompt, stop-hook) has
        // returned above, so the turn is ending. Now surface the deferred
        // no-approval-ready hint — rendered for the user AND appended to the
        // stored assistant message so it survives in history, ATIF, and
        // harness logs. A turn that continued never stores the hint. A
        // promoted untagged candidate skips the hint: it reaches the
        // persist/approval handoff below.
        if defer_no_ready_plan_hint && proposed_plan.is_none() {
            use vtcode_core::utils::ansi::MessageStyle;
            let hint =
                crate::agent::runloop::unified::planning_workflow_state::PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT;
            self.renderer.line(MessageStyle::Info, hint)?;
            append_no_ready_plan_hint_to_last_assistant(self.working_history);
        }

        if let Some(plan_text) = proposed_plan {
            use vtcode_core::utils::ansi::MessageStyle;

            let planning_active = self.is_planning_active();
            // A revision emitted while an approved-plan execution turn is in
            // flight continues without a second confirmation gate: the user
            // already approved implementing this goal, and the revision is a
            // course correction inside that approval. Telemetry records the
            // resolution as automatic so clients can reconstruct the flow.
            let approved_execution_revision = !planning_active && self.harness_state.is_approved_plan_execution();
            tracing::info!(
                target: "vtcode.planning_workflow",
                plan_ready = true,
                planning_active,
                approved_execution_revision,
                "completed plan reached approval handoff"
            );
            self.handle
                .set_input_status(Some("Validating plan...".to_string()), self.input_status_state.right.clone());
            self.handle.force_redraw();
            // Mirror the footer status into the transcript so the long
            // synthesis-to-approval gap stays visibly live. Non-blocking
            // send; a closed UI surface logs and continues validation.
            if let Err(err) = crate::agent::runloop::unified::tool_summary::render_planning_progress_indicator(
                self.renderer,
                crate::agent::runloop::unified::tool_summary::PLANNING_VALIDATING_INDICATOR,
            ) {
                tracing::warn!("failed to render planning validating indicator: {}", err);
            }
            // Persist before publishing the approval request so consumers that
            // follow the event's plan_file can read the completed draft.
            let validation = validate_plan_content(&plan_text);
            if !validation.is_ready() {
                let error = PlanArtifactError::Invalid {
                    reasons: validation.reasons().join("; "),
                    report: Box::new(validation),
                };
                return self.reject_plan_artifact(error, &plan_text, !tool_free_recovery_pass);
            }
            self.handle
                .set_input_status(Some("Persisting plan...".to_string()), self.input_status_state.right.clone());
            self.handle.force_redraw();
            if let Err(err) = crate::agent::runloop::unified::tool_summary::render_planning_progress_indicator(
                self.renderer,
                crate::agent::runloop::unified::tool_summary::PLANNING_PERSISTING_INDICATOR,
            ) {
                tracing::warn!("failed to render planning persisting indicator: {}", err);
            }

            // Execution-mode first drafts have no planning workflow behind
            // them, so `persist_plan_draft` would bail with "No active plan
            // file" even for a valid draft. Allocate the workspace-local plan
            // location via the shared helper so a valid Build-mode draft can
            // reach the same approval gate as a planning-mode draft. Planning
            // stays inactive; only the file pointer is set.
            if !planning_active && !approved_execution_revision {
                let plan_state = self.tool_registry.planning_workflow_state();
                if plan_state.get_plan_file().await.is_none()
                    && let Err(error) = allocate_plan_file_if_missing(&plan_state).await
                {
                    let error = PlanArtifactError::Persistence { reason: error.to_string() };
                    return self.reject_plan_artifact(error, &plan_text, false);
                }
            }

            let persisted = match persist_plan_draft(&self.tool_registry.planning_workflow_state(), &plan_text).await {
                Ok(persisted) => {
                    self.handle.set_input_status(
                        Some("Preparing approval...".to_string()),
                        self.input_status_state.right.clone(),
                    );
                    self.handle.force_redraw();
                    persisted
                }
                Err(error) => {
                    let error = PlanArtifactError::Persistence { reason: error.to_string() };
                    return self.reject_plan_artifact(error, &plan_text, false);
                }
            };
            // `persist_plan_draft` already validated the same immutable text
            // before writing, so re-checking `persisted.validation.is_ready()`
            // here is redundant. The persisted-readiness gate below rereads the
            // file from disk and verifies sidecar trackers exist — that check
            // is NOT redundant and stays.
            if !persisted_plan_is_ready(&self.tool_registry.planning_workflow_state()).await {
                let error = PlanArtifactError::Persistence {
                    reason: "plan, sidecar tracker, and workspace tracker were not published completely".to_string(),
                };
                return self.reject_plan_artifact(error, &plan_text, false);
            }
            // Construct from the already-validated report instead of
            // re-parsing the same immutable text a fourth time.
            let plan = ValidatedPlanArtifact::from_validated(
                persisted.plan_file.clone(),
                plan_text.clone(),
                persisted.validation.clone(),
            );
            let plan_state = self.tool_registry.planning_workflow_state();
            emit_plan_ready_events(
                self.plan_session,
                &plan_state,
                self.harness_emitter,
                &self.harness_state.run_id.0,
                &self.harness_state.turn_id.0,
                &plan_text,
            )
            .await;

            let require_confirmation = self.vt_cfg.map(|cfg| cfg.agent.require_plan_confirmation).unwrap_or(true);
            let supports_inline = self.renderer.supports_inline_ui();
            tracing::info!(
                target: "vtcode.planning_workflow",
                plan_ready = true,
                require_confirmation,
                supports_inline_ui = supports_inline,
                "plan approval overlay condition check"
            );
            let approval_route = if approved_execution_revision {
                PlanApprovalRoute::Automatic
            } else {
                plan_approval_route(require_confirmation, supports_inline, self.skip_confirmations, self.full_auto)
            };
            tracing::info!(
                target: "vtcode.planning_workflow",
                ?approval_route,
                "plan approval route selected"
            );
            if approval_route == PlanApprovalRoute::Inline {
                use crate::agent::runloop::unified::planning_workflow::{
                    PlanApprovalRequestContext, PlanApprovalTelemetryContext, execute_plan_approval,
                };
                if !planning_active {
                    self.renderer.line(
                        MessageStyle::Info,
                        "The agent proposed a plan during execution; review it before approving.",
                    )?;
                }
                // Resolve owned copies before the mutable `tool_registry`
                // borrow for the approval call begins.
                let approval_editor = self.vt_cfg.map(|cfg| cfg.tools.editor.clone()).unwrap_or_default();
                let approval_workspace_root = self.tool_registry.workspace_root().clone();
                let outcome = execute_plan_approval(
                    self.tool_registry,
                    self.plan_session,
                    self.handle,
                    self.session,
                    self.ctrl_c_state,
                    self.ctrl_c_notify,
                    PlanApprovalRequestContext {
                        plan: &plan,
                        skip_confirmations: self.skip_confirmations,
                        full_auto: self.full_auto,
                        context_usage_percent: self.context_manager.context_usage_percent(
                            vtcode_core::compaction::effective_context_budget(
                                self.vt_cfg,
                                &**self.provider_client,
                                &resolve_effective_request_model(
                                    &self.config.model,
                                    self.active_primary_agent.active(),
                                ),
                            ),
                        ),
                        editor: approval_editor,
                        workspace_root: approval_workspace_root,
                    },
                    PlanApprovalTelemetryContext {
                        emitter: self.harness_emitter,
                        thread_id: &self.harness_state.run_id.0,
                        turn_id: &self.harness_state.turn_id.0,
                    },
                )
                .await?;
                if matches!(
                    &outcome,
                    TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false })
                ) {
                    self.handle_assistant_response(
                        PLAN_APPROVAL_DISMISSED_NOTICE.to_string(),
                        Vec::new(),
                        None,
                        false,
                        Some(uni::AssistantPhase::FinalAnswer),
                    )?;
                }
                return Ok(outcome);
            }

            self.renderer.line(MessageStyle::Info, "Plan ready for approval:")?;
            // The harness normalizes plan markdown before rendering so headings,
            // lists, and formatting are preserved and raw `<proposed_plan>`
            // wrappers never leak into the transcript. Any repair is surfaced
            // explicitly so rendering issues are visible instead of silent.
            let (display_markdown, display_warnings) =
                crate::agent::runloop::unified::plan_blocks::prepare_plan_markdown_for_display(&plan_text);
            if display_markdown.trim().is_empty() {
                self.renderer.line(
                    MessageStyle::Warning,
                    "Plan content was empty after cleanup; see the persisted plan file for details.",
                )?;
            } else {
                self.renderer.line(MessageStyle::Response, &display_markdown)?;
            }
            for warning in display_warnings {
                self.renderer.line(MessageStyle::Warning, &warning)?;
            }
            if approval_route == PlanApprovalRoute::Headless {
                self.handle_assistant_response(
                    PLAN_APPROVAL_WAITING_NOTICE.to_string(),
                    Vec::new(),
                    None,
                    false,
                    Some(uni::AssistantPhase::FinalAnswer),
                )?;
                return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed {
                    plan_approved_execution_pending: false,
                }));
            }

            if !planning_active {
                // Planning-inactive arrivals (revisions of an approved
                // execution, or policy-automatic plans over a persisted draft)
                // continue without the full handoff: `persist_plan_draft`
                // already refreshed the tracker sidecar, and the planning→
                // execution handoff — which recreates the task tracker behind
                // an active planning gate — does not apply here. Resolve the
                // approval automatically and schedule the continuation turn.
                crate::agent::runloop::unified::planning_workflow::resolve_plan_approval(
                    self.plan_session,
                    self.harness_emitter,
                    &self.harness_state.run_id.0,
                    &self.harness_state.turn_id.0,
                    vtcode_core::exec::events::PlanApprovalDecision::AutoAccept,
                    true,
                );
                if approved_execution_revision {
                    self.renderer.line(
                        MessageStyle::Info,
                        "The implementation agent proposed a revised plan; continuing the approved execution with it.",
                    )?;
                } else {
                    self.renderer.line(
                        MessageStyle::Info,
                        "Plan approved by the active execution policy; starting implementation.",
                    )?;
                }
                return Ok(TurnHandlerOutcome::BreakWithPolicy {
                    result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                    target: crate::agent::runloop::unified::planning_workflow::resolve_plan_execution_target(
                        vtcode_core::exec::events::PlanApprovalDecision::AutoAccept,
                        crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Current,
                        self.skip_confirmations,
                        self.full_auto,
                    ),
                });
            }

            self.renderer
                .line(MessageStyle::Info, "Plan approved by the active execution policy; starting implementation.")?;
            let handoff = crate::agent::runloop::unified::planning_workflow::complete_approved_plan_handoff(
                self.tool_registry,
                self.plan_session,
                self.handle,
                plan,
                crate::agent::runloop::unified::planning_workflow::resolve_plan_execution_target(
                    vtcode_core::exec::events::PlanApprovalDecision::AutoAccept,
                    crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Current,
                    true,
                    self.full_auto,
                ),
            )
            .await;
            let handoff = match handoff {
                Ok(handoff) => handoff,
                Err(error) => {
                    tracing::warn!(target: "vtcode.planning_workflow", error = %error, "automatic approved-plan handoff blocked");
                    let message = format!("Plan execution is blocked: {error}");
                    self.renderer.line(MessageStyle::Error, &message)?;
                    return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed {
                        plan_approved_execution_pending: false,
                    }));
                }
            };
            crate::agent::runloop::unified::planning_workflow::resolve_plan_approval(
                self.plan_session,
                self.harness_emitter,
                &self.harness_state.run_id.0,
                &self.harness_state.turn_id.0,
                vtcode_core::exec::events::PlanApprovalDecision::AutoAccept,
                true,
            );
            let target = handoff.target;
            if target
                .agent_name()
                .eq_ignore_ascii_case(self.active_primary_agent.active().name())
            {
                return Ok(TurnHandlerOutcome::BreakWithPolicy {
                    result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                    target,
                });
            }
            return Ok(TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy { target });
        }

        Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false }))
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
    let bounded = if plan_text.len() > REJECTED_PLAN_DRAFT_HISTORY_BUDGET {
        let mut end = REJECTED_PLAN_DRAFT_HISTORY_BUDGET;
        while !plan_text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…[truncated]", &plan_text[..end])
    } else {
        plan_text.to_string()
    };
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
mod tests {
    use super::*;
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
    use vtcode_core::tools::handlers::planning_workflow::CANONICAL_STEP_FORMAT;

    fn last_final_answer_text(ctx: &TurnProcessingContext<'_>) -> String {
        ctx.working_history
            .iter()
            .rev()
            .find(|message| {
                message.role == uni::MessageRole::Assistant && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            })
            .map(|message| message.content.as_text().into_owned())
            .expect("terminal rejection must publish a final assistant message")
    }

    fn assert_history_carries_validator_feedback(final_text: &str) {
        assert!(
            final_text.contains("Plan validation issues:") && final_text.contains(CANONICAL_STEP_FORMAT),
            "history must carry validator feedback and canonical step format, got: {final_text}"
        );
    }

    /// Unparseable pseudo-tool-call markup: the `<tools:call>` name is empty,
    /// so every textual parser rejects it, but the pseudo-marker scan still
    /// sees `<tool_call`. Mirrors the raw-XML leak from turn_887/turn_888.
    const BROKEN_MARKUP_RESPONSE: &str =
        "I need to inspect the workspace.\n<tool_call>\n<tools:call name=\"\">\n</tools:call>\n</tool_call>";

    #[tokio::test]
    async fn plan_mode_pseudo_tool_call_markup_is_stripped_and_reprompts_once() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.enable_planning();
        let mut ctx = backing.turn_processing_context();

        let outcome = ctx
            .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
            .await
            .expect("text response should be handled");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Continue),
            "plan-mode pseudo-tool-call markup should re-prompt instead of ending the turn"
        );

        let assistant_texts: Vec<String> = ctx
            .working_history
            .iter()
            .filter(|message| message.role == uni::MessageRole::Assistant)
            .map(|message| message.content.as_text().into_owned())
            .collect();
        assert!(
            assistant_texts
                .iter()
                .any(|text| text.contains("I need to inspect the workspace.")),
            "the prose part of the response should be preserved: {assistant_texts:?}"
        );
        assert!(
            assistant_texts.iter().all(|text| !text.contains("<tool_call")),
            "raw tool-call markup must never be stored in history: {assistant_texts:?}"
        );

        let directive_present = ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::System && message.content.as_text().contains("not executed")
        });
        assert!(directive_present, "a re-prompt directive should be pushed into history");
    }

    #[tokio::test]
    async fn plan_mode_pseudo_tool_call_reprompt_is_bounded() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.enable_planning();
        let mut ctx = backing.turn_processing_context();
        for _ in 0..crate::agent::runloop::unified::planning_workflow_state::MAX_PLAN_PSEUDO_TOOL_CALL_REPROMPTS {
            ctx.plan_session.mark_plan_pseudo_tool_call_reprompt_used();
        }

        let outcome = ctx
            .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
            .await
            .expect("text response should be handled");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Break(_)),
            "an exhausted reprompt budget must end the turn instead of looping"
        );
        let assistant_texts: Vec<String> = ctx
            .working_history
            .iter()
            .filter(|message| message.role == uni::MessageRole::Assistant)
            .map(|message| message.content.as_text().into_owned())
            .collect();
        assert!(
            assistant_texts.iter().all(|text| !text.contains("<tool_call")),
            "even with the budget exhausted, raw markup must be stripped from the final answer: {assistant_texts:?}"
        );
    }

    #[tokio::test]
    async fn build_mode_pseudo_tool_call_markup_keeps_existing_behavior() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();

        let outcome = ctx
            .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
            .await
            .expect("text response should be handled");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Break(_)),
            "outside planning, text responses still end the turn (no new reprompt path)"
        );
    }

    const RECOVERY_VALID_PLAN: &str = r#"# Recovery plan

## Summary
Fix the read-cap recovery path from gathered evidence.

## Implementation Steps
1. Reorder the guard -> files: [src/agent/runloop/unified/turn/tool_outcomes/handlers/guards/read_guard.rs] -> verify: [cargo check --locked]

## Test Cases and Validation
1. Run cargo check --locked.

## Assumptions and Defaults
1. Keep execution-mode caps strict.
"#;

    const RECOVERY_INVALID_PLAN: &str = r#"# Recovery plan

## Summary
Fix the read-cap recovery path from gathered evidence.

## Implementation Steps
1. Reorder the guard

## Test Cases and Validation
1. Run the focused test.

## Assumptions and Defaults
1. Keep existing behavior.
"#;

    /// Regression fixture from session-vtcode-20260916T033857Z_346951-33962:
    /// the plan has otherwise concrete steps, but more than one verification
    /// item is rejected by the validator. The repair must see the complete
    /// bounded draft and validator-owned feedback before it gets a corrected
    /// plan.
    const RECOVERY_INVALID_VERIFICATION_PLAN: &str = r#"# Preview recovery plan

## Summary
Repair planning recovery from the evidence already gathered.

## Implementation Steps
1. Update the preview guard -> files: [src/agent/runloop/unified/turn/guards.rs] -> verify: [run checks]
2. Strengthen synthesis guidance -> files: [src/agent/runloop/unified/turn/turn_loop.rs] -> verify: [cargo check --locked]
3. Preserve validator feedback -> files: [src/agent/runloop/unified/turn/context/response_handling.rs] -> verify: [rg -n verify src/agent/runloop/unified/turn]
4. Keep tools disabled during repair -> files: [src/agent/runloop/unified/turn/turn_processing/result_handler.rs] -> verify: [cargo nextest run -p vtcode]
5. Add the transition regression -> files: [src/agent/runloop/unified/turn/turn_loop/tests.rs] -> verify: [git diff --check]

## Test Cases and Validation
1. Run the focused planning recovery test.

## Assumptions and Defaults
1. Keep strict plan validation and bounded recovery.
"#;

    const RECOVERY_CORRECTED_VERIFICATION_PLAN: &str = r#"# Preview recovery plan

## Summary
Repair planning recovery from the evidence already gathered with concrete verification.

## Implementation Steps
1. Update the preview guard -> files: [src/agent/runloop/unified/turn/guards.rs] -> verify: [cargo nextest run -p vtcode]
2. Strengthen synthesis guidance -> files: [src/agent/runloop/unified/turn/turn_loop.rs] -> verify: [cargo check --locked]
3. Preserve validator feedback -> files: [src/agent/runloop/unified/turn/context/response_handling.rs] -> verify: [rg -n verify src/agent/runloop/unified/turn]
4. Keep tools disabled during repair -> files: [src/agent/runloop/unified/turn/turn_processing/result_handler.rs] -> verify: [cargo nextest run -p vtcode]
5. Add the transition regression -> files: [src/agent/runloop/unified/turn/turn_loop/tests.rs] -> verify: [cargo nextest run -p vtcode]

## Test Cases and Validation
1. Run cargo nextest run -p vtcode.

## Assumptions and Defaults
1. Keep strict plan validation and bounded recovery.
"#;

    #[tokio::test]
    async fn tool_free_recovery_tolerates_intro_prose_around_valid_plan() {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.activate_planning_for_test();
        backing.activate_tool_free_recovery_for_test("per-file read cap");
        let mut ctx = backing.turn_processing_context();
        assert!(ctx.consume_recovery_pass());

        let outcome = ctx
            .handle_text_response(
                "Here is the plan from the gathered evidence.".to_string(),
                Vec::new(),
                None,
                Some(RECOVERY_VALID_PLAN.to_string()),
                false,
            )
            .await
            .expect("recovery response should be handled");

        // The old gate ended Blocked with "prose outside the required block".
        // Prose-tolerant recovery must persist the plan instead.
        assert!(
            !matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })),
            "intro prose around a valid plan must not end Blocked"
        );
    }

    #[tokio::test]
    async fn tool_free_invalid_plan_schedules_one_bounded_repair_then_blocks() {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.activate_planning_for_test();
        backing.activate_tool_free_recovery_for_test("per-file read cap");
        let mut ctx = backing.turn_processing_context();
        assert!(ctx.consume_recovery_pass());

        // First invalid draft: repair budget is fresh, so the turn continues
        // with a validator-owned repair directive instead of ending Blocked.
        let first = ctx
            .handle_text_response(String::new(), Vec::new(), None, Some(RECOVERY_INVALID_PLAN.to_string()), false)
            .await
            .expect("first invalid draft should be handled");
        assert!(
            matches!(first, TurnHandlerOutcome::Continue),
            "first invalid recovery draft must schedule bounded repair"
        );

        // Exhaust the turn-scoped repair budget and re-enter the pass:
        // the next invalid draft must take the resumable blocked handoff.
        ctx.plan_session.mark_plan_validation_repair_used();
        assert!(ctx.consume_recovery_pass());
        let second = ctx
            .handle_text_response(String::new(), Vec::new(), None, Some(RECOVERY_INVALID_PLAN.to_string()), false)
            .await
            .expect("second invalid draft should be handled");
        assert!(
            matches!(second, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })),
            "exhausted repair budget must end with the resumable blocked handoff"
        );
    }

    #[tokio::test]
    async fn tool_free_invalid_plan_repair_preserves_draft_feedback_and_accepts_corrected_plan() {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.activate_planning_for_test();
        backing.activate_tool_free_recovery_for_test("preview budget exhausted");
        let mut ctx = backing.turn_processing_context();
        assert!(ctx.consume_recovery_pass());

        let first = ctx
            .handle_text_response(
                String::new(),
                Vec::new(),
                None,
                Some(RECOVERY_INVALID_VERIFICATION_PLAN.to_string()),
                false,
            )
            .await
            .expect("invalid verification fixture should be handled");
        assert!(
            matches!(first, TurnHandlerOutcome::Continue),
            "the first invalid recovery draft must schedule bounded repair"
        );
        assert!(ctx.recovery_is_tool_free(), "the repair pass must keep tools disabled");

        let rejected_draft = ctx
            .working_history
            .iter()
            .find(|message| {
                message.role == uni::MessageRole::Assistant
                    && message.content.as_text().contains("Update the preview guard")
            })
            .expect("the rejected non-streaming draft must remain in assistant history");
        assert_eq!(rejected_draft.phase, Some(uni::AssistantPhase::Commentary));

        let feedback = ctx
            .working_history
            .iter()
            .find(|message| {
                message.role == uni::MessageRole::System
                    && message.content.as_text().contains("proposed plan was rejected")
            })
            .expect("validator-owned feedback must be retained for the repair pass")
            .content
            .as_text();
        assert!(feedback.contains("2 of 5 implementation step(s)"), "feedback should count both invalid steps");
        assert!(feedback.contains("step 1: verification item 1"));
        assert!(feedback.contains("step 5: verification item 1"));
        assert!(feedback.contains("Valid examples: `verify: [cargo nextest run -p vtcode]`"));
        assert!(
            feedback.contains("verify: [sed -n") && feedback.contains("verify: [grep -n"),
            "repair feedback must list inspection-command valid examples: {feedback}"
        );
        for (name, text) in [
            ("denied-interview-retry", DENIED_INTERVIEW_PLAN_SYNTHESIS_RETRY_DIRECTIVE),
            ("pseudo-tool-reprompt", PLAN_PSEUDO_TOOL_CALL_REPROMPT_DIRECTIVE),
        ] {
            assert!(text.contains("sed -n") && text.contains("grep -n"), "{name} missing inspection examples: {text}");
            assert!(text.contains("git diff --check"), "{name} missing invalid VCS example: {text}");
        }
        assert!(
            !feedback.contains("1. Update the preview guard -> files:"),
            "validator-owned feedback must not echo the rejected draft"
        );

        assert!(ctx.consume_recovery_pass());
        let repaired = ctx
            .handle_text_response(
                String::new(),
                Vec::new(),
                None,
                Some(RECOVERY_CORRECTED_VERIFICATION_PLAN.to_string()),
                false,
            )
            .await
            .expect("corrected recovery plan should be handled");
        assert!(
            matches!(
                repaired,
                TurnHandlerOutcome::BreakWithPolicy {
                    result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                    ..
                }
            ),
            "the corrected draft must reach the approval handoff without a blocked recovery"
        );
        assert!(
            ctx.working_history.iter().any(|message| {
                message.role == uni::MessageRole::Assistant
                    && message.content.as_text().contains("Update the preview guard")
            }),
            "the rejected draft must remain available after the corrected plan is accepted"
        );
        let plan_file = ctx
            .tool_registry
            .planning_workflow_state()
            .get_plan_file()
            .await
            .expect("the corrected plan should publish a plan file");
        let persisted_plan = std::fs::read_to_string(plan_file).expect("read the corrected plan");
        assert!(
            persisted_plan.contains("with concrete verification"),
            "the corrected fixture, rather than the rejected draft, must be persisted"
        );
    }

    #[tokio::test]
    async fn tool_free_unwrapped_plan_is_repaired_instead_of_blocking_immediately() {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.activate_planning_for_test();
        backing.activate_tool_free_recovery_for_test("per-file read cap");
        let mut ctx = backing.turn_processing_context();
        assert!(ctx.consume_recovery_pass());

        let outcome = ctx
            .handle_text_response(RECOVERY_INVALID_PLAN.to_string(), Vec::new(), None, None, false)
            .await
            .expect("unwrapped recovery draft should be handled");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Continue),
            "an unwrapped invalid draft should use the bounded repair path"
        );
        assert!(
            ctx.working_history.iter().any(|message| {
                message.role == uni::MessageRole::System
                    && message.content.as_text().contains("Rewrite every implementation step")
            }),
            "the repair directive should be retained for the next synthesis pass"
        );
    }

    /// Tagless markdown plan in the turn_1075 shape: numbered steps but no
    /// `<proposed_plan>` tags, no `files:` targets, and no `verify:` checks.
    const UNTAGGED_PLANLIKE_TEXT: &str = "## Simple plan to improve startup\n\n1. Measure the current startup path\n2. Identify the slowest steps\n3. Defer non-critical work\n";

    #[tokio::test]
    async fn untagged_planlike_text_schedules_bounded_repair_instead_of_generic_hint() {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.activate_planning_for_test();
        let mut ctx = backing.turn_processing_context();

        let outcome = ctx
            .handle_text_response(UNTAGGED_PLANLIKE_TEXT.to_string(), Vec::new(), None, None, false)
            .await
            .expect("untagged plan-like text should be handled");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Continue),
            "untagged plan-like text must schedule bounded repair, not end the turn"
        );
        assert!(
            ctx.working_history.iter().any(|message| {
                message.role == uni::MessageRole::System
                    && message.content.as_text().contains("Rewrite every implementation step")
            }),
            "the repair directive should teach the canonical step contract"
        );
    }

    #[tokio::test]
    async fn untagged_planlike_text_with_exhausted_budget_rejects_with_specific_reasons() {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.activate_planning_for_test();
        let mut ctx = backing.turn_processing_context();
        ctx.plan_session.mark_plan_validation_repair_used();
        ctx.plan_session.mark_plan_validation_repair_used();

        let outcome = ctx
            .handle_text_response(UNTAGGED_PLANLIKE_TEXT.to_string(), Vec::new(), None, None, false)
            .await
            .expect("untagged plan-like text should be handled");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })),
            "exhausted repair budget must end the turn with the specific rejection, not a silent hint"
        );
        assert_history_carries_validator_feedback(&last_final_answer_text(&ctx));
    }

    #[tokio::test]
    async fn untagged_valid_plan_promotes_to_approval_handoff() {
        // A tagless draft that already validates must not end with the generic
        // planless hint; it promotes to a plan candidate and reaches the same
        // persist/approval handoff as a tagged draft.
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.activate_planning_for_test();
        let mut ctx = backing.turn_processing_context();

        let outcome = ctx
            .handle_text_response(EXECUTION_REVISION_PLAN.to_string(), Vec::new(), None, None, false)
            .await
            .expect("valid untagged plan should be handled");

        assert!(
            matches!(
                outcome,
                TurnHandlerOutcome::BreakWithPolicy {
                    result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                    ..
                }
            ),
            "valid untagged plan must reach the approval handoff"
        );
        assert!(
            !ctx.working_history.iter().any(|message| {
                message.role == uni::MessageRole::Assistant
                    && message.content.as_text().contains("no approval-ready plan was produced")
            }),
            "a promoted plan must not store the planless hint"
        );
    }

    /// A valid revision of an already-approved plan, in the canonical section
    /// shape the artifact validator requires.
    const EXECUTION_REVISION_PLAN: &str = r#"# Revised plan

## Summary
Repairs the approved plan after the referenced paths moved.

## Implementation Steps
1. Update the module path -> files: [src/lib.rs] -> verify: [cargo check]

## Test Cases and Validation
1. Run cargo check --locked.

## Assumptions and Defaults
1. Keep the approved scope.
"#;

    #[tokio::test]
    async fn approved_execution_replan_continues_without_second_confirmation() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.persist_approved_plan_for_test(EXECUTION_REVISION_PLAN).await;
        backing.set_approved_plan_execution_for_test(true);
        backing.set_skip_confirmations_for_test(false);
        let mut ctx = backing.turn_processing_context();

        let outcome = ctx
            .handle_text_response(
                "Replanning: the approved paths no longer exist.".to_string(),
                Vec::new(),
                None,
                Some(EXECUTION_REVISION_PLAN.to_string()),
                false,
            )
            .await
            .expect("execution replan should route through the approval handoff");

        assert!(
            matches!(
                outcome,
                TurnHandlerOutcome::BreakWithPolicy {
                    result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                    target,
                } if !target.skip_confirmations,
            ),
            "the revised plan must schedule the continuation turn while keeping the session confirmation policy"
        );
    }

    #[tokio::test]
    async fn full_auto_execution_replan_continues_without_second_confirmation() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.persist_approved_plan_for_test(EXECUTION_REVISION_PLAN).await;
        backing.set_skip_confirmations_for_test(true);
        let mut ctx = backing.turn_processing_context();

        let outcome = ctx
            .handle_text_response(
                "Replanning: the approved paths no longer exist.".to_string(),
                Vec::new(),
                None,
                Some(EXECUTION_REVISION_PLAN.to_string()),
                false,
            )
            .await
            .expect("full-auto replan should continue automatically");

        assert!(
            matches!(
                outcome,
                TurnHandlerOutcome::BreakWithPolicy {
                    result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                    target,
                } if target.skip_confirmations,
            ),
            "a policy-automatic replan must schedule the continuation turn"
        );
    }

    #[tokio::test]
    async fn execution_mode_invalid_unapproved_plan_gets_tool_free_repair_and_reaches_approval() {
        // session-vtcode-20260924T133543Z_155288-07964: Build mode produced
        // a useful README plan with bold labels instead of required headings.
        const INVALID_README_PLAN: &str = "**Goal:** Improve README density.\n\n1. Fix the broken link in `README.md`.\n\n**Verification:** Check links.\n";
        const REPAIRED_README_PLAN: &str = "## Summary\nImprove README density.\n\n## Implementation Steps\n1. Fix the broken link -> files: [README.md] -> verify: [rg -n 'Loop engineering' README.md]\n\n## Test Cases and Validation\n- Confirm the link with rg -n 'Loop engineering' README.md.\n\n## Assumptions and Defaults\n- Keep unrelated README sections as they are.\n";

        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();

        let first = ctx
            .handle_text_response(String::new(), Vec::new(), None, Some(INVALID_README_PLAN.to_string()), false)
            .await
            .expect("invalid unapproved plan should schedule repair");
        assert!(matches!(first, TurnHandlerOutcome::Continue));
        assert!(ctx.recovery_is_tool_free(), "repair must not expose edit tools before approval");
        assert!(ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().contains("missing required section(s)")
                && message.content.as_text().contains("## Summary")
        }));
        assert!(ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant && message.content.as_text().contains(INVALID_README_PLAN)
        }));

        let validation = validate_plan_content(REPAIRED_README_PLAN);
        assert!(validation.is_ready(), "corrected fixture must validate: {:?}", validation.reasons());
        assert!(ctx.consume_recovery_pass());
        let repaired = ctx
            .handle_text_response(String::new(), Vec::new(), None, Some(REPAIRED_README_PLAN.to_string()), false)
            .await
            .expect("corrected plan should reach approval");
        let outcome_kind = match &repaired {
            TurnHandlerOutcome::Continue => "continue".to_string(),
            TurnHandlerOutcome::Break(result) => format!("break: {result:?}"),
            TurnHandlerOutcome::BreakWithPolicy { result, .. } => format!("break with policy: {result:?}"),
            TurnHandlerOutcome::SwitchPrimaryAgent(_) => "switch primary agent".to_string(),
            TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy { .. } => "switch primary agent with policy".to_string(),
        };
        assert!(
            matches!(
                repaired,
                TurnHandlerOutcome::BreakWithPolicy {
                    result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                    ..
                }
            ),
            "a corrected Build-mode plan should reach the approval handoff, got {outcome_kind}"
        );
    }

    #[tokio::test]
    async fn execution_mode_invalid_unapproved_plan_repair_is_bounded() {
        const INVALID_PLAN: &str = "**Goal:** Improve README.md.\n\n1. Fix a link in README.md.\n";
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();

        let first = ctx
            .handle_text_response(String::new(), Vec::new(), None, Some(INVALID_PLAN.to_string()), false)
            .await
            .expect("first invalid plan should schedule repair");
        assert!(matches!(first, TurnHandlerOutcome::Continue));

        assert!(ctx.consume_recovery_pass());
        let second = ctx
            .handle_text_response(String::new(), Vec::new(), None, Some(INVALID_PLAN.to_string()), false)
            .await
            .expect("a failed repair should end with feedback");
        assert!(matches!(
            second,
            TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false })
        ));
        assert!(ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && message.content.as_text().contains(EXECUTION_PLAN_REJECTION_NOTICE)
        }));
        // Terminal rejection must leave the validator contract in history so a
        // later `continue` can repair the draft (session-vtcode-20260924T133543Z).
        let final_text = last_final_answer_text(&ctx);
        assert_history_carries_validator_feedback(&final_text);
        assert!(
            final_text.contains("missing required section"),
            "history must name the missing sections, got: {final_text}"
        );
    }

    #[tokio::test]
    async fn execution_mode_replan_rejection_surfaces_feedback_without_repair_directive() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.set_approved_plan_execution_for_test(true);
        let mut ctx = backing.turn_processing_context();

        let outcome = ctx
            .handle_text_response(
                "Replanning: the approved plan is stale.".to_string(),
                Vec::new(),
                None,
                Some("not a valid plan artifact".to_string()),
                false,
            )
            .await
            .expect("invalid replan should end the turn with feedback");

        assert!(
            matches!(
                outcome,
                TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false })
            ),
            "a rejected revision must not schedule an execution turn"
        );
        assert!(
            !ctx.working_history
                .iter()
                .any(|message| message.role == uni::MessageRole::System
                    && message.content.as_text().contains("Planning recovery")),
            "execution-mode rejection must not push planning repair directives"
        );
        assert!(
            ctx.working_history
                .iter()
                .any(|message| message.role == uni::MessageRole::Assistant
                    && message.content.as_text().contains("not a valid plan artifact")),
            "the rejected draft stays attached to history for inspection"
        );
        assert!(
            ctx.working_history.iter().any(|message| {
                message.role == uni::MessageRole::Assistant
                    && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                    && message.content.as_text().contains(EXECUTION_PLAN_REJECTION_NOTICE)
            }),
            "a rejected revision must publish a harness-visible final response"
        );
        assert_history_carries_validator_feedback(&last_final_answer_text(&ctx));
        assert!(
            ctx.harness_state.final_response_rendered(),
            "a rejected revision must count as a rendered final response"
        );
    }

    #[tokio::test]
    async fn plan_only_execution_rejection_still_publishes_final_response() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.set_approved_plan_execution_for_test(true);
        let mut ctx = backing.turn_processing_context();

        let outcome = ctx
            .handle_text_response(String::new(), Vec::new(), None, Some("not a valid plan artifact".to_string()), false)
            .await
            .expect("plan-only invalid replan should end the turn with feedback");

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false })
        ));
        assert!(ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && message.content.as_text().contains(EXECUTION_PLAN_REJECTION_NOTICE)
        }));
        assert_history_carries_validator_feedback(&last_final_answer_text(&ctx));
        assert!(!ctx.working_history.iter().any(|message| {
            message
                .content
                .as_text()
                .contains("The turn stopped before a final assistant response")
        }));
        assert!(
            ctx.harness_state.final_response_event_emitted(),
            "a plan-only rejection must publish the final assistant event"
        );
    }

    #[tokio::test]
    async fn denied_interview_without_ready_plan_replaces_prose_with_hint() {
        // When request_user_input is permanently denied (non-interactive
        // runtime), no plan was proposed, no plan is persisted, and the model
        // emits research prose (not a clarifying question), the prose must be
        // replaced with the no-approval-ready-plan hint so the user gets an
        // actionable message instead of rambling text.
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.enable_planning();
        let mut ctx = backing.turn_processing_context();
        ctx.plan_session.mark_interview_denied();

        let outcome = ctx
            .handle_text_response(
                "I looked at the codebase and found several files.".to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("text response should be handled");

        // The first denied-interview response gets a bounded synthesis retry
        // (plan_synthesis_retry_allowed is true on the first attempt).
        assert!(
            matches!(outcome, TurnHandlerOutcome::Continue),
            "first denied-interview response without a plan should retry synthesis"
        );
        let directive_present = ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::System
                && message
                    .content
                    .as_text()
                    .contains(DENIED_INTERVIEW_PLAN_SYNTHESIS_RETRY_DIRECTIVE)
        });
        assert!(directive_present, "a plan-synthesis retry directive should be pushed");
    }

    #[tokio::test]
    async fn denied_interview_exhausted_retry_ends_with_hint_not_prose() {
        // After the bounded retry is exhausted, a second prose response must
        // end the turn with the no-approval-ready-plan hint, not the model's
        // research prose.
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.enable_planning();
        let mut ctx = backing.turn_processing_context();
        ctx.plan_session.mark_interview_denied();
        ctx.plan_session.mark_plan_synthesis_retry_used();

        let outcome = ctx
            .handle_text_response("I found more files to examine.".to_string(), Vec::new(), None, None, false)
            .await
            .expect("text response should be handled");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Break(_)),
            "after retry exhaustion, prose without a plan should end the turn"
        );
        let last = ctx
            .working_history
            .iter()
            .rev()
            .find(|m| m.role == uni::MessageRole::Assistant)
            .expect("an assistant message must be pushed");
        let text = last.content.as_text();
        assert!(
            text.contains("no approval-ready plan was produced"),
            "the no-approval-ready hint must replace prose: {text}"
        );
        assert!(
            !text.contains("I found more files"),
            "the model's research prose must NOT be the final answer: {text}"
        );
    }

    #[tokio::test]
    async fn denied_interview_preserves_clarifying_question() {
        // A clarifying question (text ending with '?') is the text-mode
        // equivalent of the unavailable interview modal. It must NOT be
        // replaced with the hint or trigger a synthesis retry — the turn ends
        // so the user can answer it (checkpoint turn_856).
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.enable_planning();
        let mut ctx = backing.turn_processing_context();
        ctx.plan_session.mark_interview_denied();

        let outcome = ctx
            .handle_text_response(
                "Should I focus on the launch path or the config loading?".to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("text response should be handled");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Break(_)),
            "a clarifying question should end the turn for user input, not retry"
        );
        let last = ctx
            .working_history
            .iter()
            .rev()
            .find(|m| m.role == uni::MessageRole::Assistant)
            .expect("an assistant message must be pushed");
        let text = last.content.as_text();
        assert!(
            text.contains("Should I focus on the launch path or the config loading?"),
            "the clarifying question must be preserved verbatim: {text}"
        );
        assert!(
            !text.contains("no approval-ready plan was produced"),
            "the hint must NOT replace a clarifying question: {text}"
        );
    }

    #[tokio::test]
    async fn planless_planning_turn_ends_with_hint_in_history() {
        // Checkpoint turn_912: after the validation repair was consumed, the
        // model answered with a planless status echo and the turn closed with
        // no visible next step. The assistant's stored answer must carry the
        // no-approval-ready hint so the user knows planning is still active.
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.enable_planning();
        let mut ctx = backing.turn_processing_context();
        ctx.plan_session.mark_plan_validation_repair_used();

        let outcome = ctx
            .handle_text_response(
                "Planning workflow is active with read-only permissions.".to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("text response should be handled");

        assert!(matches!(outcome, TurnHandlerOutcome::Break(_)), "a planless planning response should end the turn");
        let last = ctx
            .working_history
            .iter()
            .rev()
            .find(|m| m.role == uni::MessageRole::Assistant)
            .expect("an assistant message must be pushed");
        let text = last.content.as_text();
        assert!(
            text.contains("no approval-ready plan was produced"),
            "the no-approval-ready hint must accompany the planless answer: {text}"
        );
        assert!(
            text.contains("Planning workflow is active with read-only permissions."),
            "the model's own text must be preserved, not replaced: {text}"
        );
    }

    #[tokio::test]
    async fn planless_planning_clarifying_question_ends_without_hint() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.enable_planning();
        let mut ctx = backing.turn_processing_context();

        let outcome = ctx
            .handle_text_response(
                "Should I focus on the runtime loop or startup?".to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("text response should be handled");

        assert!(matches!(outcome, TurnHandlerOutcome::Break(_)));
        let last = ctx
            .working_history
            .iter()
            .rev()
            .find(|m| m.role == uni::MessageRole::Assistant)
            .expect("an assistant message must be pushed");
        let text = last.content.as_text();
        assert!(
            !text.contains("no approval-ready plan was produced"),
            "a clarifying question must not gain the hint: {text}"
        );
    }

    #[tokio::test]
    async fn planless_planning_hint_is_not_stored_when_turn_continues() {
        // Regression: the hint must be deferred until terminality is known.
        // A pseudo-tool-call response that triggers a bounded reprompt returns
        // `Continue` — storing the hint before that decision would leave stale
        // "no approval-ready plan" guidance in history for a turn that kept
        // going.
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.enable_planning();
        let mut ctx = backing.turn_processing_context();

        let outcome = ctx
            .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
            .await
            .expect("text response should be handled");

        assert!(
            matches!(outcome, TurnHandlerOutcome::Continue),
            "pseudo-tool-call markup should re-prompt (Continue), not end the turn"
        );
        let assistant_texts: Vec<String> = ctx
            .working_history
            .iter()
            .filter(|message| message.role == uni::MessageRole::Assistant)
            .map(|message| message.content.as_text().into_owned())
            .collect();
        assert!(
            assistant_texts
                .iter()
                .all(|text| !text.contains("no approval-ready plan was produced")),
            "a continued turn must never store the terminal hint: {assistant_texts:?}"
        );
    }

    #[test]
    fn no_ready_plan_hint_predicate_gates_on_denied_interview_and_questions() {
        assert!(should_render_no_ready_plan_hint(false, "Here is a research summary."));
        assert!(!should_render_no_ready_plan_hint(true, "Here is a research summary."));
        assert!(!should_render_no_ready_plan_hint(false, "Which approach should I take?"));
    }

    #[test]
    fn attempted_plan_detection_catches_untagged_turn_1075_shape() {
        // Regression for checkpoint turn_1075: a markdown plan with numbered
        // steps but no `<proposed_plan>` tags must route to bounded repair,
        // not end with only the generic no-ready-plan hint.
        let tagless = "## Simple plan to improve startup\n\n1. Measure the current startup path\n2. Identify the slowest steps\n3. Defer non-critical work\n";
        assert!(looks_like_attempted_plan(tagless));

        // Asymmetric counterpart: a single numbered line in research prose
        // is not an attempted plan.
        assert!(!looks_like_attempted_plan("Found one candidate:\n1. src/main.rs looks relevant."));
        assert!(!looks_like_attempted_plan("Here is a quick update: I searched the codebase."));
        assert!(!looks_like_attempted_plan(""));
    }

    #[test]
    fn attempted_plan_detection_catches_goal_plus_plan_headings() {
        // Checkpoint turn_1074 used `## Goal` / `## Plan` instead of the
        // canonical headings; that shape must also route to repair.
        let goal_plan = "## Goal\nImprove startup.\n\n## Plan\n1. Measure timing\n2. Defer work\n";
        assert!(looks_like_attempted_plan(goal_plan));

        let implementation = "## Implementation Steps\n1. Measure timing\n";
        assert!(looks_like_attempted_plan(implementation));
    }

    #[test]
    fn attempted_plan_detection_rejects_prose_and_non_plan_headings() {
        // Prose mentioning the phrase is research, not an attempted plan.
        assert!(!looks_like_attempted_plan("The implementation steps are unclear; need to research more."));
        // `## Planning` is not a `## Plan` heading (word boundary).
        assert!(!looks_like_attempted_plan(
            "## Planning status\nGoal is to improve startup.\n1. Found one candidate.\n"
        ));
        // A single goal heading without a plan heading is not an attempt.
        assert!(!looks_like_attempted_plan("## Goal\nImprove startup.\n"));
    }

    #[test]
    fn attempted_plan_detection_catches_step_prefixed_numbering() {
        // Mirrors `numbered_line_parts`: `Step 1:` counts as a numbered step.
        let stepped = "Step 1: Measure timing\nStep 2: Defer work\n";
        assert!(looks_like_attempted_plan(stepped));
        assert!(!looks_like_attempted_plan("Stepwise refinement is needed."));
    }

    #[test]
    fn clarifying_question_detection_keeps_formatting_edge_cases_explicit() {
        let structured_plan = "## Assumptions\n1. Preserve the quoted requirement: \"Should we keep this?\"";
        assert!(!looks_like_clarifying_question(structured_plan));

        let quoted_question = "The requirement is \"Should we keep this?\"";
        assert!(!looks_like_clarifying_question(quoted_question));

        // Keep the current terminal-line heuristic documented until a corpus
        // of production false positives justifies a more semantic classifier.
        assert!(looks_like_clarifying_question("This is rhetorical: why change it?"));
    }

    #[test]
    fn rejected_plan_draft_is_reattached_to_last_assistant_message() {
        let mut history = vec![
            uni::Message::user("make a plan".to_string()),
            uni::Message::assistant("• Planning workflow is active.".to_string()),
        ];
        append_rejected_plan_draft_to_last_assistant(&mut history, "## Summary\nDo the thing");
        let last = history.last().expect("assistant message");
        let text = last.content.as_text();
        assert!(
            text.contains("Planning workflow is active")
                && text.contains("<proposed_plan>\n## Summary\nDo the thing\n</proposed_plan>"),
            "draft must be appended, not replace the stored text: {text:?}"
        );
        assert_eq!(history.len(), 2, "no new message should be pushed");
    }

    #[test]
    fn rejected_plan_draft_replaces_empty_assistant_text_and_bounds_huge_drafts() {
        let mut history = vec![uni::Message::assistant(String::new())];
        append_rejected_plan_draft_to_last_assistant(&mut history, "  ## Summary\nOnly draft  ");
        let text = history[0].content.as_text();
        assert_eq!(text, "<proposed_plan>\n## Summary\nOnly draft\n</proposed_plan>");

        let huge = "x".repeat(REJECTED_PLAN_DRAFT_HISTORY_BUDGET * 2);
        let mut history = vec![uni::Message::assistant(String::new())];
        append_rejected_plan_draft_to_last_assistant(&mut history, &huge);
        let text = history[0].content.as_text();
        assert!(text.len() < REJECTED_PLAN_DRAFT_HISTORY_BUDGET + 64);
        assert!(text.contains("…[truncated]"));

        // A non-streaming extracted plan may have no assistant message yet;
        // preserve it as a bounded commentary message for the repair pass.
        let mut history = vec![uni::Message::user("hi".to_string())];
        append_rejected_plan_draft_to_last_assistant(&mut history, "draft");
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].role, uni::MessageRole::Assistant);
        assert_eq!(history[1].phase, Some(uni::AssistantPhase::Commentary));
        assert_eq!(history[1].content.as_text(), "<proposed_plan>\ndraft\n</proposed_plan>");
        // Empty draft → no-op.
        let mut history = vec![uni::Message::assistant("kept".to_string())];
        append_rejected_plan_draft_to_last_assistant(&mut history, "   ");
        assert_eq!(history[0].content.as_text(), "kept");
    }

    #[tokio::test]
    async fn pseudo_reprompt_queues_bounded_cap_allowance() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.enable_planning();
        let mut ctx = backing.turn_processing_context();
        assert!(!ctx.plan_session.bounded_planning_follow_up_allowed());

        let outcome = ctx
            .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
            .await
            .expect("text response should be handled");
        assert!(matches!(outcome, TurnHandlerOutcome::Continue), "pseudo markup should schedule a bounded reprompt");
        assert!(
            ctx.plan_session.bounded_planning_follow_up_allowed(),
            "the scheduled reprompt must queue one cap allowance so the next request is not immediately blocked"
        );
    }

    #[tokio::test]
    async fn denied_interview_retry_queues_bounded_cap_allowance() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.activate_planning_for_test();
        backing.mark_interview_denied_for_test();
        let mut ctx = backing.turn_processing_context();
        assert!(!ctx.plan_session.bounded_planning_follow_up_allowed());

        let outcome = ctx
            .handle_text_response("Here is a research summary.".to_string(), Vec::new(), None, None, false)
            .await
            .expect("text response should be handled");
        assert!(
            matches!(outcome, TurnHandlerOutcome::Continue),
            "denied-interview prose should schedule a bounded synthesis retry"
        );
        assert!(
            ctx.plan_session.bounded_planning_follow_up_allowed(),
            "the synthesis retry must queue one cap allowance"
        );
    }
}
