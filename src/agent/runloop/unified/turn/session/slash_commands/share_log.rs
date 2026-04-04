use std::fmt::Write as _;

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Value, json};
use vtcode_core::core::threads::ThreadEventRecord;
use vtcode_core::exec::events::{
    CommandExecutionStatus, HarnessEventKind, McpToolCallStatus, PatchApplyStatus, PatchChangeKind,
    ThreadCompletionSubtype, ThreadEvent, ThreadItem, ThreadItemDetails, ToolCallStatus, Usage,
};
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::file_utils::write_file_with_context_sync;

use crate::agent::runloop::slash_commands::SessionLogExportFormat;

use super::{SlashCommandContext, SlashCommandControl};

const TIMELINE_SOURCE_THREAD_EVENTS: &str = "thread_events";
const TIMELINE_SOURCE_CONVERSATION_FALLBACK: &str = "conversation_fallback";
const SUMMARY_PREVIEW_LIMIT: usize = 120;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct TimelineExport {
    exported_at: String,
    model: String,
    provider: String,
    workspace: String,
    thread_id: String,
    source: String,
    total_rows: usize,
    overview: SessionOverview,
    rows: Vec<TimelineRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct SessionOverview {
    provider: String,
    model: String,
    api_calls: u64,
    turns: u64,
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    cache_creation_tokens: u64,
    total_tokens: u64,
    added_files: u64,
    updated_files: u64,
    deleted_files: u64,
    total_file_changes: u64,
    source: String,
    total_rows: usize,
    outcome_code: Option<String>,
    total_cost_usd: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct TimelineRow {
    sequence: u64,
    source: String,
    event_type: String,
    item_type: Option<String>,
    category: String,
    status: Option<String>,
    turn_id: Option<String>,
    submission_id: Option<String>,
    title: String,
    summary: String,
    body: String,
    detail_json: Option<String>,
    is_low_signal: bool,
}

fn build_session_log_messages(messages: &[uni::Message]) -> Vec<Value> {
    messages
        .iter()
        .map(|msg| {
            let mut entry = json!({
                "role": format!("{:?}", msg.role),
                "content": msg.content.as_text(),
            });
            if let Some(tool_calls) = &msg.tool_calls {
                let calls: Vec<Value> = tool_calls
                    .iter()
                    .map(|tc| {
                        json!({
                            "id": tc.id,
                            "function": tc.function.as_ref().map(|f| json!({
                                "name": f.name,
                                "arguments": f.arguments,
                            })),
                        })
                    })
                    .collect();
                entry["tool_calls"] = Value::Array(calls);
            }
            if let Some(tool_call_id) = &msg.tool_call_id {
                entry["tool_call_id"] = Value::String(tool_call_id.clone());
            }
            entry
        })
        .collect()
}

fn render_session_log_markdown(
    exported_at: &str,
    model: &str,
    workspace: &std::path::Path,
    messages: &[Value],
) -> String {
    let mut markdown = String::new();
    markdown.push_str("# VT Code Session Log\n\n");
    markdown.push_str(&format!("- Exported at: {}\n", exported_at));
    markdown.push_str(&format!("- Model: `{}`\n", model));
    markdown.push_str(&format!("- Workspace: `{}`\n", workspace.display()));
    markdown.push_str(&format!("- Total messages: {}\n\n", messages.len()));
    markdown.push_str("## Messages\n\n");

    for (index, message) in messages.iter().enumerate() {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("Unknown");
        let content = message.get("content").and_then(Value::as_str).unwrap_or("");

        markdown.push_str(&format!("### {}. {}\n\n", index + 1, role));
        if content.trim().is_empty() {
            markdown.push_str("_No textual content._\n\n");
        } else {
            markdown.push_str("```text\n");
            markdown.push_str(content);
            if !content.ends_with('\n') {
                markdown.push('\n');
            }
            markdown.push_str("```\n\n");
        }

        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array)
            && !tool_calls.is_empty()
        {
            markdown.push_str("Tool calls:\n");
            for call in tool_calls {
                let id = call.get("id").and_then(Value::as_str).unwrap_or("unknown");
                let function = call.get("function");
                let function_name = function
                    .and_then(|value| value.get("name"))
                    .and_then(Value::as_str)
                    .map(canonical_tool_name)
                    .unwrap_or_else(|| "unknown".to_string());
                markdown.push_str(&format!("- `{}`: `{}`\n", id, function_name));

                if let Some(arguments) = function.and_then(|value| value.get("arguments")) {
                    let arguments_text = serde_json::to_string_pretty(arguments)
                        .unwrap_or_else(|_| arguments.to_string());
                    markdown.push_str("```json\n");
                    markdown.push_str(&arguments_text);
                    markdown.push_str("\n```\n");
                }
            }
            markdown.push('\n');
        }

        if let Some(tool_call_id) = message.get("tool_call_id").and_then(Value::as_str) {
            markdown.push_str(&format!("Tool call id: `{}`\n\n", tool_call_id));
        }
    }

    markdown
}

fn build_timeline_export(
    exported_at: &str,
    provider: &str,
    model: &str,
    workspace: &std::path::Path,
    thread_id: &str,
    event_records: &[ThreadEventRecord],
    messages: &[uni::Message],
) -> TimelineExport {
    let (source, rows) = if event_records.is_empty() {
        (
            TIMELINE_SOURCE_CONVERSATION_FALLBACK.to_string(),
            timeline_rows_from_messages(&build_session_log_messages(messages)),
        )
    } else {
        (
            TIMELINE_SOURCE_THREAD_EVENTS.to_string(),
            timeline_rows_from_thread_events(event_records),
        )
    };
    let overview = build_session_overview(provider, model, &source, rows.len(), event_records);

    TimelineExport {
        exported_at: exported_at.to_string(),
        provider: provider.to_string(),
        model: model.to_string(),
        workspace: workspace.display().to_string(),
        thread_id: thread_id.to_string(),
        source,
        total_rows: rows.len(),
        overview,
        rows,
    }
}

fn build_session_overview(
    provider: &str,
    model: &str,
    source: &str,
    total_rows: usize,
    event_records: &[ThreadEventRecord],
) -> SessionOverview {
    let mut api_calls = 0_u64;
    let mut turns = 0_u64;
    let mut input_tokens = 0_u64;
    let mut output_tokens = 0_u64;
    let mut cached_input_tokens = 0_u64;
    let mut cache_creation_tokens = 0_u64;
    let mut added_files = 0_u64;
    let mut updated_files = 0_u64;
    let mut deleted_files = 0_u64;
    let mut outcome_code = None;
    let mut total_cost_usd = None;

    for record in event_records {
        match &record.event {
            ThreadEvent::TurnCompleted(event) => {
                api_calls += 1;
                turns += 1;
                input_tokens += event.usage.input_tokens;
                output_tokens += event.usage.output_tokens;
                cached_input_tokens += event.usage.cached_input_tokens;
                cache_creation_tokens += event.usage.cache_creation_tokens;
            }
            ThreadEvent::ThreadCompleted(event) => {
                if turns == 0 {
                    turns = u64::try_from(event.num_turns).unwrap_or(u64::MAX);
                }
                outcome_code = Some(event.outcome_code.clone());
                total_cost_usd = event.total_cost_usd.as_ref().map(ToString::to_string);
            }
            ThreadEvent::ItemCompleted(event) => {
                if let ThreadItemDetails::FileChange(file_change) = &event.item.details {
                    for change in &file_change.changes {
                        match change.kind {
                            PatchChangeKind::Add => added_files += 1,
                            PatchChangeKind::Update => updated_files += 1,
                            PatchChangeKind::Delete => deleted_files += 1,
                        }
                    }
                }
            }
            ThreadEvent::ItemUpdated(event) => {
                if let ThreadItemDetails::FileChange(file_change) = &event.item.details {
                    for change in &file_change.changes {
                        match change.kind {
                            PatchChangeKind::Add => added_files += 1,
                            PatchChangeKind::Update => updated_files += 1,
                            PatchChangeKind::Delete => deleted_files += 1,
                        }
                    }
                }
            }
            _ => {}
        }
    }

    SessionOverview {
        provider: provider.to_string(),
        model: model.to_string(),
        api_calls,
        turns,
        input_tokens,
        output_tokens,
        cached_input_tokens,
        cache_creation_tokens,
        total_tokens: input_tokens.saturating_add(output_tokens),
        added_files,
        updated_files,
        deleted_files,
        total_file_changes: added_files + updated_files + deleted_files,
        source: source.to_string(),
        total_rows,
        outcome_code,
        total_cost_usd,
    }
}

fn timeline_rows_from_thread_events(records: &[ThreadEventRecord]) -> Vec<TimelineRow> {
    records
        .iter()
        .map(|record| match &record.event {
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
                    let _ = writeln!(&mut body, "Stop reason: {}", stop_reason);
                }
                if let Some(total_cost_usd) = &event.total_cost_usd {
                    let _ = writeln!(&mut body, "Total cost (USD): {}", total_cost_usd);
                }
                let _ = writeln!(&mut body, "Turns: {}", event.num_turns);
                let usage_summary = format_usage_summary(&event.usage);
                let _ = writeln!(&mut body, "Usage: {}", usage_summary);
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
                        let _ = writeln!(&mut body, "History artifact: {}", path);
                    }
                    body
                },
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::TurnStarted(_) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "turn.started",
                None,
                "turn",
                Some("in_progress"),
                record.turn_id.as_deref(),
                record.submission_id.as_ref().map(|value| value.as_str()),
                "Turn started".to_string(),
                record
                    .turn_id
                    .clone()
                    .unwrap_or_else(|| "turn started".to_string()),
                String::new(),
                pretty_json_string(&record.event),
                false,
            ),
            ThreadEvent::TurnCompleted(event) => timeline_row(
                record.sequence,
                TIMELINE_SOURCE_THREAD_EVENTS,
                "turn.completed",
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
            ThreadEvent::ItemStarted(event) => {
                timeline_row_from_item(record, "item.started", "in_progress", &event.item)
            }
            ThreadEvent::ItemUpdated(event) => {
                timeline_row_from_item(record, "item.updated", "in_progress", &event.item)
            }
            ThreadEvent::ItemCompleted(event) => {
                timeline_row_from_item(record, "item.completed", "completed", &event.item)
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
        })
        .collect()
}

fn timeline_row_from_item(
    record: &ThreadEventRecord,
    event_type: &str,
    default_status: &str,
    item: &ThreadItem,
) -> TimelineRow {
    let detail_json = pretty_json_string(&record.event);
    match &item.details {
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
                event_type == "item.updated",
            )
        }
        ThreadItemDetails::ToolInvocation(tool) => {
            let status = tool_status_label(&tool.status);
            let tool_name = canonical_tool_name(&tool.tool_name);
            let mut body = String::new();
            if let Some(arguments) = &tool.arguments {
                append_json_section(&mut body, "Arguments", arguments);
            }
            if let Some(tool_call_id) = &tool.tool_call_id {
                let _ = write!(&mut body, "Tool call id: {}", tool_call_id);
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
                event_type == "item.updated",
            )
        }
        ThreadItemDetails::ToolOutput(tool_output) => {
            let status = tool_status_label(&tool_output.status);
            let mut body = String::new();
            if let Some(tool_call_id) = &tool_output.tool_call_id {
                let _ = writeln!(&mut body, "Tool call id: {}", tool_call_id);
            }
            if let Some(spool_path) = &tool_output.spool_path {
                let _ = writeln!(&mut body, "Spool path: {}", spool_path);
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
                event_type == "item.updated",
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
            format!(
                "{} file(s) ({})",
                file_change.changes.len(),
                patch_status_label(&file_change.status)
            ),
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
                event_type == "item.updated",
            )
        }
        ThreadItemDetails::WebSearch(search) => {
            let mut body = String::new();
            let _ = writeln!(&mut body, "Query: {}", search.query);
            if let Some(provider) = &search.provider {
                let _ = writeln!(&mut body, "Provider: {}", provider);
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
            Some(harness_status_label(&event.event)),
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

fn timeline_rows_from_messages(messages: &[Value]) -> Vec<TimelineRow> {
    let mut rows = Vec::with_capacity(messages.len());
    let mut sequence = 1_u64;

    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("Unknown");
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
                    .map(canonical_tool_name)
                    .unwrap_or_else(|| "unknown".to_string());
                let tool_call_id = tool_call.get("id").and_then(Value::as_str);
                let mut body = String::new();
                if let Some(arguments) = tool_call
                    .get("function")
                    .and_then(|value| value.get("arguments"))
                {
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

fn render_session_timeline_html(export: &TimelineExport) -> Result<String> {
    let export_json = serde_json::to_string(export).context("Failed to serialize timeline data")?;
    let mut html = String::new();
    let escaped_workspace = escape_html(&export.workspace);
    let escaped_model = escape_html(&export.model);
    let escaped_provider = escape_html(&export.provider);
    let escaped_thread_id = escape_html(&export.thread_id);
    let escaped_source = escape_html(&export.source);
    let escaped_exported_at = escape_html(&export.exported_at);
    let escaped_overview_json = escape_html(
        &serde_json::to_string_pretty(&export.overview)
            .context("Failed to serialize session overview")?,
    );

    html.push_str(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>VT Code Thread Share</title>\n<style>\n:root{color-scheme:dark;--bg:#0b0d0b;--panel:#101311;--panel-soft:#141917;--panel-muted:#181d1b;--surface:#111513;--line:#232a26;--line-soft:#1a201d;--text:#f3f5f2;--muted:#a1aba4;--accent:#d0d7d1;--success:#8fd3a6;--danger:#f39aa2;--warning:#f0c27a;}*{box-sizing:border-box}html,body{background:var(--bg)}body{margin:0;font-family:ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,\"Segoe UI\",sans-serif;color:var(--text);line-height:1.55}a{color:inherit}main{max-width:980px;margin:0 auto;padding:28px 18px 80px}.masthead{max-width:760px;margin:0 auto}.eyebrow{font-size:.78rem;letter-spacing:.08em;text-transform:uppercase;color:var(--muted)}.masthead h1{margin:10px 0 14px;font-size:clamp(2rem,4vw,3.2rem);line-height:1.05;font-weight:800}.byline{display:flex;flex-wrap:wrap;gap:10px;align-items:center;color:var(--muted);font-size:.96rem}.pill{display:inline-flex;align-items:center;gap:8px;padding:6px 12px;border-radius:999px;background:var(--panel-soft);border:1px solid var(--line-soft)}.lede{margin:22px auto 0;max-width:760px;padding:18px 20px;background:var(--panel);border:1px solid var(--line);border-radius:18px;color:#d9dfdb}.lede pre,.overview pre,.body,.raw-json{margin:0;white-space:pre-wrap;overflow:auto;font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:.92rem;line-height:1.6}.facts{max-width:760px;margin:18px auto 0;display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:12px}.fact{padding:16px 18px;background:var(--panel);border:1px solid var(--line);border-radius:18px}.fact-label{display:block;font-size:.74rem;letter-spacing:.08em;text-transform:uppercase;color:var(--muted)}.fact-value{display:block;margin-top:8px;font-size:1.05rem;font-weight:700}.overview,.controls-wrap,.timeline-wrap{max-width:760px;margin:18px auto 0}.overview,.controls,.row{background:var(--panel);border:1px solid var(--line);border-radius:18px}.overview{padding:18px}.overview h2,.section-title{margin:0 0 8px;font-size:1rem;font-weight:700}.overview-copy{margin:0 0 14px;color:var(--muted)}.stats-grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:10px;margin-bottom:14px}.stat{padding:12px 14px;background:var(--panel-soft);border-radius:14px}.stat-label{display:block;font-size:.72rem;letter-spacing:.08em;text-transform:uppercase;color:var(--muted)}.stat-value{display:block;margin-top:6px;font-weight:700}.controls{padding:16px}.control-grid{display:grid;grid-template-columns:2fr repeat(2,minmax(120px,1fr));gap:10px}.control{display:grid;gap:6px}.control label{font-size:.8rem;color:var(--muted);font-weight:600}.control input,.control select{width:100%;padding:11px 12px;border-radius:12px;border:1px solid var(--line);background:var(--panel-muted);color:var(--text);font:inherit}.control input:focus,.control select:focus{outline:none;border-color:#3d4d45}.toolbar{display:flex;flex-wrap:wrap;gap:10px;align-items:center;justify-content:space-between;margin-top:12px}.toggle{display:flex;align-items:center;gap:8px;color:var(--muted);font-weight:600}.button{padding:9px 13px;border-radius:999px;border:1px solid var(--line);background:var(--panel-soft);color:var(--text);font:inherit;cursor:pointer}.button:hover{background:var(--panel-muted)}.results{color:var(--muted);font-weight:600}.section-title{margin:24px 0 10px;color:#e7ebe8}.timeline{display:grid;gap:12px}.row{padding:16px 18px}.row-head{display:flex;flex-wrap:wrap;gap:10px;align-items:flex-start;justify-content:space-between}.row-title{display:flex;gap:10px;align-items:baseline;flex-wrap:wrap}.seq{display:inline-flex;align-items:center;justify-content:center;min-width:3rem;padding:4px 10px;border-radius:999px;background:var(--panel-muted);color:var(--muted);font-size:.8rem;font-weight:700}.row h2{margin:0;font-size:1.02rem}.summary{margin:12px 0 0;color:#d6ddd8;white-space:pre-wrap}.body,.raw-json{margin:12px 0 0;padding:14px;background:var(--panel-soft);border:1px solid var(--line-soft);border-radius:14px}.badges{display:flex;flex-wrap:wrap;gap:8px;justify-content:flex-end}.badge{display:inline-flex;align-items:center;padding:5px 10px;border-radius:999px;background:var(--panel-soft);border:1px solid var(--line-soft);font-size:.76rem;color:var(--muted);font-weight:700}.badge.status-completed{color:var(--success)}.badge.status-in_progress{color:#a9c9ff}.badge.status-failed,.badge.status-cancelled{color:var(--danger)}.badge.low-signal{color:var(--warning)}details{margin-top:12px}details summary{cursor:pointer;color:var(--muted);font-weight:700}.empty{padding:28px;text-align:center;color:var(--muted);background:var(--panel);border:1px solid var(--line);border-radius:18px}.footer-note{max-width:760px;margin:26px auto 0;color:var(--muted);font-size:.9rem}@media (max-width:760px){main{padding:20px 12px 48px}.facts,.stats-grid,.control-grid{grid-template-columns:1fr}.row{padding:14px}}\n</style>\n</head>\n<body>\n<main>\n<header class=\"masthead\">\n<div class=\"eyebrow\">Shared Thread</div>\n<h1>VT Code Session Timeline</h1>\n<div class=\"byline\">\n<span class=\"pill\">Provider: ");
    html.push_str(&escaped_provider);
    html.push_str("</span>\n<span class=\"pill\">Model: ");
    html.push_str(&escaped_model);
    html.push_str("</span>\n<span class=\"pill\">Exported: ");
    html.push_str(&escaped_exported_at);
    html.push_str("</span>\n</div>\n</header>\n<section class=\"lede\"><pre>");
    html.push_str(&escaped_workspace);
    html.push_str("\nThread: ");
    html.push_str(&escaped_thread_id);
    html.push_str("\nSource: ");
    html.push_str(&escaped_source);
    html.push_str("</pre></section>\n<section class=\"facts\">\n<div class=\"fact\"><span class=\"fact-label\">API Calls</span><span class=\"fact-value\" id=\"overview-api-calls\"></span></div>\n<div class=\"fact\"><span class=\"fact-label\">Tokens</span><span class=\"fact-value\" id=\"overview-tokens\"></span></div>\n<div class=\"fact\"><span class=\"fact-label\">Diff</span><span class=\"fact-value\" id=\"overview-diff\"></span></div>\n</section>\n<section class=\"overview\">\n<h2>Session Overview</h2>\n<p class=\"overview-copy\">Session, provider, token usage, diff totals, and API call counts in a thread-style export view.</p>\n<div class=\"stats-grid\">\n<div class=\"stat\"><span class=\"stat-label\">Rows</span><span class=\"stat-value\">");
    let _ = write!(&mut html, "{}", export.total_rows);
    html.push_str("</span></div>\n<div class=\"stat\"><span class=\"stat-label\">Thread Source</span><span class=\"stat-value\">");
    html.push_str(&escaped_source);
    html.push_str("</span></div>\n</div>\n<pre>");
    html.push_str(&escaped_overview_json);
    html.push_str("</pre>\n</section>\n<section class=\"controls-wrap\"><div class=\"controls\">\n<div class=\"control-grid\">\n<div class=\"control\"><label for=\"search-input\">Search</label><input id=\"search-input\" type=\"search\" placeholder=\"Search messages, tools, commands, output\"></div>\n<div class=\"control\"><label for=\"category-filter\">Category</label><select id=\"category-filter\"></select></div>\n<div class=\"control\"><label for=\"status-filter\">Status</label><select id=\"status-filter\"></select></div>\n</div>\n<div class=\"toolbar\">\n<label class=\"toggle\" for=\"hide-low-signal\"><input id=\"hide-low-signal\" type=\"checkbox\">Hide low-signal rows</label>\n<button id=\"clear-filters\" class=\"button\" type=\"button\">Clear filters</button>\n<div id=\"results-count\" class=\"results\"></div>\n</div>\n</div></section>\n<section class=\"timeline-wrap\">\n<h2 class=\"section-title\">Thread</h2>\n<section id=\"timeline\" class=\"timeline\"></section>\n</section>\n<p class=\"footer-note\">This HTML file is self-contained and can be shared offline.</p>\n</main>\n<script id=\"vtcode-session-data\" type=\"application/json\">");
    html.push_str(&sanitize_json_for_script_tag(&export_json));
    html.push_str("</script>\n<script>\nconst exportData = JSON.parse(document.getElementById('vtcode-session-data').textContent);\nconst rows = exportData.rows || [];\nconst overview = exportData.overview || {};\nconst timelineEl = document.getElementById('timeline');\nconst searchInput = document.getElementById('search-input');\nconst categoryFilter = document.getElementById('category-filter');\nconst statusFilter = document.getElementById('status-filter');\nconst hideLowSignal = document.getElementById('hide-low-signal');\nconst clearFilters = document.getElementById('clear-filters');\nconst resultsCount = document.getElementById('results-count');\nfunction escapeHtml(text){return String(text).replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;').replaceAll('\"','&quot;').replaceAll(\"'\",'&#39;');}\nfunction formatNumber(value){return Number(value || 0).toLocaleString('en-US');}\nfunction fillOverview(){const apiCalls = document.getElementById('overview-api-calls'); const tokens = document.getElementById('overview-tokens'); const diff = document.getElementById('overview-diff'); if(apiCalls){apiCalls.textContent = `${formatNumber(overview.api_calls)} call(s) / ${formatNumber(overview.turns)} turn(s)`;} if(tokens){tokens.textContent = `${formatNumber(overview.input_tokens)} in / ${formatNumber(overview.output_tokens)} out`; } if(diff){diff.textContent = `+${formatNumber(overview.added_files)} ~${formatNumber(overview.updated_files)} -${formatNumber(overview.deleted_files)}`;}}\nfunction fillSelect(select, values, label){select.innerHTML = `<option value=\"\">${escapeHtml(label)}</option>` + values.map(value => `<option value=\"${escapeHtml(value)}\">${escapeHtml(value)}</option>`).join('');}\nfunction uniqueValues(key){return [...new Set(rows.map(row => row[key]).filter(Boolean))].sort((a,b) => String(a).localeCompare(String(b)));}\nfunction rowSearchText(row){return [row.event_type,row.item_type,row.category,row.status,row.title,row.summary,row.body,row.detail_json].filter(Boolean).join('\\n').toLowerCase();}\nfunction statusClass(status){return status ? ` status-${status}` : '';}\nfunction renderRow(row){const badges = [row.category,row.event_type,row.item_type,row.status].filter(Boolean).map(value => `<span class=\"badge${value === row.status ? statusClass(value) : ''}\">${escapeHtml(value)}</span>`); if(row.is_low_signal){badges.push('<span class=\"badge low-signal\">low-signal</span>');} const summary = row.summary ? `<p class=\"summary\">${escapeHtml(row.summary)}</p>` : ''; const body = row.body ? `<pre class=\"body\">${escapeHtml(row.body)}</pre>` : ''; const raw = row.detail_json ? `<details><summary>Raw event JSON</summary><pre class=\"raw-json\">${escapeHtml(row.detail_json)}</pre></details>` : ''; return `<article class=\"row\"><div class=\"row-head\"><div class=\"row-title\"><span class=\"seq\">#${row.sequence}</span><h2>${escapeHtml(row.title)}</h2></div><div class=\"badges\">${badges.join('')}</div></div>${summary}${body}${raw}</article>`;}\nfunction render(){const query = searchInput.value.trim().toLowerCase(); const category = categoryFilter.value; const status = statusFilter.value; const hideLow = hideLowSignal.checked; const filtered = rows.filter(row => { if (hideLow && row.is_low_signal) return false; if (category && row.category !== category) return false; if (status && row.status !== status) return false; if (query && !rowSearchText(row).includes(query)) return false; return true; }); resultsCount.textContent = `${filtered.length} of ${rows.length} rows`; if (!filtered.length){timelineEl.innerHTML = '<div class=\"empty\">No timeline rows match the current filters.</div>'; return;} timelineEl.innerHTML = filtered.map(renderRow).join('');}\nfillSelect(categoryFilter, uniqueValues('category'), 'All categories');\nfillSelect(statusFilter, uniqueValues('status'), 'All statuses');\nfillOverview();\nsearchInput.addEventListener('input', render);\ncategoryFilter.addEventListener('change', render);\nstatusFilter.addEventListener('change', render);\nhideLowSignal.addEventListener('change', render);\nclearFilters.addEventListener('click', () => { searchInput.value = ''; categoryFilter.value = ''; statusFilter.value = ''; hideLowSignal.checked = false; render(); });\nrender();\n</script>\n</body>\n</html>\n");

    Ok(html)
}

#[allow(clippy::too_many_arguments)]
fn timeline_row(
    sequence: u64,
    source: &str,
    event_type: &str,
    item_type: Option<&str>,
    category: &str,
    status: Option<&str>,
    turn_id: Option<&str>,
    submission_id: Option<&str>,
    title: String,
    summary: String,
    body: String,
    detail_json: Option<String>,
    is_low_signal: bool,
) -> TimelineRow {
    TimelineRow {
        sequence,
        source: source.to_string(),
        event_type: event_type.to_string(),
        item_type: item_type.map(str::to_string),
        category: category.to_string(),
        status: status.map(str::to_string),
        turn_id: turn_id.map(str::to_string),
        submission_id: submission_id.map(str::to_string),
        title,
        summary,
        body,
        detail_json,
        is_low_signal,
    }
}

fn append_text_section(body: &mut String, label: &str, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if !body.is_empty() {
        body.push_str("\n\n");
    }
    let _ = write!(body, "{label}:\n{trimmed}");
}

fn append_json_section(body: &mut String, label: &str, value: &Value) {
    if let Ok(pretty) = serde_json::to_string_pretty(value) {
        append_text_section(body, label, &pretty);
    }
}

fn format_usage_summary(usage: &Usage) -> String {
    format!(
        "input={} cached={} cache_create={} output={}",
        usage.input_tokens,
        usage.cached_input_tokens,
        usage.cache_creation_tokens,
        usage.output_tokens
    )
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    let candidate = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .trim();

    if candidate.is_empty() {
        return "No textual content.".to_string();
    }

    let mut truncated = String::new();
    let mut chars = candidate.chars();
    for _ in 0..max_chars {
        let Some(ch) = chars.next() else {
            return candidate.to_string();
        };
        truncated.push(ch);
    }
    if chars.next().is_some() {
        truncated.push('…');
    }
    truncated
}

fn pretty_json_string<T: Serialize>(value: &T) -> Option<String> {
    serde_json::to_string_pretty(value).ok()
}

fn pretty_json_value(value: &Value) -> Option<String> {
    serde_json::to_string_pretty(value).ok()
}

fn canonical_tool_name(name: &str) -> String {
    vtcode_core::tools::tool_intent::canonical_unified_exec_tool_name(name)
        .unwrap_or(name)
        .to_string()
}

fn command_status_label(status: &CommandExecutionStatus) -> &'static str {
    match status {
        CommandExecutionStatus::Completed => "completed",
        CommandExecutionStatus::Failed => "failed",
        CommandExecutionStatus::InProgress => "in_progress",
    }
}

fn tool_status_label(status: &ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Completed => "completed",
        ToolCallStatus::Failed => "failed",
        ToolCallStatus::InProgress => "in_progress",
    }
}

fn mcp_status_label(status: Option<&McpToolCallStatus>) -> Option<&'static str> {
    match status {
        Some(McpToolCallStatus::Started) => Some("in_progress"),
        Some(McpToolCallStatus::Completed) => Some("completed"),
        Some(McpToolCallStatus::Failed) => Some("failed"),
        None => None,
    }
}

fn patch_status_label(status: &PatchApplyStatus) -> &'static str {
    match status {
        PatchApplyStatus::Completed => "completed",
        PatchApplyStatus::Failed => "failed",
    }
}

fn thread_completion_status(subtype: &ThreadCompletionSubtype) -> &'static str {
    match subtype {
        ThreadCompletionSubtype::Success => "completed",
        ThreadCompletionSubtype::Cancelled => "cancelled",
        ThreadCompletionSubtype::ErrorMaxTurns
        | ThreadCompletionSubtype::ErrorMaxBudgetUsd
        | ThreadCompletionSubtype::ErrorDuringExecution => "failed",
    }
}

fn summarize_status_with_exit(status: &str, exit_code: Option<i32>) -> String {
    match exit_code {
        Some(code) => format!("{status} (exit {code})"),
        None => status.to_string(),
    }
}

fn summarize_status_with_label(status: &str, label: Option<&str>) -> String {
    match label {
        Some(value) => format!("{status} ({value})"),
        None => status.to_string(),
    }
}

fn harness_title(event: &HarnessEventKind) -> &'static str {
    match event {
        HarnessEventKind::PlanningStarted => "Planning started",
        HarnessEventKind::PlanningCompleted => "Planning completed",
        HarnessEventKind::ContinuationStarted => "Continuation started",
        HarnessEventKind::ContinuationSkipped => "Continuation skipped",
        HarnessEventKind::BlockedHandoffWritten => "Blocked handoff written",
        HarnessEventKind::EvaluationStarted => "Evaluation started",
        HarnessEventKind::EvaluationPassed => "Evaluation passed",
        HarnessEventKind::EvaluationFailed => "Evaluation failed",
        HarnessEventKind::RevisionStarted => "Revision started",
        HarnessEventKind::VerificationStarted => "Verification started",
        HarnessEventKind::VerificationPassed => "Verification passed",
        HarnessEventKind::VerificationFailed => "Verification failed",
    }
}

fn harness_status_label(event: &HarnessEventKind) -> &'static str {
    match event {
        HarnessEventKind::PlanningCompleted
        | HarnessEventKind::EvaluationPassed
        | HarnessEventKind::VerificationPassed
        | HarnessEventKind::BlockedHandoffWritten => "completed",
        HarnessEventKind::EvaluationFailed | HarnessEventKind::VerificationFailed => "failed",
        HarnessEventKind::PlanningStarted
        | HarnessEventKind::ContinuationStarted
        | HarnessEventKind::ContinuationSkipped
        | HarnessEventKind::EvaluationStarted
        | HarnessEventKind::RevisionStarted
        | HarnessEventKind::VerificationStarted => "in_progress",
    }
}

fn harness_summary(event: &vtcode_core::exec::events::HarnessEventItem) -> String {
    event
        .message
        .as_ref()
        .cloned()
        .or_else(|| event.command.as_ref().cloned())
        .or_else(|| event.path.as_ref().cloned())
        .unwrap_or_else(|| harness_title(&event.event).to_string())
}

fn harness_body(event: &vtcode_core::exec::events::HarnessEventItem) -> String {
    let mut body = String::new();
    if let Some(message) = &event.message {
        let _ = writeln!(&mut body, "Message: {}", message);
    }
    if let Some(command) = &event.command {
        let _ = writeln!(&mut body, "Command: {}", command);
    }
    if let Some(path) = &event.path {
        let _ = writeln!(&mut body, "Path: {}", path);
    }
    if let Some(exit_code) = event.exit_code {
        let _ = writeln!(&mut body, "Exit code: {}", exit_code);
    }
    body.trim_end().to_string()
}

fn escape_html(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn sanitize_json_for_script_tag(input: &str) -> String {
    input
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
}

pub(crate) async fn handle_share_log(
    ctx: SlashCommandContext<'_>,
    format: SessionLogExportFormat,
) -> Result<SlashCommandControl> {
    use chrono::Local;

    let exported_at = Local::now().to_rfc3339();
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let log_messages = build_session_log_messages(ctx.conversation_history);
    let thread_events = ctx.thread_handle.replay_recent();
    let json_output_path = ctx
        .config
        .workspace
        .join(format!("vtcode-session-log-{}.json", timestamp));
    let markdown_output_path = ctx
        .config
        .workspace
        .join(format!("vtcode-session-log-{}.md", timestamp));
    let html_output_path = ctx
        .config
        .workspace
        .join(format!("vtcode-session-timeline-{}.html", timestamp));

    if matches!(
        format,
        SessionLogExportFormat::Both | SessionLogExportFormat::Json
    ) {
        let export = json!({
            "exported_at": exported_at,
            "model": &ctx.config.model,
            "workspace": ctx.config.workspace.display().to_string(),
            "total_messages": log_messages.len(),
            "messages": log_messages,
        });

        let json =
            serde_json::to_string_pretty(&export).context("Failed to serialize session log")?;
        write_file_with_context_sync(&json_output_path, &json, "session log")?;
    }

    if matches!(format, SessionLogExportFormat::Markdown) {
        let markdown = render_session_log_markdown(
            &exported_at,
            &ctx.config.model,
            &ctx.config.workspace,
            &log_messages,
        );
        write_file_with_context_sync(&markdown_output_path, &markdown, "session log")?;
    }

    if matches!(
        format,
        SessionLogExportFormat::Both | SessionLogExportFormat::Html
    ) {
        let timeline_export = build_timeline_export(
            &exported_at,
            ctx.provider_client.name(),
            &ctx.config.model,
            &ctx.config.workspace,
            ctx.thread_id,
            &thread_events,
            ctx.conversation_history,
        );
        let html = render_session_timeline_html(&timeline_export)?;
        write_file_with_context_sync(&html_output_path, &html, "session timeline")?;
    }

    match format {
        SessionLogExportFormat::Both => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Share exports ready:\nJSON: {}\nHTML: {}\nHTML is self-contained for offline sharing; JSON is useful for debugging.",
                    json_output_path.display(),
                    html_output_path.display()
                ),
            )?;
        }
        SessionLogExportFormat::Html => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Share HTML ready:\n{}\nThis HTML file is self-contained and can be shared offline.",
                    html_output_path.display()
                ),
            )?;
        }
        SessionLogExportFormat::Json => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Share JSON ready:\n{}\nYou can share this file for debugging purposes.",
                    json_output_path.display()
                ),
            )?;
        }
        SessionLogExportFormat::Markdown => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Session log exported to: {} ({})",
                    markdown_output_path.display(),
                    "Markdown"
                ),
            )?;
            ctx.renderer.line(
                MessageStyle::Info,
                "You can share this file for debugging purposes.",
            )?;
        }
    }

    Ok(SlashCommandControl::Continue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::core::threads::{ThreadEventRecord, ThreadId};
    use vtcode_core::exec::events::{
        AgentMessageItem, CommandExecutionItem, ItemCompletedEvent, ItemStartedEvent,
        ThreadStartedEvent, ToolInvocationItem, TurnCompletedEvent, Usage,
    };

    fn sample_event_record(sequence: u64, event: ThreadEvent) -> ThreadEventRecord {
        ThreadEventRecord {
            sequence,
            thread_id: ThreadId::new("thread-1"),
            submission_id: None,
            turn_id: Some("turn-1".to_string()),
            event,
        }
    }

    #[test]
    fn timeline_export_prefers_thread_events() {
        let records = vec![
            sample_event_record(
                1,
                ThreadEvent::ThreadStarted(ThreadStartedEvent {
                    thread_id: "thread-1".to_string(),
                }),
            ),
            sample_event_record(
                2,
                ThreadEvent::ItemCompleted(ItemCompletedEvent {
                    item: vtcode_core::exec::events::ThreadItem {
                        id: "msg-1".to_string(),
                        details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                            text: "assistant reply".to_string(),
                        }),
                    },
                }),
            ),
        ];
        let messages = vec![uni::Message {
            role: uni::MessageRole::User,
            content: uni::MessageContent::Text("hello".to_string()),
            reasoning: None,
            reasoning_details: None,
            tool_calls: None,
            tool_call_id: None,
            phase: None,
            origin_tool: None,
        }];

        let export = build_timeline_export(
            "2026-04-04T00:00:00Z",
            "openai",
            "gpt-test",
            std::path::Path::new("/tmp/workspace"),
            "thread-1",
            &records,
            &messages,
        );

        assert_eq!(export.source, TIMELINE_SOURCE_THREAD_EVENTS);
        assert_eq!(export.rows.len(), 2);
        assert_eq!(export.rows[1].category, "message");
    }

    #[test]
    fn timeline_export_falls_back_to_conversation_messages() {
        let messages = vec![json!({
            "role": "Assistant",
            "content": "assistant output",
            "tool_calls": [{
                "id": "call-1",
                "function": {
                    "name": "exec_command",
                    "arguments": {"cmd": "pwd"}
                }
            }]
        })];

        let rows = timeline_rows_from_messages(&messages);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].source, TIMELINE_SOURCE_CONVERSATION_FALLBACK);
        assert_eq!(rows[0].category, "message");
        assert_eq!(rows[1].category, "tool");
    }

    #[test]
    fn html_timeline_is_self_contained_and_escapes_embedded_json() {
        let records = vec![
            sample_event_record(
                1,
                ThreadEvent::ItemCompleted(ItemCompletedEvent {
                    item: vtcode_core::exec::events::ThreadItem {
                        id: "msg-1".to_string(),
                        details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                            text: "<script>alert('xss')</script>".to_string(),
                        }),
                    },
                }),
            ),
            sample_event_record(
                2,
                ThreadEvent::ItemStarted(ItemStartedEvent {
                    item: vtcode_core::exec::events::ThreadItem {
                        id: "tool-1".to_string(),
                        details: ThreadItemDetails::ToolInvocation(ToolInvocationItem {
                            tool_name: "exec_command".to_string(),
                            arguments: Some(json!({"cmd": "pwd"})),
                            tool_call_id: Some("call-1".to_string()),
                            status: ToolCallStatus::InProgress,
                        }),
                    },
                }),
            ),
            sample_event_record(
                3,
                ThreadEvent::TurnCompleted(TurnCompletedEvent {
                    usage: Usage {
                        input_tokens: 10,
                        cached_input_tokens: 2,
                        cache_creation_tokens: 0,
                        output_tokens: 4,
                    },
                }),
            ),
        ];

        let export = build_timeline_export(
            "2026-04-04T00:00:00Z",
            "openai",
            "gpt-test",
            std::path::Path::new("/tmp/workspace"),
            "thread-1",
            &records,
            &[],
        );
        let html = render_session_timeline_html(&export).expect("html");

        assert!(html.contains("id=\"search-input\""));
        assert!(html.contains("id=\"category-filter\""));
        assert!(html.contains("id=\"status-filter\""));
        assert!(html.contains("id=\"hide-low-signal\""));
        assert!(html.contains("vtcode-session-data"));
        assert!(!html.contains("http://"));
        assert!(!html.contains("https://"));
        assert!(!html.contains("<script>alert('xss')</script>"));
        assert!(html.contains("\\u003cscript\\u003ealert('xss')\\u003c/script\\u003e"));
    }

    #[test]
    fn html_timeline_uses_flat_surfaces_without_shadows() {
        let export = TimelineExport {
            exported_at: "2026-04-04T00:00:00Z".to_string(),
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
            workspace: "/tmp/workspace".to_string(),
            thread_id: "thread-1".to_string(),
            source: TIMELINE_SOURCE_THREAD_EVENTS.to_string(),
            total_rows: 0,
            overview: SessionOverview {
                provider: "openai".to_string(),
                model: "gpt-test".to_string(),
                api_calls: 1,
                turns: 1,
                input_tokens: 10,
                output_tokens: 4,
                cached_input_tokens: 2,
                cache_creation_tokens: 0,
                total_tokens: 14,
                added_files: 1,
                updated_files: 2,
                deleted_files: 0,
                total_file_changes: 3,
                source: TIMELINE_SOURCE_THREAD_EVENTS.to_string(),
                total_rows: 0,
                outcome_code: Some("completed".to_string()),
                total_cost_usd: None,
            },
            rows: Vec::new(),
        };

        let html = render_session_timeline_html(&export).expect("html");

        assert!(!html.contains("--shadow"));
        assert!(!html.contains("box-shadow"));
        assert!(!html.contains("border-top:4px solid var(--accent)"));
        assert!(html.contains("Session Overview"));
        assert!(html.contains("Shared Thread"));
        assert!(html.contains("VT Code Thread Share"));
    }

    #[test]
    fn command_rows_surface_status_and_output() {
        let row = timeline_row_from_item(
            &sample_event_record(
                7,
                ThreadEvent::ItemCompleted(ItemCompletedEvent {
                    item: vtcode_core::exec::events::ThreadItem {
                        id: "cmd-1".to_string(),
                        details: ThreadItemDetails::CommandExecution(Box::new(
                            CommandExecutionItem {
                                command: "cargo check".to_string(),
                                arguments: Some(json!({"args": ["-p", "vtcode"]})),
                                aggregated_output: "Finished dev [unoptimized]".to_string(),
                                exit_code: Some(0),
                                status: CommandExecutionStatus::Completed,
                            },
                        )),
                    },
                }),
            ),
            "item.completed",
            "completed",
            &vtcode_core::exec::events::ThreadItem {
                id: "cmd-1".to_string(),
                details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                    command: "cargo check".to_string(),
                    arguments: Some(json!({"args": ["-p", "vtcode"]})),
                    aggregated_output: "Finished dev [unoptimized]".to_string(),
                    exit_code: Some(0),
                    status: CommandExecutionStatus::Completed,
                })),
            },
        );

        assert_eq!(row.category, "command");
        assert_eq!(row.status.as_deref(), Some("completed"));
        assert!(row.summary.contains("exit 0"));
        assert!(row.body.contains("Finished dev"));
    }
}
