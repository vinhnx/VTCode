use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use async_stream::stream;
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::RwLock;
use vtcode_config::auth::CopilotAuthConfig;
use vtcode_core::acp::{PermissionGrant, ToolPermissionCache};
use vtcode_core::config::PtyConfig;
use vtcode_core::copilot::{
    CopilotAcpCompatibilityState, CopilotObservedToolCall, CopilotObservedToolCallStatus,
    CopilotPermissionDecision, CopilotPermissionRequest, CopilotRuntimeRequest,
    CopilotTerminalCreateRequest, CopilotTerminalCreateResponse, CopilotTerminalEnvVar,
    CopilotTerminalExitStatus, CopilotTerminalOutputResponse, CopilotToolCallFailure,
    CopilotToolCallRequest, CopilotToolCallResponse, CopilotToolCallSuccess, PromptSession,
    PromptSessionCancelHandle, PromptUpdate,
};
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::exec::events::ToolCallStatus;
use vtcode_core::exec_policy::AskForApproval;
use vtcode_core::llm::provider::{self as uni, LLMStreamEvent, LLMStreamEvent::Completed};
use vtcode_core::llm::provider::{LLMResponse, ToolDefinition};
use vtcode_core::tools::registry::{ToolProgressCallback, ToolRegistry};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::app::{InlineHandle, InlineSession};

use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::tool_output::resolve_stdout_tail_limit;
use crate::agent::runloop::unified::async_mcp_manager::approval_policy_from_human_in_the_loop;
use crate::agent::runloop::unified::inline_events::harness::{
    HarnessEventEmitter, tool_invocation_completed_event, tool_output_completed_event,
    tool_output_started_event, tool_started_event, tool_updated_event,
};
use crate::agent::runloop::unified::progress::{
    ProgressReporter, ProgressUpdateGuard, spawn_elapsed_time_updater,
};
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, RunLoopContext};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output;
use crate::agent::runloop::unified::tool_pipeline::{
    PtyStreamRuntime, ToolExecutionStatus, run_tool_call_with_args,
};
use crate::agent::runloop::unified::tool_routing::{
    HitlDecision, ToolPermissionFlow, ToolPermissionsContext, ensure_tool_permission,
    prompt_external_tool_permission,
};
use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;
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
    local_terminal_sessions: HashMap<String, LocalTerminalSession>,
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
            local_terminal_sessions: HashMap::new(),
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
                permissions_config: self.vt_cfg.map(|cfg| &cfg.permissions),
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
            Some(arguments),
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
            Some(arguments),
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

    fn emit_tool_output_updated_event(&self, tool_call_id: &str, tool_name: &str, output: &str) {
        let Some(emitter) = self.harness_emitter else {
            return;
        };
        let item_id = harness_call_item_id(&self.harness_item_prefix, tool_call_id, tool_name);
        let raw_tool_call_id = raw_tool_call_id(tool_call_id);
        let _ = emitter.emit(tool_updated_event(item_id, raw_tool_call_id, output));
    }

    async fn handle_terminal_create(
        &mut self,
        request: CopilotTerminalCreateRequest,
    ) -> Result<CopilotTerminalCreateResponse> {
        let command_display = terminal_command_display(&request.command, &request.args);
        let response = self
            .tool_registry
            .execute_harness_unified_exec_terminal_run(terminal_run_args(&request))
            .await
            .context("copilot local terminal create")?;

        let terminal_id = response
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("copilot local terminal create missing session_id"))?;
        let initial_output = response
            .get("output")
            .and_then(Value::as_str)
            .map(str::to_string);
        let initial_exit_status = response
            .get("exit_code")
            .and_then(Value::as_i64)
            .and_then(terminal_exit_status_from_code);
        let initial_session_completed = initial_exit_status.is_some();
        let released = Arc::new(AtomicBool::new(false));
        let exit_notify = Arc::new(tokio::sync::Notify::new());
        let state = Arc::new(Mutex::new(LocalTerminalSessionState::new(
            request.output_byte_limit,
        )));
        {
            let mut session_state = lock_local_terminal_state(&state);
            if let Some(output) = initial_output.as_deref() {
                session_state.append_output(output);
            }
            session_state.exit_status = initial_exit_status.clone();
        }
        if initial_session_completed {
            exit_notify.notify_waiters();
        }

        let task = tokio::spawn(run_local_terminal_session(LocalTerminalTaskContext {
            tool_registry: self.tool_registry.clone(),
            exec_session_id: terminal_id.clone(),
            released: Arc::clone(&released),
            exit_notify: Arc::clone(&exit_notify),
            state: Arc::clone(&state),
            harness_emitter: self.harness_emitter.cloned(),
            harness_item_prefix: self.harness_item_prefix.clone(),
            handle: self.handle.clone(),
            tail_limit: resolve_stdout_tail_limit(self.vt_cfg),
            command_display,
            initial_output,
            pty_config: self.tool_registry.pty_config().clone(),
        }));

        self.local_terminal_sessions.insert(
            terminal_id.clone(),
            LocalTerminalSession {
                exec_session_id: terminal_id.clone(),
                released,
                exit_notify,
                state,
                task,
            },
        );

        Ok(CopilotTerminalCreateResponse { terminal_id })
    }

    async fn handle_terminal_output(
        &self,
        terminal_id: &str,
    ) -> Result<CopilotTerminalOutputResponse> {
        let session = self
            .local_terminal_sessions
            .get(terminal_id)
            .ok_or_else(|| anyhow!("copilot terminal '{terminal_id}' not found"))?;
        Ok(session.snapshot_output())
    }

    async fn handle_terminal_release(&mut self, terminal_id: &str) -> Result<()> {
        let Some(session) = self.local_terminal_sessions.remove(terminal_id) else {
            return Ok(());
        };
        session.released.store(true, Ordering::Relaxed);
        session.exit_notify.notify_waiters();
        let _ = self
            .tool_registry
            .close_harness_exec_session(&session.exec_session_id)
            .await;
        session.task.abort();
        Ok(())
    }

    async fn handle_terminal_kill(&self, terminal_id: &str) -> Result<()> {
        let session = self
            .local_terminal_sessions
            .get(terminal_id)
            .ok_or_else(|| anyhow!("copilot terminal '{terminal_id}' not found"))?;
        self.tool_registry
            .terminate_harness_exec_session(&session.exec_session_id)
            .await
            .with_context(|| format!("copilot terminal kill '{}'", session.exec_session_id))
    }

    async fn handle_terminal_wait_for_exit(
        &self,
        terminal_id: &str,
    ) -> Result<CopilotTerminalExitStatus> {
        let session = self
            .local_terminal_sessions
            .get(terminal_id)
            .ok_or_else(|| anyhow!("copilot terminal '{terminal_id}' not found"))?;
        Ok(session.wait_for_exit().await)
    }

    fn handle_observed_tool_call(&mut self, update: CopilotObservedToolCall) {
        if let Some(terminal_id) = update.terminal_id.as_deref()
            && let Some(session) = self.local_terminal_sessions.get(terminal_id)
        {
            let bind_result = session.bind_observed_tool_call(&update);
            if bind_result.emit_started {
                self.record_tool_use(&bind_result.association.tool_name);
                self.emit_tool_started_event(
                    &bind_result.association.tool_call_id,
                    &bind_result.association.tool_name,
                    &bind_result.association.arguments,
                );
            }
            if let Some(output) = bind_result.buffered_output.as_deref() {
                self.emit_tool_output_updated_event(
                    &bind_result.association.tool_call_id,
                    &bind_result.association.tool_name,
                    output,
                );
            }
            if let Some(status) = bind_result.finish_status {
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
        let mut started_tool_name = None;
        let mut output_update = None;
        let mut finished = None;
        let mut finished_stream = None;
        let tail_limit = resolve_stdout_tail_limit(self.vt_cfg);
        let command_display = observed_tool_command_display(&update);
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
            if state.pty_stream.is_none()
                && let Some(command_display) = command_display.as_ref()
            {
                state.pty_stream = Some(ObservedToolPtyStream::start(
                    self.handle,
                    tail_limit,
                    command_display.clone(),
                    self.tool_registry.pty_config().clone(),
                ));
            }
            if let Some(output) = update
                .output
                .as_deref()
                .filter(|text| !text.trim().is_empty())
                && state.last_output.as_deref() != Some(output)
            {
                if let Some(stream) = state.pty_stream.as_ref()
                    && let Some(delta) =
                        observed_tool_output_delta(state.last_output.as_deref(), output)
                    && !delta.is_empty()
                {
                    stream.push_output(delta);
                }
                state.last_output = Some(output.to_string());
                output_update = Some((state.tool_name.clone(), output.to_string()));
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
                finished_stream = state.pty_stream.take();
                finished = Some((state.tool_name.clone(), status));
            }
        }
        if let Some(stream) = finished_stream {
            stream.finish();
        }
        if let Some(tool_name) = started_tool_name {
            self.record_tool_use(&tool_name);
            self.emit_tool_started_event(
                &tool_call_id,
                &tool_name,
                update.arguments.as_ref().unwrap_or(&Value::Null),
            );
        }
        if let Some((tool_name, output)) = output_update {
            self.emit_tool_output_updated_event(&tool_call_id, &tool_name, &output);
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
            CopilotRuntimeRequest::TerminalCreate(request_event) => {
                let response = self
                    .handle_terminal_create(request_event.request.clone())
                    .await
                    .map_err(map_runtime_error)?;
                request_event.respond(response).map_err(map_runtime_error)?;
            }
            CopilotRuntimeRequest::TerminalOutput(request_event) => {
                let response = self
                    .handle_terminal_output(&request_event.request.terminal_id)
                    .await
                    .map_err(map_runtime_error)?;
                request_event.respond(response).map_err(map_runtime_error)?;
            }
            CopilotRuntimeRequest::TerminalRelease(request_event) => {
                self.handle_terminal_release(&request_event.request.terminal_id)
                    .await
                    .map_err(map_runtime_error)?;
                request_event.respond().map_err(map_runtime_error)?;
            }
            CopilotRuntimeRequest::TerminalKill(request_event) => {
                self.handle_terminal_kill(&request_event.request.terminal_id)
                    .await
                    .map_err(map_runtime_error)?;
                request_event.respond().map_err(map_runtime_error)?;
            }
            CopilotRuntimeRequest::TerminalWaitForExit(request_event) => {
                let response = self
                    .handle_terminal_wait_for_exit(&request_event.request.terminal_id)
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

impl Drop for CopilotRuntimeHost<'_> {
    fn drop(&mut self) {
        for (_, session) in self.local_terminal_sessions.drain() {
            session.abort();
        }
    }
}

struct ObservedToolCallState {
    tool_name: String,
    started: bool,
    finished: bool,
    last_output: Option<String>,
    pty_stream: Option<ObservedToolPtyStream>,
}

impl ObservedToolCallState {
    fn new(tool_name: String) -> Self {
        Self {
            tool_name,
            started: false,
            finished: false,
            last_output: None,
            pty_stream: None,
        }
    }
}

struct ObservedToolPtyStream {
    _progress_reporter: ProgressReporter,
    _spinner: PlaceholderSpinner,
    _runtime: PtyStreamRuntime,
    callback: ToolProgressCallback,
}

impl ObservedToolPtyStream {
    fn start(
        handle: &InlineHandle,
        tail_limit: usize,
        command_display: String,
        pty_config: PtyConfig,
    ) -> Self {
        let progress_reporter = ProgressReporter::new();
        let spinner = PlaceholderSpinner::with_progress(
            handle,
            Some(String::new()),
            Some(String::new()),
            format!("Running command: {command_display}"),
            Some(&progress_reporter),
        );
        let (runtime, callback) = PtyStreamRuntime::start(
            handle.clone(),
            progress_reporter.clone(),
            tail_limit,
            Some(command_display),
            pty_config,
        );

        Self {
            _progress_reporter: progress_reporter,
            _spinner: spinner,
            _runtime: runtime,
            callback,
        }
    }

    fn push_output(&self, chunk: &str) {
        (self.callback)("unified_exec", chunk);
    }

    fn finish(self) {
        self._spinner.finish();
        let progress_reporter = self._progress_reporter.clone();
        let mut runtime = self._runtime;
        drop(self.callback);

        tokio::spawn(async move {
            progress_reporter.complete().await;
            let _ = runtime.sender.take();
            if let Some(task) = runtime.task.take() {
                let _ = task.await;
            }
            runtime.active.store(false, Ordering::Relaxed);
        });
    }
}

#[derive(Clone)]
struct LocalTerminalAssociation {
    tool_call_id: String,
    tool_name: String,
    arguments: Value,
}

struct LocalTerminalSessionState {
    output: String,
    truncated: bool,
    output_byte_limit: Option<usize>,
    exit_status: Option<CopilotTerminalExitStatus>,
    association: Option<LocalTerminalAssociation>,
    tool_started: bool,
    tool_finished: bool,
}

impl LocalTerminalSessionState {
    fn new(output_byte_limit: Option<usize>) -> Self {
        Self {
            output: String::new(),
            truncated: false,
            output_byte_limit,
            exit_status: None,
            association: None,
            tool_started: false,
            tool_finished: false,
        }
    }

    fn append_output(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        self.output.push_str(chunk);
        if let Some(limit) = self.output_byte_limit
            && self.output.len() > limit
        {
            let mut drain_until = self.output.len() - limit;
            while drain_until < self.output.len() && !self.output.is_char_boundary(drain_until) {
                drain_until += 1;
            }
            if drain_until > 0 {
                self.output.drain(..drain_until);
                self.truncated = true;
            }
        }
    }
}

struct LocalTerminalSession {
    exec_session_id: String,
    released: Arc<AtomicBool>,
    exit_notify: Arc<tokio::sync::Notify>,
    state: Arc<Mutex<LocalTerminalSessionState>>,
    task: tokio::task::JoinHandle<()>,
}

struct LocalTerminalBindResult {
    association: LocalTerminalAssociation,
    emit_started: bool,
    buffered_output: Option<String>,
    finish_status: Option<ToolCallStatus>,
}

struct LocalTerminalTaskContext {
    tool_registry: ToolRegistry,
    exec_session_id: String,
    released: Arc<AtomicBool>,
    exit_notify: Arc<tokio::sync::Notify>,
    state: Arc<Mutex<LocalTerminalSessionState>>,
    harness_emitter: Option<HarnessEventEmitter>,
    harness_item_prefix: String,
    handle: InlineHandle,
    tail_limit: usize,
    command_display: String,
    initial_output: Option<String>,
    pty_config: PtyConfig,
}

impl LocalTerminalSession {
    fn bind_observed_tool_call(&self, update: &CopilotObservedToolCall) -> LocalTerminalBindResult {
        let mut state = lock_local_terminal_state(&self.state);
        let association = if let Some(association) = state.association.as_mut() {
            if association.tool_name == "copilot_tool" && update.tool_name != "copilot_tool" {
                association.tool_name = update.tool_name.clone();
            }
            if association.arguments.is_null()
                && let Some(arguments) = update.arguments.clone()
            {
                association.arguments = arguments;
            }
            association.clone()
        } else {
            let association = LocalTerminalAssociation {
                tool_call_id: update.tool_call_id.clone(),
                tool_name: update.tool_name.clone(),
                arguments: update.arguments.clone().unwrap_or(Value::Null),
            };
            state.association = Some(association.clone());
            association
        };

        let emit_started = if state.tool_started {
            false
        } else {
            state.tool_started = true;
            true
        };
        let buffered_output = emit_started
            .then(|| state.output.clone())
            .filter(|output| !output.trim().is_empty());
        let finish_status = if let Some(exit_status) = state.exit_status.clone() {
            if state.tool_finished {
                None
            } else {
                state.tool_finished = true;
                Some(local_terminal_tool_status(&exit_status))
            }
        } else {
            None
        };

        LocalTerminalBindResult {
            association,
            emit_started,
            buffered_output,
            finish_status,
        }
    }

    fn snapshot_output(&self) -> CopilotTerminalOutputResponse {
        let state = lock_local_terminal_state(&self.state);
        CopilotTerminalOutputResponse {
            output: state.output.clone(),
            truncated: state.truncated,
            exit_status: state.exit_status.clone(),
        }
    }

    async fn wait_for_exit(&self) -> CopilotTerminalExitStatus {
        loop {
            if let Some(exit_status) = lock_local_terminal_state(&self.state).exit_status.clone() {
                return exit_status;
            }
            self.exit_notify.notified().await;
        }
    }

    fn abort(self) {
        self.released.store(true, Ordering::Relaxed);
        self.exit_notify.notify_waiters();
        self.task.abort();
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
                            let delta = normalize_copilot_reasoning_delta(&reasoning, delta);
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
                                let delta = normalize_copilot_reasoning_delta(&reasoning, delta);
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

async fn run_local_terminal_session(task: LocalTerminalTaskContext) {
    let LocalTerminalTaskContext {
        tool_registry,
        exec_session_id,
        released,
        exit_notify,
        state,
        harness_emitter,
        harness_item_prefix,
        handle,
        tail_limit,
        command_display,
        initial_output,
        pty_config,
    } = task;
    let progress_reporter = ProgressReporter::new();
    progress_reporter.set_total(100).await;
    progress_reporter.set_progress(40).await;
    progress_reporter
        .set_message(format!("Running command: {command_display}"))
        .await;
    let _elapsed_guard = ProgressUpdateGuard::new(spawn_elapsed_time_updater(
        progress_reporter.clone(),
        format!("command: {command_display}"),
        500,
    ));
    let spinner = PlaceholderSpinner::with_progress(
        &handle,
        Some(String::new()),
        Some(String::new()),
        format!("Running command: {command_display}"),
        Some(&progress_reporter),
    );
    let (pty_stream_runtime, progress_callback) = PtyStreamRuntime::start(
        handle,
        progress_reporter.clone(),
        tail_limit,
        Some(command_display),
        pty_config,
    );

    if let Some(initial_output) = initial_output.as_deref() {
        progress_callback("unified_exec", initial_output);
    }

    loop {
        if released.load(Ordering::Relaxed) {
            break;
        }

        match tool_registry
            .read_harness_exec_session_output(&exec_session_id, true)
            .await
        {
            Ok(Some(chunk)) if !chunk.is_empty() => {
                progress_callback("unified_exec", &chunk);
                if let Some((tool_call_id, tool_name, output)) =
                    update_local_terminal_output(&state, &chunk)
                {
                    emit_terminal_output_event(
                        harness_emitter.as_ref(),
                        &harness_item_prefix,
                        &tool_call_id,
                        &tool_name,
                        &output,
                    );
                }
            }
            Ok(Some(_)) | Ok(None) => {}
            Err(err) => {
                tracing::warn!(
                    terminal_id = %exec_session_id,
                    error = %err,
                    "Failed to read Copilot local terminal output"
                );
                break;
            }
        }

        match tool_registry
            .harness_exec_session_completed(&exec_session_id)
            .await
        {
            Ok(Some(code)) => {
                let exit_status = terminal_exit_status_from_code(i64::from(code));
                let finish = finalize_local_terminal_exit(&state, exit_status.clone());
                if let Some((tool_call_id, tool_name, arguments, output, status)) = finish {
                    emit_terminal_finished_event(
                        harness_emitter.as_ref(),
                        &harness_item_prefix,
                        &tool_call_id,
                        &tool_name,
                        &arguments,
                        status,
                        output,
                    );
                }
                progress_reporter.set_progress(100).await;
                progress_reporter.complete().await;
                exit_notify.notify_waiters();
                break;
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(
                    terminal_id = %exec_session_id,
                    error = %err,
                    "Failed to poll Copilot local terminal exit state"
                );
                break;
            }
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    pty_stream_runtime.shutdown().await;
    spinner.finish();
}

fn terminal_run_args(request: &CopilotTerminalCreateRequest) -> Value {
    json!({
        "action": "run",
        "command": request.command,
        "args": request.args,
        "cwd": request.cwd.as_ref().map(|path| path.to_string_lossy().to_string()),
        "tty": true,
        "yield_time_ms": 100,
        "env": terminal_env_payload(&request.env),
    })
}

fn terminal_env_payload(env: &[CopilotTerminalEnvVar]) -> Value {
    if env.is_empty() {
        Value::Array(Vec::new())
    } else {
        Value::Array(
            env.iter()
                .map(|entry| {
                    json!({
                        "name": entry.name,
                        "value": entry.value,
                    })
                })
                .collect(),
        )
    }
}

fn terminal_command_display(command: &str, args: &[String]) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        let mut parts = Vec::with_capacity(1 + args.len());
        parts.push(command.to_string());
        parts.extend(args.iter().cloned());
        shell_words::join(parts.iter().map(String::as_str))
    }
}

fn terminal_exit_status_from_code(code: i64) -> Option<CopilotTerminalExitStatus> {
    u32::try_from(code)
        .ok()
        .map(|exit_code| CopilotTerminalExitStatus {
            exit_code: Some(exit_code),
            signal: None,
        })
}

fn local_terminal_tool_status(status: &CopilotTerminalExitStatus) -> ToolCallStatus {
    match status.exit_code {
        Some(0) => ToolCallStatus::Completed,
        Some(_) | None => ToolCallStatus::Failed,
    }
}

fn update_local_terminal_output(
    state: &Arc<Mutex<LocalTerminalSessionState>>,
    chunk: &str,
) -> Option<(String, String, String)> {
    let mut state = lock_local_terminal_state(state);
    state.append_output(chunk);
    let association = state.association.clone()?;
    if !state.tool_started || chunk.trim().is_empty() {
        return None;
    }
    Some((
        association.tool_call_id,
        association.tool_name,
        state.output.clone(),
    ))
}

fn finalize_local_terminal_exit(
    state: &Arc<Mutex<LocalTerminalSessionState>>,
    exit_status: Option<CopilotTerminalExitStatus>,
) -> Option<(String, String, Value, String, ToolCallStatus)> {
    let mut state = lock_local_terminal_state(state);
    state.exit_status = exit_status;
    let association = state.association.clone()?;
    if state.tool_finished {
        return None;
    }
    let exit_status = state.exit_status.clone()?;
    state.tool_finished = true;
    Some((
        association.tool_call_id,
        association.tool_name,
        association.arguments,
        state.output.clone(),
        local_terminal_tool_status(&exit_status),
    ))
}

fn emit_terminal_output_event(
    emitter: Option<&HarnessEventEmitter>,
    harness_item_prefix: &str,
    tool_call_id: &str,
    tool_name: &str,
    output: &str,
) {
    let Some(emitter) = emitter else {
        return;
    };
    let item_id = harness_call_item_id(harness_item_prefix, tool_call_id, tool_name);
    let raw_tool_call_id = raw_tool_call_id(tool_call_id);
    let _ = emitter.emit(tool_updated_event(item_id, raw_tool_call_id, output));
}

fn emit_terminal_finished_event(
    emitter: Option<&HarnessEventEmitter>,
    harness_item_prefix: &str,
    tool_call_id: &str,
    tool_name: &str,
    arguments: &Value,
    status: ToolCallStatus,
    output: String,
) {
    let Some(emitter) = emitter else {
        return;
    };
    let item_id = harness_call_item_id(harness_item_prefix, tool_call_id, tool_name);
    let raw_tool_call_id = raw_tool_call_id(tool_call_id);
    let _ = emitter.emit(tool_invocation_completed_event(
        item_id.clone(),
        tool_name,
        Some(arguments),
        raw_tool_call_id,
        status.clone(),
    ));
    let _ = emitter.emit(tool_output_completed_event(
        item_id,
        raw_tool_call_id,
        status,
        None,
        None,
        output,
    ));
}

fn lock_local_terminal_state(
    state: &Arc<Mutex<LocalTerminalSessionState>>,
) -> MutexGuard<'_, LocalTerminalSessionState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn normalize_copilot_reasoning_delta(existing: &str, delta: String) -> String {
    let delta = collapse_reasoning_single_newlines(delta);
    if existing.is_empty()
        || existing.chars().last().is_some_and(char::is_whitespace)
        || delta.chars().next().is_some_and(char::is_whitespace)
        || delta
            .chars()
            .next()
            .is_some_and(is_reasoning_closing_punctuation)
    {
        delta
    } else {
        format!(" {delta}")
    }
}

fn collapse_reasoning_single_newlines(delta: String) -> String {
    let chars: Vec<char> = delta.chars().collect();
    let mut normalized = String::with_capacity(delta.len());

    for (index, ch) in chars.iter().copied().enumerate() {
        if ch != '\n' {
            normalized.push(ch);
            continue;
        }

        let prev = index.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
        let next = chars.get(index + 1).copied();

        if prev.is_some() && next.is_some() && prev != Some('\n') && next != Some('\n') {
            if next.is_some_and(char::is_whitespace) || prev.is_some_and(char::is_whitespace) {
                continue;
            }
            if next.is_some_and(is_reasoning_closing_punctuation) {
                continue;
            }
            normalized.push(' ');
            continue;
        }

        normalized.push('\n');
    }

    normalized
}

fn is_reasoning_closing_punctuation(ch: char) -> bool {
    matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}')
}

fn observed_tool_command_display(update: &CopilotObservedToolCall) -> Option<String> {
    observed_tool_command_from_args(update.arguments.as_ref()).or_else(|| {
        update
            .tool_name
            .strip_prefix("Run ")
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToString::to_string)
    })
}

fn observed_tool_command_from_args(arguments: Option<&Value>) -> Option<String> {
    let arguments = arguments?;
    for key in ["command", "cmd", "raw_command"] {
        let value = arguments.get(key)?;
        match value {
            Value::String(text) if !text.trim().is_empty() => return Some(text.to_string()),
            Value::Array(parts) => {
                let command = parts
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ");
                if !command.trim().is_empty() {
                    return Some(command);
                }
            }
            _ => {}
        }
    }
    None
}

fn observed_tool_output_delta<'a>(previous: Option<&str>, current: &'a str) -> Option<&'a str> {
    if current.is_empty() {
        return None;
    }

    match previous {
        None => Some(current),
        Some(previous) if previous == current => None,
        Some(previous) if current.starts_with(previous) => Some(&current[previous.len()..]),
        Some(previous) => {
            let prefix_len = common_prefix_len(previous, current);
            if prefix_len == 0 || prefix_len >= current.len() {
                Some(current)
            } else {
                Some(&current[prefix_len..])
            }
        }
    }
}

fn common_prefix_len(left: &str, right: &str) -> usize {
    let mut bytes = 0;
    for (left_char, right_char) in left.chars().zip(right.chars()) {
        if left_char != right_char {
            break;
        }
        bytes += left_char.len_utf8();
    }
    bytes
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
        map_copilot_finish_reason, normalize_copilot_reasoning_delta, summarize_permission_request,
    };
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Duration;
    use vtcode_core::copilot::{
        CopilotObservedToolCall, CopilotObservedToolCallStatus, CopilotPermissionRequest,
        CopilotTerminalCreateRequest, CopilotTerminalEnvVar, CopilotToolCallResponse,
    };
    use vtcode_core::core::decision_tracker::DecisionTracker;
    use vtcode_core::core::trajectory::TrajectoryLogger;
    use vtcode_core::llm::provider::{FinishReason, ToolDefinition};
    use vtcode_core::tools::registry::ToolRegistry;
    use vtcode_core::tools::{ApprovalRecorder, ToolResultCache};
    use vtcode_core::utils::ansi::AnsiRenderer;
    use vtcode_core::utils::transcript;
    use vtcode_tui::app::{InlineCommand, InlineHandle, InlineSession};

    use crate::agent::runloop::mcp_events::McpPanelState;
    use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
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

    fn collect_inline_output(
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<InlineCommand>,
    ) -> String {
        let mut lines: Vec<String> = Vec::new();
        while let Ok(command) = receiver.try_recv() {
            match command {
                InlineCommand::AppendLine { segments, .. } => {
                    lines.push(
                        segments
                            .into_iter()
                            .map(|segment| segment.text)
                            .collect::<String>(),
                    );
                }
                InlineCommand::ReplaceLast {
                    lines: replacement_lines,
                    ..
                } => {
                    for line in replacement_lines {
                        lines.push(
                            line.into_iter()
                                .map(|segment| segment.text)
                                .collect::<String>(),
                        );
                    }
                }
                _ => {}
            }
        }
        lines.join("\n")
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

    #[test]
    fn copilot_reasoning_delta_normalizes_chunk_boundaries() {
        assert_eq!(
            normalize_copilot_reasoning_delta(
                "The user wants me to run `cargo check` and report what I see.",
                "Running cargo check".to_string()
            ),
            " Running cargo check"
        );
        assert_eq!(
            normalize_copilot_reasoning_delta("prefix\n", "next".to_string()),
            "next"
        );
    }

    #[test]
    fn copilot_reasoning_delta_collapses_single_newlines_inside_chunk() {
        assert_eq!(
            normalize_copilot_reasoning_delta(
                "Run",
                " cargo fmt and report the\n results\n.\nRunning cargo fmt".to_string()
            ),
            " cargo fmt and report the results. Running cargo fmt"
        );
    }

    #[tokio::test]
    async fn observed_tool_calls_emit_incremental_output_updates() {
        let temp = TempDir::new().expect("temp workspace");
        let workspace = temp.path().to_path_buf();
        let harness_path = workspace.join("harness.jsonl");

        let mut tool_registry = ToolRegistry::new(workspace.clone()).await;
        let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
        let mut session = create_headless_session();
        let handle = session.clone_inline_handle();
        let approval_recorder = ApprovalRecorder::new(workspace.clone());
        let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
        let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
        let safety_validator = Arc::new(RwLock::new(ToolCallSafetyValidator::new()));
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());
        let traj = TrajectoryLogger::new(&workspace);
        let mut session_stats = SessionStats::default();
        let mut mcp_panel_state = McpPanelState::default();
        let mut harness_state = HarnessTurnState::new(
            TurnRunId("run-test".to_string()),
            TurnId("turn-test".to_string()),
            8,
            60,
            0,
        );
        let emitter = HarnessEventEmitter::new(harness_path.clone()).expect("harness emitter");

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
            None,
            true,
            Some(&emitter),
            "turn-test-step-1".to_string(),
        );

        runtime_host.handle_observed_tool_call(CopilotObservedToolCall {
            tool_call_id: "call_1".to_string(),
            tool_name: "Run cargo check on the workspace".to_string(),
            status: CopilotObservedToolCallStatus::InProgress,
            arguments: Some(json!({"command": "cargo check"})),
            output: Some("Compiling vtcode-core".to_string()),
            terminal_id: None,
        });
        runtime_host.handle_observed_tool_call(CopilotObservedToolCall {
            tool_call_id: "call_1".to_string(),
            tool_name: "Run cargo check on the workspace".to_string(),
            status: CopilotObservedToolCallStatus::Completed,
            arguments: Some(json!({"command": "cargo check"})),
            output: Some("Compiling vtcode-core\nFinished `dev` profile".to_string()),
            terminal_id: None,
        });

        let payload = std::fs::read_to_string(harness_path).expect("read harness log");
        let events: Vec<serde_json::Value> = payload
            .lines()
            .map(|line| serde_json::from_str(line).expect("parse harness event"))
            .collect();

        assert!(events.iter().any(|entry| {
            entry["event"]["type"] == "item.updated"
                && entry["event"]["item"]["type"] == "tool_output"
                && entry["event"]["item"]["output"] == "Compiling vtcode-core"
                && entry["event"]["item"]["status"] == "in_progress"
        }));
        assert!(events.iter().any(|entry| {
            entry["event"]["type"] == "item.completed"
                && entry["event"]["item"]["type"] == "tool_output"
                && entry["event"]["item"]["status"] == "completed"
                && entry["event"]["item"]["output"]
                    .as_str()
                    .is_some_and(|output| output.contains("Finished `dev` profile"))
        }));
    }

    #[tokio::test]
    async fn observed_shell_tool_calls_stream_into_inline_pty_ui() {
        let temp = TempDir::new().expect("temp workspace");
        let workspace = temp.path().to_path_buf();

        let mut tool_registry = ToolRegistry::new(workspace.clone()).await;
        let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
        let (command_tx, mut command_rx) = unbounded_channel();
        let (_event_tx, event_rx) = unbounded_channel();
        let mut session = InlineSession {
            handle: InlineHandle::new_for_tests(command_tx),
            events: event_rx,
        };
        let handle = session.clone_inline_handle();
        let approval_recorder = ApprovalRecorder::new(workspace.clone());
        let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
        let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
        let safety_validator = Arc::new(RwLock::new(ToolCallSafetyValidator::new()));
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());
        let traj = TrajectoryLogger::new(&workspace);
        let mut session_stats = SessionStats::default();
        let mut mcp_panel_state = McpPanelState::default();
        let mut harness_state = HarnessTurnState::new(
            TurnRunId("run-test".to_string()),
            TurnId("turn-test".to_string()),
            8,
            60,
            0,
        );

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
            None,
            true,
            None,
            "turn-test-step-1".to_string(),
        );

        runtime_host.handle_observed_tool_call(CopilotObservedToolCall {
            tool_call_id: "call_shell".to_string(),
            tool_name: "Run cargo check on workspace".to_string(),
            status: CopilotObservedToolCallStatus::InProgress,
            arguments: Some(json!({
                "command": "cd /tmp && cargo check 2>&1",
                "description": "Run cargo check on workspace",
                "mode": "sync",
            })),
            output: Some("Checking vtcode-core\n".to_string()),
            terminal_id: None,
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        runtime_host.handle_observed_tool_call(CopilotObservedToolCall {
            tool_call_id: "call_shell".to_string(),
            tool_name: "Run cargo check on workspace".to_string(),
            status: CopilotObservedToolCallStatus::Completed,
            arguments: Some(json!({
                "command": "cd /tmp && cargo check 2>&1",
                "description": "Run cargo check on workspace",
                "mode": "sync",
            })),
            output: Some("Checking vtcode-core\nFinished `dev` profile\n".to_string()),
            terminal_id: None,
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let inline_output = collect_inline_output(&mut command_rx);
        assert!(inline_output.contains("• Ran cd /tmp && cargo check 2>&1"));
        assert!(inline_output.contains("Checking vtcode-core"));
        assert!(inline_output.contains("Finished `dev` profile"));
    }

    #[tokio::test]
    async fn copilot_terminal_sessions_bind_local_pty_output_and_release_cleanly() {
        let temp = TempDir::new().expect("temp workspace");
        let workspace = temp.path().to_path_buf();
        let harness_path = workspace.join("harness-terminal.jsonl");

        let mut tool_registry = ToolRegistry::new(workspace.clone()).await;
        let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
        let mut session = create_headless_session();
        let handle = session.clone_inline_handle();
        let approval_recorder = ApprovalRecorder::new(workspace.clone());
        let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
        let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
        let safety_validator = Arc::new(RwLock::new(ToolCallSafetyValidator::new()));
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());
        let traj = TrajectoryLogger::new(&workspace);
        let mut session_stats = SessionStats::default();
        let mut mcp_panel_state = McpPanelState::default();
        let mut harness_state = HarnessTurnState::new(
            TurnRunId("run-test".to_string()),
            TurnId("turn-test".to_string()),
            8,
            60,
            0,
        );
        let emitter = HarnessEventEmitter::new(harness_path.clone()).expect("harness emitter");

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
            None,
            true,
            Some(&emitter),
            "turn-test-step-1".to_string(),
        );

        let response = runtime_host
            .handle_terminal_create(CopilotTerminalCreateRequest {
                session_id: "session-terminal".to_string(),
                command: "/bin/sh".to_string(),
                args: vec!["-lc".to_string(), "printf \"$ACP_TEST_TOKEN\"".to_string()],
                env: vec![CopilotTerminalEnvVar {
                    name: "ACP_TEST_TOKEN".to_string(),
                    value: "vtcode-terminal".to_string(),
                }],
                cwd: None,
                output_byte_limit: Some(1024),
            })
            .await
            .expect("create local terminal");

        let exit_status = runtime_host
            .handle_terminal_wait_for_exit(&response.terminal_id)
            .await
            .expect("wait for local terminal exit");
        assert_eq!(exit_status.exit_code, Some(0));

        let output = runtime_host
            .handle_terminal_output(&response.terminal_id)
            .await
            .expect("read local terminal output");
        assert_eq!(output.output, "vtcode-terminal");
        assert!(!output.truncated);
        assert_eq!(output.exit_status, Some(exit_status.clone()));

        runtime_host.handle_observed_tool_call(CopilotObservedToolCall {
            tool_call_id: "call_terminal".to_string(),
            tool_name: "Run cargo check on the workspace".to_string(),
            status: CopilotObservedToolCallStatus::InProgress,
            arguments: Some(json!({"command": "cargo check"})),
            output: Some("ignored remote output".to_string()),
            terminal_id: Some(response.terminal_id.clone()),
        });

        let payload = std::fs::read_to_string(&harness_path).expect("read harness log");
        let events: Vec<serde_json::Value> = payload
            .lines()
            .map(|line| serde_json::from_str(line).expect("parse harness event"))
            .collect();

        assert!(events.iter().any(|entry| {
            entry["event"]["type"] == "item.started"
                && entry["event"]["item"]["type"] == "tool_invocation"
                && entry["event"]["item"]["tool_name"] == "Run cargo check on the workspace"
        }));
        assert!(events.iter().any(|entry| {
            entry["event"]["type"] == "item.updated"
                && entry["event"]["item"]["type"] == "tool_output"
                && entry["event"]["item"]["output"] == "vtcode-terminal"
        }));
        assert!(events.iter().any(|entry| {
            entry["event"]["type"] == "item.completed"
                && entry["event"]["item"]["type"] == "tool_output"
                && entry["event"]["item"]["status"] == "completed"
                && entry["event"]["item"]["output"] == "vtcode-terminal"
        }));

        runtime_host
            .handle_terminal_release(&response.terminal_id)
            .await
            .expect("release local terminal");
        assert!(
            runtime_host
                .handle_terminal_output(&response.terminal_id)
                .await
                .is_err()
        );
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
