//! Shared planning approval events for interactive and headless runtimes.

use std::path::Path;
use vtcode_commons::workspace_relative_display;
use vtcode_core::exec::events::{
    ContextResetEvent, ContextResetTrigger, HarnessEventKind, ItemCompletedEvent, ItemStartedEvent,
    PlanApprovalDecision, PlanApprovalRequestedEvent, PlanApprovalResolvedEvent, PlanDeltaEvent, PlanItem, ThreadEvent,
    ThreadItem, ThreadItemDetails,
};

use super::PlanningWorkflowState;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::inline_events::harness::harness_event;
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;

fn display_plan_path(plan_state: &PlanningWorkflowState, path: &Path) -> String {
    plan_state
        .workspace_root()
        .map(|workspace| workspace_relative_display(&workspace, path))
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Publish the complete approval-ready plan lifecycle after the persisted
/// artifact and its tracker have passed validation.
///
/// Keeping this sequence behind the planning facade prevents recovery,
/// interactive, and headless paths from drifting apart. The session state is
/// marked pending before the first event so a later cancellation can resolve
/// the exact request identity that was published.
pub(crate) async fn emit_plan_ready_events(
    plan_session: &mut PlanningWorkflowSessionState,
    plan_state: &PlanningWorkflowState,
    emitter: Option<&HarnessEventEmitter>,
    thread_id: &str,
    turn_id: &str,
    plan_text: &str,
) {
    plan_session.mark_plan_approval_pending(thread_id.to_owned(), turn_id.to_owned());
    let Some(emitter) = emitter else {
        return;
    };

    let item_id = format!("{turn_id}-plan");
    let plan_path = plan_state
        .get_plan_file()
        .await
        .map(|path| display_plan_path(plan_state, &path));

    let _ = emitter.emit(harness_event(
        HarnessEventKind::PlanningStarted,
        Some("Planning workflow produced a plan for review.".to_string()),
        plan_path.clone(),
        None,
        None,
    ));

    let start_item = ThreadItem {
        context: None,
        id: item_id.clone(),
        details: ThreadItemDetails::Plan(PlanItem { text: String::new() }),
    };
    let _ = emitter.emit(ThreadEvent::ItemStarted(ItemStartedEvent { item: start_item }));

    let _ = emitter.emit(ThreadEvent::PlanDelta(Box::new(PlanDeltaEvent {
        thread_id: thread_id.to_owned(),
        turn_id: turn_id.to_owned(),
        item_id: item_id.clone(),
        delta: plan_text.to_owned(),
    })));

    let completed_item = ThreadItem {
        context: None,
        id: item_id,
        details: ThreadItemDetails::Plan(PlanItem { text: plan_text.to_owned() }),
    };
    let _ = emitter.emit(ThreadEvent::ItemCompleted(ItemCompletedEvent { item: completed_item }));
    let _ = emitter.emit(harness_event(
        HarnessEventKind::PlanningCompleted,
        Some("Plan is ready for user approval.".to_string()),
        plan_path.clone(),
        None,
        None,
    ));
    emit_plan_approval_requested(Some(emitter), thread_id, turn_id, plan_path);
}

pub(crate) fn emit_plan_approval_requested(
    emitter: Option<&HarnessEventEmitter>,
    thread_id: impl Into<String>,
    turn_id: impl Into<String>,
    plan_file: Option<String>,
) {
    let Some(emitter) = emitter else {
        return;
    };

    if let Err(err) = emitter.emit(ThreadEvent::PlanApprovalRequested(PlanApprovalRequestedEvent {
        thread_id: thread_id.into(),
        turn_id: turn_id.into(),
        plan_file,
    })) {
        tracing::debug!(error = %err, "failed to emit plan approval request event");
    }
}

pub(crate) fn emit_context_reset(
    emitter: Option<&HarnessEventEmitter>,
    thread_id: impl Into<String>,
    turn_id: impl Into<String>,
    previous_context_usage_percent: u8,
) {
    let Some(emitter) = emitter else {
        return;
    };

    if let Err(err) = emitter.emit(ThreadEvent::ContextReset(ContextResetEvent {
        thread_id: thread_id.into(),
        turn_id: turn_id.into(),
        trigger: ContextResetTrigger::PlanApproval,
        plan_preserved: true,
        previous_context_usage_percent,
        tool_budget_reset: true,
    })) {
        tracing::debug!(error = %err, "failed to emit context reset event");
    }
}

pub(crate) fn emit_plan_approval_resolved(
    emitter: Option<&HarnessEventEmitter>,
    thread_id: impl Into<String>,
    turn_id: impl Into<String>,
    decision: PlanApprovalDecision,
    automatic: bool,
) {
    let Some(emitter) = emitter else {
        return;
    };

    if let Err(err) = emitter.emit(ThreadEvent::PlanApprovalResolved(PlanApprovalResolvedEvent {
        thread_id: thread_id.into(),
        turn_id: turn_id.into(),
        decision,
        automatic,
    })) {
        tracing::debug!(error = %err, "failed to emit plan approval resolution event");
    }
}

#[cfg(test)]
mod tests {
    use super::PlanningWorkflowState;
    use super::{display_plan_path, emit_plan_approval_requested, emit_plan_approval_resolved};
    use std::path::PathBuf;
    use tempfile::tempdir;
    use vtcode_core::exec::events::{PlanApprovalDecision, ThreadEvent, VersionedThreadEvent};

    #[test]
    fn plan_event_paths_use_workspace_relative_display() {
        let directory = tempdir().expect("temporary workspace");
        let state = PlanningWorkflowState::new(directory.path().to_path_buf());
        let plan_file = directory.path().join(".vtcode/plans/task.md");

        assert_eq!(display_plan_path(&state, &plan_file), ".vtcode/plans/task.md");
    }

    #[tokio::test]
    async fn plan_ready_events_emit_workspace_relative_plan_paths() {
        use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
        use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
        use vtcode_core::exec::events::{HarnessEventKind, ThreadItemDetails};

        let directory = tempdir().expect("temporary workspace");
        let plan_file = directory.path().join(".vtcode/plans/task.md");
        std::fs::create_dir_all(plan_file.parent().expect("plan directory")).expect("plan directory");
        std::fs::write(&plan_file, "# Task\n").expect("plan file");

        let state = PlanningWorkflowState::new(directory.path().to_path_buf());
        state.set_plan_file(Some(plan_file)).await;
        let event_path = directory.path().join("events.jsonl");
        let emitter = HarnessEventEmitter::new(event_path.clone()).expect("harness emitter");
        let mut plan_session = PlanningWorkflowSessionState::default();

        super::emit_plan_ready_events(&mut plan_session, &state, Some(&emitter), "thread-1", "turn-1", "# Task").await;

        let events = std::fs::read_to_string(event_path)
            .expect("event log")
            .lines()
            .map(|line| serde_json::from_str::<VersionedThreadEvent>(line).expect("versioned event"))
            .map(VersionedThreadEvent::into_event)
            .collect::<Vec<_>>();
        let plan_paths = events.iter().filter_map(|event| match event {
            ThreadEvent::ItemCompleted(item) => match &item.item.details {
                ThreadItemDetails::Harness(details)
                    if matches!(
                        details.event,
                        HarnessEventKind::PlanningStarted | HarnessEventKind::PlanningCompleted
                    ) =>
                {
                    details.path.as_deref()
                }
                _ => None,
            },
            _ => None,
        });

        assert!(plan_paths.clone().all(|path| path == ".vtcode/plans/task.md"));
        assert_eq!(plan_paths.count(), 2);
    }

    #[test]
    fn approval_events_are_written_to_the_shared_harness_stream() {
        let directory = tempdir().expect("temporary event directory");
        let path = directory.path().join(PathBuf::from("events.jsonl"));
        let emitter = super::HarnessEventEmitter::new(path.clone()).expect("harness emitter");

        emit_plan_approval_requested(Some(&emitter), "thread-1", "turn-1", Some(".vtcode/plans/task.md".to_string()));
        emit_plan_approval_resolved(Some(&emitter), "thread-1", "turn-2", PlanApprovalDecision::SwitchAuto, false);

        let lines = std::fs::read_to_string(path)
            .expect("event log")
            .lines()
            .map(|line| serde_json::from_str::<VersionedThreadEvent>(line).expect("versioned event"))
            .map(VersionedThreadEvent::into_event)
            .collect::<Vec<_>>();

        assert!(matches!(lines[0], ThreadEvent::PlanApprovalRequested(_)));
        assert!(matches!(
            lines[1],
            ThreadEvent::PlanApprovalResolved(ref event)
                if event.decision == PlanApprovalDecision::SwitchAuto && !event.automatic
        ));
    }
}
