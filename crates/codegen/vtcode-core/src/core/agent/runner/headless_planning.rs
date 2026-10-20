//! Headless planning handoffs use the canonical planning checkpoint and diagnostics.

use crate::core::agent::events::SessionStoreSinkHandle;
use crate::exec::events::planning::{PlanningRecoverySnapshot, PlanningResponseClass};
use crate::exec::events::{PlanDeltaEvent, ThreadEvent};
use crate::llm::provider::{FinishReason, LLMResponse, Message, MessageRole};
use crate::tools::handlers::planning_workflow::validate_plan_content;
use anyhow::{Context, Result};
use vtcode_commons::diagnostics::{Diagnostic, DiagnosticAction, DiagnosticSeverity, redact_diagnostic};
use vtcode_commons::formatting::truncate_byte_budget;
use vtcode_commons::sanitizer::redact_secrets;

pub(super) fn request_fingerprint(task: &crate::core::agent::task::Task) -> u64 {
    crate::core::agent::hash_utils::hash_value(&(&task.description, &task.instructions))
}

pub(super) fn bounded_public_text(text: &str, bytes: usize) -> String {
    truncate_byte_budget(&redact_secrets(text.to_owned()), bytes, "\n[truncated]")
}

/// Close research once, retaining public tool evidence and useful draft content.
pub(super) fn retain_progress(
    snapshot: &mut PlanningRecoverySnapshot,
    history: &[Message],
    request: &str,
    provider: &str,
    model: &str,
) {
    snapshot.research_closed = true;
    snapshot.provider = provider.to_owned();
    snapshot.model = model.to_owned();
    snapshot.latest_request = bounded_public_text(request, 8192);
    let mut remaining = 12 * 1024;
    let evidence: Vec<_> = history
        .iter()
        .rev()
        .filter(|message| message.role == MessageRole::Tool)
        .take(8)
        .filter_map(|message| {
            let text = bounded_public_text(&message.content.as_text(), remaining.min(2048));
            remaining = remaining.saturating_sub(text.len());
            (!text.is_empty()).then_some(text)
        })
        .collect();
    if !evidence.is_empty() {
        snapshot.evidence = evidence;
        snapshot.evidence.reverse();
    }
    for message in history.iter().filter(|message| message.role == MessageRole::Assistant) {
        retain_draft(snapshot, &message.content.as_text());
    }
}

pub(super) use crate::tools::handlers::planning_workflow::retain_plan_recovery_draft as retain_draft;

pub(super) fn classify_response(response: &LLMResponse, tool_violation: bool) -> PlanningResponseClass {
    if tool_violation || response.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty()) {
        PlanningResponseClass::NativeToolCalls
    } else if response.finish_reason == FinishReason::Length {
        PlanningResponseClass::Truncation
    } else if response.content_text().trim().is_empty() {
        if response.reasoning.as_deref().is_some_and(|text| !text.trim().is_empty())
            || response.reasoning_details.as_ref().is_some_and(|details| !details.is_empty())
        {
            PlanningResponseClass::ReasoningOnly
        } else {
            PlanningResponseClass::EmptyContent
        }
    } else if validate_plan_content(response.content_text()).is_ready() {
        PlanningResponseClass::ValidPlan
    } else if response.content_text().trim_end().ends_with('?') {
        PlanningResponseClass::ClarifyingQuestion
    } else {
        PlanningResponseClass::MalformedPlan
    }
}

pub(super) async fn persist_checkpoint(
    sink: Option<&SessionStoreSinkHandle>,
    snapshot: &PlanningRecoverySnapshot,
    session_id: &str,
    turn: usize,
) -> Result<()> {
    let sink = sink.context("headless planning requires canonical checkpoint persistence")?;
    sink.persist_checkpoint(&ThreadEvent::PlanDelta(Box::new(PlanDeltaEvent {
        thread_id: session_id.to_owned(),
        turn_id: format!("turn-{turn}"),
        item_id: "planning-recovery".to_owned(),
        delta: String::new(),
        recovery: Some(Box::new(snapshot.clone())),
    })))
    .await
}

pub(super) fn awaiting_input_diagnostic(
    snapshot: &PlanningRecoverySnapshot,
    session_id: &str,
    failure: &str,
) -> Diagnostic {
    redact_diagnostic(&Diagnostic {
        severity: DiagnosticSeverity::Warning,
        code: "planning.awaiting_input".into(),
        summary: "Planning needs your input. Saved progress is available.".into(),
        agent_details: serde_json::json!({
            "outcome": "awaiting_input", "category": snapshot.response_class,
            "provider": snapshot.provider, "model": snapshot.model,
            "requests_used": snapshot.requests_used, "remaining_allowance": 0,
            "cycle": snapshot.cycle, "failure": bounded_public_text(failure, 2048),
            "validator_defects": snapshot.validation_report, "evidence_items": snapshot.evidence.len(),
            "retained_draft": snapshot.best_draft.is_some(),
            "recovery": "Wait for explicit retry, clarification, or model selection. Research stays closed. Implementation requires separate validated persisted plan approval."
        }).to_string(),
        correlation_id: format!("{session_id}-{}-planning-{}", snapshot.request_id.as_deref().unwrap_or("request"), snapshot.cycle),
        actions: vec![
            DiagnosticAction { id: "retry".into(), label: "Retry from retained evidence".into() },
            DiagnosticAction { id: "clarify".into(), label: "Add clarification".into() },
            DiagnosticAction { id: "model".into(), label: "Choose another model".into() },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_identity_includes_input_beyond_the_display_budget_and_instructions() {
        let prefix = "x".repeat(9000);
        let mut task = crate::core::agent::task::Task::new("exec-task".into(), "Plan".into(), format!("{prefix}first"));
        let first = request_fingerprint(&task);
        task.description = format!("{prefix}second");
        let second = request_fingerprint(&task);
        assert_ne!(first, second);
        task.instructions = Some("Use saved evidence only".into());
        assert_ne!(second, request_fingerprint(&task));
    }

    #[test]
    fn input_handoffs_from_different_tasks_keep_distinct_incidents() {
        let mut snapshot = PlanningRecoverySnapshot {
            request_id: Some("task-1".into()),
            ..Default::default()
        };
        let first = awaiting_input_diagnostic(&snapshot, "session", "empty content");
        snapshot.request_id = Some("task-2".into());
        let second = awaiting_input_diagnostic(&snapshot, "session", "empty content");
        assert_ne!(first.correlation_id, second.correlation_id);
    }
}
