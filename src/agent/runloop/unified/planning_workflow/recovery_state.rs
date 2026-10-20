//! Request-scoped planning recovery. Canonical checkpoints precede model admission.
use vtcode_commons::formatting::truncate_byte_budget;
use vtcode_core::exec::events::planning::{PlanningOutcome, PlanningRecoverySnapshot, PlanningResponseClass};
use vtcode_core::llm::provider as uni;

use super::validate_plan_content;
use crate::agent::runloop::unified::plan_blocks::{
    extract_any_plan, looks_like_attempted_plan, looks_like_clarifying_question,
};

pub(crate) const MAX_PLANNING_RECOVERY_REQUESTS: u8 = PlanningRecoverySnapshot::MAX_REQUESTS;
const EVIDENCE_BYTES: usize = 12 * 1024;

pub(crate) fn checkpoint_event(
    snapshot: &PlanningRecoverySnapshot,
    thread_id: &str,
    turn_id: &str,
) -> vtcode_core::exec::events::ThreadEvent {
    vtcode_core::exec::events::ThreadEvent::PlanDelta(Box::new(vtcode_core::exec::events::PlanDeltaEvent {
        thread_id: thread_id.to_owned(),
        turn_id: turn_id.to_owned(),
        item_id: "planning-recovery".to_owned(),
        delta: String::new(),
        recovery: Some(Box::new(snapshot.clone())),
    }))
}

/// Only public output is eligible for retention. Private reasoning is classified by presence.
pub(crate) fn response_class(response: &uni::LLMResponse) -> PlanningResponseClass {
    if response.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty()) {
        return PlanningResponseClass::NativeToolCalls;
    }
    if response.finish_reason == uni::FinishReason::Length {
        return PlanningResponseClass::Truncation;
    }
    let text = response.content.as_deref().unwrap_or_default().trim();
    if text.is_empty() {
        return if response.reasoning.as_deref().is_some_and(|text| !text.trim().is_empty())
            || response.reasoning_details.as_ref().is_some_and(|details| !details.is_empty())
        {
            PlanningResponseClass::ReasoningOnly
        } else {
            PlanningResponseClass::EmptyContent
        };
    }
    let candidate = extract_any_plan(text).plan_text.unwrap_or_else(|| text.to_owned());
    if validate_plan_content(&candidate).is_ready() {
        PlanningResponseClass::ValidPlan
    } else if looks_like_clarifying_question(text) && !looks_like_attempted_plan(text) {
        PlanningResponseClass::ClarifyingQuestion
    } else {
        PlanningResponseClass::MalformedPlan
    }
}

pub(crate) fn close_research(
    snapshot: &mut PlanningRecoverySnapshot,
    history: &[uni::Message],
    provider: &str,
    model: &str,
) {
    if snapshot.research_closed {
        if snapshot.outcome == PlanningOutcome::Researching {
            retain_evidence(snapshot, history);
            snapshot.outcome = PlanningOutcome::Synthesizing;
        }
        return;
    }
    snapshot.research_closed = true;
    snapshot.outcome = PlanningOutcome::Synthesizing;
    snapshot.provider = provider.to_owned();
    snapshot.model = model.to_owned();
    snapshot.latest_request = history
        .iter()
        .rev()
        .filter(|message| message.role == uni::MessageRole::User)
        .map(|message| message.content.as_text())
        .find(|text| !crate::agent::runloop::unified::turn::is_internal_harness_follow_up(text))
        .map(|text| redact_bounded(&text, 8192))
        .unwrap_or_default();
    retain_evidence(snapshot, history);
    // A complete earlier draft must survive approval-only repair prose.
    for message in history.iter().filter(|message| message.role == uni::MessageRole::Assistant) {
        retain_draft(snapshot, &message.content.as_text());
    }
}

fn retain_evidence(snapshot: &mut PlanningRecoverySnapshot, history: &[uni::Message]) {
    // Merge a bounded public tail with saved evidence, including facts learned
    // by an explicitly requested clarification batch after context compaction.
    let mut recent: Vec<_> = history
        .iter()
        .rev()
        .filter(|message| message.role == uni::MessageRole::Tool)
        .take(8)
        .map(|message| redact_bounded(&message.content.as_text(), 2048))
        .filter(|text| !text.is_empty())
        .collect();
    recent.reverse();
    for evidence in recent {
        if !snapshot.evidence.contains(&evidence) {
            snapshot.evidence.push(evidence);
        }
    }
    let mut bytes: usize = snapshot.evidence.iter().map(String::len).sum();
    while bytes > EVIDENCE_BYTES || snapshot.evidence.len() > 8 {
        bytes = bytes.saturating_sub(snapshot.evidence.remove(0).len());
    }
}

pub(crate) fn admit_request(snapshot: &mut PlanningRecoverySnapshot) -> bool {
    snapshot.admit_request()
}

pub(crate) fn retain_draft(snapshot: &mut PlanningRecoverySnapshot, text: &str) {
    let candidate = extract_any_plan(text).plan_text.unwrap_or_else(|| text.trim().to_owned());
    vtcode_core::tools::handlers::planning_workflow::retain_plan_recovery_draft(snapshot, &candidate);
}

pub(crate) fn begin_explicit_cycle(snapshot: &mut PlanningRecoverySnapshot) {
    snapshot.begin_explicit_cycle();
}

/// Old sessions cannot prove unused recovery allowance. Retain their public
/// progress and require explicit input before admitting another model request.
pub(crate) fn recover_legacy_session(
    history: &[uni::Message],
    provider: &str,
    model: &str,
) -> PlanningRecoverySnapshot {
    let mut snapshot = PlanningRecoverySnapshot::default();
    close_research(&mut snapshot, history, provider, model);
    snapshot.requests_used = MAX_PLANNING_RECOVERY_REQUESTS;
    snapshot.outcome = PlanningOutcome::AwaitingInput;
    snapshot
}

pub(crate) fn record_clarification(snapshot: &mut PlanningRecoverySnapshot, text: &str, research_missing_facts: bool) {
    let text = redact_bounded(text.trim(), 4096);
    if text.is_empty() {
        return;
    }
    let clarification = format!("\nClarification: {text}");
    let previous = redact_bounded(&snapshot.latest_request, 8192usize.saturating_sub(clarification.len()));
    snapshot.latest_request = format!("{previous}{clarification}");
    if research_missing_facts {
        snapshot.targeted_research = Some(text);
    }
}

pub(crate) fn redact_bounded(text: &str, bytes: usize) -> String {
    let redacted = vtcode_commons::sanitizer::redact_secrets(text.to_owned());
    if redacted.len() <= bytes {
        return redacted;
    }
    let suffix = if bytes >= "…".len() { "…" } else { "" };
    truncate_byte_budget(&redacted, bytes.saturating_sub(suffix.len()), suffix)
}

#[cfg(test)]
#[path = "recovery_state_tests.rs"]
mod tests;
