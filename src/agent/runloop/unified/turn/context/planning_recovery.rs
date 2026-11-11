//! Planning recovery admission and runtime-generated input; never approves implementation.
use super::*;
use crate::agent::runloop::unified::planning_workflow::recovery_state;
use vtcode_commons::diagnostics::{Diagnostic, DiagnosticAction, DiagnosticSeverity, redact_diagnostic};
use vtcode_commons::ui_protocol::ActivityState;
use vtcode_core::exec::events::planning::PlanningOutcome;

pub(crate) const RETRY_LABEL: &str = "Retry from retained evidence";
pub(crate) const RETRY_ANYWAY_LABEL: &str = "Retry once anyway";
pub(crate) const CLARIFY_LABEL: &str = "Add clarification";
pub(crate) const MODEL_LABEL: &str = "Choose another model";

pub(crate) fn recovery_question(repeated: bool) -> serde_json::Value {
    if !repeated {
        return serde_json::json!({"questions": [{
            "id": "planning_recovery", "header": "Planning",
            "question": "Planning needs your input. Saved progress is available. How should planning continue?",
            "options": [
                {"label": RETRY_LABEL, "description": "Start one bounded synthesis cycle using the saved evidence."},
                {"label": CLARIFY_LABEL, "description": "Add missing facts or revise the retained draft."},
                {"label": MODEL_LABEL, "description": "Select a model, then resume synthesis with saved evidence."}
            ]
        }]});
    }
    // Consecutive identical failures: a blind retry has already been shown
    // not to work. Lead with the options that change the outcome; the first
    // option also picks up the UI's Recommended decoration.
    serde_json::json!({"questions": [{
        "id": "planning_recovery", "header": "Planning",
        "question": "Planning needs your input. Consecutive attempts failed identically, so retrying unchanged is unlikely to help. How should planning continue?",
        "options": [
            {"label": CLARIFY_LABEL, "description": "Add missing facts or revise the retained draft."},
            {"label": MODEL_LABEL, "description": "Select a model, then resume synthesis with saved evidence."},
            {"label": RETRY_ANYWAY_LABEL, "description": "Retry once more without changes; likely to fail the same way."}
        ]
    }]})
}

impl TurnProcessingContext<'_> {
    pub(crate) async fn persist_planning_recovery(&self) -> anyhow::Result<()> {
        if let Some(emitter) = self.harness_emitter {
            emitter
                .persist_checkpoint(&recovery_state::checkpoint_event(
                    &self.plan_session.recovery,
                    &self.harness_state.run_id.0,
                    &self.harness_state.turn_id.0,
                ))
                .await?;
        }
        Ok(())
    }

    /// Called once before each tool-free provider request, including resumed turns.
    pub(crate) async fn admit_planning_recovery_request(&mut self, model: &str) -> anyhow::Result<bool> {
        if !self.is_planning_active() {
            return Ok(true);
        }
        self.plan_session.recovery.provider = self.provider_client.name().to_owned();
        self.plan_session.recovery.model = model.to_owned();
        if self.plan_session.recovery.guard_reason.is_none() {
            self.plan_session.recovery.guard_reason = self
                .harness_state
                .recovery_prompt_reason()
                .map(|reason| recovery_state::redact_bounded(reason, 2048));
        }
        let targeted_research = self.plan_session.recovery.targeted_research.take();
        if targeted_research.is_some()
            && self.plan_session.recovery.outcome == PlanningOutcome::Synthesizing
            && !self.harness_state.tool_budget_exhausted()
            && !self.harness_state.wall_clock_exhausted()
            && !self.plan_session.is_budget_exhausted()
        {
            self.plan_session.recovery.outcome = PlanningOutcome::Researching;
            self.harness_state.resume_after_navigation_synthesis();
            self.persist_planning_recovery().await?;
            self.push_system_message("The user introduced missing facts and explicitly requested one targeted read-only research batch for that clarification. Inspect only those facts. Then synthesize using the retained draft and evidence; do not restart broad research or implementation.");
            return Ok(true);
        }
        if self.harness_state.recovery_is_tool_free() || self.plan_session.recovery.research_closed {
            recovery_state::close_research(
                &mut self.plan_session.recovery,
                self.working_history,
                self.provider_client.name(),
                model,
            );
            if !recovery_state::admit_request(&mut self.plan_session.recovery) {
                return Ok(false);
            }
            // An explicit retry otherwise resends an identical request: name
            // the last classified failure so the correction carries new
            // information. First request of a retry cycle only; in-cycle
            // corrections already carry repair-specific directives.
            if self.plan_session.recovery.cycle > 0
                && self.plan_session.recovery.requests_used == 1
                && let Some(class) = self.plan_session.recovery.response_class
                && recovery_state::is_failure_response_class(class)
            {
                let repeated = recovery_state::repeated_identical_failure(&self.plan_session.recovery);
                self.push_system_message(recovery_state::planning_retry_correction_directive(class, repeated));
            }
            // Reestablish the tool-free boundary after automatic continuation or resume.
            self.harness_state.switch_to_tool_free_recovery();
            self.persist_planning_recovery().await?;
        }
        Ok(true)
    }

    pub(crate) async fn record_planning_response(&mut self, response: &uni::LLMResponse) -> anyhow::Result<()> {
        if !self.is_planning_active() || vtcode_core::core::agent::refusal::is_refusal(response) {
            return Ok(());
        }
        let previous_draft = self.plan_session.recovery.best_draft.clone();
        let previous_defects = self.plan_session.recovery.best_draft_defects;
        if let Some(text) = response.content.as_deref() {
            recovery_state::retain_draft(&mut self.plan_session.recovery, text);
        }
        let recording_recovery = self.plan_session.recovery.research_closed
            && self.plan_session.recovery.outcome != PlanningOutcome::Researching;
        if recording_recovery {
            recovery_state::record_response_class(
                &mut self.plan_session.recovery,
                recovery_state::response_class(response),
            );
        }
        if recording_recovery
            || self.plan_session.recovery.best_draft != previous_draft
            || self.plan_session.recovery.best_draft_defects != previous_defects
        {
            self.persist_planning_recovery().await?;
        }
        Ok(())
    }

    /// A failed response uses the same remaining admission allowance regardless of category.
    pub(crate) async fn recover_planning_failure(
        &mut self,
        detail: &str,
        draft: Option<&str>,
    ) -> anyhow::Result<TurnHandlerOutcome> {
        if let Some(draft) = draft {
            recovery_state::retain_draft(&mut self.plan_session.recovery, draft);
        }
        if self.plan_session.recovery_retry_allowed() {
            self.plan_session.queue_bounded_planning_follow_up();
            self.push_system_message("Planning correction: repair the retained draft against the saved validator report. Emit public final content only, without tool calls or approval prose.");
            self.harness_state.switch_to_tool_free_recovery();
            return Ok(TurnHandlerOutcome::Continue);
        }
        self.await_planning_recovery_input(detail).await
    }

    pub(crate) async fn await_planning_recovery_input(&mut self, detail: &str) -> anyhow::Result<TurnHandlerOutcome> {
        self.await_planning_recovery_input_with_fallback(detail, None).await
    }

    pub(crate) async fn await_planning_recovery_input_with_fallback(
        &mut self,
        detail: &str,
        fallback: Option<(&str, &str)>,
    ) -> anyhow::Result<TurnHandlerOutcome> {
        if self.is_planning_active() {
            recovery_state::close_research(
                &mut self.plan_session.recovery,
                self.working_history,
                self.provider_client.name(),
                &self.config.model,
            );
        }
        self.plan_session.recovery.outcome = PlanningOutcome::AwaitingInput;
        let already_shown = self.plan_session.recovery.input_prompt_shown;
        self.plan_session.recovery.input_prompt_shown = true;
        self.plan_session.clear_interview_pending();
        self.finish_recovery_pass();
        self.handle.set_activity_state(ActivityState::Idle);
        self.handle.set_input_status(None, self.input_status_state.right.clone());
        self.handle.force_redraw();
        // Persistence failure cannot authorize another provider request or approval.
        self.persist_planning_recovery().await?;
        // Consecutive identical failures need an honest handoff: a blind retry
        // has already been shown not to work, so lead with clarification and
        // model selection instead.
        let repeated = recovery_state::repeated_identical_failure(&self.plan_session.recovery);
        let retry_label = if repeated { RETRY_ANYWAY_LABEL } else { RETRY_LABEL };
        let snapshot = &self.plan_session.recovery;
        let diagnostic = redact_diagnostic(&Diagnostic {
            severity: DiagnosticSeverity::Warning,
            code: "planning.awaiting_input".into(),
            summary: "Planning needs your input. Saved progress is available.".into(),
            agent_details: serde_json::json!({
                "outcome": "awaiting_input", "category": snapshot.response_class,
                "provider": snapshot.provider, "model": snapshot.model,
                "requests_used": snapshot.requests_used, "remaining_allowance": 0,
                "cycle": snapshot.cycle, "failure": recovery_state::redact_bounded(detail, 2048),
                "guard_reason": snapshot.guard_reason,
                "validator_defects": snapshot.validation_report, "evidence_items": snapshot.evidence.len(),
                "retained_draft": snapshot.best_draft.is_some(),
                "repeated_identical_failure": repeated,
                "recovery": "Wait for explicit retry, clarification, or model selection. Research stays closed. Implementation requires separate validated persisted plan approval."
            }).to_string(),
            correlation_id: format!("{}-planning-{}", self.harness_state.run_id.0, snapshot.cycle),
            actions: vec![DiagnosticAction { id: "retry".into(), label: retry_label.into() },
                DiagnosticAction { id: "clarify".into(), label: CLARIFY_LABEL.into() },
                DiagnosticAction { id: "model".into(), label: MODEL_LABEL.into() }],
        });
        if !already_shown {
            self.renderer.diagnostic(&diagnostic)?;
        }
        if !already_shown
            && self.renderer.supports_inline_ui()
            && !self.plan_session.is_interview_denied()
            && crate::agent::runloop::unified::stop_requests::stop_outcome(self.ctrl_c_state).is_none()
        {
            let answer = crate::agent::runloop::unified::request_user_input::execute_request_user_input_tool(
                self.handle,
                self.session,
                &recovery_question(repeated),
                self.ctrl_c_state,
                self.ctrl_c_notify,
            )
            .await;
            match answer {
                Ok(value) => {
                    let answer = &value["answers"]["planning_recovery"];
                    let selected = answer["selected"]
                        .as_array()
                        .and_then(|items| items.first())
                        .and_then(serde_json::Value::as_str);
                    let mut clarification = answer["other"]
                        .as_str()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_owned);
                    let mut research_missing_facts = false;
                    if selected == Some(CLARIFY_LABEL) && clarification.is_none() {
                        let args = serde_json::json!({"questions": [
                            {"id":"clarification", "header":"Clarify", "question":"What missing facts or changes should the retained plan use?"},
                            {"id":"clarification_evidence", "header":"Evidence", "question":"Does this clarification introduce missing facts that need research?", "options":[
                                {"label":"Use saved evidence", "description":"Revise the draft using retained evidence and your clarification."},
                                {"label":"Research missing facts", "description":"Allow one targeted read-only research batch for newly missing facts before synthesis."}
                            ]}
                        ]});
                        let answer =
                            crate::agent::runloop::unified::request_user_input::execute_request_user_input_tool(
                                self.handle,
                                self.session,
                                &args,
                                self.ctrl_c_state,
                                self.ctrl_c_notify,
                            )
                            .await.unwrap_or_else(|error| {
                                tracing::warn!(%error, "clarification interview unavailable; retaining awaiting-input handoff");
                                serde_json::json!({"cancelled": true})
                            });
                        research_missing_facts = answer["answers"]["clarification_evidence"]["selected"]
                            .as_array()
                            .is_some_and(|choices| {
                                choices.iter().any(|choice| choice.as_str() == Some("Research missing facts"))
                            });
                        clarification = answer["answers"]["clarification"]["other"]
                            .as_str()
                            .map(str::trim)
                            .filter(|text| !text.is_empty())
                            .map(str::to_owned);
                    }
                    if selected == Some(MODEL_LABEL) {
                        self.plan_session.recovery.model_selection_pending = true;
                        self.persist_planning_recovery().await?;
                    } else if selected == Some(RETRY_LABEL)
                        || selected == Some(RETRY_ANYWAY_LABEL)
                        || clarification.is_some()
                    {
                        self.plan_session.begin_explicit_recovery_cycle();
                        if let Some(text) = clarification {
                            recovery_state::record_clarification(
                                &mut self.plan_session.recovery,
                                &text,
                                research_missing_facts,
                            );
                            *self.auto_finish_planning_attempted = true;
                            self.working_history
                                .push(uni::Message::user(recovery_state::redact_bounded(&text, 8192)));
                        }
                        self.persist_planning_recovery().await?;
                        self.plan_session.queue_bounded_planning_follow_up();
                        self.harness_state.switch_to_tool_free_recovery();
                        return Ok(TurnHandlerOutcome::Continue);
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "planning recovery input unavailable; retaining structured handoff")
                }
            }
        }
        self.harness_state.mark_final_response_fallback();
        // The blocked reason stays byte-identical: continuation gates match on
        // it. Only the human-facing message varies.
        let repeated_fallback = (
            "Planning needs your input. Saved progress is available. Consecutive attempts failed identically; retrying \
             unchanged is unlikely to help. Add clarification or use `/model` to switch models; implementation still \
             requires plan approval.",
            "planning awaiting input",
        );
        let standard_fallback = (
            "Planning needs your input. Saved progress is available. Choose retry, add clarification, or use `/model`; implementation still requires plan approval.",
            "planning awaiting input",
        );
        let (message, reason) = fallback.unwrap_or(if repeated { repeated_fallback } else { standard_fallback });
        self.handle_assistant_response(
            message.into(),
            Vec::new(),
            None,
            false,
            Some(uni::AssistantPhase::FinalAnswer),
        )?;
        Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason.into()) }))
    }
}
