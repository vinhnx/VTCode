use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use async_stream::stream;
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::RwLock;
use vtcode_config::auth::CopilotAuthConfig;
use vtcode_core::acp::{PermissionGrant, ToolPermissionCache};
use vtcode_core::copilot::{
    CopilotAcpCompatibilityState, CopilotObservedToolCall, CopilotObservedToolCallStatus,
    CopilotPermissionDecision, CopilotPermissionRequest, CopilotRuntimeRequest,
    CopilotToolCallFailure, CopilotToolCallRequest, CopilotToolCallResponse,
    CopilotToolCallSuccess, PromptSession, PromptSessionCancelHandle, PromptUpdate,
};
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::exec::events::ToolCallStatus;
use vtcode_core::exec_policy::AskForApproval;
use vtcode_core::llm::provider::{self as uni, LLMStreamEvent, LLMStreamEvent::Completed};
use vtcode_core::llm::provider::{LLMResponse, ToolDefinition};
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::app::{InlineHandle, InlineSession};

use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::async_mcp_manager::approval_policy_from_human_in_the_loop;
use crate::agent::runloop::unified::inline_events::harness::{
    HarnessEventEmitter, tool_invocation_completed_event, tool_output_completed_event,
    tool_output_started_event, tool_started_event,
};
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, RunLoopContext};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output;
use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, run_tool_call_with_args};
use crate::agent::runloop::unified::tool_routing::{
    HitlDecision, ToolPermissionFlow, ToolPermissionsContext, ensure_tool_permission,
    prompt_external_tool_permission,
};
use crate::agent::runloop::unified::ui_interaction_stream::CopilotRuntimeRequestHandler;

pub(super) struct CopilotRuntimeHost<'a> {
    tool_registry: &'a mut ToolRegistry,
    tool_result_cache: &'a Arc<RwLock<vtcode_core::tools::ToolResultCache>>,
    session: &'a mut InlineSession,
    session_stats: &'a mut SessionStats,
    mcp_panel_state: &'a mut McpPanelState,
    handle: &'a InlineHandle,
    ctrl_c_state: &'a Arc<CtrlCState>,
    ctrl_c_notify: &'a Arc<tokio::sync::Notify>,
    default_placeholder: Option<String>,
    approval_recorder: &'a vtcode_core::tools::ApprovalRecorder,
    decision_ledger: &'a Arc<RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
    tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
    safety_validator: &'a Arc<RwLock<ToolCallSafetyValidator>>,
    lifecycle_hooks: Option<&'a vtcode_core::hooks::LifecycleHookEngine>,
    approval_policy: AskForApproval,
    hitl_notification_bell: bool,
    skip_confirmations: bool,
    vt_cfg: Option<&'a vtcode_config::loader::VTCodeConfig>,
    traj: &'a TrajectoryLogger,
    harness_state: &'a mut HarnessTurnState,
    exposed_tools: Vec<ToolDefinition>,
    exposed_tool_names: BTreeSet<String>,
    harness_emitter: Option<&'a HarnessEventEmitter>,
    harness_item_prefix: String,
    observed_tool_calls: HashMap<String, ObservedToolCallState>,
    compatibility_notice_shown: bool,
}

impl<'a> CopilotRuntimeHost<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        tool_registry: &'a mut ToolRegistry,
        tool_result_cache: &'a Arc<RwLock<vtcode_core::tools::ToolResultCache>>,
        session: &'a mut InlineSession,
        session_stats: &'a mut SessionStats,
        mcp_panel_state: &'a mut McpPanelState,
        handle: &'a InlineHandle,
        ctrl_c_state: &'a Arc<CtrlCState>,
        ctrl_c_notify: &'a Arc<tokio::sync::Notify>,
        default_placeholder: Option<String>,
        approval_recorder: &'a vtcode_core::tools::ApprovalRecorder,
        decision_ledger: &'a Arc<RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
        tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
        safety_validator: &'a Arc<RwLock<ToolCallSafetyValidator>>,
        lifecycle_hooks: Option<&'a vtcode_core::hooks::LifecycleHookEngine>,
        vt_cfg: Option<&'a vtcode_config::loader::VTCodeConfig>,
        traj: &'a TrajectoryLogger,
        harness_state: &'a mut HarnessTurnState,
        available_tools: Option<&Arc<Vec<ToolDefinition>>>,
        skip_confirmations: bool,
        harness_emitter: Option<&'a HarnessEventEmitter>,
        harness_item_prefix: String,
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

        Self {
            tool_registry,
            tool_result_cache,
            session,
            session_stats,
            mcp_panel_state,
            handle,
            ctrl_c_state,
            ctrl_c_notify,
            default_placeholder,
            approval_recorder,
            decision_ledger,
            tool_permission_cache,
            safety_validator,
            lifecycle_hooks,
            approval_policy: vt_cfg
                .map(|cfg| approval_policy_from_human_in_the_loop(cfg.security.human_in_the_loop))
                .unwrap_or(AskForApproval::OnRequest),
            hitl_notification_bell: vt_cfg
                .map(|cfg| cfg.security.hitl_notification_bell)
                .unwrap_or(true),
            skip_confirmations,
            vt_cfg,
            traj,
            harness_state,
            exposed_tools,
            exposed_tool_names,
            harness_emitter,
            harness_item_prefix,
            observed_tool_calls: HashMap::new(),
            compatibility_notice_shown: false,
        }
    }

    pub(super) fn exposed_tools(&self) -> &[ToolDefinition] {
        &self.exposed_tools
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

        if let Some((permission_decision, cache_for_session)) =
            auto_approve_builtin_permission(&request)
        {
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
        )
        .await?;

        let (permission_decision, cache_for_session) =
            map_builtin_permission_prompt_decision(decision, summary.reason.clone());
        if cache_for_session {
            let mut cache = self.tool_permission_cache.write().await;
            cache.cache_grant(summary.cache_key, PermissionGrant::Session);
        }
        Ok(permission_decision)
    }

    async fn cached_permission_decision(
        &self,
        cache_key: &str,
    ) -> Option<CopilotPermissionDecision> {
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
            return Ok(CopilotToolCallResponse::Failure(CopilotToolCallFailure {
                text_result_for_llm: format!(
                    "VT Code does not expose the client tool `{}` to GitHub Copilot.",
                    request.tool_name
                ),
                error: format!("tool '{}' is not allowlisted in VT Code", request.tool_name),
            }));
        }

        let preflight = self
            .tool_registry
            .preflight_validate_call(&request.tool_name, &request.arguments)
            .with_context(|| format!("copilot tool preflight for '{}'", request.tool_name))?;
        let canonical_tool_name = preflight.normalized_tool_name;

        if !self
            .exposed_tool_names
            .contains(canonical_tool_name.as_str())
        {
            return Ok(CopilotToolCallResponse::Failure(CopilotToolCallFailure {
                text_result_for_llm: format!(
                    "VT Code does not expose the client tool `{}` to GitHub Copilot.",
                    canonical_tool_name
                ),
                error: format!(
                    "tool '{}' is not allowlisted in VT Code",
                    canonical_tool_name
                ),
            }));
        }

        if let Some(response) = self
            .prepare_vtcode_tool_execution(renderer, &canonical_tool_name, &request.arguments)
            .await?
        {
            return Ok(response);
        }

        // Record tool use AFTER permission is granted (prepare_vtcode_tool_execution returned None).
        self.record_tool_use(&canonical_tool_name);

        let tools = Arc::new(RwLock::new(self.exposed_tools.clone()));
        let turn_index = self.harness_state.tool_calls;
        let tool_item_id = harness_call_item_id(
            &self.harness_item_prefix,
            &request.tool_call_id,
            &canonical_tool_name,
        );
        let (pipeline_outcome, last_stdout) = {
            let mut run_loop_ctx = RunLoopContext::new(
                renderer,
                self.handle,
                self.tool_registry,
                &tools,
                self.tool_result_cache,
                self.tool_permission_cache,
                self.decision_ledger,
                self.session_stats,
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
                &request.arguments,
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
                &request.arguments,
                &pipeline_outcome,
                self.vt_cfg,
            )
            .await
            .with_context(|| {
                format!("copilot tool output rendering for '{canonical_tool_name}'")
            })?;
            if !modified_files.is_empty() {
                run_loop_ctx.session_stats.record_touched_files(
                    modified_files.iter().map(|path| path.display().to_string()),
                );
            }

            (pipeline_outcome, last_stdout)
        };

        match pipeline_outcome.status {
            ToolExecutionStatus::Success { output, .. } => {
                // Prefer human-readable stdout (e.g., search/bash output) over raw JSON.
                // Fall back to pretty-printed JSON when stdout is absent or empty.
                let text_result = last_stdout
                    .filter(|s: &String| !s.trim().is_empty())
                    .unwrap_or_else(|| {
                        serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string())
                    });
                Ok(CopilotToolCallResponse::Success(CopilotToolCallSuccess {
                    text_result_for_llm: text_result,
                }))
            }
            ToolExecutionStatus::Failure { error } => {
                Ok(CopilotToolCallResponse::Failure(CopilotToolCallFailure {
                    text_result_for_llm: format!(
                        "VT Code failed to execute the tool `{canonical_tool_name}`."
                    ),
                    error: format!("tool '{canonical_tool_name}' failed: {error}"),
                }))
            }
            ToolExecutionStatus::Timeout { error } => {
                Ok(CopilotToolCallResponse::Failure(CopilotToolCallFailure {
                    text_result_for_llm: format!(
                        "VT Code timed out while executing the tool `{canonical_tool_name}`."
                    ),
                    error: format!("tool '{canonical_tool_name}' timed out: {}", error.message),
                }))
            }
            ToolExecutionStatus::Cancelled => {
                Ok(CopilotToolCallResponse::Failure(CopilotToolCallFailure {
                    text_result_for_llm: format!(
                        "VT Code cancelled the tool `{canonical_tool_name}`."
                    ),
                    error: format!("tool '{canonical_tool_name}' execution cancelled"),
                }))
            }
        }
    }

    async fn prepare_vtcode_tool_execution(
        &mut self,
        renderer: &mut AnsiRenderer,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<Option<CopilotToolCallResponse>> {
        self.safety_validator
            .write()
            .await
            .validate_call(tool_name, arguments)
            .await
            .with_context(|| format!("copilot tool safety validation for '{tool_name}'"))?;

        match ensure_tool_permission(
            ToolPermissionsContext {
                tool_registry: self.tool_registry,
                renderer,
                handle: self.handle,
                session: self.session,
                default_placeholder: self.default_placeholder.clone(),
                ctrl_c_state: self.ctrl_c_state,
                ctrl_c_notify: self.ctrl_c_notify,
                hooks: self.lifecycle_hooks,
                justification: None,
                approval_recorder: Some(self.approval_recorder),
                decision_ledger: Some(self.decision_ledger),
                tool_permission_cache: Some(self.tool_permission_cache),
                hitl_notification_bell: self.hitl_notification_bell,
                autonomous_mode: self.session_stats.is_autonomous_mode(),
                approval_policy: self.approval_policy,
                skip_confirmations: self.skip_confirmations,
            },
            tool_name,
            Some(arguments),
        )
        .await?
        {
            ToolPermissionFlow::Approved => {}
            ToolPermissionFlow::Denied => {
                return Ok(Some(denied_tool_execution_response(
                    tool_name,
                    "denied by user or policy",
                )));
            }
            ToolPermissionFlow::Exit | ToolPermissionFlow::Interrupted => {
                return Ok(Some(denied_tool_execution_response(
                    tool_name,
                    "permission request interrupted",
                )));
            }
        }

        if let Some(max_tool_calls) = self.harness_state.exhausted_tool_call_limit() {
            return Ok(Some(CopilotToolCallResponse::Failure(
                CopilotToolCallFailure {
                    text_result_for_llm: format!(
                        "VT Code denied the tool `{tool_name}` because the turn exceeded its tool-call budget."
                    ),
                    error: format!(
                        "tool '{tool_name}' exceeded max tool calls per turn ({max_tool_calls})"
                    ),
                },
            )));
        }

        self.harness_state.record_tool_call();
        if self.harness_state.should_emit_tool_budget_warning(0.75) {
            let used = self.harness_state.tool_calls;
            let max = self.harness_state.max_tool_calls;
            let remaining = self.harness_state.remaining_tool_calls();
            tracing::info!(
                used,
                max,
                remaining,
                "Tool-call budget warning threshold reached in copilot ACP path"
            );
            self.harness_state.mark_tool_budget_warning_emitted();
        }

        Ok(None)
    }

    fn record_tool_use(&mut self, tool_name: &str) {
        self.session_stats.record_tool(tool_name);
    }

    fn emit_tool_started_event(&self, tool_call_id: &str, tool_name: &str, arguments: &Value) {
        let Some(emitter) = self.harness_emitter else {
            return;
        };
        let item_id = harness_call_item_id(&self.harness_item_prefix, tool_call_id, tool_name);
        let raw_tool_call_id = raw_tool_call_id(tool_call_id);
        let _ = emitter.emit(tool_started_event(
            item_id.clone(),
            tool_name,
            arguments,
            raw_tool_call_id,
        ));
        let _ = emitter.emit(tool_output_started_event(item_id, raw_tool_call_id));
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
        let raw_tool_call_id = raw_tool_call_id(tool_call_id);
        let _ = emitter.emit(tool_invocation_completed_event(
            item_id.clone(),
            tool_name,
            arguments,
            raw_tool_call_id,
            status.clone(),
        ));
        let _ = emitter.emit(tool_output_completed_event(
            item_id,
            raw_tool_call_id,
            status,
            None,
            None,
            output.unwrap_or_default(),
        ));
    }

    fn handle_observed_tool_call(&mut self, update: CopilotObservedToolCall) {
        let tool_call_id = update.tool_call_id.clone();
        let mut started_tool_name = None;
        let mut finished = None;
        {
            let state = self
                .observed_tool_calls
                .entry(tool_call_id.clone())
                .or_insert_with(|| ObservedToolCallState::new(update.tool_name.clone()));
            if state.tool_name == "copilot_tool" && update.tool_name != "copilot_tool" {
                state.tool_name = update.tool_name.clone();
            }
            if !state.started {
                state.started = true;
                started_tool_name = Some(state.tool_name.clone());
            }
            if !state.finished
                && matches!(
                    update.status,
                    CopilotObservedToolCallStatus::Completed
                        | CopilotObservedToolCallStatus::Failed
                )
            {
                state.finished = true;
                let status = match update.status {
                    CopilotObservedToolCallStatus::Completed => ToolCallStatus::Completed,
                    CopilotObservedToolCallStatus::Failed => ToolCallStatus::Failed,
                    CopilotObservedToolCallStatus::Pending
                    | CopilotObservedToolCallStatus::InProgress => ToolCallStatus::InProgress,
                };
                finished = Some((state.tool_name.clone(), status));
            }
        }
        if let Some(tool_name) = started_tool_name {
            self.record_tool_use(&tool_name);
            self.emit_tool_started_event(
                &tool_call_id,
                &tool_name,
                update.arguments.as_ref().unwrap_or(&Value::Null),
            );
        }
        if let Some((tool_name, status)) = finished {
            self.emit_tool_finished_event(
                &tool_call_id,
                &tool_name,
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

#[async_trait]
impl CopilotRuntimeRequestHandler for CopilotRuntimeHost<'_> {
    async fn handle_runtime_request(
        &mut self,
        renderer: &mut AnsiRenderer,
        request: CopilotRuntimeRequest,
    ) -> Result<(), uni::LLMError> {
        match request {
            CopilotRuntimeRequest::Permission(request_event) => {
                let decision = self
                    .handle_builtin_permission(renderer, request_event.request.clone())
                    .await
                    .map_err(map_runtime_error)?;
                request_event.respond(decision).map_err(map_runtime_error)?;
            }
            CopilotRuntimeRequest::ToolCall(request_event) => {
                let response = self
                    .handle_vtcode_tool_call(renderer, request_event.request.clone())
                    .await
                    .map_err(map_runtime_error)?;
                request_event.respond(response).map_err(map_runtime_error)?;
            }
            CopilotRuntimeRequest::ObservedToolCall(update) => {
                self.handle_observed_tool_call(update);
            }
            CopilotRuntimeRequest::CompatibilityNotice(notice) => {
                self.handle_compatibility_notice(renderer, notice.state, notice.message)
                    .map_err(map_runtime_error)?;
            }
        }
        Ok(())
    }
}

struct ObservedToolCallState {
    tool_name: String,
    started: bool,
    finished: bool,
}

impl ObservedToolCallState {
    fn new(tool_name: String) -> Self {
        Self {
            tool_name,
            started: false,
            finished: false,
        }
    }
}

pub(super) fn prompt_session_to_stream(
    model: String,
    prompt_session: PromptSession,
) -> (
    uni::LLMStream,
    tokio::sync::mpsc::UnboundedReceiver<CopilotRuntimeRequest>,
) {
    struct PromptCancellationGuard {
        cancel_handle: Option<PromptSessionCancelHandle>,
    }

    impl PromptCancellationGuard {
        fn new(cancel_handle: PromptSessionCancelHandle) -> Self {
            Self {
                cancel_handle: Some(cancel_handle),
            }
        }

        fn disarm(&mut self) {
            self.cancel_handle = None;
        }
    }

    impl Drop for PromptCancellationGuard {
        fn drop(&mut self) {
            if let Some(cancel_handle) = self.cancel_handle.take() {
                cancel_handle.cancel();
            }
        }
    }

    let (mut updates, runtime_requests, completion, cancel_handle) = prompt_session.into_parts();

    let stream = stream! {
        let mut cancellation_guard = PromptCancellationGuard::new(cancel_handle);
        let completion = completion;
        tokio::pin!(completion);

        let mut content = String::new();
        let mut reasoning = String::new();
        // Once the updates channel closes (all tokens delivered), disable that arm so
        // the select no longer spins on None and immediately picks `completion`.
        let mut updates_done = false;

        loop {
            tokio::select! {
                update = updates.recv(), if !updates_done => {
                    match update {
                        Some(PromptUpdate::Text(delta)) => {
                            content.push_str(&delta);
                            yield Ok(LLMStreamEvent::Token { delta });
                        }
                        Some(PromptUpdate::Thought(delta)) => {
                            reasoning.push_str(&delta);
                            yield Ok(LLMStreamEvent::Reasoning { delta });
                        }
                        None => {
                            // All tokens delivered; completion will be ready on next tick.
                            updates_done = true;
                        }
                    }
                }
                result = &mut completion => {
                    let completion = match result {
                        Ok(completion) => completion,
                        Err(err) => {
                            yield Err(map_runtime_error(anyhow!("copilot acp prompt task join failed: {err}")));
                            break;
                        }
                    };
                    let completion = match completion {
                        Ok(completion) => completion,
                        Err(err) => {
                            yield Err(map_runtime_error(err));
                            break;
                        }
                    };
                    while let Ok(update) = updates.try_recv() {
                        match update {
                            PromptUpdate::Text(delta) => {
                                content.push_str(&delta);
                                yield Ok(LLMStreamEvent::Token { delta });
                            }
                            PromptUpdate::Thought(delta) => {
                                reasoning.push_str(&delta);
                                yield Ok(LLMStreamEvent::Reasoning { delta });
                            }
                        }
                    }

                    let mut response = LLMResponse::new(model, content);
                    response.finish_reason =
                        map_copilot_finish_reason(&completion.stop_reason);
                    if !reasoning.is_empty() {
                        response.reasoning = Some(reasoning);
                    }
                    cancellation_guard.disarm();
                    yield Ok(Completed {
                        response: Box::new(response),
                    });
                    break;
                }
            }
        }
    };

    (Box::pin(stream), runtime_requests)
}

fn filter_copilot_tools(
    available_tools: Option<&Arc<Vec<ToolDefinition>>>,
    allowlist: &[String],
) -> Vec<ToolDefinition> {
    let allowlist: BTreeSet<&str> = allowlist.iter().map(String::as_str).collect();
    available_tools
        .into_iter()
        .flat_map(|tools| tools.iter())
        .filter(|tool| tool_definition_name(tool).is_some_and(|name| allowlist.contains(name)))
        .cloned()
        .collect()
}

fn tool_definition_name(tool: &ToolDefinition) -> Option<&str> {
    tool.function
        .as_ref()
        .map(|function| function.name.as_str())
}

struct PermissionPromptSummary {
    cache_key: String,
    tool_name: String,
    display_name: String,
    learning_label: String,
    tool_args: Option<Value>,
    reason: Option<String>,
}

fn map_copilot_finish_reason(stop_reason: &str) -> vtcode_core::llm::provider::FinishReason {
    match stop_reason.trim() {
        "end_turn" => vtcode_core::llm::provider::FinishReason::Stop,
        "max_tokens" | "length" => vtcode_core::llm::provider::FinishReason::Length,
        "refusal" => vtcode_core::llm::provider::FinishReason::Refusal,
        "cancelled" => vtcode_core::llm::provider::FinishReason::Error("cancelled".to_string()),
        other => vtcode_core::llm::provider::FinishReason::Error(other.to_string()),
    }
}

fn scoped_cache_key(prefix: &str, scope: Value) -> String {
    serde_json::to_string(&json!({
        "prefix": prefix,
        "scope": scope,
    }))
    .unwrap_or_else(|_| prefix.to_string())
}

fn map_builtin_permission_prompt_decision(
    decision: HitlDecision,
    feedback: Option<String>,
) -> (CopilotPermissionDecision, bool) {
    match decision {
        HitlDecision::Approved => (CopilotPermissionDecision::Approved, false),
        HitlDecision::ApprovedSession | HitlDecision::ApprovedPermanent => {
            (CopilotPermissionDecision::ApprovedAlways, true)
        }
        HitlDecision::Denied
        | HitlDecision::DeniedOnce
        | HitlDecision::Exit
        | HitlDecision::Interrupt => (
            CopilotPermissionDecision::DeniedInteractivelyByUser { feedback },
            false,
        ),
    }
}

fn auto_approve_builtin_permission(
    request: &CopilotPermissionRequest,
) -> Option<(CopilotPermissionDecision, bool)> {
    match request {
        // Copilot ACP keeps asking permission for these descriptive custom-tool
        // requests even though VT Code still controls the actual tool-call path.
        CopilotPermissionRequest::CustomTool { .. } => {
            Some((CopilotPermissionDecision::ApprovedAlways, true))
        }
        _ => None,
    }
}

fn denied_tool_execution_response(tool_name: &str, error_reason: &str) -> CopilotToolCallResponse {
    CopilotToolCallResponse::Failure(CopilotToolCallFailure {
        text_result_for_llm: format!("VT Code denied the tool `{tool_name}`."),
        error: format!("tool '{tool_name}' {error_reason}"),
    })
}

fn summarize_permission_request(
    request: &CopilotPermissionRequest,
) -> Option<PermissionPromptSummary> {
    match request {
        CopilotPermissionRequest::Shell {
            full_command_text,
            intention,
            possible_paths,
            possible_urls,
            has_write_file_redirection,
            warning,
            ..
        } => Some(PermissionPromptSummary {
            cache_key: scoped_cache_key(
                "copilot:shell",
                json!({
                    "command": full_command_text,
                    "paths": possible_paths,
                    "urls": possible_urls,
                    "write_redirection": has_write_file_redirection,
                }),
            ),
            tool_name: "copilot_shell".to_string(),
            display_name: "GitHub Copilot shell command".to_string(),
            learning_label: "GitHub Copilot shell command".to_string(),
            tool_args: Some(json!({
                "command": full_command_text,
                "paths": possible_paths,
                "urls": possible_urls,
            })),
            reason: warning.clone().or_else(|| Some(intention.clone())),
        }),
        CopilotPermissionRequest::Write {
            file_name,
            intention,
            ..
        } => Some(PermissionPromptSummary {
            cache_key: scoped_cache_key(
                "copilot:write",
                json!({
                    "file": file_name,
                }),
            ),
            tool_name: "copilot_write".to_string(),
            display_name: "GitHub Copilot file write".to_string(),
            learning_label: "GitHub Copilot file write".to_string(),
            tool_args: Some(json!({
                "file": file_name,
                "intention": intention,
            })),
            reason: Some(intention.clone()),
        }),
        CopilotPermissionRequest::Read {
            path, intention, ..
        } => Some(PermissionPromptSummary {
            cache_key: scoped_cache_key(
                "copilot:read",
                json!({
                    "path": path,
                }),
            ),
            tool_name: "copilot_read".to_string(),
            display_name: "GitHub Copilot file read".to_string(),
            learning_label: "GitHub Copilot file read".to_string(),
            tool_args: Some(json!({
                "path": path,
                "intention": intention,
            })),
            reason: Some(intention.clone()),
        }),
        CopilotPermissionRequest::Mcp {
            server_name,
            tool_name,
            tool_title,
            args,
            read_only,
            ..
        } => Some(PermissionPromptSummary {
            cache_key: scoped_cache_key(
                "copilot:mcp",
                json!({
                    "server": server_name,
                    "tool": tool_name,
                    "args": args,
                    "read_only": read_only,
                }),
            ),
            tool_name: format!("copilot_mcp_{tool_name}"),
            display_name: format!("GitHub Copilot MCP tool {tool_title}"),
            learning_label: format!("GitHub Copilot MCP tool {tool_title}"),
            tool_args: args.clone(),
            reason: Some(format!("Server: {server_name}")),
        }),
        CopilotPermissionRequest::Url { url, intention, .. } => Some(PermissionPromptSummary {
            cache_key: scoped_cache_key(
                "copilot:url",
                json!({
                    "url": url,
                }),
            ),
            tool_name: "copilot_url".to_string(),
            display_name: "GitHub Copilot URL access".to_string(),
            learning_label: "GitHub Copilot URL access".to_string(),
            tool_args: Some(json!({
                "url": url,
                "intention": intention,
            })),
            reason: Some(intention.clone()),
        }),
        CopilotPermissionRequest::Memory { subject, fact, .. } => Some(PermissionPromptSummary {
            cache_key: scoped_cache_key(
                "copilot:memory",
                json!({
                    "subject": subject,
                    "fact": fact,
                }),
            ),
            tool_name: "copilot_memory".to_string(),
            display_name: "GitHub Copilot memory update".to_string(),
            learning_label: "GitHub Copilot memory update".to_string(),
            tool_args: Some(json!({
                "subject": subject,
                "fact": fact,
            })),
            reason: Some("GitHub Copilot wants to store a memory fact.".to_string()),
        }),
        CopilotPermissionRequest::CustomTool {
            tool_name,
            tool_description,
            args,
            ..
        } => Some(PermissionPromptSummary {
            cache_key: scoped_cache_key(
                "copilot:custom-tool",
                json!({
                    "tool": tool_name,
                    "args": args,
                }),
            ),
            tool_name: format!("copilot_custom_{tool_name}"),
            display_name: format!("GitHub Copilot custom tool {tool_name}"),
            learning_label: format!("GitHub Copilot custom tool {tool_name}"),
            tool_args: args.clone(),
            reason: Some(tool_description.clone()),
        }),
        CopilotPermissionRequest::Hook {
            tool_name,
            tool_args,
            hook_message,
            ..
        } => Some(PermissionPromptSummary {
            cache_key: scoped_cache_key(
                "copilot:hook",
                json!({
                    "tool": tool_name,
                    "args": tool_args,
                }),
            ),
            tool_name: format!("copilot_hook_{tool_name}"),
            display_name: format!("GitHub Copilot hook {tool_name}"),
            learning_label: format!("GitHub Copilot hook {tool_name}"),
            tool_args: tool_args.clone(),
            reason: hook_message.clone(),
        }),
        CopilotPermissionRequest::Unknown { .. } => None,
    }
}

fn map_runtime_error(err: anyhow::Error) -> uni::LLMError {
    uni::LLMError::Provider {
        message: format!("GitHub Copilot runtime bridge failed: {err}"),
        metadata: None,
    }
}

fn raw_tool_call_id(tool_call_id: &str) -> Option<&str> {
    (!tool_call_id.trim().is_empty()).then_some(tool_call_id)
}

fn harness_call_item_id(prefix: &str, tool_call_id: &str, tool_name: &str) -> String {
    if tool_call_id.trim().is_empty() {
        format!("{prefix}-copilot-tool-{}", tool_name.replace(' ', "_"))
    } else {
        format!("{prefix}-copilot-tool-{tool_call_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CopilotRuntimeHost, auto_approve_builtin_permission, denied_tool_execution_response,
        filter_copilot_tools, harness_call_item_id, map_builtin_permission_prompt_decision,
        map_copilot_finish_reason, summarize_permission_request,
    };
    use serde_json::json;
    use std::sync::Arc;
    use vtcode_core::copilot::{CopilotPermissionRequest, CopilotToolCallResponse};
    use vtcode_core::core::decision_tracker::DecisionTracker;
    use vtcode_core::core::trajectory::TrajectoryLogger;
    use vtcode_core::llm::provider::{FinishReason, ToolDefinition};
    use vtcode_core::tools::registry::ToolRegistry;
    use vtcode_core::tools::{ApprovalRecorder, ToolResultCache};
    use vtcode_core::utils::ansi::AnsiRenderer;
    use vtcode_core::utils::transcript;
    use vtcode_tui::app::{InlineHandle, InlineSession};

    use crate::agent::runloop::mcp_events::McpPanelState;
    use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
    use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
    use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
    use crate::agent::runloop::unified::tool_routing::HitlDecision;
    use tempfile::TempDir;
    use tokio::sync::{Notify, RwLock, mpsc::unbounded_channel};
    use vtcode_core::acp::ToolPermissionCache;

    fn create_headless_session() -> InlineSession {
        let (command_tx, _command_rx) = unbounded_channel();
        let (_event_tx, event_rx) = unbounded_channel();
        InlineSession {
            handle: InlineHandle::new_for_tests(command_tx),
            events: event_rx,
        }
    }

    #[test]
    fn filter_copilot_tools_keeps_only_allowlisted_names() {
        let tools = Arc::new(vec![
            ToolDefinition::function(
                "unified_search".to_string(),
                "Search".to_string(),
                json!({"type": "object"}),
            ),
            ToolDefinition::function(
                "apply_patch".to_string(),
                "Patch".to_string(),
                json!({"type": "object"}),
            ),
        ]);

        let filtered = filter_copilot_tools(Some(&tools), &["unified_search".to_string()]);
        let names: Vec<_> = filtered
            .iter()
            .filter_map(|tool| {
                tool.function
                    .as_ref()
                    .map(|function| function.name.as_str())
            })
            .collect();

        assert_eq!(names, vec!["unified_search"]);
    }

    #[test]
    fn summarize_shell_permission_request_uses_command_cache_key() {
        let summary = summarize_permission_request(&CopilotPermissionRequest::Shell {
            tool_call_id: Some("call_1".to_string()),
            full_command_text: "git status".to_string(),
            intention: "Inspect repository status".to_string(),
            commands: Vec::new(),
            possible_paths: vec!["/workspace".to_string()],
            possible_urls: Vec::new(),
            has_write_file_redirection: false,
            can_offer_session_approval: true,
            warning: None,
        })
        .expect("summary");

        assert_eq!(summary.display_name, "GitHub Copilot shell command");
        assert!(summary.cache_key.contains("\"prefix\":\"copilot:shell\""));
        assert!(summary.cache_key.contains("git status"));
    }

    #[test]
    fn shell_permission_cache_key_scopes_paths_and_urls() {
        let first = summarize_permission_request(&CopilotPermissionRequest::Shell {
            tool_call_id: None,
            full_command_text: "git status".to_string(),
            intention: "Inspect repository status".to_string(),
            commands: Vec::new(),
            possible_paths: vec!["/workspace/a".to_string()],
            possible_urls: Vec::new(),
            has_write_file_redirection: false,
            can_offer_session_approval: true,
            warning: None,
        })
        .expect("first summary");
        let second = summarize_permission_request(&CopilotPermissionRequest::Shell {
            tool_call_id: None,
            full_command_text: "git status".to_string(),
            intention: "Inspect repository status".to_string(),
            commands: Vec::new(),
            possible_paths: vec!["/workspace/b".to_string()],
            possible_urls: vec!["https://example.com".to_string()],
            has_write_file_redirection: false,
            can_offer_session_approval: true,
            warning: None,
        })
        .expect("second summary");

        assert_ne!(first.cache_key, second.cache_key);
    }

    #[test]
    fn custom_tool_permission_cache_key_scopes_arguments() {
        let first = summarize_permission_request(&CopilotPermissionRequest::CustomTool {
            tool_call_id: None,
            tool_name: "demo".to_string(),
            tool_description: "Run demo".to_string(),
            args: Some(json!({"path": "/tmp/a"})),
        })
        .expect("first summary");
        let second = summarize_permission_request(&CopilotPermissionRequest::CustomTool {
            tool_call_id: None,
            tool_name: "demo".to_string(),
            tool_description: "Run demo".to_string(),
            args: Some(json!({"path": "/tmp/b"})),
        })
        .expect("second summary");

        assert_ne!(first.cache_key, second.cache_key);
    }

    #[test]
    fn copilot_finish_reason_maps_protocol_values() {
        assert_eq!(map_copilot_finish_reason("end_turn"), FinishReason::Stop);
        assert_eq!(
            map_copilot_finish_reason("max_tokens"),
            FinishReason::Length
        );
        assert_eq!(map_copilot_finish_reason("length"), FinishReason::Length);
        assert_eq!(map_copilot_finish_reason("refusal"), FinishReason::Refusal);
        assert_eq!(
            map_copilot_finish_reason("cancelled"),
            FinishReason::Error("cancelled".to_string())
        );
    }

    #[test]
    fn harness_call_item_id_prefers_tool_call_id() {
        let id = harness_call_item_id("turn-1-step-1", "call_7", "copilot_read");
        assert_eq!(id, "turn-1-step-1-copilot-tool-call_7");
    }

    #[test]
    fn interrupted_builtin_permission_becomes_interactive_denial() {
        let (decision, cache_for_session) = map_builtin_permission_prompt_decision(
            HitlDecision::Interrupt,
            Some("Run cargo check".to_string()),
        );

        assert!(!cache_for_session);
        assert_eq!(
            decision,
            vtcode_core::copilot::CopilotPermissionDecision::DeniedInteractivelyByUser {
                feedback: Some("Run cargo check".to_string()),
            }
        );
    }

    #[test]
    fn interrupted_tool_permission_returns_failure_response() {
        let response =
            denied_tool_execution_response("unified_exec", "permission request interrupted");

        assert_eq!(
            response,
            CopilotToolCallResponse::Failure(vtcode_core::copilot::CopilotToolCallFailure {
                text_result_for_llm: "VT Code denied the tool `unified_exec`.".to_string(),
                error: "tool 'unified_exec' permission request interrupted".to_string(),
            })
        );
    }

    #[test]
    fn custom_tool_permissions_are_auto_approved_for_session() {
        let approval = auto_approve_builtin_permission(&CopilotPermissionRequest::CustomTool {
            tool_call_id: Some("call-1".to_string()),
            tool_name: "Read last 100 lines of CHANGELOG".to_string(),
            tool_description: "Read the changelog tail".to_string(),
            args: Some(json!({"path": "CHANGELOG.md"})),
        });

        assert_eq!(
            approval,
            Some((
                vtcode_core::copilot::CopilotPermissionDecision::ApprovedAlways,
                true
            ))
        );
    }

    #[test]
    fn shell_permissions_are_not_auto_approved() {
        let approval = auto_approve_builtin_permission(&CopilotPermissionRequest::Shell {
            tool_call_id: Some("call-2".to_string()),
            full_command_text: "cargo check".to_string(),
            intention: "Verify the workspace builds".to_string(),
            commands: vec![],
            possible_paths: vec!["./".to_string()],
            possible_urls: vec![],
            has_write_file_redirection: false,
            can_offer_session_approval: true,
            warning: None,
        });

        assert_eq!(approval, None);
    }

    #[tokio::test]
    async fn vtcode_tool_calls_render_transcript_output_via_shared_pipeline() {
        let temp = TempDir::new().expect("temp workspace");
        let workspace = temp.path().to_path_buf();
        let sample_file = workspace.join("sample.txt");
        std::fs::write(&sample_file, "hello from acp\n").expect("write sample file");

        let mut tool_registry = ToolRegistry::new(workspace.clone()).await;
        let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
        let mut session = create_headless_session();
        let handle = session.clone_inline_handle();
        let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
        let mut session_stats = SessionStats::default();
        let mut mcp_panel_state = McpPanelState::default();
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());
        let approval_recorder = ApprovalRecorder::new(workspace.clone());
        let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
        let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
        let safety_validator = Arc::new(RwLock::new(ToolCallSafetyValidator::new()));
        safety_validator.write().await.start_turn().await;
        let traj = TrajectoryLogger::new(&workspace);
        let mut harness_state = HarnessTurnState::new(
            TurnRunId("run-test".to_string()),
            TurnId("turn-test".to_string()),
            8,
            60,
            0,
        );
        let tools = Arc::new(vec![ToolDefinition::function(
            "unified_exec".to_string(),
            "Run a VT Code command".to_string(),
            json!({"type": "object"}),
        )]);

        transcript::clear();
        let mut runtime_host = CopilotRuntimeHost::new(
            &mut tool_registry,
            &tool_result_cache,
            &mut session,
            &mut session_stats,
            &mut mcp_panel_state,
            &handle,
            &ctrl_c_state,
            &ctrl_c_notify,
            None,
            &approval_recorder,
            &decision_ledger,
            &tool_permission_cache,
            &safety_validator,
            None,
            None,
            &traj,
            &mut harness_state,
            Some(&tools),
            true,
            None,
            "turn-test-step-1".to_string(),
        );

        let response = runtime_host
            .handle_vtcode_tool_call(
                &mut renderer,
                vtcode_core::copilot::CopilotToolCallRequest {
                    tool_call_id: "call_1".to_string(),
                    tool_name: "unified_exec".to_string(),
                    arguments: json!({
                        "action": "run",
                        "command": "printf 'hello from acp\\n'"
                    }),
                },
            )
            .await
            .expect("copilot VT Code tool call should succeed");

        match response {
            CopilotToolCallResponse::Success(success) => {
                assert!(success.text_result_for_llm.contains("hello from acp"));
            }
            other => panic!("unexpected tool response: {other:?}"),
        }

        let transcript_text = transcript::snapshot().join("\n");
        assert!(runtime_host.harness_state.tool_calls >= 1);
        assert!(transcript_text.contains("hello from acp"));
        assert!(
            transcript_text.contains("Ran printf") || transcript_text.contains("Run command"),
            "expected command preview in transcript, got: {transcript_text}"
        );
    }
}
