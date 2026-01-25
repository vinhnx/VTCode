use super::AgentRunner;
use super::constants::ROLE_USER;
use super::types::ToolFailureContext;
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::state::TaskRunState;
use crate::exec::events::CommandExecutionStatus;
use crate::gemini::{Content, Part};
use crate::utils::colors::style;
use crate::utils::error_messages::ERR_TOOL_DENIED;

impl AgentRunner {
    pub(super) fn record_warning(
        &self,
        agent_prefix: &str,
        task_state: &mut TaskRunState,
        event_recorder: &mut ExecEventRecorder,
        warning_message: impl Into<String>,
    ) {
        let warning_message = warning_message.into();
        if !self.quiet {
            println!(
                "{} {} {}",
                agent_prefix,
                style("(WARN)").yellow().bold(),
                warning_message
            );
        }
        event_recorder.warning(&warning_message);
        task_state.warnings.push(warning_message);
    }

    pub(super) fn record_tool_failure(
        &self,
        failure_ctx: &mut ToolFailureContext<'_>,
        tool_name: &str,
        error: &anyhow::Error,
        tool_response_id: Option<&str>,
    ) {
        let failure_text = format!("Tool {} failed: {}", tool_name, error);
        if !self.quiet {
            println!(
                "{} {} {}",
                failure_ctx.agent_prefix,
                style("(ERR)").red().bold(),
                failure_text
            );
        }
        failure_ctx.event_recorder.command_finished(
            failure_ctx.command_event,
            CommandExecutionStatus::Failed,
            None,
            &failure_text,
        );
        failure_ctx.event_recorder.warning(&failure_text);
        // Move failure_text into warnings first, then reference for conversation
        failure_ctx.task_state.warnings.push(failure_text.clone());

        if let Some(call_id) = tool_response_id {
            failure_ctx.task_state.push_tool_error(
                call_id.to_string(),
                tool_name,
                failure_text,
                failure_ctx.is_gemini,
            );
        } else {
            // Fallback for when we don't have a call_id (should be rare in Codex-style)
            failure_ctx.task_state.conversation.push(Content {
                role: ROLE_USER.into(),
                parts: vec![Part::Text {
                    text: failure_text,
                    thought_signature: None,
                }],
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn record_tool_denied(
        &self,
        agent_prefix: &str,
        task_state: &mut TaskRunState,
        event_recorder: &mut ExecEventRecorder,
        call_id: &str,
        tool_name: &str,
        command_event: Option<&crate::core::agent::events::ActiveCommandHandle>,
        is_gemini: bool,
    ) {
        let detail = format!("{ERR_TOOL_DENIED}: {tool_name}");
        if !self.quiet {
            println!(
                "{} {} {}",
                agent_prefix,
                style("(WARN)").yellow().bold(),
                detail
            );
        }
        task_state.warnings.push(detail.clone());

        task_state.push_tool_error(call_id.to_string(), tool_name, detail.clone(), is_gemini);

        if let Some(event) = command_event {
            event_recorder.command_finished(event, CommandExecutionStatus::Failed, None, &detail);
        } else {
            event_recorder.warning(&detail);
        }
    }
}
