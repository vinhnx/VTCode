//! Post-tool failure classification: recovery routing and error-kind predicates.
//!
//! Extracted from post_tool_recovery.rs so the recovery enum, event context,
//! and misconfiguration/transient/capacity classifiers live in one place.
//! No behavior change: routing variants and predicate semantics are verbatim.

use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use vtcode_core::llm::provider as uni;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PostToolFailureRecovery {
    NotApplicable,
    RetryToolEnabled,
    RetryToolFree,
    StopAfterDirective,
}

#[derive(Clone, Copy)]
pub(crate) struct PlanRecoveryEventContext<'a> {
    pub(crate) emitter: Option<&'a HarnessEventEmitter>,
    pub(crate) thread_id: &'a str,
    pub(crate) turn_id: &'a str,
}

pub(crate) fn has_tool_response_since(messages: &[uni::Message], baseline_len: usize) -> bool {
    messages
        .get(baseline_len..)
        .is_some_and(|recent| recent.iter().any(|msg| msg.role == uni::MessageRole::Tool))
}

/// Context-capacity recovery is valid only for the provider request itself.
/// Parsing and tool-result failures may contain similar wording in arbitrary
/// payloads; treating those as prompt overflow would discard useful history
/// and schedule an unrelated write-capable retry.
pub(crate) fn is_provider_context_capacity_failure(failure_stage: &str, err: &anyhow::Error) -> bool {
    failure_stage == "execute_llm_request" && vtcode_commons::is_context_capacity_error(err)
}

pub(crate) fn misconfiguration_guidance(err: &anyhow::Error) -> Option<vtcode_commons::ConfigGuidance> {
    let message = format!("{err:#}");
    if message.contains("Correct the configuration before retrying.") {
        return None;
    }
    vtcode_commons::detect_misconfiguration_in_anyhow(err)
}

pub(crate) fn is_misconfiguration_error(err: &anyhow::Error) -> bool {
    vtcode_commons::detect_misconfiguration_in_anyhow(err).is_some()
}

pub(crate) fn is_transient_recovery_error(err: &anyhow::Error) -> bool {
    vtcode_commons::classify_anyhow_error(err).is_retryable() && !is_misconfiguration_error(err)
}
