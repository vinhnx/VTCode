//! Planning workflow tools and supporting logic.
//!
//! The original 1800-line `planning_workflow.rs` monolith was decomposed into
//! focused, individually testable modules while preserving the exact public
//! surface consumed by `handlers/mod.rs` and the task-tracker / exec-harness
//! readers:
//!
//! - [`artifacts`]: pure, side-effect-free plan/tracker marker handling,
//!   section parsing, validation, and tracker generation.
//! - [`persistence`]: file I/O — draft persistence, plan<->tracker sync,
//!   validation-command detection.
//! - [`state`]: [`PlanningWorkflowState`] shared permission state.
//! - [`start`]: `start_planning` tool (enter planning workflow).

pub mod artifacts;
pub mod persistence;
pub mod start;
pub mod state;

// Preserved external surface. Do not remove without updating the consumers in
// `handlers/mod.rs`, `task_tracker.rs`, `planning_task_tracker.rs`,
// `continuation.rs`, `turn/context.rs`, and `turn/.../plan_seed.rs`.
pub(in crate::tools::handlers) use artifacts::render_plan_with_tracker;
pub use artifacts::{
    CANONICAL_STEP_FORMAT, PLANNING_VERIFY_INVALID_EXAMPLES, PLANNING_VERIFY_VALID_EXAMPLES, PlanValidationReport,
    generate_tracker_markdown_from_plan, merge_plan_content, plan_file_for_tracker_file, retain_plan_recovery_draft,
    split_bracket_items, tracker_file_for_plan_file, validate_plan_content,
};
pub use persistence::{
    PersistedPlanDraft, allocate_plan_file_if_missing, persist_plan_draft, sync_tracker_into_plan_file,
};
pub use start::StartPlanningTool;
pub use state::PlanningWorkflowState;

#[cfg(test)]
mod planning_artifact_regression_tests;
#[cfg(test)]
mod tests;
