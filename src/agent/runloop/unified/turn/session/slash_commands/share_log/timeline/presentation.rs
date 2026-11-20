//! Timeline presentation helpers for share-log exports.

#![allow(
    clippy::let_underscore_must_use,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]

use std::fmt::Write as _;

use super::{
    REDACTION_NOTICE, SUMMARY_PREVIEW_LIMIT, TIMELINE_SOURCE_CONVERSATION_FALLBACK, TIMELINE_SOURCE_THREAD_EVENTS,
    TimelineExport, TimelineRow,
};
use anyhow::{Context, Result};
use serde_json::Value;
use vtcode_core::core::threads::ThreadEventRecord;
use vtcode_core::exec::events::{ThreadEvent, ThreadItem, ThreadItemDetails, ToolCallStatus, ToolOutcome, wire_type};

pub(super) fn timeline_rows_from_thread_events(records: &[ThreadEventRecord]) -> Vec<TimelineRow> {
    records
        .iter()
        .map(|record| match &record.event {
            ThreadEvent::MatrixUpdated(snapshot) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "matrix.updated",
                None,
                "matrix",
                Some(match snapshot.lifecycle {
                    vtcode_core::exec::events::matrix::MatrixLifecycle::Succeeded => "completed",
                    vtcode_core::exec::events::matrix::MatrixLifecycle::Cancelled => "cancelled",
                    vtcode_core::exec::events::matrix::MatrixLifecycle::Blocked => "failed",
                    vtcode_core::exec::events::matrix::MatrixLifecycle::Created => "pending",
                    vtcode_core::exec::events::matrix::MatrixLifecycle::Paused => "paused",
                    _ => "in_progress",
                }),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                format!("Matrix {}: {:?}", snapshot.spec.id, snapshot.lifecycle),
                String::new(),
                String::new(),
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::TrackerUpdated(snapshot) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "tracker.updated",
                None,
                "tracker",
                Some(if snapshot.total == 0 {
                    "pending"
                } else if snapshot.completed >= snapshot.total {
                    "completed"
                } else {
                    "in_progress"
                }),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                format!("Tracker {}: {}/{}", snapshot.title, snapshot.completed, snapshot.total),
                String::new(),
                String::new(),
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::GoalUpdated(event) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "goal.updated",
                None,
                "goal",
                Some(match &event.goal {
                    Some(snapshot) => match snapshot.state {
                        vtcode_core::exec::events::goal::GoalState::Active => "in_progress",
                        vtcode_core::exec::events::goal::GoalState::Paused => "paused",
                        vtcode_core::exec::events::goal::GoalState::Complete => "completed",
                        vtcode_core::exec::events::goal::GoalState::Blocked
                        | vtcode_core::exec::events::goal::GoalState::BudgetLimited => "failed",
                        vtcode_core::exec::events::goal::GoalState::Unknown => "pending",
                    },
                    None => "cancelled",
                }),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                match &event.goal {
                    Some(snapshot) => format!("Goal {}: {}", snapshot.state.as_str(), snapshot.objective),
                    None => "Goal cleared".to_string(),
                },
                String::new(),
                String::new(),
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::ThreadStarted(event) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "thread.started",
                None,
                "thread",
                Some("in_progress"),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                "Thread started".to_string(),
                truncate_preview(&event.thread_id, SUMMARY_PREVIEW_LIMIT),
                event.thread_id.clone(),
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::ThreadCompleted(event) => {
                let status = thread_completion_status(&event.subtype);
                let mut body = String::new();
                let _ = writeln!(&mut body, "Outcome: {}", event.outcome_code);
                let _ = writeln!(&mut body, "Subtype: {}", event.subtype.as_str());
                if let Some(stop_reason) = &event.stop_reason {
                    let _ = writeln!(&mut body, "Stop reason: {stop_reason}");
                }
                if let Some(total_cost_usd) = &event.total_cost_usd {
                    let _ = writeln!(&mut body, "Total cost (USD): {total_cost_usd}");
                }
                let _ = writeln!(&mut body, "Turns: {}", event.num_turns);
                let usage_summary = format_usage_summary(&event.usage);
                let _ = writeln!(&mut body, "Usage: {usage_summary}");
                if let Some(result) = &event.result {
                    append_text_section(&mut body, "Result", result);
                }

                timeline_row(
                    record.sequence,
                    TIMELINE_SOURCE_THREAD_EVENTS,
                    "thread.completed",
                    None,
                    "thread",
                    Some(status),
                    record.turn_id.as_deref(),
                    record.submission_id.as_ref().map(|value| value.as_str()),
                    "Thread completed".to_string(),
                    format!("{} ({})", event.outcome_code, status),
                    body,
                    pretty_json_string(&record.event),
                    false,
                )
            }
            ThreadEvent::ThreadCompactBoundary(event) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "thread.compact_boundary",
                None,
                "thread",
                Some("completed"),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                "Compaction boundary".to_string(),
                format!(
                    "{} -> {} messages ({}/{})",
                    event.original_message_count,
                    event.compacted_message_count,
                    event.trigger.as_str(),
                    event.mode.as_str()
                ),
                {
                    let mut body = String::new();
                    let _ = writeln!(&mut body, "Trigger: {}", event.trigger.as_str());
                    let _ = writeln!(&mut body, "Mode: {}", event.mode.as_str());
                    let _ = writeln!(
                        &mut body,
                        "Messages: {} -> {}",
                        event.original_message_count, event.compacted_message_count
                    );
                    if let Some(path) = &event.history_artifact_path {
                        let _ = writeln!(&mut body, "History artifact: {path}");
                    }
                    body
                },
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::ContextReset(event) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                record.event.wire_name(),
                Some("context_reset"),
                "context",
                Some("completed"),
                Some(&event.turn_id),
                record.submission_id.as_ref().map(|value| value.as_str()),
                "Fresh execution context started".to_string(),
                format!("{}% used before reset", event.previous_context_usage_percent),
                format!("Plan preserved: {}\nTool budget reset: {}", event.plan_preserved, event.tool_budget_reset),
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::TurnStarted(_) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                record.event.wire_name(),
                None,
                "turn",
                Some("in_progress"),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                "Turn started".to_string(),
                record.turn_id.clone().unwrap_or_else(|| "turn started".to_string()),
                String::new(),
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::TurnCompleted(event) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                record.event.wire_name(),
                None,
                "turn",
                Some("completed"),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                "Turn completed".to_string(),
                format_usage_summary(&event.usage),
                format!("Usage: {}", format_usage_summary(&event.usage)),
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::TurnFailed(event) => {
                let mut body = event.message.clone();
                if let Some(usage) = &event.usage {
                    let _ = write!(&mut body, "\n\nUsage: {}", format_usage_summary(usage));
                }
                timeline_row(
                    record.sequence,
                    TIMELINE_SOURCE_THREAD_EVENTS,
                    "turn.failed",
                    None,
                    "turn",
                    Some("failed"),
                    record.turn_id.as_deref(),
                    record.submission_id.as_ref().map(|value| value.as_str()),
                    "Turn failed".to_string(),
                    truncate_preview(&event.message, SUMMARY_PREVIEW_LIMIT),
                    body,
                    pretty_json_string(&record.event),
                    false,
                )
            }
            ThreadEvent::TurnBlocked(event) => {
                let mut body = format!(
                    "{}\n\nBlocked streak: {} total: {} caps: {}/{} recovery: {}",
                    event.message,
                    event.blocked_streak,
                    event.blocked_total,
                    event.consecutive_cap,
                    event.total_cap,
                    event.recovery_active
                );
                if let Some(usage) = &event.usage {
                    let _ = write!(&mut body, "\n\nUsage: {}", format_usage_summary(usage));
                }
                timeline_row(
                    record.sequence,
                    TIMELINE_SOURCE_THREAD_EVENTS,
                    "turn.blocked",
                    None,
                    "turn",
                    Some("failed"),
                    record.turn_id.as_deref(),
                    record.submission_id.as_ref().map(|value| value.as_str()),
                    "Turn blocked".to_string(),
                    truncate_preview(&event.message, SUMMARY_PREVIEW_LIMIT),
                    body,
                    pretty_json_string(&record.event),
                    false,
                )
            }
            ThreadEvent::AppSandboxDenied(event) => {
                let body = format!("tool: {}\ntier: {}\nrefused:\n{}", event.tool, event.tier, event.uris.join("\n"));
                timeline_row(
                    record.sequence,
                    TIMELINE_SOURCE_THREAD_EVENTS,
                    "app.sandbox_denied",
                    None,
                    "app",
                    Some("failed"),
                    record.turn_id.as_deref(),
                    record.submission_id.as_ref().map(|value| value.as_str()),
                    format!("App UI refused for {}", event.tool),
                    truncate_preview(&body, SUMMARY_PREVIEW_LIMIT),
                    body,
                    pretty_json_string(&record.event),
                    false,
                )
            }
            ThreadEvent::ItemStarted(event) => {
                timeline_row_from_item(record, record.event.wire_name(), "in_progress", &event.item)
            }
            ThreadEvent::ItemUpdated(event) => {
                timeline_row_from_item(record, record.event.wire_name(), "in_progress", &event.item)
            }
            ThreadEvent::ItemCompleted(event) => {
                timeline_row_from_item(record, record.event.wire_name(), "completed", &event.item)
            }
            ThreadEvent::PlanDelta(event) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "plan.delta",
                Some("plan_delta"),
                "plan",
                Some("in_progress"),
                Some(&event.turn_id),
                record.submission_id.as_ref().map(|value| value.as_str()),
                "Plan update".to_string(),
                truncate_preview(&event.delta, SUMMARY_PREVIEW_LIMIT),
                event.delta.clone(),
                pretty_json_string(&record.event),
                true,
            ),
            ThreadEvent::PlanApprovalRequested(event) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "plan.approval.requested",
                Some("plan_approval"),
                "plan",
                Some("in_progress"),
                Some(&event.turn_id),
                record.submission_id.as_ref().map(|value| value.as_str()),
                "Plan approval requested".to_string(),
                event.plan_file.clone().unwrap_or_else(|| "Awaiting approval".to_string()),
                event.plan_file.clone().unwrap_or_default(),
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::PlanApprovalResolved(event) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "plan.approval.resolved",
                Some("plan_approval"),
                "plan",
                Some("completed"),
                Some(&event.turn_id),
                record.submission_id.as_ref().map(|value| value.as_str()),
                format!("Plan approval resolved: {:?}", event.decision),
                format!("automatic={}", event.automatic),
                format!("Decision: {:?}\nAutomatic: {}", event.decision, event.automatic),
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::Error(event) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "error",
                Some("error"),
                "error",
                Some("failed"),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                "Error".to_string(),
                truncate_preview(&event.message, SUMMARY_PREVIEW_LIMIT),
                event.message.clone(),
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::Unknown => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "unknown",
                None,
                "unknown",
                None,
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                "Unknown event".to_string(),
                String::new(),
                String::new(),
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::PermissionRequested(event) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "permission.requested",
                None,
                "permission",
                Some("in_progress"),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                "Permission requested".to_string(),
                truncate_preview(&event.tool_name, SUMMARY_PREVIEW_LIMIT),
                event.tool_name.clone(),
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::PermissionResolved(event) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "permission.resolved",
                None,
                "permission",
                Some("completed"),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                format!("Permission resolved: {:?}", event.decision),
                truncate_preview(&event.tool_name, SUMMARY_PREVIEW_LIMIT),
                event.tool_name.clone(),
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::Interjected(event) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "interjected",
                None,
                "interjection",
                Some("in_progress"),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                format!("Interjected ({:?}, {} images)", event.source, event.image_count),
                String::new(),
                String::new(),
                pretty_json_string(&record.event),
                false,
            ),
        })
        .collect()
}

pub(super) fn timeline_row_from_item(
    record: &ThreadEventRecord,
    event_type: &str,
    default_status: &str,
    item: &ThreadItem,
) -> TimelineRow {
    let detail_json = pretty_json_string(&record.event);
    match &item.details {
        ThreadItemDetails::Decision(d) => timeline_row(
            record.sequence,
            TIMELINE_SOURCE_THREAD_EVENTS,
            event_type,
            Some("decision"),
            "decision",
            Some(default_status),
            record.turn_id.as_deref(),
            record.submission_id.as_ref().map(|v| v.as_str()),
            d.summary.clone(),
            "Agent-reported public rationale".into(),
            d.rationale.clone(),
            detail_json,
            false,
        ),
        ThreadItemDetails::AgentMessage(message) => timeline_row(
            record.sequence,
            TIMELINE_SOURCE_THREAD_EVENTS,
            event_type,
            Some("agent_message"),
            "message",
            Some(default_status),
            record.turn_id.as_deref(),
            record.submission_id.as_ref().map(|value| value.as_str()),
            "Agent message".to_string(),
            truncate_preview(&message.text, SUMMARY_PREVIEW_LIMIT),
            message.text.clone(),
            detail_json,
            false,
        ),
        ThreadItemDetails::Plan(plan) => timeline_row(
            record.sequence,
            TIMELINE_SOURCE_THREAD_EVENTS,
            event_type,
            Some("plan"),
            "plan",
            Some(default_status),
            record.turn_id.as_deref(),
            record.submission_id.as_ref().map(|value| value.as_str()),
            "Plan".to_string(),
            truncate_preview(&plan.text, SUMMARY_PREVIEW_LIMIT),
            plan.text.clone(),
            detail_json,
            false,
        ),
        ThreadItemDetails::Reasoning(reasoning) => timeline_row(
            record.sequence,
            TIMELINE_SOURCE_THREAD_EVENTS,
            event_type,
            Some("reasoning"),
            "reasoning",
            Some(default_status),
            record.turn_id.as_deref(),
            record.submission_id.as_ref().map(|value| value.as_str()),
            reasoning
                .stage
                .as_deref()
                .map(|stage| format!("Reasoning ({stage})"))
                .unwrap_or_else(|| "Reasoning".to_string()),
            truncate_preview(&reasoning.text, SUMMARY_PREVIEW_LIMIT),
            reasoning.text.clone(),
            detail_json,
            true,
        ),
        ThreadItemDetails::CommandExecution(command) => {
            let status = command_status_label(&command.status);
            let mut body = String::new();
            if let Some(arguments) = &command.arguments {
                append_json_section(&mut body, "Arguments", arguments);
            }
            append_text_section(&mut body, "Output", &command.aggregated_output);
            if body.is_empty() {
                body = command.command.clone();
            }
            timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                event_type,
                Some("command_execution"),
                "command",
                Some(status),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                command.command.clone(),
                summarize_status_with_exit(status, command.exit_code),
                body,
                detail_json,
                event_type == wire_type::ITEM_UPDATED,
            )
        }
        ThreadItemDetails::ToolInvocation(tool) => {
            let status = if tool.status == ToolCallStatus::Failed && tool.outcome == Some(ToolOutcome::Cancelled) {
                "cancelled"
            } else {
                tool_status_label(&tool.status)
            };
            let tool_name = super::super::canonical_tool_name(&tool.tool_name);
            let mut body = String::new();
            if let Some(arguments) = &tool.arguments {
                append_json_section(&mut body, "Arguments", arguments);
            }
            if let Some(tool_call_id) = &tool.tool_call_id {
                let _ = write!(&mut body, "Tool call id: {tool_call_id}");
            }
            timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                event_type,
                Some("tool_invocation"),
                "tool",
                Some(status),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                tool_name,
                summarize_status_with_label(status, tool.tool_call_id.as_deref()),
                body,
                detail_json,
                event_type == wire_type::ITEM_UPDATED,
            )
        }
        ThreadItemDetails::ToolOutput(tool_output) => {
            let status = if tool_output.status == ToolCallStatus::Failed
                && tool_output.exit_code.is_none()
                && tool_output.spool_path.is_none()
                && tool_output.output
                    == crate::agent::runloop::unified::turn::turn_loop::UNDISPATCHED_TOOL_CALL_CLOSURE_TEXT
            {
                "not_executed"
            } else {
                tool_status_label(&tool_output.status)
            };
            let mut body = String::new();
            if let Some(tool_call_id) = &tool_output.tool_call_id {
                let _ = writeln!(&mut body, "Tool call id: {tool_call_id}");
            }
            if let Some(spool_path) = &tool_output.spool_path {
                let _ = writeln!(&mut body, "Spool path: {spool_path}");
            }
            append_text_section(&mut body, "Output", &tool_output.output);
            if body.trim().is_empty() {
                body = format!("Call id: {}", tool_output.call_id);
            }
            timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                event_type,
                Some("tool_output"),
                "tool",
                Some(status),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                "Tool output".to_string(),
                summarize_status_with_exit(status, tool_output.exit_code),
                body,
                detail_json,
                event_type == wire_type::ITEM_UPDATED,
            )
        }
        ThreadItemDetails::FileChange(file_change) => timeline_row(
            record.sequence,
            TIMELINE_SOURCE_THREAD_EVENTS,
            event_type,
            Some("file_change"),
            "file_change",
            Some(patch_status_label(&file_change.status)),
            record.turn_id.as_deref(),
            record.submission_id.as_ref().map(|value| value.as_str()),
            "File change".to_string(),
            format!("{} file(s) ({})", file_change.changes.len(), patch_status_label(&file_change.status)),
            file_change
                .changes
                .iter()
                .map(|change| format!("{:?}: {}", change.kind, change.path))
                .collect::<Vec<_>>()
                .join("\n"),
            detail_json,
            false,
        ),
        ThreadItemDetails::McpToolCall(tool_call) => {
            let status = mcp_status_label(tool_call.status.as_ref()).unwrap_or(default_status);
            let mut body = String::new();
            if let Some(arguments) = &tool_call.arguments {
                append_json_section(&mut body, "Arguments", arguments);
            }
            if let Some(result) = &tool_call.result {
                append_text_section(&mut body, "Result", result);
            }
            timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                event_type,
                Some("mcp_tool_call"),
                "mcp",
                Some(status),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                tool_call.tool_name.clone(),
                summarize_status_with_label(status, None),
                body,
                detail_json,
                event_type == wire_type::ITEM_UPDATED,
            )
        }
        ThreadItemDetails::WebSearch(search) => {
            let mut body = String::new();
            let _ = writeln!(&mut body, "Query: {}", search.query);
            if let Some(provider) = &search.provider {
                let _ = writeln!(&mut body, "Provider: {provider}");
            }
            if let Some(results) = &search.results {
                let _ = writeln!(&mut body, "Results: {}", results.len());
                append_text_section(&mut body, "Top results", &results.join("\n"));
            }
            timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                event_type,
                Some("web_search"),
                "web_search",
                Some(default_status),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                "Web search".to_string(),
                truncate_preview(&search.query, SUMMARY_PREVIEW_LIMIT),
                body,
                detail_json,
                false,
            )
        }
        ThreadItemDetails::Harness(event) => timeline_row(
            record.sequence,
            TIMELINE_SOURCE_THREAD_EVENTS,
            event_type,
            Some("harness"),
            "harness",
            Some(event.status.as_deref().unwrap_or_else(|| harness_status_label(&event.event))),
            record.turn_id.as_deref(),
            record.submission_id.as_ref().map(|value| value.as_str()),
            harness_title(&event.event).to_string(),
            truncate_preview(&harness_summary(event), SUMMARY_PREVIEW_LIMIT),
            harness_body(event),
            detail_json,
            false,
        ),
        ThreadItemDetails::Error(error) => timeline_row(
            record.sequence,
            TIMELINE_SOURCE_THREAD_EVENTS,
            event_type,
            Some("error"),
            "error",
            Some("failed"),
            record.turn_id.as_deref(),
            record.submission_id.as_ref().map(|value| value.as_str()),
            "Warning".to_string(),
            truncate_preview(&error.message, SUMMARY_PREVIEW_LIMIT),
            error.message.clone(),
            detail_json,
            false,
        ),
    }
}

pub(super) fn timeline_rows_from_messages(messages: &[Value]) -> Vec<TimelineRow> {
    let mut rows = Vec::with_capacity(messages.len());
    let mut sequence = 1_u64;

    for message in messages {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("Unknown");
        let content = message.get("content").and_then(Value::as_str).unwrap_or("");
        let tool_call_id = message.get("tool_call_id").and_then(Value::as_str);
        let role_lower = role.to_ascii_lowercase();

        if tool_call_id.is_some() || role_lower.contains("tool") {
            rows.push(timeline_row(
                sequence,
                TIMELINE_SOURCE_CONVERSATION_FALLBACK,
                "conversation.tool_output",
                Some("tool_output"),
                "tool",
                Some("completed"),
                None,
                None,
                "Tool output".to_string(),
                tool_call_id
                    .map(|value| format!("tool call id: {value}"))
                    .unwrap_or_else(|| truncate_preview(content, SUMMARY_PREVIEW_LIMIT)),
                content.to_string(),
                pretty_json_value(message),
                false,
            ));
            sequence += 1;
        } else {
            rows.push(timeline_row(
                sequence,
                TIMELINE_SOURCE_CONVERSATION_FALLBACK,
                "conversation.message",
                Some("message"),
                "message",
                Some("completed"),
                None,
                None,
                format!("{role} message"),
                truncate_preview(content, SUMMARY_PREVIEW_LIMIT),
                content.to_string(),
                pretty_json_value(message),
                false,
            ));
            sequence += 1;
        }

        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
            for tool_call in tool_calls {
                let tool_name = tool_call
                    .get("function")
                    .and_then(|value| value.get("name"))
                    .and_then(Value::as_str)
                    .map(super::super::canonical_tool_name)
                    .unwrap_or_else(|| "unknown".to_string());
                let tool_call_id = tool_call.get("id").and_then(Value::as_str);
                let mut body = String::new();
                if let Some(arguments) = tool_call.get("function").and_then(|value| value.get("arguments")) {
                    append_json_section(&mut body, "Arguments", arguments);
                }
                rows.push(timeline_row(
                    sequence,
                    TIMELINE_SOURCE_CONVERSATION_FALLBACK,
                    "conversation.tool_call",
                    Some("tool_invocation"),
                    "tool",
                    Some("completed"),
                    None,
                    None,
                    tool_name,
                    tool_call_id
                        .map(|value| format!("tool call id: {value}"))
                        .unwrap_or_else(|| "tool invocation".to_string()),
                    body,
                    pretty_json_value(tool_call),
                    false,
                ));
                sequence += 1;
            }
        }
    }

    rows
}

#[path = "presentation/export.rs"]
mod export;
pub(super) use export::render_session_timeline_html;

use super::formatting::{
    append_json_section, append_text_section, command_status_label, escape_html, format_usage_summary, harness_body,
    harness_status_label, harness_summary, harness_title, mcp_status_label, patch_status_label, pretty_json_string,
    pretty_json_value, sanitize_json_for_script_tag, summarize_status_with_exit, summarize_status_with_label,
    thread_completion_status, timeline_row, tool_status_label, truncate_preview,
};
