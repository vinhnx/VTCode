//! Planning-workflow facade.
//!
//! Single module boundary for the planning domain. The runloop must depend only
//! on this facade (the `pub(crate)` re-exports below), never on individual
//! submodule paths or on `vtcode-core`'s planning tool internals. Submodules are
//! `pub(crate)` internals so the domain stays cohesively isolated and each piece
//! remains independently testable (intent detection, HITL confirmation, tool
//! dispatch, truncation recovery).
//!
//! This is the interface guard rail for the next-generation planning refactor:
//! widening the public surface means editing the re-exports here, which makes
//! accidental cross-module coupling visible at review time.

pub(crate) mod confirmation;
pub(crate) mod execution;
pub(crate) mod exit_trigger;
pub(crate) mod intent;
pub(crate) mod plan_approval;
pub(crate) mod recovery;
pub(crate) mod start_confirmation;

// --- Stable interface (the only planning symbols the runloop should name) ---

pub(crate) use super::planning_workflow_state::finish_planning_workflow;
pub(crate) use confirmation::{StartPlanningDecision, execute_plan_approval, present_start_planning_confirmation};
pub(crate) use execution::handle_start_planning;
pub(crate) use exit_trigger::maybe_handle_planning_exit_trigger;
pub(crate) use intent::{
    PlanningIntent, assistant_recently_prompted_implementation, detect_enter_planning_intent, detect_planning_intent,
};
pub(crate) use recovery::maybe_condense_truncated_plan;
