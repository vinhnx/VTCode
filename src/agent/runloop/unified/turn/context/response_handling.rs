use super::*;

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
                        let cleaned_for_display =
                            vtcode_core::llm::providers::clean_reasoning_text(reasoning_text);
                        if cleaned_for_display.trim().is_empty() {
                            continue;
                        }
                        self.renderer
                            .line(MessageStyle::Reasoning, &cleaned_for_display)?;
                        if let Some(rendered_reasoning) = rendered_reasoning.as_mut() {
                            rendered_reasoning.push(cleaned_for_display);
                        }
                    }
                }
            }

            if let Some(detail_text) = detail_reasoning.as_deref() {
                let cleaned_detail = vtcode_core::llm::providers::clean_reasoning_text(detail_text);
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
                    self.renderer
                        .line(MessageStyle::Reasoning, &cleaned_detail)?;
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
        let final_text = text.clone();
        let continuation_decision = if tool_free_recovery_pass {
            InterimTextContinuationDecision {
                should_continue: false,
                reason: "recovery_pass",
                is_interim_progress: false,
                last_user_follow_up: false,
                recent_tool_activity: false,
                last_user_requested_progressive_work: false,
            }
        } else {
            evaluate_interim_text_continuation(
                self.full_auto,
                self.is_planning_active(),
                self.working_history,
                &text,
            )
        };
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
            self.tool_registry.planning_workflow_state().set_phase(
                if persisted.validation.is_ready() {
                    PlanLifecyclePhase::DraftReady
                } else {
                    PlanLifecyclePhase::ActiveDrafting
                },
            );
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
