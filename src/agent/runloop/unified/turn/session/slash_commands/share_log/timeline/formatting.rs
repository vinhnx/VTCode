use std::fmt::Write as _;

use super::*;
use vtcode_core::exec::events::{
    CommandExecutionStatus, HarnessEventKind, McpToolCallStatus, PatchApplyStatus, ThreadCompletionSubtype,
    ToolCallStatus, Usage,
};

#[expect(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
pub(super) fn timeline_row(
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
    let role = infer_row_role(category, item_type, &title);
    let transcript_kind = infer_transcript_kind(category, item_type, &title);
    TimelineRow {
        sequence,
        source: source.to_string(),
        event_type: event_type.to_string(),
        item_type: item_type.map(str::to_string),
        category: category.to_string(),
        role,
        transcript_kind,
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

fn infer_row_role(category: &str, item_type: Option<&str>, title: &str) -> String {
    let normalized_title = title.to_ascii_lowercase();
    if normalized_title.starts_with("user ") {
        return "user".to_string();
    }
    if normalized_title.starts_with("assistant ")
        || matches!(item_type, Some("agent_message" | "plan" | "reasoning" | "plan_delta"))
    {
        return "assistant".to_string();
    }
    if category == "message" && normalized_title.contains("assistant") {
        return "assistant".to_string();
    }
    "system".to_string()
}

fn infer_transcript_kind(category: &str, item_type: Option<&str>, title: &str) -> String {
    let normalized_title = title.to_ascii_lowercase();
    if normalized_title.contains("pty") {
        return "pty".to_string();
    }
    match item_type {
        Some("agent_message" | "message") => "message".to_string(),
        Some("plan" | "plan_delta") => "plan".to_string(),
        Some("reasoning") => "reasoning".to_string(),
        Some("tool_invocation" | "tool_output" | "mcp_tool_call" | "web_search") => {
            if normalized_title.contains("pty") {
                "pty".to_string()
            } else {
                "tool".to_string()
            }
        }
        Some("command_execution") => "command".to_string(),
        Some("file_change") => "diff".to_string(),
        _ => match category {
            "message" => "message".to_string(),
            "tool" | "mcp" | "web_search" => "tool".to_string(),
            "command" => "command".to_string(),
            "file_change" => "diff".to_string(),
            _ => "status".to_string(),
        },
    }
}

pub(super) fn append_text_section(body: &mut String, label: &str, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if !body.is_empty() {
        body.push_str("\n\n");
    }
    let _ = write!(body, "{label}:\n{trimmed}");
}

pub(super) fn append_json_section(body: &mut String, label: &str, value: &Value) {
    if let Ok(pretty) = serde_json::to_string_pretty(value) {
        append_text_section(body, label, &pretty);
    }
}

pub(super) fn format_usage_summary(usage: &Usage) -> String {
    format!(
        "input={} cached={} cache_create={} output={}",
        usage.input_tokens, usage.cached_input_tokens, usage.cache_creation_tokens, usage.output_tokens
    )
}

pub(super) fn truncate_preview(text: &str, max_chars: usize) -> String {
    let candidate = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("").trim();

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

pub(super) fn pretty_json_string<T: Serialize>(value: &T) -> Option<String> {
    serde_json::to_string_pretty(value).ok()
}

pub(super) fn pretty_json_value(value: &Value) -> Option<String> {
    serde_json::to_string_pretty(value).ok()
}

pub(super) fn redact_timeline_export(export: &TimelineExport) -> TimelineExport {
    let mut redacted = export.clone();
    redacted.workspace = super::super::redact_sensitive_text(&redacted.workspace);
    redacted.thread_id = super::super::redact_sensitive_text(&redacted.thread_id);
    if let Some(outcome_code) = &redacted.overview.outcome_code {
        redacted.overview.outcome_code = Some(super::super::redact_sensitive_text(outcome_code));
    }
    for row in &mut redacted.rows {
        row.title = super::super::redact_sensitive_text(&row.title);
        row.summary = super::super::redact_sensitive_text(&row.summary);
        row.body = super::super::redact_sensitive_text(&row.body);
        row.turn_id = row.turn_id.as_ref().map(|value| super::super::redact_sensitive_text(value));
        row.submission_id = row
            .submission_id
            .as_ref()
            .map(|value| super::super::redact_sensitive_text(value));
        row.detail_json = row.detail_json.as_ref().map(|value| super::super::redact_sensitive_text(value));
    }
    redacted
}

pub(super) fn command_status_label(status: &CommandExecutionStatus) -> &'static str {
    match status {
        CommandExecutionStatus::Completed => "completed",
        CommandExecutionStatus::Failed => "failed",
        CommandExecutionStatus::InProgress => "in_progress",
    }
}

pub(super) fn tool_status_label(status: &ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Completed => "completed",
        ToolCallStatus::Failed => "failed",
        ToolCallStatus::InProgress => "in_progress",
    }
}

pub(super) fn mcp_status_label(status: Option<&McpToolCallStatus>) -> Option<&'static str> {
    match status {
        Some(McpToolCallStatus::Started) => Some("in_progress"),
        Some(McpToolCallStatus::Completed) => Some("completed"),
        Some(McpToolCallStatus::Failed) => Some("failed"),
        None => None,
    }
}

pub(super) fn patch_status_label(status: &PatchApplyStatus) -> &'static str {
    match status {
        PatchApplyStatus::Completed => "completed",
        PatchApplyStatus::Failed => "failed",
    }
}

pub(super) fn thread_completion_status(subtype: &ThreadCompletionSubtype) -> &'static str {
    match subtype {
        ThreadCompletionSubtype::Success => "completed",
        ThreadCompletionSubtype::Cancelled => "cancelled",
        ThreadCompletionSubtype::ErrorMaxTurns
        | ThreadCompletionSubtype::ErrorMaxBudgetUsd
        | ThreadCompletionSubtype::ErrorDuringExecution => "failed",
        ThreadCompletionSubtype::Unknown => "unknown",
    }
}

pub(super) fn summarize_status_with_exit(status: &str, exit_code: Option<i32>) -> String {
    match exit_code {
        Some(code) => format!("{status} (exit {code})"),
        None => status.to_string(),
    }
}

pub(super) fn summarize_status_with_label(status: &str, label: Option<&str>) -> String {
    match label {
        Some(value) => format!("{status} ({value})"),
        None => status.to_string(),
    }
}

pub(super) fn harness_title(event: &HarnessEventKind) -> &'static str {
    match event {
        HarnessEventKind::PlanningStarted => "Planning started",
        HarnessEventKind::PlanningCompleted => "Planning completed",
        HarnessEventKind::ContinuationStarted => "Continuation started",
        HarnessEventKind::ContinuationSkipped => "Continuation skipped",
        HarnessEventKind::TurnBlocked => "Turn blocked",
        HarnessEventKind::BlockedRecoveryStarted => "Blocked recovery started",
        HarnessEventKind::BlockedRecoveryFinished => "Blocked recovery finished",
        HarnessEventKind::BlockedHandoffWritten => "Blocked handoff written",
        HarnessEventKind::BlockedHandoffResolved => "Blocked handoff resolved (turn recovered)",
        HarnessEventKind::EvaluationStarted => "Evaluation started",
        HarnessEventKind::EvaluationPassed => "Evaluation passed",
        HarnessEventKind::EvaluationFailed => "Evaluation failed",
        HarnessEventKind::RevisionStarted => "Revision started",
        HarnessEventKind::VerificationStarted => "Verification started",
        HarnessEventKind::VerificationPassed => "Verification passed",
        HarnessEventKind::VerificationFailed => "Verification failed",
        HarnessEventKind::EscalationTriggered => "Escalation triggered",
        HarnessEventKind::EscalationBypassed => "Escalation bypassed",
        HarnessEventKind::ErrorRecovered => "Error recovered",
        HarnessEventKind::ToolRetryAttempted => "Tool retry attempted",
        HarnessEventKind::ToolLatencyRecorded => "Tool latency recorded",
        HarnessEventKind::SnapshotCreated => "Snapshot created",
        HarnessEventKind::SnapshotRestored => "Snapshot restored",
        HarnessEventKind::SessionToolLimitIncreased => "Session tool limit increased",
        HarnessEventKind::ToolLoopLimitIncreased => "Tool loop limit increased",
        HarnessEventKind::BackgroundSubprocessCompleted => "Background subprocess completed",
        HarnessEventKind::DelegatedAgentStatus => "Delegated agent status",
        HarnessEventKind::SessionMessageSent => "Session message sent",
        HarnessEventKind::SessionMessageReceived => "Session message received",
        HarnessEventKind::SessionMessageExpired => "Session message expired",
    }
}

pub(super) fn harness_status_label(event: &HarnessEventKind) -> &'static str {
    match event {
        HarnessEventKind::DelegatedAgentStatus => "observed",
        HarnessEventKind::PlanningCompleted
        | HarnessEventKind::EvaluationPassed
        | HarnessEventKind::VerificationPassed
        | HarnessEventKind::BlockedHandoffWritten
        | HarnessEventKind::BlockedHandoffResolved
        | HarnessEventKind::BlockedRecoveryFinished => "completed",
        HarnessEventKind::EvaluationFailed | HarnessEventKind::VerificationFailed => "failed",
        HarnessEventKind::TurnBlocked => "failed",
        HarnessEventKind::PlanningStarted
        | HarnessEventKind::ContinuationStarted
        | HarnessEventKind::ContinuationSkipped
        | HarnessEventKind::EvaluationStarted
        | HarnessEventKind::RevisionStarted
        | HarnessEventKind::VerificationStarted
        | HarnessEventKind::EscalationTriggered
        | HarnessEventKind::BlockedRecoveryStarted
        | HarnessEventKind::ToolRetryAttempted => "in_progress",
        HarnessEventKind::EscalationBypassed | HarnessEventKind::ErrorRecovered => "completed",
        HarnessEventKind::ToolLatencyRecorded => "completed",
        HarnessEventKind::SnapshotCreated
        | HarnessEventKind::SnapshotRestored
        | HarnessEventKind::SessionToolLimitIncreased
        | HarnessEventKind::ToolLoopLimitIncreased
        | HarnessEventKind::BackgroundSubprocessCompleted
        | HarnessEventKind::SessionMessageSent
        | HarnessEventKind::SessionMessageReceived
        | HarnessEventKind::SessionMessageExpired => "completed",
    }
}

pub(super) fn harness_summary(event: &vtcode_core::exec::events::HarnessEventItem) -> String {
    event
        .message
        .as_ref()
        .cloned()
        .or_else(|| event.command.as_ref().cloned())
        .or_else(|| event.path.as_ref().cloned())
        .unwrap_or_else(|| harness_title(&event.event).to_string())
}

pub(super) fn harness_body(event: &vtcode_core::exec::events::HarnessEventItem) -> String {
    let mut body = String::new();
    if let Some(message) = &event.message {
        let _ = writeln!(&mut body, "Message: {message}");
    }
    if let Some(command) = &event.command {
        let _ = writeln!(&mut body, "Command: {command}");
    }
    if let Some(path) = &event.path {
        let _ = writeln!(&mut body, "Path: {path}");
    }
    if let Some(exit_code) = event.exit_code {
        let _ = writeln!(&mut body, "Exit code: {exit_code}");
    }
    body.trim_end().to_string()
}

pub(super) fn escape_html(input: &str) -> String {
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

pub(super) fn sanitize_json_for_script_tag(input: &str) -> String {
    input.replace('&', "\\u0026").replace('<', "\\u003c").replace('>', "\\u003e")
}
