//! Sidebar snapshot for the session's approved plan.
//!
//! Built once at the approval boundary from the validated artifact and pushed
//! to the TUI via [`InlineHandle::set_active_plan`](vtcode_ui::tui::app::InlineHandle::set_active_plan).
//! The snapshot is display-only: the plan file on disk stays authoritative and
//! the sidebar entry is cleared on the next planning entry.

use super::ValidatedPlanArtifact;
use super::events::display_plan_path;
use super::{PlanExecutionContext, PlanExecutionDestination, PlanExecutionTarget, PlanningWorkflowState};
use vtcode_ui::tui::app::{ActivePlanEntry, PlanContent};

/// Sidebar step titles are capped so the panel keeps a stable height; the
/// entry carries the true total and the widget renders a `+N more` row.
const MAX_SIDEBAR_PLAN_STEP_TITLES: usize = 8;

/// Preformatted approval badge for the sidebar. Auto ignores the execution
/// context: the destination is the decision-relevant information.
#[must_use]
fn approval_status_label(target: PlanExecutionTarget) -> &'static str {
    match (target.destination, target.execution_context) {
        (PlanExecutionDestination::Auto, _) => "approved -> auto",
        (PlanExecutionDestination::Build, PlanExecutionContext::Fresh) => "approved -> build (fresh)",
        (PlanExecutionDestination::Build, PlanExecutionContext::Current) => "approved -> build",
    }
}

/// Build the sidebar snapshot from the validated approval artifact.
///
/// Titles and total come from the same [`PlanContent`] parse the approval
/// overlay flattens, so they stay consistent: every numbered plan line counts,
/// not only implementation steps. Titles keep their full canonical text
/// (`Action -> files: [..] -> verify: [..]`); the action leads, so sidebar
/// width truncation degrades gracefully without re-parsing step metadata here.
pub(crate) fn active_plan_entry(
    plan: &ValidatedPlanArtifact,
    plan_state: &PlanningWorkflowState,
    target: PlanExecutionTarget,
) -> ActivePlanEntry {
    let content = PlanContent::from_markdown(String::new(), &plan.text, None);
    let step_titles: Vec<String> = content
        .phases
        .iter()
        .flat_map(|phase| phase.steps.iter().map(|step| step.description.clone()))
        .take(MAX_SIDEBAR_PLAN_STEP_TITLES)
        .collect();
    ActivePlanEntry {
        file_path: display_plan_path(plan_state, &plan.plan_file),
        status: approval_status_label(target).to_string(),
        step_titles,
        total_steps: content.total_steps,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{PlanExecutionContext, PlanExecutionTarget, validate_plan_content};
    use super::*;
    use vtcode_core::tools::registry::ToolRegistry;
    use vtcode_ui::tui::app::{InlineCommand, InlineHandle};

    const VALID_PLAN: &str = r#"# Plan

## Summary
A valid plan.

## Implementation Steps
1. Do it -> files: [src/lib.rs] -> verify: [cargo check]

## Test Cases and Validation
1. Run cargo check.

## Assumptions and Defaults
1. Keep existing behavior.
"#;

    fn plan_text_with_steps(step_count: usize) -> String {
        let mut text = String::from("# Plan\n\n## Summary\nA valid plan.\n\n## Implementation Steps\n");
        for index in 1..=step_count {
            text.push_str(&format!("{index}. Do step {index} -> files: [src/lib.rs] -> verify: [cargo check]\n"));
        }
        text.push_str("\n## Test Cases and Validation\n1. Run cargo check.\n\n## Assumptions and Defaults\n1. Keep existing behavior.\n");
        text
    }

    async fn test_registry() -> (tempfile::TempDir, ToolRegistry) {
        let tmp = tempfile::TempDir::new().unwrap();
        let workspace = vtcode_commons::canonicalize(tmp.path()).unwrap();
        let registry = ToolRegistry::new(workspace).await;
        (tmp, registry)
    }

    fn test_handle() -> (InlineHandle, tokio::sync::mpsc::UnboundedReceiver<InlineCommand>) {
        let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
        (InlineHandle::new_for_tests(command_tx), command_rx)
    }

    fn drain_set_active_plan_commands(
        command_rx: &mut tokio::sync::mpsc::UnboundedReceiver<InlineCommand>,
    ) -> Vec<Option<ActivePlanEntry>> {
        let mut entries = Vec::new();
        while let Ok(command) = command_rx.try_recv() {
            if let InlineCommand::SetActivePlan { entry } = command {
                entries.push(entry);
            }
        }
        entries
    }

    #[test]
    fn approval_status_label_covers_all_targets() {
        let build = PlanExecutionTarget::build(PlanExecutionContext::Current, false);
        let fresh_build = PlanExecutionTarget::build(PlanExecutionContext::Fresh, false);
        let auto = PlanExecutionTarget::auto(PlanExecutionContext::Current);
        let fresh_auto = PlanExecutionTarget::auto(PlanExecutionContext::Fresh);
        assert_eq!(approval_status_label(build), "approved -> build");
        assert_eq!(approval_status_label(fresh_build), "approved -> build (fresh)");
        assert_eq!(approval_status_label(auto), "approved -> auto");
        assert_eq!(approval_status_label(fresh_auto), "approved -> auto");
    }

    #[tokio::test]
    async fn active_plan_entry_flattens_steps_and_caps_titles() {
        let (_tmp, registry) = test_registry().await;
        let plan_state = registry.planning_workflow_state();
        let text = plan_text_with_steps(10);
        let validation = validate_plan_content(&text);
        assert!(validation.is_ready());
        let plan_file = plan_state.plans_dir().join("task.md");
        let plan = ValidatedPlanArtifact::from_validated(plan_file, text, validation);
        let target = PlanExecutionTarget::build(PlanExecutionContext::Current, false);

        let entry = active_plan_entry(&plan, &plan_state, target);

        // Total covers every numbered plan line (10 implementation + validation
        // + assumption), matching the parser the approval overlay uses.
        assert_eq!(entry.file_path, ".vtcode/plans/task.md");
        assert_eq!(entry.status, "approved -> build");
        assert_eq!(entry.total_steps, 12);
        assert_eq!(entry.step_titles.len(), MAX_SIDEBAR_PLAN_STEP_TITLES);
        assert!(entry.step_titles[0].starts_with("Do step 1 "));
        assert!(entry.step_titles[7].starts_with("Do step 8 "));
        assert_eq!(entry.remaining_step_count(), 4);
    }

    #[tokio::test]
    async fn active_plan_entry_keeps_short_plan_whole() {
        let (_tmp, registry) = test_registry().await;
        let plan_state = registry.planning_workflow_state();
        let validation = validate_plan_content(VALID_PLAN);
        assert!(validation.is_ready());
        let plan_file = plan_state.plans_dir().join("task.md");
        let plan = ValidatedPlanArtifact::from_validated(plan_file, VALID_PLAN.to_string(), validation);
        let target = PlanExecutionTarget::auto(PlanExecutionContext::Current);

        let entry = active_plan_entry(&plan, &plan_state, target);

        // 1 implementation step + validation + assumption lines, all retained.
        assert_eq!(entry.total_steps, 3);
        assert_eq!(entry.step_titles.len(), 3);
        assert_eq!(entry.remaining_step_count(), 0);
        assert_eq!(entry.status, "approved -> auto");
    }

    #[tokio::test]
    async fn complete_approved_plan_handoff_publishes_sidebar_snapshot() {
        use super::super::{
            allocate_plan_file_if_missing, complete_approved_plan_handoff, load_plan_text_for_approval,
            persist_plan_draft,
        };
        use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;

        let (_tmp, registry) = test_registry().await;
        registry.allow_all_tools().await.unwrap();
        registry.enable_planning();
        let (handle, mut command_rx) = test_handle();
        let mut plan_session = PlanningWorkflowSessionState::default();
        let plan_state = registry.planning_workflow_state();
        allocate_plan_file_if_missing(&plan_state).await.unwrap();
        persist_plan_draft(&plan_state, VALID_PLAN).await.unwrap();
        let artifact = load_plan_text_for_approval(&registry).await.unwrap();

        complete_approved_plan_handoff(
            &registry,
            &mut plan_session,
            &handle,
            artifact,
            PlanExecutionTarget::build(PlanExecutionContext::Current, false),
        )
        .await
        .unwrap();

        let entries = drain_set_active_plan_commands(&mut command_rx);
        assert_eq!(entries.len(), 1, "handoff must publish exactly one snapshot");
        let entry = entries[0].as_ref().expect("handoff snapshot must be Some");
        assert_eq!(entry.status, "approved -> build");
        assert!(entry.file_path.ends_with(".md"), "unexpected plan path: {}", entry.file_path);
        assert_eq!(entry.total_steps, 3);
        assert!(!registry.is_planning_active());
    }

    #[tokio::test]
    async fn transition_to_planning_workflow_clears_sidebar_plan() {
        use crate::agent::runloop::unified::planning_workflow_state::{
            PlanningWorkflowSessionState, transition_to_planning_workflow,
        };
        use crate::agent::runloop::unified::state::SessionStats;
        use vtcode_core::core::interfaces::session::PlanningEntrySource;

        let (_tmp, registry) = test_registry().await;
        let (handle, mut command_rx) = test_handle();
        let mut session_stats = SessionStats::default();
        let mut plan_session = PlanningWorkflowSessionState::default();

        transition_to_planning_workflow(
            &registry,
            &mut session_stats,
            &mut plan_session,
            &handle,
            PlanningEntrySource::UserRequest,
            None,
            None,
            true,
            true,
        )
        .await;

        let entries = drain_set_active_plan_commands(&mut command_rx);
        assert_eq!(entries, vec![None], "new planning entry must clear the sidebar plan");
    }

    #[tokio::test]
    async fn cancelled_finish_clears_sidebar_plan() {
        use super::super::{PlanningFinishReason, finish_planning_workflow};
        use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;

        let (_tmp, registry) = test_registry().await;
        let (handle, mut command_rx) = test_handle();
        let mut plan_session = PlanningWorkflowSessionState::default();

        finish_planning_workflow(&registry, &mut plan_session, &handle, PlanningFinishReason::Cancelled)
            .await
            .unwrap();

        let entries = drain_set_active_plan_commands(&mut command_rx);
        assert_eq!(entries, vec![None], "cancelled planning must clear the sidebar plan");
    }
}
