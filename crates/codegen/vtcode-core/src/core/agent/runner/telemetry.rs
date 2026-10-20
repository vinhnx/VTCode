use super::AgentRunner;
use super::constants::ROLE_USER;
use super::types::ToolFailureContext;
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::session::AgentSessionState;
use crate::exec::events::ToolCallStatus;
use crate::llm::providers::gemini::wire::{Content, Part};
use crate::tools::registry::ToolExecutionError;
use serde::Serialize;
use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity, redact_diagnostic};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct ObservabilityFields {
    provider: String,
    model: String,
    turn: usize,
    tool_count: usize,
    parallelized: bool,
    compaction_mode: &'static str,
    grounded_fact_count: usize,
    previous_response_chain_present: bool,
}

impl AgentRunner {
    fn observability_fields(
        &self,
        model: &str,
        turn: usize,
        tool_count: usize,
        parallelized: bool,
        compaction_mode: &'static str,
        grounded_fact_count: usize,
        previous_response_chain_present: bool,
    ) -> ObservabilityFields {
        ObservabilityFields {
            provider: self.provider_client.name().to_string(),
            model: model.to_string(),
            turn,
            tool_count,
            parallelized,
            compaction_mode,
            grounded_fact_count,
            previous_response_chain_present,
        }
    }

    pub(super) fn emit_tool_batch(
        &self,
        model: &str,
        turn: usize,
        tool_count: usize,
        parallelized: bool,
        previous_response_chain_present: bool,
    ) {
        let fields = self.observability_fields(
            model,
            turn,
            tool_count,
            parallelized,
            "none",
            0,
            previous_response_chain_present,
        );
        tracing::info!(
            provider = %fields.provider,
            model = %fields.model,
            turn = fields.turn,
            tool_count = fields.tool_count,
            parallelized = fields.parallelized,
            compaction_mode = fields.compaction_mode,
            grounded_fact_count = fields.grounded_fact_count,
            previous_response_chain_present = fields.previous_response_chain_present,
            "Tool batch execution started"
        );
    }
}

impl AgentRunner {
    /// Record through the active task recorder so context and result events stay aligned.
    pub(super) fn record_diagnostic(&self, recorder: &mut ExecEventRecorder, diagnostic: &Diagnostic) {
        let diagnostic = redact_diagnostic(diagnostic);
        recorder.diagnostic(&diagnostic);
        if !self.quiet {
            vtcode_commons::diagnostics::stderr_diagnostic(&diagnostic);
        }
    }

    /// Retry transitions outside an individual task attempt still reach the canonical thread.
    pub(super) fn emit_diagnostic(&self, diagnostic: &Diagnostic) {
        use crate::exec::events::{ItemUpdatedEvent, ReasoningItem, ThreadEvent, ThreadItem, ThreadItemDetails};
        let diagnostic = redact_diagnostic(diagnostic);
        let event = ThreadEvent::ItemUpdated(ItemUpdatedEvent {
            item: ThreadItem {
                context: None,
                id: diagnostic.correlation_id.clone(),
                details: ThreadItemDetails::Reasoning(Box::new(ReasoningItem {
                    text: diagnostic.summary.clone(),
                    stage: Some("diagnosis".into()),
                    diagnostic: Some(Box::new(diagnostic.clone())),
                })),
            },
        });
        if let Some(sink) = &self.event_sink {
            let mut callback = sink.lock();
            callback(&event);
        }
        self.thread_handle.record_event(None, None, event);
        if !self.quiet {
            vtcode_commons::diagnostics::stderr_diagnostic(&diagnostic);
        }
    }

    #[track_caller]
    pub(super) fn record_warning(
        &self,
        agent_prefix: &str,
        session_state: &mut AgentSessionState,
        event_recorder: &mut ExecEventRecorder,
        warning_message: impl Into<String>,
    ) {
        let warning_message = warning_message.into();
        let mut diagnostic =
            vtcode_commons::diagnostics::legacy_ui_diagnostic(DiagnosticSeverity::Warning, &warning_message);
        diagnostic.code = "agent.warning".into();
        diagnostic.agent_details = format!("Agent: {agent_prefix}\n\n{}", diagnostic.agent_details);
        let diagnostic = redact_diagnostic(&diagnostic);
        self.record_diagnostic(event_recorder, &diagnostic);
        session_state.push_warning(diagnostic.text());
    }

    pub(super) fn record_tool_failure(
        &self,
        failure_ctx: &mut ToolFailureContext<'_>,
        tool_name: &str,
        error: &ToolExecutionError,
        tool_response_id: Option<&str>,
    ) {
        let failure_text = vtcode_commons::sanitizer::redact_secrets(error.user_message());
        let mut diagnostic = vtcode_commons::diagnostics::legacy_diagnostic(
            DiagnosticSeverity::Error,
            &failure_text,
            format!("tool:{}", failure_ctx.tool_call_id),
        );
        diagnostic.code = "tool.execution_failed".into();
        diagnostic.agent_details = vtcode_commons::sanitizer::sanitize_provider_diagnostic(
            serde_json::json!({
                "agent": failure_ctx.agent_prefix, "tool": tool_name, "error": error.to_json_value(),
            })
            .to_string(),
        );
        self.record_diagnostic(failure_ctx.event_recorder, &diagnostic);
        if let Some(call_item_id) = failure_ctx.call_item_id {
            failure_ctx.event_recorder.tool_output_finished(
                call_item_id,
                Some(failure_ctx.tool_call_id),
                ToolCallStatus::Failed,
                None,
                &failure_text,
                None,
            );
        }
        // Move failure_text into warnings first, then reference for conversation
        failure_ctx.session_state.push_warning(failure_text.clone());

        if let Some(call_id) = tool_response_id {
            failure_ctx.session_state.push_tool_error(
                call_id.to_string(),
                tool_name,
                &error.to_json_value(),
                failure_ctx.is_gemini,
            );
        } else {
            // Fallback for when we don't have a call_id (should be rare in Codex-style)
            failure_ctx.session_state.conversation.push(Content {
                role: ROLE_USER.into(),
                parts: vec![Part::Text { text: failure_text, thought_signature: None }],
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ObservabilityFields;

    #[test]
    fn observability_fields_capture_standard_shape() {
        let fields = ObservabilityFields {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            turn: 4,
            tool_count: 2,
            parallelized: true,
            compaction_mode: "local",
            grounded_fact_count: 3,
            previous_response_chain_present: true,
        };

        let serialized = serde_json::to_value(&fields).expect("serialize fields");
        assert_eq!(serialized["provider"], "openai");
        assert_eq!(serialized["model"], "gpt-5");
        assert_eq!(serialized["turn"], 4);
        assert_eq!(serialized["tool_count"], 2);
        assert_eq!(serialized["parallelized"], true);
        assert_eq!(serialized["compaction_mode"], "local");
        assert_eq!(serialized["grounded_fact_count"], 3);
        assert_eq!(serialized["previous_response_chain_present"], true);
    }
}
