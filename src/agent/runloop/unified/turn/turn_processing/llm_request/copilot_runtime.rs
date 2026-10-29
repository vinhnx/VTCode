mod observed;
mod presentation;
mod streaming;
mod terminal;

use observed::ObservedToolCallState;
use terminal::LocalTerminalSession;

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::RwLock;
use vtcode_config::auth::CopilotAuthConfig;
use vtcode_config::core::permissions::AgentPermissionsConfig;
use vtcode_core::acp::{PermissionGrant, ToolPermissionCache};
use vtcode_core::copilot::{
    CopilotAcpCompatibilityState, CopilotObservedToolCall, CopilotObservedToolCallStatus, CopilotPermissionDecision,
    CopilotPermissionRequest, CopilotRuntimeRequest, CopilotToolCallFailure, CopilotToolCallRequest,
    CopilotToolCallResponse, CopilotToolCallSuccess, PromptSession,
};
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::exec::events::ToolCallStatus;
use vtcode_core::exec_policy::AskForApproval;
use vtcode_core::llm::provider as uni;
use vtcode_core::llm::provider::ToolDefinition;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::types::CompactStr;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::{InlineHandle, InlineSession};

use super::request_builder::COLLAPSED_TOOL_OUTPUT_NOTICE;
use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::tool_output::resolve_stdout_tail_limit;
use crate::agent::runloop::unified::async_mcp_manager::approval_policy_from_human_in_the_loop;
use crate::agent::runloop::unified::inline_events::harness::{
    HarnessEventEmitter, tool_invocation_completed_event, tool_output_completed_event, tool_output_started_event,
    tool_started_event, tool_updated_event,
};
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::run_loop_context::{
    HarnessTurnState, RunLoopContext, SESSION_LIMIT_GRANT_DIRECTIVE, full_auto_loop_grants_enabled,
};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_call_safety::{ToolCallSafetyValidator, invocation_id_from_call_id};
use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output;
use crate::agent::runloop::unified::tool_pipeline::{
    ToolExecutionStatus, run_tool_call_with_args,
    validation::{SafetyValidationFailure, validate_tool_call_with_limit_prompt},
};
use crate::agent::runloop::unified::tool_routing::{
    HitlDecision, PreToolHookPhaseResult, ToolPermissionFlow, ToolPermissionsContext,
    ensure_tool_permission_with_call_id, prompt_external_tool_permission,
};
use crate::agent::runloop::unified::turn::tool_outcomes::error_handling::tool_denial_diagnostic;
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
    LoopTracker, update_repetition_tracker_for_registry,
};
use crate::agent::runloop::unified::turn::tool_outcomes::{
    ToolFailureDiagnosis, bounded_diagnostic_field, bounded_error_evidence, bounded_output_evidence,
    deterministic_error_diagnosis, deterministic_output_diagnosis, escape_untrusted_evidence, render_diagnosis,
};
use crate::agent::runloop::unified::ui_interaction_stream::CopilotRuntimeRequestHandler;

pub(super) struct CopilotRuntimeHost<'a> {
    tool_registry: &'a mut ToolRegistry,
    tool_result_cache: &'a Arc<RwLock<vtcode_core::tools::ToolResultCache>>,
    session: &'a mut InlineSession,
    session_stats: &'a mut SessionStats,
    plan_session: &'a mut PlanningWorkflowSessionState,
    mcp_panel_state: &'a mut McpPanelState,
    handle: &'a InlineHandle,
    ctrl_c_state: &'a Arc<CtrlCState>,
    ctrl_c_notify: &'a Arc<tokio::sync::Notify>,
    default_placeholder: Option<String>,
    approval_recorder: &'a vtcode_core::tools::ApprovalRecorder,
    decision_ledger: &'a Arc<RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
    tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
    permissions_state: &'a Arc<RwLock<vtcode_core::config::PermissionsConfig>>,
    active_agent_permissions: Option<&'a AgentPermissionsConfig>,
    safety_validator: &'a Arc<ToolCallSafetyValidator>,
    lifecycle_hooks: Option<&'a vtcode_core::hooks::LifecycleHookEngine>,
    approval_policy: AskForApproval,
    hitl_notification_bell: bool,
    skip_confirmations: bool,
    full_auto: bool,
    vt_cfg: Option<&'a vtcode_config::loader::VTCodeConfig>,
    traj: &'a TrajectoryLogger,
    harness_state: &'a mut HarnessTurnState,
    loop_tracker: Option<&'a mut LoopTracker>,
    suppress_output_signal: Option<Arc<AtomicBool>>,
    exposed_tools: Vec<ToolDefinition>,
    exposed_tool_names: BTreeSet<String>,
    harness_emitter: Option<&'a HarnessEventEmitter>,
    harness_item_prefix: String,
    agent_name: Option<String>,
    observed_tool_calls: HashMap<String, ObservedToolCallState>,
    local_terminal_sessions: HashMap<String, LocalTerminalSession>,
    compatibility_notice_shown: bool,
    pending_hook_rewritten_args: HashMap<CompactStr, Value>,
}

impl<'a> CopilotRuntimeHost<'a> {
    #[expect(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    pub(super) fn new(
        tool_registry: &'a mut ToolRegistry,
        tool_result_cache: &'a Arc<RwLock<vtcode_core::tools::ToolResultCache>>,
        session: &'a mut InlineSession,
        session_stats: &'a mut SessionStats,
        plan_session: &'a mut PlanningWorkflowSessionState,
        mcp_panel_state: &'a mut McpPanelState,
        handle: &'a InlineHandle,
        ctrl_c_state: &'a Arc<CtrlCState>,
        ctrl_c_notify: &'a Arc<tokio::sync::Notify>,
        default_placeholder: Option<String>,
        approval_recorder: &'a vtcode_core::tools::ApprovalRecorder,
        decision_ledger: &'a Arc<RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
        tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
        permissions_state: &'a Arc<RwLock<vtcode_core::config::PermissionsConfig>>,
        active_agent_permissions: Option<&'a AgentPermissionsConfig>,
        safety_validator: &'a Arc<ToolCallSafetyValidator>,
        lifecycle_hooks: Option<&'a vtcode_core::hooks::LifecycleHookEngine>,
        vt_cfg: Option<&'a vtcode_config::loader::VTCodeConfig>,
        traj: &'a TrajectoryLogger,
        harness_state: &'a mut HarnessTurnState,
        loop_tracker: Option<&'a mut LoopTracker>,
        suppress_output_signal: Option<Arc<AtomicBool>>,
        available_tools: Option<&Arc<Vec<ToolDefinition>>>,
        skip_confirmations: bool,
        full_auto: bool,
        harness_emitter: Option<&'a HarnessEventEmitter>,
        harness_item_prefix: String,
        agent_name: Option<String>,
    ) -> Self {
        let allowlist = vt_cfg
            .map(|cfg| cfg.auth.copilot.vtcode_tool_allowlist.clone())
            .unwrap_or_else(|| CopilotAuthConfig::default().vtcode_tool_allowlist);
        let exposed_tools = filter_copilot_tools(available_tools, &allowlist);
        let exposed_tool_names = exposed_tools
            .iter()
            .filter_map(tool_definition_name)
            .map(str::to_string)
            .collect();

        let (approval_policy, hitl_bell) = vt_cfg
            .map(|cfg| {
                (
                    approval_policy_from_human_in_the_loop(cfg.security.human_in_the_loop),
                    cfg.security.hitl_notification_bell,
                )
            })
            .unwrap_or((AskForApproval::OnRequest, true));

        Self {
            tool_registry,
            tool_result_cache,
            session,
            session_stats,
            plan_session,
            mcp_panel_state,
            handle,
            ctrl_c_state,
            ctrl_c_notify,
            default_placeholder,
            approval_recorder,
            decision_ledger,
            tool_permission_cache,
            permissions_state,
            active_agent_permissions,
            safety_validator,
            lifecycle_hooks,
            approval_policy,
            hitl_notification_bell: hitl_bell,
            skip_confirmations,
            full_auto,
            vt_cfg,
            traj,
            harness_state,
            loop_tracker,
            suppress_output_signal,
            exposed_tools,
            exposed_tool_names,
            harness_emitter,
            harness_item_prefix,
            agent_name,
            observed_tool_calls: HashMap::new(),
            local_terminal_sessions: HashMap::new(),
            compatibility_notice_shown: false,
            pending_hook_rewritten_args: HashMap::new(),
        }
    }

    pub(super) fn exposed_tools(&self) -> &[ToolDefinition] {
        &self.exposed_tools
    }

    fn session_limit_auto_grant(&self) -> bool {
        full_auto_loop_grants_enabled(self.full_auto, self.vt_cfg)
    }

    async fn handle_builtin_permission(
        &mut self,
        renderer: &mut AnsiRenderer,
        request: CopilotPermissionRequest,
    ) -> Result<CopilotPermissionDecision> {
        let Some(summary) = summarize_permission_request(&request) else {
            return Ok(CopilotPermissionDecision::DeniedNoApprovalRule);
        };

        if let Some(decision) = self.cached_permission_decision(&summary.cache_key).await {
            return Ok(decision);
        }

        if let Some((permission_decision, cache_for_session)) = auto_approve_builtin_permission(&request) {
            if cache_for_session {
                let mut cache = self.tool_permission_cache.write().await;
                cache.cache_grant(summary.cache_key, PermissionGrant::Session);
            }
            return Ok(permission_decision);
        }

        if self.approval_policy.rejects_request_permission_prompt() {
            return Ok(CopilotPermissionDecision::DeniedNoApprovalRule);
        }

        let decision = prompt_external_tool_permission(
            renderer,
            self.handle,
            self.session,
            self.ctrl_c_state,
            self.ctrl_c_notify,
            self.default_placeholder.clone(),
            &summary.tool_name,
            summary.tool_args.as_ref(),
            &summary.display_name,
            &summary.cache_key,
            &summary.learning_label,
            summary.reason.as_deref(),
            Some(self.approval_recorder),
            self.hitl_notification_bell,
            None,
        )
        .await?;

        // Record the user's decision for auto-accept pattern learning.
        let approved = matches!(
            decision,
            HitlDecision::Approved | HitlDecision::ApprovedSession | HitlDecision::ApprovedPermanent
        );
        if let Err(err) = self
            .approval_recorder
            .record_approval(&summary.cache_key, Some(&summary.learning_label), approved, None)
            .await
        {
            tracing::debug!(
                approval_key = %summary.cache_key,
                approved,
                error = %err,
                "Failed to record builtin permission decision for pattern learning"
            );
        }

        let (permission_decision, cache_for_session) =
            map_builtin_permission_prompt_decision(decision, summary.reason.clone());
        if cache_for_session {
            let mut cache = self.tool_permission_cache.write().await;
            cache.cache_grant(summary.cache_key, PermissionGrant::Session);
        }
        Ok(permission_decision)
    }

    async fn cached_permission_decision(&self, cache_key: &str) -> Option<CopilotPermissionDecision> {
        let mut cache = self.tool_permission_cache.write().await;
        match cache.get_permission(cache_key) {
            Some(PermissionGrant::Session | PermissionGrant::Permanent) => {
                Some(CopilotPermissionDecision::ApprovedAlways)
            }
            Some(PermissionGrant::Denied) => Some(CopilotPermissionDecision::DeniedByRules),
            _ => None,
        }
    }

    async fn handle_vtcode_tool_call(
        &mut self,
        renderer: &mut AnsiRenderer,
        request: CopilotToolCallRequest,
    ) -> Result<CopilotToolCallResponse> {
        if !self.exposed_tool_names.contains(request.tool_name.as_str()) {
            return Ok(tool_not_exposed_response(&request.tool_name));
        }

        let prepared = self
            .tool_registry
            .admit_public_tool_call(&request.tool_name, &request.arguments)
            .with_context(|| format!("copilot tool preflight for '{}'", request.tool_name))?;
        let canonical_tool_name = prepared.canonical_name;
        let effective_arguments = prepared.effective_args;

        if !self.exposed_tool_names.contains(canonical_tool_name.as_str()) {
            return Ok(tool_not_exposed_response(&canonical_tool_name));
        }

        if let Some(response) = self
            .prepare_vtcode_tool_execution(renderer, &request.tool_call_id, &canonical_tool_name, &effective_arguments)
            .await?
        {
            return Ok(response);
        }

        let effective_arguments = self
            .pending_hook_rewritten_args
            .remove(request.tool_call_id.as_str())
            .unwrap_or(effective_arguments);

        self.record_tool_use(&canonical_tool_name);

        let tools = Arc::new(RwLock::new(self.exposed_tools.clone()));
        let turn_index = self.harness_state.tool_calls;
        let tool_item_id = harness_call_item_id(&self.harness_item_prefix, &request.tool_call_id, &canonical_tool_name);
        let (pipeline_outcome, last_stdout) = {
            let mut run_loop_ctx = RunLoopContext::new(
                renderer,
                self.handle,
                self.tool_registry,
                &tools,
                self.tool_result_cache,
                self.tool_permission_cache,
                self.permissions_state,
                self.decision_ledger,
                self.session_stats,
                self.plan_session,
                self.mcp_panel_state,
                self.approval_recorder,
                self.session,
                Some(self.safety_validator),
                self.traj,
                self.harness_state,
                self.harness_emitter,
            );

            let pipeline_outcome = run_tool_call_with_args(
                &mut run_loop_ctx,
                tool_item_id,
                &canonical_tool_name,
                &effective_arguments,
                self.ctrl_c_state,
                self.ctrl_c_notify,
                self.default_placeholder.clone(),
                self.lifecycle_hooks,
                self.skip_confirmations,
                self.vt_cfg,
                turn_index,
                true,
            )
            .await
            .with_context(|| format!("copilot tool execution for '{canonical_tool_name}'"))?;

            let (modified_files, last_stdout) = handle_pipeline_output(
                &mut run_loop_ctx,
                &canonical_tool_name,
                &effective_arguments,
                &pipeline_outcome,
                self.vt_cfg,
            )
            .await
            .with_context(|| format!("copilot tool output rendering for '{canonical_tool_name}'"))?;
            if !modified_files.is_empty() {
                run_loop_ctx
                    .session_stats
                    .record_touched_files(modified_files.iter().map(|path| path.display().to_string()));
            }

            if let Some(loop_tracker) = self.loop_tracker.as_deref_mut() {
                if update_repetition_tracker_for_registry(
                    loop_tracker,
                    &pipeline_outcome,
                    &canonical_tool_name,
                    &effective_arguments,
                    run_loop_ctx.tool_registry,
                ) {
                    // A failed verifier grants fix-up edits; reset the text
                    // streak so the diagnostic summary does not immediately
                    // trip the pending-verification text cap. Reset through
                    // the run-loop context, which owns the `&mut`
                    // `harness_state` borrow for this block.
                    run_loop_ctx.harness_state.reset_assistant_text_response_streak();
                }
                run_loop_ctx
                    .session_stats
                    .set_verification_snapshot(loop_tracker.verification_snapshot());
            }
            if let Some(signal) = self.suppress_output_signal.as_ref() {
                signal.store(run_loop_ctx.tool_registry.completion_verification_pending(), Ordering::Release);
            }

            (pipeline_outcome, last_stdout)
        };

        if pipeline_outcome.status.is_failure_like() {
            self.harness_state.record_failed_tool_call();
        }

        let mut response = match pipeline_outcome.status {
            ToolExecutionStatus::Success { output, command_success, .. } if command_success => {
                let text_result = last_stdout
                    .filter(|s: &String| !s.trim().is_empty())
                    .unwrap_or_else(|| serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string()));
                CopilotToolCallResponse::Success(CopilotToolCallSuccess {
                    text_result_for_llm: copilot_tool_result_text(&canonical_tool_name, text_result),
                })
            }
            ToolExecutionStatus::Success { output, .. } => {
                let diagnosis = deterministic_output_diagnosis(&canonical_tool_name, &effective_arguments, &output);
                render_diagnosis(
                    renderer,
                    self.harness_emitter,
                    &self.harness_state.turn_id.0,
                    Some(request.tool_call_id.as_str()),
                    &canonical_tool_name,
                    &diagnosis,
                );
                let evidence = bounded_output_evidence(&canonical_tool_name, &effective_arguments, &output);
                copilot_failure_response_with_diagnosis(
                    &canonical_tool_name,
                    &evidence,
                    &format!(
                        "tool '{}' returned a non-zero exit code; {}",
                        canonical_tool_name, diagnosis.likely_cause
                    ),
                    &diagnosis,
                )
            }
            ToolExecutionStatus::Failure { error } => {
                let diagnosis = deterministic_error_diagnosis(&error, "execution");
                render_diagnosis(
                    renderer,
                    self.harness_emitter,
                    &self.harness_state.turn_id.0,
                    Some(request.tool_call_id.as_str()),
                    &canonical_tool_name,
                    &diagnosis,
                );
                let evidence = bounded_error_evidence(&canonical_tool_name, &effective_arguments, &error, "execution");
                copilot_failure_response_with_diagnosis(
                    &canonical_tool_name,
                    &evidence,
                    &format!("tool '{canonical_tool_name}' failed: {}", error.message),
                    &diagnosis,
                )
            }
            ToolExecutionStatus::Timeout { error } => {
                let diagnosis = deterministic_error_diagnosis(&error, "timeout");
                render_diagnosis(
                    renderer,
                    self.harness_emitter,
                    &self.harness_state.turn_id.0,
                    Some(request.tool_call_id.as_str()),
                    &canonical_tool_name,
                    &diagnosis,
                );
                let evidence = bounded_error_evidence(&canonical_tool_name, &effective_arguments, &error, "timeout");
                copilot_failure_response_with_diagnosis(
                    &canonical_tool_name,
                    &evidence,
                    &format!("tool '{canonical_tool_name}' timed out: {}", error.message),
                    &diagnosis,
                )
            }
            ToolExecutionStatus::Cancelled => return Ok(tool_cancelled_response(&canonical_tool_name)),
        };
        // Native ACP replies bypass the ordinary history flush. Consume the
        // same notices here after the outcome; cancelled work returns above.
        if let Some(loop_tracker) = self.loop_tracker.as_deref_mut() {
            if let Some(guidance) = loop_tracker.take_redundant_navigation_guidance() {
                response = append_tool_reply_guidance(response, &guidance);
            }
            if let Some(guidance) = loop_tracker.take_verifier_failed_guidance() {
                response = append_tool_reply_guidance(response, &guidance);
            }
        }
        Ok(response)
    }

    async fn prepare_vtcode_tool_execution(
        &mut self,
        renderer: &mut AnsiRenderer,
        tool_call_id: &str,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<Option<CopilotToolCallResponse>> {
        // PreToolUse hooks run before the safety gateway so rewritten
        // arguments are what every downstream check evaluates, mirroring the
        // non-prevalidated pipeline path.
        let hook_phase = match crate::agent::runloop::unified::tool_routing::pipeline_pre_tool_hooks(
            self.lifecycle_hooks,
            renderer,
            tool_name,
            arguments,
            tool_call_id,
        )
        .await
        {
            Ok(phase) => phase,
            Err(err) => {
                return Ok(Some(denied_tool_response(tool_name, &format!("pre-tool hook phase failed: {err}"))));
            }
        };
        let rewritten_arguments = match &hook_phase {
            Some(PreToolHookPhaseResult::Deny) => {
                return Ok(Some(denied_tool_response(tool_name, "Tool permission denied")));
            }
            Some(PreToolHookPhaseResult::Proceed { rewritten_args, .. }) => rewritten_args.clone(),
            None => None,
        };

        // Re-validate rewritten arguments against the tool schema: preflight
        // ran on the original payload, so a hook rewrite could otherwise
        // bypass schema checks.
        if let Some(rewritten) = rewritten_arguments.as_ref()
            && let Err(err) = self.tool_registry.preflight_validate_harness_call(tool_name, rewritten)
        {
            return Ok(Some(denied_tool_response(
                tool_name,
                &format!("PreToolUse hook produced invalid arguments: {err}"),
            )));
        }

        let invocation_id = invocation_id_from_call_id(tool_call_id);
        let safety_args = rewritten_arguments.as_ref().unwrap_or(arguments);
        let session_limit_auto_grant = self.session_limit_auto_grant();
        let safety_approval_justification = match validate_tool_call_with_limit_prompt(
            self.safety_validator,
            self.handle,
            self.session,
            self.ctrl_c_state,
            self.ctrl_c_notify,
            tool_name,
            safety_args,
            invocation_id,
            Some(self.harness_state),
            self.harness_emitter,
            self.agent_name.as_deref(),
            session_limit_auto_grant,
            self.traj,
            self.tool_registry.is_planning_active(),
        )
        .await
        {
            Ok(()) => None,
            Err(SafetyValidationFailure::SessionLimitNotIncreased) => {
                return Ok(Some(denied_tool_response(
                    tool_name,
                    "session tool limit reached and not increased by user",
                )));
            }
            Err(SafetyValidationFailure::SessionLimitPromptFailed(error)) => {
                return Ok(Some(denied_tool_response(
                    tool_name,
                    &format!("failed while requesting a session tool-limit increase: {error}"),
                )));
            }
            Err(SafetyValidationFailure::NeedsApproval(justification)) => Some(justification),
            Err(SafetyValidationFailure::Validation(error)) => {
                return Ok(Some(denied_tool_response(tool_name, &format!("safety validation failed: {error}"))));
            }
        };

        match ensure_tool_permission_with_call_id(
            self.tool_permissions_context_with_safety(renderer, safety_approval_justification.as_deref()),
            tool_name,
            Some(rewritten_arguments.as_ref().unwrap_or(arguments)),
            Some(tool_call_id),
            hook_phase,
        )
        .await?
        {
            ToolPermissionFlow::Approved { updated_args } => {
                let final_args = updated_args.or_else(|| rewritten_arguments.clone());
                if let Some(updated) = final_args {
                    // A PermissionRequest hook may supply its own rewrite via
                    // `updated_input`; validate the schema and re-run the
                    // safety gateway against the final arguments so the
                    // replacement does not execute under decisions made for
                    // earlier arguments.
                    if let Err(err) = self.tool_registry.preflight_validate_harness_call(tool_name, &updated) {
                        return Ok(Some(denied_tool_response(
                            tool_name,
                            &format!("PermissionRequest hook produced invalid arguments: {err}"),
                        )));
                    }
                    if safety_args != &updated {
                        let invocation_id = invocation_id_from_call_id(tool_call_id);
                        match validate_tool_call_with_limit_prompt(
                            self.safety_validator,
                            self.handle,
                            self.session,
                            self.ctrl_c_state,
                            self.ctrl_c_notify,
                            tool_name,
                            &updated,
                            invocation_id,
                            Some(self.harness_state),
                            self.harness_emitter,
                            self.agent_name.as_deref(),
                            session_limit_auto_grant,
                            self.traj,
                            self.tool_registry.is_planning_active(),
                        )
                        .await
                        {
                            Ok(()) => {}
                            Err(SafetyValidationFailure::SessionLimitNotIncreased) => {
                                return Ok(Some(denied_tool_response(
                                    tool_name,
                                    "session tool limit reached and not increased by user",
                                )));
                            }
                            Err(SafetyValidationFailure::SessionLimitPromptFailed(error)) => {
                                return Ok(Some(denied_tool_response(
                                    tool_name,
                                    &format!("failed while requesting a session tool-limit increase: {error}"),
                                )));
                            }
                            Err(SafetyValidationFailure::NeedsApproval(_)) => {
                                // The user already approved the final arguments
                                // in the PermissionRequest prompt; the gateway's
                                // NeedsApproval for the rewritten command is
                                // subsumed by that human approval.
                            }
                            Err(SafetyValidationFailure::Validation(error)) => {
                                return Ok(Some(denied_tool_response(
                                    tool_name,
                                    &format!("safety validation failed: {error}"),
                                )));
                            }
                        }
                    }
                    self.pending_hook_rewritten_args.insert(CompactStr::from(tool_call_id), updated);
                }
            }
            ToolPermissionFlow::Denied => {
                let diagnostic = tool_denial_diagnostic(tool_name);
                let text = if let Some(diag) = diagnostic.as_ref() {
                    let impact = diag["impact"].as_str().unwrap_or("");
                    let fix = diag["fix"]["action"].as_str().unwrap_or("");
                    format!("VT Code denied the tool `{tool_name}`. {impact} {fix}")
                } else {
                    format!("VT Code denied the tool `{tool_name}`.")
                };
                return Ok(Some(CopilotToolCallResponse::Failure(CopilotToolCallFailure {
                    text_result_for_llm: text,
                    error: format!("tool '{tool_name}' denied by user or policy"),
                })));
            }
            ToolPermissionFlow::Blocked { reason } => {
                return Ok(Some(denied_tool_response(tool_name, &reason)));
            }
            ToolPermissionFlow::Exit | ToolPermissionFlow::Interrupted => {
                return Ok(Some(denied_tool_response(tool_name, "permission request interrupted")));
            }
        }

        // Control-plane exec calls (wait/inspect) bypass the per-turn budget
        // exhaustion rejection so a long build stays observable.
        if !vtcode_core::tools::tool_intent::is_turn_budget_exempt_call(tool_name, safety_args)
            && let Some(exhaustion) = self.harness_state.tool_budget_exhaustion()
        {
            // Nothing will execute for this call id; drop the pending
            // rewrite so a later retry with the same id cannot inherit stale
            // arguments.
            self.pending_hook_rewritten_args.remove(tool_call_id);
            self.harness_state.record_tool_budget_rejection();
            return Ok(Some(tool_exceeded_budget_response(tool_name, exhaustion.max)));
        }

        self.harness_state.record_admitted_tool_call();
        // Control-plane exec calls (wait/inspect) do not consume the per-turn
        // tool-call budget; see `record_tool_call_budget_usage` for rationale.
        // The exemption is judged on `safety_args` — the same arguments the
        // safety gateway and permission flow evaluated — so a hook rewrite
        // that changes the action cannot swap the budget treatment after
        // admission.
        if !vtcode_core::tools::tool_intent::is_turn_budget_exempt_call(tool_name, safety_args)
            && let Some(warning) = self.harness_state.record_tool_call_with_default_warning()
        {
            warning.log_threshold_reached("Tool-call budget warning threshold reached in copilot ACP path");
        }

        Ok(None)
    }

    fn tool_permissions_context_with_safety<'b>(
        &'b mut self,
        renderer: &'b mut AnsiRenderer,
        safety_approval_justification: Option<&str>,
    ) -> ToolPermissionsContext<'b, InlineSession> {
        ToolPermissionsContext {
            tool_registry: self.tool_registry,
            renderer,
            handle: self.handle,
            session: self.session,
            active_thread_label: None,
            default_placeholder: self.default_placeholder.clone(),
            ctrl_c_state: self.ctrl_c_state,
            ctrl_c_notify: self.ctrl_c_notify,
            hooks: self.lifecycle_hooks,
            justification: None,
            approval_recorder: Some(self.approval_recorder),
            decision_ledger: Some(self.decision_ledger),
            tool_permission_cache: Some(self.tool_permission_cache),
            permissions_state: Some(self.permissions_state),
            active_agent_permissions: self.active_agent_permissions,
            hitl_notification_bell: self.hitl_notification_bell,
            approval_policy: self.approval_policy,
            skip_confirmations: self.skip_confirmations,
            permissions_config: self.vt_cfg.map(|cfg| &cfg.permissions),
            auto_permission_runtime: None,
            session_stats: Some(self.session_stats),
            safety_approval_justification: safety_approval_justification.map(String::from),
            harness_emitter: self.harness_emitter,
        }
    }

    fn record_tool_use(&mut self, tool_name: &str) {
        // Copilot executes VTCode tools inside the streaming request, so its
        // progress boundary is absent from `working_history`. Record it in the
        // authoritative turn state only after admission; denied calls never
        // reach this method and cannot erase the response streak.
        self.harness_state.record_out_of_band_tool_progress();
        self.session_stats.record_tool(tool_name);
    }

    fn record_out_of_band_tool_use(&mut self, tool_name: &str) {
        self.harness_state.record_out_of_band_tool_call();
        self.session_stats.record_tool(tool_name);
    }

    fn emit_tool_started_event(&self, tool_call_id: &str, tool_name: &str, arguments: &Value) {
        let Some(emitter) = self.harness_emitter else {
            return;
        };
        let item_id = harness_call_item_id(&self.harness_item_prefix, tool_call_id, tool_name);
        let raw_id = raw_tool_call_id(tool_call_id);
        let _ = emitter.emit(tool_started_event(item_id.clone(), tool_name, Some(arguments), raw_id));
        let _ = emitter.emit(tool_output_started_event(item_id, raw_id));
    }

    fn emit_tool_finished_event(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        arguments: &Value,
        status: ToolCallStatus,
        output: Option<String>,
    ) {
        let Some(emitter) = self.harness_emitter else {
            return;
        };
        let item_id = harness_call_item_id(&self.harness_item_prefix, tool_call_id, tool_name);
        let raw_id = raw_tool_call_id(tool_call_id);
        let _ = emitter.emit(tool_invocation_completed_event(
            item_id.clone(),
            tool_name,
            Some(arguments),
            raw_id,
            status.clone(),
        ));
        let _ =
            emitter.emit(tool_output_completed_event(item_id, raw_id, status, None, None, output.unwrap_or_default()));
    }

    fn emit_tool_output_event(&self, tool_call_id: &str, tool_name: &str, output: &str) {
        let Some(emitter) = self.harness_emitter else {
            return;
        };
        let item_id = harness_call_item_id(&self.harness_item_prefix, tool_call_id, tool_name);
        let _ = emitter.emit(tool_updated_event(item_id, raw_tool_call_id(tool_call_id), output));
    }

    fn handle_observed_tool_call(&mut self, update: CopilotObservedToolCall) {
        if let Some(terminal_id) = update.terminal_id.as_deref()
            && let Some(session) = self.local_terminal_sessions.get(terminal_id)
        {
            let bind_result = session.bind_observed_tool_call(&update);
            if bind_result.emit_started {
                self.record_out_of_band_tool_use(&bind_result.association.tool_name);
                self.emit_tool_started_event(
                    &bind_result.association.tool_call_id,
                    &bind_result.association.tool_name,
                    &bind_result.association.arguments,
                );
            }
            if let Some(output) = bind_result.buffered_output.as_deref() {
                self.emit_tool_output_event(
                    &bind_result.association.tool_call_id,
                    &bind_result.association.tool_name,
                    output,
                );
            }
            if let Some(status) = bind_result.finish_status {
                if matches!(status, ToolCallStatus::Failed) {
                    self.harness_state.record_failed_tool_call();
                }
                self.emit_tool_finished_event(
                    &bind_result.association.tool_call_id,
                    &bind_result.association.tool_name,
                    &bind_result.association.arguments,
                    status,
                    bind_result.buffered_output,
                );
            }
            return;
        }

        let tool_call_id = update.tool_call_id.clone();
        let tail_limit = resolve_stdout_tail_limit(self.vt_cfg);

        let tool_update = {
            let state = self
                .observed_tool_calls
                .entry(tool_call_id.clone())
                .or_insert_with(|| ObservedToolCallState::new(update.tool_name.clone()));
            state.apply(&update, tail_limit, self.handle, self.tool_registry.pty_config())
        };

        if tool_update.started {
            let tool_name = self.observed_tool_calls[&tool_call_id].tool_name().to_string();
            self.record_out_of_band_tool_use(&tool_name);
            self.emit_tool_started_event(&tool_call_id, &tool_name, update.arguments.as_ref().unwrap_or(&Value::Null));
        }

        if let Some(output) = tool_update.output_snapshot {
            let tool_name = self.observed_tool_calls[&tool_call_id].tool_name().to_string();
            self.emit_tool_output_event(&tool_call_id, &tool_name, &output);
        }

        if tool_update.finished {
            let state = &self.observed_tool_calls[&tool_call_id];
            let status = match update.status {
                CopilotObservedToolCallStatus::Completed => ToolCallStatus::Completed,
                CopilotObservedToolCallStatus::Failed => ToolCallStatus::Failed,
                _ => ToolCallStatus::InProgress,
            };
            if matches!(status, ToolCallStatus::Failed) {
                self.harness_state.record_failed_tool_call();
            }
            self.emit_tool_finished_event(
                &tool_call_id,
                state.tool_name(),
                update.arguments.as_ref().unwrap_or(&Value::Null),
                status,
                update.output,
            );
        }
    }

    fn handle_compatibility_notice(
        &mut self,
        renderer: &mut AnsiRenderer,
        state: CopilotAcpCompatibilityState,
        message: String,
    ) -> Result<()> {
        if self.compatibility_notice_shown {
            return Ok(());
        }
        self.compatibility_notice_shown = true;
        tracing::warn!(
            target: "copilot.acp",
            ?state,
            message = %message,
            "GitHub Copilot ACP compatibility changed"
        );
        crate::agent::runloop::unified::turn::turn_helpers::display_status(renderer, &message)?;
        Ok(())
    }
}

impl Drop for CopilotRuntimeHost<'_> {
    fn drop(&mut self) {
        for (_, session) in self.local_terminal_sessions.drain() {
            session.abort();
        }
    }
}

pub(super) fn prompt_session_to_stream(
    model: String,
    prompt_session: PromptSession,
) -> (uni::LLMStream, tokio::sync::mpsc::UnboundedReceiver<CopilotRuntimeRequest>) {
    streaming::prompt_session_to_stream(model, prompt_session)
}

mod handler;
mod responses;

use responses::{
    append_session_limit_grant_guidance, append_tool_reply_guidance, auto_approve_builtin_permission,
    copilot_failure_response_with_diagnosis, copilot_tool_result_text, denied_tool_response,
    emit_terminal_finished_event, emit_terminal_output_event, filter_copilot_tools, harness_call_item_id,
    map_builtin_permission_prompt_decision, map_runtime_error, raw_tool_call_id, summarize_permission_request,
    tool_cancelled_response, tool_definition_name, tool_exceeded_budget_response, tool_not_exposed_response,
};

#[cfg(test)]
#[path = "copilot_runtime_tests.rs"]
mod tests;
