pub(super) use serde_json::json;
pub(super) use vtcode_core::config::constants::tools as tool_names;

pub(super) use super::{
    apply_balancer_recovery, is_readonly_signature, navigation_loop_guidance, validate_tool_args_security,
};
pub(super) use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
pub(super) use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult};
pub(super) use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
    EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD, LoopTracker, NAVIGATION_LOOP_THRESHOLD,
    PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD, PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD, update_repetition_tracker,
};
pub(super) use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

fn assert_recovery_expires_at_fresh_turn(history: &[vtcode_core::llm::provider::Message]) {
    use crate::agent::runloop::unified::turn::turn_loop_helpers::restore_fresh_turn_tool_guidance;

    let mut restored = history.to_vec();
    restore_fresh_turn_tool_guidance(&mut restored, false);
    assert_eq!(restored.len(), history.len() + 1, "the guard's emitted directive must be superseded");
    restore_fresh_turn_tool_guidance(&mut restored, false);
    assert_eq!(restored.len(), history.len() + 1, "restoration must not duplicate itself");
}

mod churn_reclaim;
mod empty_search;
mod execution_guard;
mod listing_convergence;
mod planning_productive;
mod planning_thresholds;
mod validation_guidance;
