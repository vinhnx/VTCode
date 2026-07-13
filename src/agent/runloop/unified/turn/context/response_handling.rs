use super::*;
use crate::agent::runloop::unified::ui_interaction_stream_helpers::render_compact_reasoning_block;

impl<'a> TurnProcessingContext<'a> {
    pub(crate) fn handle_assistant_response(
        &mut self,
        text: String,
        reasoning: Vec<ReasoningSegment>,
        reasoning_details: Option<Vec<String>>,
        response_streamed: bool,
        phase: Option<uni::AssistantPhase>,
    ) -> anyhow::Result<()> {
        let mut text = text;
        let detail_reasoning = reasoning_details.as_deref().and_then(
            vtcode_core::llm::providers::common::extract_reasoning_text_from_serialized_details,
        );
        if should_suppress_redundant_diff_recap(self.working_history, &text) {
            text.clear();
        }
        let has_visible_text = !text.trim().is_empty();
        if !reasoning.is_empty()
            || reasoning_details
                .as_ref()
                .is_some_and(|details| !details.is_empty())
        {
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
            let mut rendered_reasoning = detail_reasoning
                .is_some()
                .then(|| Vec::with_capacity(reasoning.len()));

            for segment in &reasoning {
                if let Some(stage) = &segment.stage {
                    self.handle.set_reasoning_stage(Some(stage.clone()));
                }

                let reasoning_text = &segment.text;
                if !reasoning_text.trim().is_empty() {
                    let duplicates_content =
                        has_visible_text && reasoning_duplicates_content(reasoning_text, &text);
                    if !duplicates_content {
                        let compact =
                            vtcode_commons::formatting::compact_reasoning_text(reasoning_text);
                        if compact.trim().is_empty() {
                            continue;
                        }
                        let rendered =
                            render_compact_reasoning_block(self.renderer, reasoning_text)?;
                        if rendered && let Some(rendered_reasoning) = rendered_reasoning.as_mut() {
                            rendered_reasoning.push(compact);
                        }
                    }
                }
            }

            if let Some(detail_text) = detail_reasoning.as_deref() {
                let cleaned_detail =
                    vtcode_commons::formatting::compact_reasoning_text(detail_text);
                let duplicates_content =
                    has_visible_text && reasoning_duplicates_content(&cleaned_detail, &text);
                let duplicates_rendered =
                    rendered_reasoning
                        .as_ref()
                        .is_some_and(|rendered_reasoning| {
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
            .is_some_and(|combined_reasoning| {
                !reasoning_duplicates_content(combined_reasoning, &text)
            });
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
        } else {
            evaluate_interim_text_continuation(
                self.full_auto,
                self.is_planning_active(),
                self.working_history,
                &text,
                consecutive_relaxed,
            )
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
        self.handle_assistant_response(
            text,
            reasoning,
            reasoning_details,
            response_streamed,
            assistant_phase,
        )?;

        // Count this text response so the recovery loop can short-circuit
        // when the model has already produced a final answer but the loop
        // keeps re-prompting. See `MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN`.
        self.harness_state.record_assistant_text_response();

        if recovery_pass_response {
            self.finish_recovery_pass();
        }

        tracing::info!(
            target: "vtcode.turn.metrics",
            metric = "text_response_decision",
            run_id = %self.harness_state.run_id.0,
            turn_id = %self.harness_state.turn_id.0,
            should_continue = continuation_decision.should_continue,
            reason = continuation_decision.reason,
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

        if continuation_decision.should_continue {
            push_system_directive_once(self.working_history, AUTONOMOUS_CONTINUE_DIRECTIVE);
            return Ok(TurnHandlerOutcome::Continue);
        }

        if let Some(hooks) = self.lifecycle_hooks {
            let outcome = hooks
                .run_stop(&final_text, self.harness_state.stop_hook_active)
                .await?;
            crate::agent::runloop::unified::turn::utils::render_hook_messages(
                self.renderer,
                &outcome.messages,
            )?;
            if let Some(reason) = outcome.block_reason {
                push_system_directive_once(self.working_history, &reason);
                self.harness_state.stop_hook_active = true;
                return Ok(TurnHandlerOutcome::Continue);
            }
        }
        self.harness_state.stop_hook_active = false;

        if self.is_planning_active()
            && let Some(plan_text) = proposed_plan
        {
            self.emit_plan_events(&plan_text).await;
            let persisted =
                persist_plan_draft(&self.tool_registry.planning_workflow_state(), &plan_text)
                    .await?;
            self.tool_registry
                .set_planning_phase(if persisted.validation.is_ready() {
                    PlanLifecyclePhase::DraftReady
                } else {
                    PlanLifecyclePhase::ActiveDrafting
                });
        }

        Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed))
    }

    async fn emit_plan_events(&self, plan_text: &str) {
        let Some(emitter) = self.harness_emitter else {
            return;
        };

        let turn_id = self.harness_state.turn_id.0.clone();
        let thread_id = self.harness_state.run_id.0.clone();
        let item_id = format!("{turn_id}-plan");

        let start_item = ThreadItem {
            id: item_id.clone(),
            details: ThreadItemDetails::Plan(PlanItem {
                text: String::new(),
            }),
        };
        let _ = emitter.emit(ThreadEvent::ItemStarted(ItemStartedEvent {
            item: start_item,
        }));

        let _ = emitter.emit(ThreadEvent::PlanDelta(PlanDeltaEvent {
            thread_id,
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            delta: plan_text.to_string(),
        }));

        let completed_item = ThreadItem {
            id: item_id,
            details: ThreadItemDetails::Plan(PlanItem {
                text: plan_text.to_string(),
            }),
        };
        let _ = emitter.emit(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: completed_item,
        }));
    }
}

// NOTE: Provider-noise stripping (MiniMax `]<]minimax[>[` and similar) has been
// centralized in `turn::provider_noise`. All call sites — textual tool parsers,
// response handling, and the live stream renderer — delegate to
// `strip_provider_noise` / `sanitize_recovery_answer` there. See that module
// for the canonical noise vocabulary and comprehensive tests.
