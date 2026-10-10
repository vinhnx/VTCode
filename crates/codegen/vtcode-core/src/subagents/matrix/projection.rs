use super::*;
use crate::subagents::{SubagentStatus, SubagentStatusEntry};

impl SubagentController {
    /// Existing Local Agents rows are derived from durable task checkpoints.
    pub(in crate::subagents) async fn matrix_projection_entries(&self) -> Vec<SubagentStatusEntry> {
        let guard = self.matrix.state.lock().await;
        let Some(state) = guard.as_ref() else {
            return vec![];
        };
        let snapshot = state.snapshot();
        let parent = self.parent_session_id.read().await.clone();
        let updated_at = *self.matrix.updated_at.read();
        snapshot
            .tasks
            .iter()
            .zip(&snapshot.spec.tasks)
            .map(|(state, task)| {
                let attempt = state.attempts.last();
                let (status, summary) = match state.status {
                    MatrixTaskStatus::Queued => (
                        SubagentStatus::Queued,
                        if task.dependencies.iter().any(|id| {
                            snapshot.tasks.iter().any(|task| {
                                &task.id == id
                                    && !matches!(task.status, MatrixTaskStatus::Executed | MatrixTaskStatus::Verified)
                            })
                        }) {
                            "waiting for dependencies"
                        } else {
                            "waiting for worker, resources or workspace lease"
                        },
                    ),
                    MatrixTaskStatus::Assigned => (
                        SubagentStatus::Running,
                        if attempt.is_some_and(|attempt| attempt.phase == MatrixPhase::Verify) {
                            "final verification in progress"
                        } else {
                            "execution in progress"
                        },
                    ),
                    MatrixTaskStatus::Executed => {
                        (SubagentStatus::Waiting, "execution finished; final verification pending")
                    }
                    MatrixTaskStatus::Verified => (
                        if snapshot.lifecycle == MatrixLifecycle::Succeeded {
                            SubagentStatus::Completed
                        } else {
                            SubagentStatus::Waiting
                        },
                        "verified against current generation",
                    ),
                    MatrixTaskStatus::CleanupUncertain => {
                        (SubagentStatus::Failed, "owned cleanup uncertain; resources held")
                    }
                    _ => (SubagentStatus::Failed, "coordinator decision required"),
                };
                let status = if snapshot.lifecycle == MatrixLifecycle::Cancelled {
                    SubagentStatus::Closed
                } else {
                    status
                };
                SubagentStatusEntry {
                    id: format!("matrix-{}-{}", snapshot.spec.id, task.id),
                    session_id: attempt
                        .map(|attempt| format!("matrix-{}-{}", parent, attempt.worker_id))
                        .unwrap_or_default(),
                    parent_thread_id: parent.clone(),
                    agent_name: "matrix".into(),
                    display_label: format!("{} / {}", snapshot.spec.id, task.id),
                    description: task.instructions.clone(),
                    source: "matrix".into(),
                    color: None,
                    status,
                    background: false,
                    depth: self.config.depth + 1,
                    created_at: updated_at,
                    updated_at,
                    completed_at: status.is_terminal().then_some(updated_at),
                    summary: Some(summary.into()),
                    error: self.matrix.error.read().clone(),
                    transcript_path: None,
                    nickname: None,
                }
            })
            .collect()
    }
}

pub(super) fn tracker_result(snapshot: &MatrixSnapshot) -> Value {
    let items = snapshot
        .tasks
        .iter()
        .map(|task| {
            let status = if snapshot.lifecycle == MatrixLifecycle::Cancelled {
                "blocked"
            } else {
                match task.status {
                    MatrixTaskStatus::Queued => "pending",
                    MatrixTaskStatus::Assigned | MatrixTaskStatus::Executed => "in_progress",
                    MatrixTaskStatus::Verified if snapshot.lifecycle == MatrixLifecycle::Succeeded => "completed",
                    MatrixTaskStatus::Verified => "in_progress",
                    _ => "blocked",
                }
            };
            json!({"description":task.id,"status":status})
        })
        .collect::<Vec<_>>();
    let lines = crate::tools::handlers::task_tracking::compact_task_tree_view_from_items(&items);
    json!({"success":true,"matrix_id":snapshot.spec.id,"revision":snapshot.revision,
        "view":{"title":format!("Matrix {}",snapshot.spec.id),"lines":lines},
        "summary":{"total":items.len(),"completed":items.iter().filter(|item|item["status"]=="completed").count(),"in_progress":items.iter().filter(|item|item["status"]=="in_progress").count(),"pending":items.iter().filter(|item|item["status"]=="pending").count(),"blocked":items.iter().filter(|item|item["status"]=="blocked").count()}})
}

impl SubagentController {
    pub async fn matrix_tracker_projection(&self) -> Option<Value> {
        self.matrix
            .state
            .lock()
            .await
            .as_ref()
            .map(|state| tracker_result(state.snapshot()))
    }
}
