//! Pure planning-artifact helpers behind stable public imports.
//!
//! Parsing, validation, and tracker conversion remain side-effect-free.
//! Tool wiring and I/O live in the surrounding planning-workflow module.

mod sections;
mod steps;
mod tracker;
mod validation;
mod verification;

pub use steps::split_bracket_items;
pub(in crate::tools::handlers) use tracker::render_plan_with_tracker;
pub(super) use tracker::{extract_embedded_tracker, tracker_has_progress_or_notes};
pub use tracker::{
    generate_tracker_markdown_from_plan, merge_plan_content, plan_file_for_tracker_file, tracker_file_for_plan_file,
};
pub use validation::{
    CANONICAL_STEP_FORMAT, PLANNING_VERIFY_INVALID_EXAMPLES, PLANNING_VERIFY_VALID_EXAMPLES, PlanValidationReport,
    validate_plan_content,
};

pub(super) const PLAN_TRACKER_START: &str = "<!-- vtcode:plan-tracker:start -->";
pub(super) const PLAN_TRACKER_END: &str = "<!-- vtcode:plan-tracker:end -->";

#[cfg(test)]
mod agentic_testing_tests;
