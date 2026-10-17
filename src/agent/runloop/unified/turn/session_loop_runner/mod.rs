//! Session-loop orchestration and focused lifecycle helpers.

mod archive;
mod blocked_handoff;
mod handoff;
mod harness;
mod metrics;
mod notifications;
mod orchestration;
mod plan_seed;
mod support;
mod turn_tail;
mod verification_recovery;

#[cfg(test)]
mod tests;

pub(super) use orchestration::run_single_agent_loop_unified_impl;

/// Re-exported so harness-quiet matching in `turn_loop_helpers` can key off
/// the producer-owned prompt openings instead of duplicated literals.
pub(crate) use blocked_handoff::VERIFICATION_AUTO_RECOVERY_PREFIX;
pub(crate) use orchestration::BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX;
