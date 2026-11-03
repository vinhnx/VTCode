use super::*;

impl<'a> TurnProcessingContext<'a> {
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
        let navigation_summary_valid =
            !crate::agent::runloop::unified::turn::provider_noise::strip_provider_noise(&text)
                .trim()
                .is_empty();
        // Tool-free recovery normally ends the turn; navigation has one bounded
        // continuation below. Either path needs meaningful synthesis text.
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
            if proposed_plan.is_none() && looks_like_clarifying_question(&text) && !looks_like_attempted_plan(&text) {
                self.plan_session.recovery.outcome =
                    vtcode_core::exec::events::planning::PlanningOutcome::AwaitingInput;
                self.plan_session.recovery.input_prompt_shown = true;
                self.persist_planning_recovery().await?;
                self.handle_assistant_response(
                    text,
                    Vec::new(),
                    None,
                    response_streamed,
                    Some(uni::AssistantPhase::FinalAnswer),
                )?;
                self.finish_recovery_pass();
                return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed {
                    plan_approved_execution_pending: false,
                }));
            }
            if proposed_plan.is_none() {
                if text.trim().is_empty() {
                    return self
                        .break_planning_recovery_with_handoff(
                            "the response did not contain exactly one completed <proposed_plan> block",
                            None,
                        )
                        .await;
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
        let navigation_resume_allowed = tool_free_recovery_pass
            && navigation_summary_valid
            && self.session_stats.navigation_recovery
                == crate::agent::runloop::unified::state::NavigationRecovery::Synthesizing
            && !self.is_planning_active()
            && proposed_plan.is_none()
            && !self.harness_state.wall_clock_exhausted()
            && !self.harness_state.wall_clock_exhausted_emitted
            && !self.harness_state.tool_budget_exhausted()
            && !self.harness_state.tool_budget_exhausted_emitted
            && !self.harness_state.turn_refused()
            && self.session_stats.preflight_recovery.is_clear()
            && crate::agent::runloop::unified::stop_requests::stop_outcome(self.ctrl_c_state).is_none()
            && crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_auto_continue_enabled(self.vt_cfg);
        let continuation_decision = if tool_free_recovery_pass && !navigation_resume_allowed {
            // Non-navigation recovery remains terminal. Only typed navigation
            // synthesis can spend the request's single execution allowance.
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
        // into unrelated workspace tracker work. Require successful tracker
        // adoption or an explicit follow-up in this session before
        // the workspace tracker can force continuation. Restoration uses the
        // same adoption boundary.
        //
        // Only navigation synthesis may use this override to resume in-turn.
        // Completed plans and all other tool-free recoveries stay terminal.
        // In-turn override is gated on `auto_continue_tracker` only —
        // `cross_turn_turns == 0` disables only the outer auto-queue.
        // Complete probes clear the cache so auto-continue stops when the
        // tracker finishes; Unavailable keeps the last known incomplete set.
        let tracker_adoption_allowed = self.tool_registry.tracker_adopted_for_request();
        let live_probe = if !continuation_decision.should_continue
            && (!tool_free_recovery_pass || navigation_resume_allowed)
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
            && (!tool_free_recovery_pass || navigation_resume_allowed)
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

        if navigation_resume_allowed && continuation_decision.should_continue {
            self.session_stats.navigation_recovery.consume();
            self.harness_state.resume_after_navigation_synthesis();
            self.push_system_message("Navigation synthesis is complete. Tools are available under current policy for one bounded execution attempt. Reuse retained evidence; take the next concrete action and verify it. No tool, loop, approval, or verification limit is increased.".to_string());
        }

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

        if recovery_pass_response && !(navigation_resume_allowed && continuation_decision.should_continue) {
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
                return self.reject_plan_artifact(error, &final_text, false).await;
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
                return self.reject_plan_artifact(error, &plan_text, !tool_free_recovery_pass).await;
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
                if plan_state.get_plan_file().is_none()
                    && let Err(error) = allocate_plan_file_if_missing(&plan_state).await
                {
                    let error = PlanArtifactError::Persistence { reason: error.to_string() };
                    return self.reject_plan_artifact(error, &plan_text, false).await;
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
                    return self.reject_plan_artifact(error, &plan_text, false).await;
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
                return self.reject_plan_artifact(error, &plan_text, false).await;
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
            .await?;

            let explicit_approval_required = planning_active
                && (self.plan_session.recovery.research_closed || self.plan_session.requires_explicit_plan_approval());
            let require_confirmation = explicit_approval_required
                || self.vt_cfg.map(|cfg| cfg.agent.require_plan_confirmation).unwrap_or(true);
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
                plan_approval_route(
                    require_confirmation,
                    supports_inline,
                    self.skip_confirmations && !explicit_approval_required,
                    self.full_auto && !explicit_approval_required,
                )
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
                        skip_confirmations: self.skip_confirmations && !explicit_approval_required,
                        full_auto: self.full_auto && !explicit_approval_required,
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
