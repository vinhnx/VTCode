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
pub use artifacts::{
    PlanValidationReport, generate_tracker_markdown_from_plan, merge_plan_content, plan_file_for_tracker_file,
    tracker_file_for_plan_file, validate_plan_content,
};
pub use persistence::{PersistedPlanDraft, persist_plan_draft, sync_tracker_into_plan_file};
pub use start::StartPlanningTool;
pub use state::PlanningWorkflowState;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    use super::artifacts::{
        PLAN_TRACKER_END, PLAN_TRACKER_START, generate_tracker_markdown_from_plan, render_plan_with_tracker,
    };
    use super::persistence::detect_validation_command_hints;
    use crate::tools::traits::Tool;
    use serde_json::json;

    #[tokio::test]
    async fn test_start_planning() {
        let temp_dir = TempDir::new().unwrap();
        let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
        let tool = StartPlanningTool::new(state.clone());

        // Initially not in planning workflow
        assert!(!state.is_active());

        // Enter planning workflow
        let result = tool
            .execute(json!({
                "plan_name": "test-plan",
                "description": "Test planning"
            }))
            .await
            .unwrap();

        // Should be in planning workflow now
        assert!(state.is_active());
        assert_eq!(result["status"], "success");

        // Plan file should exist
        let plan_file = state.get_plan_file().await.unwrap();
        assert!(plan_file.exists());
        assert_eq!(plan_file, temp_dir.path().join(".vtcode").join("plans").join("test-plan.md"));

        let content = std::fs::read_to_string(&plan_file).unwrap();
        assert!(content.contains("# Test Plan"));
        assert!(content.contains("Status: drafting"));
        assert!(content.contains(&format!("Plan file: `{}`", plan_file.display())));
        assert!(content.contains("Description: Test planning"));
        assert!(!content.contains("Repository facts checked"));
        assert!(!content.contains("[Step]"));
        assert!(!content.contains("## Implementation Steps"));
    }

    #[tokio::test]
    async fn test_start_planning_returns_pending_confirmation_when_requested() {
        let temp_dir = TempDir::new().unwrap();
        let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
        let tool = StartPlanningTool::new(state.clone());

        let result = tool
            .execute(json!({
                "plan_name": "confirm-me",
                "require_confirmation": true
            }))
            .await
            .unwrap();

        assert_eq!(result["status"], "pending_confirmation");
        assert_eq!(result["requires_confirmation"], true);
        assert!(!state.is_active());
        assert!(state.get_plan_file().await.is_none());
    }

    #[test]
    fn test_detect_validation_hints_for_rust_workspace() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(temp_dir.path().join("Cargo.toml"), "[package]\nname='x'\n").unwrap();

        let hints = detect_validation_command_hints(temp_dir.path());
        assert!(hints.build_and_lint.contains("cargo check"));
        assert!(hints.build_and_lint.contains("cargo clippy"));
        assert!(hints.tests.contains("cargo test"));
    }

    #[test]
    fn test_detect_validation_hints_for_node_workspace() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(
            temp_dir.path().join("package.json"),
            r#"{"name":"x","scripts":{"build":"tsc","lint":"eslint .","test":"vitest run"}}"#,
        )
        .unwrap();
        std::fs::write(temp_dir.path().join("pnpm-lock.yaml"), "lockfileVersion: 9").unwrap();

        let hints = detect_validation_command_hints(temp_dir.path());
        assert!(hints.build_and_lint.contains("pnpm run build"));
        assert!(hints.build_and_lint.contains("pnpm run lint"));
        assert_eq!(hints.tests, "`pnpm run test`");
    }

    #[tokio::test]
    async fn test_already_in_planning_workflow() {
        let temp_dir = TempDir::new().unwrap();
        let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
        state.enable();
        let plans_dir = state.plans_dir();
        std::fs::create_dir_all(&plans_dir).unwrap();
        let plan_file = plans_dir.join("test.md");
        std::fs::write(&plan_file, "# Test Plan\n").unwrap();
        state.set_plan_file(Some(plan_file)).await;

        let tool = StartPlanningTool::new(state);
        let result = tool.execute(json!({})).await.unwrap();

        assert_eq!(result["status"], "already_active");
    }

    #[tokio::test]
    async fn test_already_active_initializes_missing_plan_file() {
        let temp_dir = TempDir::new().unwrap();
        let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
        state.enable();

        let tool = StartPlanningTool::new(state.clone());
        let result = tool
            .execute(json!({
                "plan_name": "missing-plan"
            }))
            .await
            .unwrap();

        assert_eq!(result["status"], "already_active");
        let plan_file = state.get_plan_file().await.expect("plan file should be set");
        assert!(plan_file.exists());
        assert!(
            plan_file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .contains("missing-plan")
        );
    }

    #[test]
    fn validate_plan_content_rejects_placeholder_template() {
        let report = validate_plan_content(
            r#"# Test Plan

Repository facts checked:
- [file, symbol, or behavior confirmed from the repo]

Next open decision: [if any], otherwise: No remaining scope decisions.

## Summary
[2-4 lines: goal, user impact, what will change, what will not]

## Implementation Steps
1. [Step] -> files: [paths] -> verify: [check]

## Test Cases and Validation
1. Build and lint: [project build and lint command(s)]

## Assumptions and Defaults
1. [Explicit assumption]
"#,
        );

        assert!(!report.is_ready());
        assert!(!report.placeholder_tokens.is_empty());
        assert!(report.placeholder_tokens.iter().any(|token| token.contains("file, symbol")));
    }

    #[test]
    fn validate_plan_content_accepts_concrete_plan() {
        let report = validate_plan_content(
            r#"# Fix Planning workflow

## Summary
Persist the reviewed plan draft and route execution through explicit approval.

## Implementation Steps
1. Add plan lifecycle state -> files: [crates/codegen/vtcode-core/src/tools/handlers/planning_workflow.rs] -> verify: [cargo test -p vtcode-core test_start_planning -- --nocapture]
2. Gate plan entry with overlay approval -> files: [src/agent/runloop/unified/tool_pipeline/execution_planning.rs] -> verify: [cargo test -p vtcode test_run_tool_call_prevalidated_allows_task_tracker_in_planning_workflow -- --nocapture]

## Test Cases and Validation
1. Build and lint: cargo check
2. Tests: cargo test -p vtcode-core test_start_planning -- --nocapture

## Assumptions and Defaults
1. Keep tracker sidecars for compatibility.
2. Reuse the existing overlay infrastructure.
"#,
        );

        assert!(report.is_ready());
    }

    #[tokio::test]
    async fn persist_plan_draft_generates_tracker_and_global_task_file() {
        let temp_dir = TempDir::new().unwrap();
        let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
        let tool = StartPlanningTool::new(state.clone());
        tool.execute(json!({"plan_name":"draft-sync","approved":true})).await.unwrap();

        let persisted = persist_plan_draft(
            &state,
            r#"# Draft Sync

## Summary
Persist a concrete draft and seed tracker state.

## Implementation Steps
1. Persist the plan -> files: [crates/codegen/vtcode-core/src/tools/handlers/planning_workflow.rs] -> verify: [cargo test]
2. Sync the tracker -> files: [crates/codegen/vtcode-core/src/tools/handlers/task_tracker.rs] -> verify: [cargo test]

## Test Cases and Validation
1. Build and lint: cargo check
2. Tests: cargo test

## Assumptions and Defaults
1. Keep task tracker mirrors.
"#,
        )
        .await
        .unwrap();

        let tracker_file = persisted.tracker_file.expect("tracker file should exist");
        let plan_content = std::fs::read_to_string(&persisted.plan_file).unwrap();
        let tracker_content = std::fs::read_to_string(&tracker_file).unwrap();
        let global_task =
            std::fs::read_to_string(temp_dir.path().join(".vtcode").join("tasks").join("current_task.md")).unwrap();

        assert!(persisted.validation.is_ready());
        assert!(plan_content.contains(PLAN_TRACKER_START));
        assert!(plan_content.contains("Persist the plan"));
        assert!(tracker_content.contains("- [ ] Persist the plan"));
        assert!(global_task.contains("- [ ] Persist the plan"));
    }

    #[test]
    fn merge_plan_content_uses_canonical_marker_form() {
        let plan = "# Test Plan\n\n## Summary\nConcrete summary.\n\n## Implementation Steps\n1. Step one -> files: [src/a.rs] -> verify: [cargo test]\n\n## Test Cases and Validation\n1. Build and lint: cargo check\n\n## Assumptions and Defaults\n1. Assume nothing.\n";
        let tracker = "# Updated Plan\n\n## Plan of Work\n- [~] Embedded step\n";

        // A plan file that was already persisted (carries markers) must not
        // double-embed the tracker when merged with the sidecar again.
        let persisted_plan = render_plan_with_tracker(plan, Some(tracker));
        assert!(persisted_plan.contains(PLAN_TRACKER_START));
        assert!(persisted_plan.contains(PLAN_TRACKER_END));

        let merged = merge_plan_content(Some(persisted_plan.clone()), Some(tracker.to_string()))
            .expect("merge should produce content");
        assert!(merged.contains(PLAN_TRACKER_START));
        assert!(merged.contains(PLAN_TRACKER_END));
        assert_eq!(merged.matches(PLAN_TRACKER_START).count(), 1, "tracker must be embedded exactly once");
        assert!(merged.contains("- [~] Embedded step"));
    }

    #[test]
    fn generate_tracker_markdown_from_plan_emits_checklist() {
        let plan = "# Test Plan\n\n## Summary\nConcrete.\n\n## Implementation Steps\n1. Step one -> files: [src/a.rs] -> verify: [cargo test]\n2. Step two -> files: [src/b.rs] -> verify: [cargo check]\n\n## Test Cases and Validation\n1. Build and lint: cargo check\n\n## Assumptions and Defaults\n1. Assume nothing.\n";
        let tracker = generate_tracker_markdown_from_plan(plan).expect("tracker generated");
        assert!(tracker.contains("- [ ] Step one"));
        assert!(tracker.contains("- [ ] Step two"));
        assert!(!tracker.contains("[ ] Step one -> files"));
    }

    #[test]
    fn planning_tool_descriptions_do_not_expose_internal_unified_tools() {
        fn internal_unified_tool_name(suffix: &str) -> String {
            format!("unified_{suffix}")
        }

        let temp_dir = TempDir::new().unwrap();
        let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
        let start_tool = StartPlanningTool::new(state);

        let description = start_tool.description();
        assert!(!description.contains(&internal_unified_tool_name("file")));
        assert!(!description.contains(&internal_unified_tool_name("exec")));
        assert!(!description.contains(&internal_unified_tool_name("search")));

        assert!(start_tool.description().contains("exec_command"));
        assert!(start_tool.description().contains("apply_patch"));
    }
}
