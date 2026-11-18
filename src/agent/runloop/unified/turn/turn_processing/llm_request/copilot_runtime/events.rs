use super::*;

impl<'a> CopilotRuntimeHost<'a> {
    pub(super) fn emit_tool_started_event(&self, tool_call_id: &str, tool_name: &str, arguments: &Value) {
        let Some(emitter) = self.harness_emitter else {
            return;
        };
        let item_id = harness_call_item_id(&self.harness_item_prefix, tool_call_id, tool_name);
        let raw_id = raw_tool_call_id(tool_call_id);
        let _ = emitter.emit(tool_started_event(item_id.clone(), tool_name, Some(arguments), raw_id));
        let _ = emitter.emit(tool_output_started_event(item_id, raw_id));
    }

    pub(super) fn emit_tool_finished_event(
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
        let _ = emitter.emit(tool_output_completed_event(
            item_id,
            raw_id,
            status,
            None,
            None,
            output.unwrap_or_default(),
            Vec::new(),
        ));
    }

    pub(super) fn emit_tool_output_event(&self, tool_call_id: &str, tool_name: &str, output: &str) {
        let Some(emitter) = self.harness_emitter else {
            return;
        };
        let item_id = harness_call_item_id(&self.harness_item_prefix, tool_call_id, tool_name);
        let _ = emitter.emit(tool_updated_event(item_id, raw_tool_call_id(tool_call_id), output));
    }
}
