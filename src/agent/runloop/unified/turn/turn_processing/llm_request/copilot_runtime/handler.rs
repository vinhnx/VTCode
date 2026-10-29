use super::*;

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
                self.harness_state.record_requested_tool_calls(1);
                let admitted_before = self.harness_state.admitted_tool_call_count();
                let response = self
                    .handle_vtcode_tool_call(renderer, request_event.request.clone())
                    .await
                    .map_err(|error| {
                        self.harness_state.record_failed_tool_call();
                        map_runtime_error(error)
                    })?;
                let response = append_session_limit_grant_guidance(
                    response,
                    self.harness_state.take_session_limit_grant_directive_pending(),
                );
                let budget_rejected = self.harness_state.take_tool_budget_rejection();
                if self.harness_state.admitted_tool_call_count() == admitted_before
                    && matches!(&response, CopilotToolCallResponse::Failure(_))
                    && !budget_rejected
                {
                    self.harness_state.record_denied_tool_call();
                }
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
