//! Outcome recording, invocation lifecycle, and halt policy for tool execution.

use super::super::AgentRunner;
use super::super::tool_access::PreparedToolExecution;
use super::super::tool_types::ToolCallItemRef;
use crate::core::agent::events::{ExecEventRecorder, tool_invocation_completed_event, tool_output_payload_from_value};
use crate::core::agent::harness_kernel::{project_model_tool_result, reduce_tool_result};
use crate::core::agent::runtime::AgentRuntime;
use crate::exec::events::AppSandboxDeniedEvent;
use crate::exec::events::ThreadEvent;
use crate::exec::events::ToolCallStatus;
use crate::exec::events::ToolOutcome;
use crate::exec::events::tool_outcome_from_status;
use std::sync::Arc;
use tracing::info;
use vtcode_commons::ErrorCategory;

pub(super) fn record_tool_execution_observation(
    state: &mut crate::core::agent::session::AgentSessionState,
    tool_name: &str,
    execution: &PreparedToolExecution,
) {
    state
        .turn_tool_observations
        .push(crate::core::agent::session::ToolExecutionObservation {
            tool_name: tool_name.to_string(),
            attempts: execution.attempts,
            duration_ms: execution.total_duration.as_millis().min(u128::from(u64::MAX)) as u64,
            error_category: execution.last_error_category,
        });
}

pub(super) fn snapshot_circuit_diagnostics(
    runner: &AgentRunner,
    tool_name: &str,
) -> Option<crate::tools::circuit_breaker::ToolCircuitDiagnostics> {
    runner
        .tool_registry
        .shared_circuit_breaker()
        .map(|breaker| breaker.get_diagnostics(tool_name))
}

pub(super) fn record_circuit_transition(
    runner: &AgentRunner,
    error_recovery: &Arc<parking_lot::Mutex<crate::core::agent::error_recovery::ErrorRecoveryState>>,
    tool_name: &str,
    before: Option<crate::tools::circuit_breaker::ToolCircuitDiagnostics>,
) {
    let Some(before) = before else {
        return;
    };
    let Some(breaker) = runner.tool_registry.shared_circuit_breaker() else {
        return;
    };
    error_recovery
        .lock()
        .record_circuit_transition_from_snapshot(&breaker, tool_name, &before);
}

pub(super) fn resolve_tool_call_item(
    runtime: &AgentRuntime,
    event_recorder: &mut ExecEventRecorder,
    tool_name: &str,
    args: &serde_json::Value,
    tool_call_id: &str,
) -> ToolCallItemRef {
    if let Some(call_item_id) = runtime.tool_call_item_id(tool_call_id) {
        return ToolCallItemRef { call_item_id, synthetic_invocation: false };
    }

    let handle = event_recorder.tool_started(tool_name, Some(args), Some(tool_call_id));
    ToolCallItemRef {
        call_item_id: handle.item_id().to_string(),
        synthetic_invocation: true,
    }
}

pub(super) fn complete_tool_invocation(
    runtime: &mut AgentRuntime,
    event_recorder: &mut ExecEventRecorder,
    tool_call_id: &str,
    tool_name: &str,
    args: &serde_json::Value,
    tool_call_item: &ToolCallItemRef,
    status: ToolCallStatus,
    outcome: Option<ToolOutcome>,
) {
    if tool_call_item.synthetic_invocation {
        let resolved_outcome = outcome.unwrap_or_else(|| tool_outcome_from_status(&status));
        event_recorder.record_thread_event(tool_invocation_completed_event(
            tool_call_item.call_item_id.clone(),
            tool_name,
            Some(args),
            Some(tool_call_id),
            status,
            resolved_outcome,
        ));
        return;
    }

    runtime.complete_tool_call(tool_call_id, status, outcome);
    super::super::tool_dispatch_common::drain_and_record_runtime_events(runtime, event_recorder);
}

/// Build the sandbox-denied event for `off`-tier refused app URIs, if any.
///
/// Pure constructor so the emission decision stays unit-testable; the caller
/// owns recorder access.
fn app_sandbox_denied_event(tool_name: &str, suppressed_apps: &[String]) -> Option<ThreadEvent> {
    if suppressed_apps.is_empty() {
        return None;
    }
    Some(ThreadEvent::AppSandboxDenied(Box::new(AppSandboxDeniedEvent {
        tool: tool_name.to_owned(),
        tier: "off".to_owned(),
        uris: suppressed_apps.to_vec(),
    })))
}

pub(super) fn record_successful_tool_result(
    event_recorder: &mut ExecEventRecorder,
    call_item_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    args: &serde_json::Value,
    output: &serde_json::Value,
) {
    let payload = tool_output_payload_from_value(output);
    if let Some(denied) = app_sandbox_denied_event(tool_name, &payload.suppressed_apps) {
        event_recorder.record_thread_event(denied);
    }
    event_recorder.tool_output_finished(
        call_item_id,
        Some(tool_call_id),
        ToolCallStatus::Completed,
        output
            .get("exit_code")
            .and_then(serde_json::Value::as_i64)
            .and_then(|code| i32::try_from(code).ok()),
        &payload.aggregated_output,
        payload.spool_path.as_deref(),
        payload.app_previews,
    );
    event_recorder.record_exec_session_output(call_item_id, tool_name, args, output);
    if let Some(event) = crate::core::agent::events::file_change_completed_event(call_item_id, tool_name, args, output)
    {
        event_recorder.record_thread_event(event);
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)] // internal pipeline function, all params needed
pub(super) fn apply_tool_success(
    runner: &AgentRunner,
    runtime: &mut AgentRuntime,
    event_recorder: &mut ExecEventRecorder,
    agent_prefix: &str,
    call_id: &str,
    name: &str,
    args: &serde_json::Value,
    tool_call_item: &ToolCallItemRef,
    result: serde_json::Value,
    is_gemini: bool,
) {
    if !runner.quiet {
        info!(agent = %agent_prefix, tool = %name, "Tool executed successfully");
    }
    let optimized_result = reduce_tool_result(name, result);
    runner.update_last_paths_from_args(name, args, &mut runtime.state);
    // Strip TUI-only display fields (e.g. task_tracker `view`) before the
    // result enters model context. The TUI event below still uses
    // `optimized_result` (with `view`) so display rendering is unaffected.
    let llm_result = project_model_tool_result(name, args, &optimized_result);
    runtime
        .state
        .push_tool_result(call_id.to_string(), name, &llm_result, is_gemini);
    complete_tool_invocation(
        runtime,
        event_recorder,
        call_id,
        name,
        args,
        tool_call_item,
        ToolCallStatus::Completed,
        None,
    );
    record_successful_tool_result(event_recorder, &tool_call_item.call_item_id, call_id, name, args, &optimized_result);
}

/// The outcome of evaluating whether a tool failure should halt further tool
/// calls in the current turn.
#[must_use]
#[derive(Debug, Clone, PartialEq)]
enum ToolHaltDecision {
    /// Continue executing remaining tool calls normally.
    Continue,
    /// Stop dispatching further tool calls and surface a warning.
    Halt {
        /// Human-readable reason appended to session warnings.
        warning: &'static str,
        /// Whether the session's tool-loop-limit counter should be incremented.
        mark_loop_limit: bool,
    },
}

/// Pure classification function: maps an `ErrorCategory` to a halt decision
/// without touching any mutable state. This is the single place that encodes
/// which error categories should abort the current tool-call sequence.
#[inline]
fn classify_halt_decision(category: ErrorCategory) -> ToolHaltDecision {
    match category {
        ErrorCategory::RateLimit => ToolHaltDecision::Halt {
            warning: "Tool was rate limited; halting further tool calls this turn.",
            mark_loop_limit: true,
        },
        ErrorCategory::PolicyViolation | ErrorCategory::PlanningPolicyViolation => ToolHaltDecision::Halt {
            warning: "Tool denied by policy; halting further tool calls this turn.",
            mark_loop_limit: false,
        },
        _ => ToolHaltDecision::Continue,
    }
}

pub(super) fn apply_tool_failure_halt_policy(
    session_state: &mut crate::core::agent::session::AgentSessionState,
    category: ErrorCategory,
) -> bool {
    match classify_halt_decision(category) {
        ToolHaltDecision::Continue => false,
        ToolHaltDecision::Halt { warning, mark_loop_limit } => {
            session_state.push_warning(warning);
            if mark_loop_limit {
                session_state.mark_tool_loop_limit_hit();
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::events::{ThreadEvent, ThreadItemDetails};
    use serde_json::json;

    #[test]
    fn suppressed_apps_emit_sandbox_denied_event() {
        let mut recorder = ExecEventRecorder::new("thread-denied", None, None);
        recorder.take_events();
        record_successful_tool_result(
            &mut recorder,
            "item-1",
            "call-1",
            "mcp::demo::get_time",
            &json!({}),
            &json!({"output": "12:00", "uiSuppressed": ["ui://demo/app.html", "ui://demo/other.html"]}),
        );

        let denied = recorder.take_events().into_iter().find_map(|event| match event {
            ThreadEvent::AppSandboxDenied(denied) => Some(denied),
            _ => None,
        });
        let denied = denied.expect("suppressed apps must emit a sandbox-denied event");
        assert_eq!(denied.tool, "mcp::demo::get_time");
        assert_eq!(denied.tier, "off");
        assert_eq!(denied.uris, ["ui://demo/app.html", "ui://demo/other.html"]);
    }

    #[test]
    fn results_without_suppressed_apps_emit_no_denied_event() {
        assert!(app_sandbox_denied_event("mcp::demo::get_time", &[]).is_none());

        let mut recorder = ExecEventRecorder::new("thread-clean", None, None);
        recorder.take_events();
        record_successful_tool_result(
            &mut recorder,
            "item-1",
            "call-1",
            "mcp::demo::get_time",
            &json!({}),
            &json!({"output": "12:00"}),
        );
        assert!(
            recorder
                .take_events()
                .iter()
                .all(|event| !matches!(event, ThreadEvent::AppSandboxDenied(_))),
            "plain results must stay silent"
        );
    }

    #[test]
    fn successful_result_records_output_before_file_change() {
        let mut recorder = ExecEventRecorder::new("thread-result", None, None);
        recorder.take_events();
        let args = json!({"path":"a.rs", "content":"new"});
        let output = json!({"success":true,"output":"updated a.rs","diff":[{
            "path":"a.rs","operation":"updated", "content":"--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new",
            "additions":1,"deletions":1
        }]});
        record_successful_tool_result(&mut recorder, "invocation", "call-write", "write_file", &args, &output);
        let events = recorder.take_events();
        assert_eq!(events.len(), 2);
        let ThreadEvent::ItemCompleted(completed_output) = &events[0] else {
            panic!("completed output expected")
        };
        let ThreadItemDetails::ToolOutput(result) = &completed_output.item.details else {
            panic!("tool output expected")
        };
        assert_eq!(result.call_id, "invocation");
        assert_eq!(result.tool_call_id.as_deref(), Some("call-write"));
        assert_eq!(
            result.output,
            "diff preview for a.rs (updated) (+1 -1):\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\nupdated a.rs"
        );
        assert_eq!(result.status, ToolCallStatus::Completed);
        let ThreadEvent::ItemCompleted(completed_change) = &events[1] else {
            panic!("completed change expected")
        };
        let ThreadItemDetails::FileChange(change) = &completed_change.item.details else {
            panic!("file change expected")
        };
        assert_eq!(change.changes.len(), 1);
        assert_eq!(change.changes[0].path, "a.rs");
        assert_eq!(change.additions, Some(1));
        assert_eq!(change.deletions, Some(1));
    }
}
