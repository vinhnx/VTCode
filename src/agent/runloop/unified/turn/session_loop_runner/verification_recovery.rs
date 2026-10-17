//! Verification recovery decisions and queue admission, independent of session UI.

use super::blocked_handoff::{
    verification_auto_recovery_follow_up, verification_auto_recovery_status_line, verification_exhausted_handoff_reason,
};
use super::support::latest_assistant_result_text;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::turn::tool_outcomes::helpers as verification_gate;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::runtime::AgentRuntime;
use vtcode_core::llm::provider::Message;
use vtcode_core::tools::ToolRegistry;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum VerificationBlockResult {
    NotVerification,
    Queued { status_line: String },
    Handoff { reason: String },
}

/// Queue before publishing a directive; the session loop retains Exit/continue ownership.
pub(super) fn handle_verification_block(
    base: &str,
    cfg: Option<&VTCodeConfig>,
    registry: &ToolRegistry,
    runtime: &mut AgentRuntime,
    stats: &mut SessionStats,
) -> VerificationBlockResult {
    if !verification_gate::is_pending_verification_block(base) {
        return VerificationBlockResult::NotVerification;
    }

    let escalated =
        stats.verification_consecutive_failures() >= verification_gate::verification_max_consecutive_failures(cfg);
    let max_turns = verification_gate::verification_cross_turn_turns(cfg);
    let verifier = verification_gate::resolve_pending_verifier_command(cfg, registry, &runtime.state.messages);
    if !stats.preflight_recovery.exhausted()
        && !escalated
        && verification_gate::verification_block_allows_auto_recovery(
            base,
            latest_assistant_result_text(&runtime.state.messages).as_deref().unwrap_or(""),
        )
        && stats.verification_auto_recovery_turns() < max_turns
    {
        let attempt = stats.verification_auto_recovery_turns().saturating_add(1);
        let directive =
            vtcode_core::tools::tool_intent::verification_recovery_directive(verifier.as_deref(), attempt, max_turns);
        match runtime.try_queue_follow_up_input(verification_auto_recovery_follow_up(verifier.as_deref())) {
            Ok(()) => {
                let recorded = stats.record_verification_auto_recovery_turn_with_limit(max_turns);
                debug_assert!(recorded, "exclusive recovery admission must retain its checked budget");
                std::sync::Arc::make_mut(&mut runtime.state.messages).push(Message::system(directive));
                stats.mark_turn_stalled(true, Some(base.to_owned()));
                // Queued input bypasses the interaction loop. Suppressing the
                // next follow-up would instead hide the next genuine user continue.
                return VerificationBlockResult::Queued {
                    status_line: verification_auto_recovery_status_line(verifier.as_deref(), attempt, max_turns),
                };
            }
            Err(error) => {
                tracing::warn!(%error, "Verification auto-recovery queue full; writing blocked handoff");
            }
        }
    }

    VerificationBlockResult::Handoff {
        reason: verification_exhausted_handoff_reason(
            base,
            verifier.as_deref(),
            stats.verification_auto_recovery_turns(),
            max_turns,
            stats.last_verification_failure(),
        ),
    }
}

#[cfg(test)]
mod tests;
