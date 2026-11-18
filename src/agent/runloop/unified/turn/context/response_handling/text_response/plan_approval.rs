use super::*;

impl TurnProcessingContext<'_> {
    /// Persist and route a completed plan through the approval handoff.
    /// Every branch returns a terminal outcome for the enclosing turn.
    pub(crate) async fn handle_plan_approval_handoff(
        &mut self,
        plan_text: String,
        tool_free_recovery_pass: bool,
    ) -> anyhow::Result<TurnHandlerOutcome> {
        use vtcode_tui_runtime::utils::ansi::MessageStyle;

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
                self.handle
                    .set_input_status(Some("Preparing approval...".to_string()), self.input_status_state.right.clone());
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
        let require_confirmation =
            explicit_approval_required || self.vt_cfg.map(|cfg| cfg.agent.require_plan_confirmation).unwrap_or(true);
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
                            &resolve_effective_request_model(&self.config.model, self.active_primary_agent.active()),
                        ),
                    ),
                    auto_agent_active: self
                        .active_primary_agent
                        .active()
                        .name()
                        .eq_ignore_ascii_case(vtcode_config::builtin_primary_auto_agent().name.as_str()),
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
            return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false }));
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
        Ok(TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy { target })
    }
}
