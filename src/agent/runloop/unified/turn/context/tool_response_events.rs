use super::TurnProcessingContext;
use vtcode_core::core::agent::events::{ToolOutputPayload, tool_output_payload_from_value};
use vtcode_core::exec::events::ToolCallStatus;

struct ToolCallCompletion {
    status: ToolCallStatus,
    exit_code: Option<i32>,
    output: ToolOutputPayload,
}

impl TurnProcessingContext<'_> {
    /// Emit the harness item events that make a rejected/blocked tool call
    /// visible in the session log as a completed `tool_output` item carrying
    /// the rejection text.
    ///
    /// Rejections decided before the pipeline (safety/policy guards, the
    /// anti-blind-editing gate, the blocked-call fuse) never execute, so
    /// without this the log shows either a dangling `item.started` (when the
    /// LLM runtime streamed the call) or no item at all. Mirrors the
    /// pipeline's item identity: complete the streamed item when one exists,
    /// otherwise replay the started pair with the pipeline's fallback item id.
    pub(crate) fn emit_rejected_tool_call_item(
        &mut self,
        tool_call_id: &str,
        tool_name: Option<&str>,
        args: Option<&serde_json::Value>,
        rejection_text: &str,
    ) {
        self.emit_pre_execution_tool_call_item(
            tool_call_id,
            tool_name,
            args,
            ToolCallCompletion {
                status: ToolCallStatus::Failed,
                exit_code: None,
                output: ToolOutputPayload {
                    aggregated_output: rejection_text.to_owned(),
                    spool_path: None,
                },
            },
        );
    }

    pub(crate) fn push_reused_tool_response(
        &mut self,
        tool_call_id: &str,
        tool_name: &str,
        args: &serde_json::Value,
        content: String,
    ) {
        let output = serde_json::from_str::<serde_json::Value>(&content)
            .unwrap_or_else(|_| serde_json::Value::String(content.clone()));
        let exit_code = output
            .get("exit_code")
            .and_then(serde_json::Value::as_i64)
            .and_then(|code| i32::try_from(code).ok());
        let status = if exit_code.is_some_and(|code| code != 0)
            && !crate::agent::runloop::unified::turn::tool_outcomes::is_grep_style_no_match(tool_name, args, &output)
        {
            ToolCallStatus::Failed
        } else {
            ToolCallStatus::Completed
        };
        self.emit_pre_execution_tool_call_item(
            tool_call_id,
            Some(tool_name),
            Some(args),
            ToolCallCompletion {
                status,
                exit_code,
                output: tool_output_payload_from_value(&output),
            },
        );
        self.push_tool_response(tool_call_id, Some(tool_name), content);
    }

    fn emit_pre_execution_tool_call_item(
        &mut self,
        tool_call_id: &str,
        tool_name: Option<&str>,
        args: Option<&serde_json::Value>,
        completion: ToolCallCompletion,
    ) {
        let ToolCallCompletion { status, exit_code, output } = completion;
        use vtcode_core::core::agent::events::{
            tool_invocation_completed_event, tool_output_completed_event, tool_output_started_event, tool_started_event,
        };
        use vtcode_core::exec::events::tool_outcome_from_status;

        let streamed = self.harness_state.take_streamed_tool_call_item_id(tool_call_id);
        let Some(emitter) = self.harness_emitter else {
            return;
        };
        let streamed_item_id = streamed.as_ref().map(|item| item.item_id.clone());
        let item_id = streamed_item_id.clone().unwrap_or_else(|| {
            crate::agent::runloop::unified::tool_pipeline::resolve_harness_item_identity(tool_call_id).1
        });
        let raw_id = (!tool_call_id.trim().is_empty()).then_some(tool_call_id);
        let tool_label = tool_name
            .map(str::to_string)
            .or_else(|| streamed.map(|item| item.tool_name))
            .unwrap_or_default();
        if streamed_item_id.is_none() {
            let _ = emitter.emit(tool_started_event(item_id.clone(), &tool_label, args, raw_id));
            let _ = emitter.emit(tool_output_started_event(item_id.clone(), raw_id));
        }
        let _ = emitter.emit(tool_invocation_completed_event(
            item_id.clone(),
            &tool_label,
            args,
            raw_id,
            status.clone(),
            tool_outcome_from_status(&status),
        ));
        let _ = emitter.emit(tool_output_completed_event(
            item_id,
            raw_id,
            status,
            exit_code,
            output.spool_path.as_deref(),
            output.aggregated_output,
        ));
    }
}

#[cfg(test)]
mod tests;
