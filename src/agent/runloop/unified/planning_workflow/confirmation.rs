//! Plan confirmation HITL flows — facade.
//!
//! Re-exports the two split modules so callers can import from a single path.
//! The start-planning entry confirmation and the plan-approval overlay are
//! independent flows with separate lifecycles.

pub(crate) use super::plan_approval::execute_plan_approval;
pub(crate) use super::start_confirmation::{StartPlanningDecision, present_start_planning_confirmation};
