use crate::agent::runloop::unified::inline_events::harness::{
    HarnessEventEmitter, tool_completed_event,
};
use vtcode_core::exec::events::CommandExecutionStatus;

use super::status::ToolExecutionStatus;

pub(super) fn emit_tool_completion_status(
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_started_emitted: bool,
    tool_item_id: &str,
    tool_name: &str,
    status: CommandExecutionStatus,
) {
    if !tool_started_emitted {
        return;
    }

    if let Some(emitter) = harness_emitter {
        let _ = emitter.emit(tool_completed_event(
            tool_item_id.to_string(),
            tool_name,
            status,
            None,
        ));
    }
}

pub(super) fn emit_tool_completion_for_status(
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_started_emitted: bool,
    tool_item_id: &str,
    tool_name: &str,
    tool_status: &ToolExecutionStatus,
) {
    let status = if matches!(tool_status, ToolExecutionStatus::Success { .. }) {
        CommandExecutionStatus::Completed
    } else {
        CommandExecutionStatus::Failed
    };
    emit_tool_completion_status(
        harness_emitter,
        tool_started_emitted,
        tool_item_id,
        tool_name,
        status,
    );
}
