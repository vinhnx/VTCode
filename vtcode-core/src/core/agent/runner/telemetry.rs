use super::AgentRunner;
use super::constants::ROLE_USER;
use super::types::ToolFailureContext;
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::session::AgentSessionState;
use crate::exec::events::ToolCallStatus;
use crate::llm::providers::gemini::wire::{Content, Part};
use crate::tools::registry::ToolExecutionError;
use crate::utils::colors::style;
use serde::Serialize;

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
    pub(super) fn user_facing_tool_error_message(
        &self,
        _tool_name: &str,
        error: &ToolExecutionError,
    ) -> String {
        error.user_message()
    }

    pub(super) fn observability_fields(
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
    pub(super) fn record_warning(
        &self,
        agent_prefix: &str,
        session_state: &mut AgentSessionState,
        event_recorder: &mut ExecEventRecorder,
        warning_message: impl Into<String>,
    ) {
        let warning_message = warning_message.into();
        if !self.quiet {
            println!(
                "{} {} {}",
                agent_prefix,
                style("(WARN)").red().bold(),
                warning_message
            );
        }
        event_recorder.warning(&warning_message);
        session_state.warnings.push(warning_message);
    }

    pub(super) fn record_tool_failure(
        &self,
        failure_ctx: &mut ToolFailureContext<'_>,
        tool_name: &str,
        error: &ToolExecutionError,
        tool_response_id: Option<&str>,
    ) {
        let failure_text = self.user_facing_tool_error_message(tool_name, error);
        if !self.quiet {
            println!(
                "{} {} {}",
                failure_ctx.agent_prefix,
                style("(ERR)").red().bold(),
                failure_text
            );
        }
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
        failure_ctx.event_recorder.warning(&failure_text);
        // Move failure_text into warnings first, then reference for conversation
        failure_ctx
            .session_state
            .warnings
            .push(failure_text.clone());

        if let Some(call_id) = tool_response_id {
            failure_ctx.session_state.push_tool_error(
                call_id.to_string(),
                tool_name,
                error.to_json_value().to_string(),
                failure_ctx.is_gemini,
            );
        } else {
            // Fallback for when we don't have a call_id (should be rare in Codex-style)
            failure_ctx.session_state.conversation.push(Content {
                role: ROLE_USER.into(),
                parts: vec![Part::Text {
                    text: failure_text,
                    thought_signature: None,
                }],
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
