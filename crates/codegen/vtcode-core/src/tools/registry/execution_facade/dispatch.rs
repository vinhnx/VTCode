use super::*;

impl ToolRegistry {
    pub(super) fn annotate_timeout_error_payload(
        payload: &mut Value,
        timeout_category: &str,
        timeout_ms: u64,
        circuit_breaker: bool,
    ) {
        if let Some(obj) = payload.get_mut("error").and_then(|value| value.as_object_mut()) {
            obj.insert("timeout_category".into(), Value::String(timeout_category.to_string()));
            obj.insert("timeout_ms".into(), Value::from(timeout_ms));
            obj.insert("circuit_breaker".into(), Value::Bool(circuit_breaker));
        }
    }

    pub fn safety_gateway(&self) -> std::sync::Arc<crate::tools::safety_gateway::SafetyGateway> {
        std::sync::Arc::clone(&self.safety_gateway)
    }

    /// Inline-delegating wrapper that returns the inner future directly to
    /// avoid an extra coroutine state machine (audit section 16).
    pub fn execute_public_tool_request(
        &self,
        request: ToolExecutionRequest,
    ) -> impl Future<Output = ToolExecutionOutcome> + '_ {
        self.execute_tool_request_internal(request, None)
    }

    /// Execute with the caller's remaining wall-clock budget. Retry waits must
    /// fit this deadline; the caller still owns cancellation and timeout delivery.
    pub fn execute_public_tool_request_with_deadline(
        &self,
        request: ToolExecutionRequest,
        deadline: Instant,
    ) -> impl Future<Output = ToolExecutionOutcome> + '_ {
        self.execute_tool_request_internal(request, Some(deadline))
    }

    pub async fn execute_prepared_public_tool_request(
        &self,
        prepared: &PreparedToolCall,
        policy: ExecutionPolicySnapshot,
    ) -> ToolExecutionOutcome {
        self.execute_prepared_public_tool_request_until(prepared, policy, None).await
    }

    /// Keep the caller's outer deadline available to retry admission as well as
    /// timeout enforcement. Prepared calls still require fresh safety admission.
    pub(crate) async fn execute_prepared_public_tool_request_until(
        &self,
        prepared: &PreparedToolCall,
        policy: ExecutionPolicySnapshot,
        deadline: Option<Instant>,
    ) -> ToolExecutionOutcome {
        let request = ToolExecutionRequest::new(prepared.canonical_name.clone(), prepared.effective_args.clone())
            .with_policy(
                policy
                    .with_prevalidated(prepared.already_preflighted)
                    .with_safety_prevalidated(false),
            );
        self.execute_tool_request_internal(request, deadline).await
    }

    pub(super) async fn should_skip_loop_detection_for_exec_continuation(&self, tool_name: &str, args: &Value) -> bool {
        if tool_name == tools::WRITE_STDIN {
            return matches!(
                crate::tools::command_args::write_stdin_dispatch(args),
                Ok(crate::tools::command_args::WriteStdinDispatch::Poll
                    | crate::tools::command_args::WriteStdinDispatch::Wait,)
            );
        }

        if tool_name != tools::UNIFIED_EXEC {
            return false;
        }

        if !tool_intent::command_session_action_in(args, &["poll", "continue"]) {
            return false;
        }
        if tool_intent::command_session_action_is(args, "continue")
            && crate::tools::command_args::interactive_input_text(args).is_some()
        {
            return false;
        }

        let Some(session_id) = crate::tools::command_args::session_id_text(args) else {
            return false;
        };

        matches!(self.exec_session_completed(session_id).await, Ok(None))
    }

    pub(super) async fn public_tool_catalog_for_error(&self, requested_name: &str) -> (Vec<String>, Vec<String>) {
        let mut tool_names = self.available_tools().await;
        tool_names.sort_unstable();
        tool_names.dedup();

        let requested_candidates = public_tool_name_candidates(requested_name);
        let mut similar_tools = Vec::new();

        if let Ok(resolved) = self.resolve_public_tool_name_sync(requested_name)
            && tool_names.iter().any(|tool| tool == &resolved)
        {
            similar_tools.push(resolved);
        }

        super::super::assembly::push_similar_tools(&mut similar_tools, &requested_candidates, &tool_names);

        (tool_names, similar_tools)
    }

    pub fn preflight_validate_call(&self, name: &str, args: &Value) -> Result<super::super::ToolPreflightOutcome> {
        execution_kernel::preflight_validate_call(self, name, args)
    }

    /// Preflight a tool call from the harness path, allowing dispatch to
    /// internal (model-hidden) tool registrations such as read_file/write_file.
    /// Direct model-originated entry should use `preflight_validate_call`.
    pub fn preflight_validate_harness_call(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<super::super::ToolPreflightOutcome> {
        execution_kernel::preflight_validate_call_with_mode(self, name, args, execution_kernel::DispatchMode::Harness)
    }

    pub fn admit_public_tool_call(&self, name: &str, args: &Value) -> Result<PreparedToolCall> {
        let preflight = self.preflight_validate_harness_call(name, args)?;
        Ok(PreparedToolCall::new(
            preflight.normalized_tool_name,
            preflight.readonly_classification,
            preflight.parallel_safe_after_preflight,
            preflight.effective_args,
        ))
    }

    pub async fn execute_tool(&self, name: &str, args: Value) -> Result<Value> {
        self.execute_tool_ref(name, &args).await
    }

    /// Execute a model-originated tool call through the public routing assembly.
    pub async fn execute_public_tool_ref(&self, name: &str, args: &Value) -> Result<Value> {
        self.execute_public_tool_ref_internal(name, args, false).await
    }

    /// Reference-taking version of execute_tool to avoid cloning by callers
    /// that already have access to an existing `Value`.
    pub async fn execute_tool_ref(&self, name: &str, args: &Value) -> Result<Value> {
        self.execute_tool_ref_internal(name, args, false, ExecSettlementMode::Manual)
            .await
    }

    /// Reference-taking execution entrypoint for calls that were already preflight-validated.
    ///
    /// This avoids re-running argument/schema/path/command preflight in hot paths
    /// where validation already happened in the runloop.
    pub async fn execute_tool_ref_prevalidated(&self, name: &str, args: &Value) -> Result<Value> {
        self.execute_tool_ref_internal(name, args, true, ExecSettlementMode::Manual)
            .await
    }

    /// Prevalidated model-originated execution that still routes through the public assembly.
    pub async fn execute_public_tool_ref_prevalidated(&self, name: &str, args: &Value) -> Result<Value> {
        self.execute_public_tool_ref_prevalidated_with_mode(name, args, ExecSettlementMode::Manual)
            .await
    }

    #[doc(hidden)]
    pub async fn execute_public_tool_ref_prevalidated_with_mode(
        &self,
        name: &str,
        args: &Value,
        exec_settlement_mode: ExecSettlementMode,
    ) -> Result<Value> {
        self.execute_public_tool_ref_internal_with_mode(name, args, true, exec_settlement_mode)
            .await
    }

    pub async fn execute_prepared_public_tool_ref_with_mode(
        &self,
        prepared: &PreparedToolCall,
        exec_settlement_mode: ExecSettlementMode,
    ) -> Result<Value> {
        // Prepared calls come from the harness admission gate
        // (`admit_public_tool_call`), which is the only producer of
        // `PreparedToolCall`, so internal (model-hidden) dispatch is authorized.
        self.execute_public_tool_ref_dispatch(
            prepared.canonical_name.as_str(),
            &prepared.effective_args,
            prepared.already_preflighted,
            execution_kernel::DispatchMode::Harness,
            exec_settlement_mode,
        )
        .await
    }

    async fn execute_public_tool_ref_internal(&self, name: &str, args: &Value, prevalidated: bool) -> Result<Value> {
        self.execute_public_tool_ref_dispatch(
            name,
            args,
            prevalidated,
            execution_kernel::DispatchMode::ModelPublic,
            ExecSettlementMode::Manual,
        )
        .await
    }

    async fn execute_public_tool_ref_internal_with_mode(
        &self,
        name: &str,
        args: &Value,
        prevalidated: bool,
        exec_settlement_mode: ExecSettlementMode,
    ) -> Result<Value> {
        self.execute_public_tool_ref_dispatch(
            name,
            args,
            prevalidated,
            execution_kernel::DispatchMode::ModelPublic,
            exec_settlement_mode,
        )
        .await
    }

    /// Core public-routing execution entrypoint.
    ///
    /// `dispatch_mode` is the authority to fall back to internal (model-hidden)
    /// tool registrations and is fully independent of `prevalidated` (a pure
    /// performance flag that skips re-running preflight). Only callers that went
    /// through the harness admission gate (`admit_public_tool_call`) pass
    /// [`execution_kernel::DispatchMode::Harness`]; direct model-originated entry always passes
    /// [`execution_kernel::DispatchMode::ModelPublic`], so a stray `prevalidated=true` can never by
    /// itself widen the dispatchable surface.
    pub(in crate::tools::registry) async fn execute_public_tool_ref_dispatch(
        &self,
        name: &str,
        args: &Value,
        prevalidated: bool,
        dispatch_mode: execution_kernel::DispatchMode,
        exec_settlement_mode: ExecSettlementMode,
    ) -> Result<Value> {
        let routed_name = execution_kernel::resolve_dispatch_target(self, name, dispatch_mode)
            .map_err(|err| anyhow!(err.to_string()))?;
        let effective_args = execution_kernel::remap_public_file_operation_alias_args(name, routed_name.as_str(), args)
            .or_else(|| execution_kernel::remap_consolidated_action_alias_args(name, routed_name.as_str(), args));
        self.execute_tool_ref_internal(
            routed_name.as_str(),
            effective_args.as_ref().unwrap_or(args),
            prevalidated,
            exec_settlement_mode,
        )
        .await
    }
}
