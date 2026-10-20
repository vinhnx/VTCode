use super::AgentRunner;
use crate::core::agent::events::ExecEventRecorder;
use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity};

impl AgentRunner {
    #[cold]
    pub(super) fn warn_on_empty_response(
        &self,
        agent_prefix: &str,
        response_content: &str,
        has_tool_calls: bool,
        recorder: &mut ExecEventRecorder,
    ) {
        if response_content.trim().is_empty() && !has_tool_calls {
            self.record_diagnostic(recorder, &Diagnostic {
                severity: DiagnosticSeverity::Warning,
                code: "provider.empty_response".into(),
                summary: "The provider returned no content or tool calls. Review Details before retrying.".into(),
                agent_details: serde_json::json!({"agent": agent_prefix, "provider": self.provider_client.name(), "model": self.model, "classification": "empty_content"}).to_string(),
                correlation_id: format!("provider-empty:{}:{agent_prefix}", self.session_id),
                actions: Vec::new(),
            });
        }
    }
}
