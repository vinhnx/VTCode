use std::sync::Arc;

use anyhow::{Result, anyhow, ensure};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};

const PLAN_UPDATE_PROGRESS: &str = "Plan updated. Continue working through TODOs.";
const PLAN_UPDATE_COMPLETE: &str = "Plan completed. All TODOs are done.";
const PLAN_UPDATE_CLEARED: &str = "Plan cleared. Start a new TODO list.";
const MAX_PLAN_STEPS: usize = 12;
const MIN_PLAN_STEPS: usize = 1;
const CHECKBOX_PENDING: &str = "[ ]";
const CHECKBOX_IN_PROGRESS: &str = "[ ]";
const CHECKBOX_COMPLETED: &str = "[x]";
const IN_PROGRESS_NOTE: &str = " _(in progress)_";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
}

impl Default for StepStatus {
    fn default() -> Self {
        StepStatus::Pending
    }
}

impl StepStatus {
    pub fn label(&self) -> &'static str {
        match self {
            StepStatus::Pending => "pending",
            StepStatus::InProgress => "in_progress",
            StepStatus::Completed => "completed",
        }
    }

    pub fn checkbox(&self) -> &'static str {
        match self {
            StepStatus::Pending => CHECKBOX_PENDING,
            StepStatus::InProgress => CHECKBOX_IN_PROGRESS,
            StepStatus::Completed => CHECKBOX_COMPLETED,
        }
    }

    pub fn status_note(&self) -> Option<&'static str> {
        match self {
            StepStatus::InProgress => Some(IN_PROGRESS_NOTE),
            StepStatus::Pending | StepStatus::Completed => None,
        }
    }

    pub fn is_complete(&self) -> bool {
        matches!(self, StepStatus::Completed)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PlanStep {
    pub step: String,
    pub status: StepStatus,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum PlanStepInput {
    Simple(String),
    Detailed {
        step: String,
        #[serde(default)]
        status: StepStatus,
    },
}

impl<'de> Deserialize<'de> for PlanStep {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let input = PlanStepInput::deserialize(deserializer)?;
        let plan_step = match input {
            PlanStepInput::Simple(step) => PlanStep {
                step,
                status: StepStatus::Pending,
            },
            PlanStepInput::Detailed { step, status } => PlanStep { step, status },
        };
        Ok(plan_step)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanCompletionState {
    Empty,
    InProgress,
    Done,
}

impl PlanCompletionState {
    pub fn label(&self) -> &'static str {
        match self {
            PlanCompletionState::Empty => "no_todos",
            PlanCompletionState::InProgress => "todos_remaining",
            PlanCompletionState::Done => "done",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            PlanCompletionState::Empty => "No TODOs recorded in the current plan.",
            PlanCompletionState::InProgress => "TODOs remain in the current plan.",
            PlanCompletionState::Done => "All TODOs have been completed.",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanSummary {
    pub total_steps: usize,
    pub completed_steps: usize,
    pub status: PlanCompletionState,
}

impl Default for PlanSummary {
    fn default() -> Self {
        Self {
            total_steps: 0,
            completed_steps: 0,
            status: PlanCompletionState::Empty,
        }
    }
}

impl PlanSummary {
    pub fn from_steps(steps: &[PlanStep]) -> Self {
        if steps.is_empty() {
            return Self::default();
        }

        let total_steps = steps.len();
        let completed_steps = steps
            .iter()
            .filter(|step| step.status.is_complete())
            .count();
        let status = if completed_steps == total_steps {
            PlanCompletionState::Done
        } else {
            PlanCompletionState::InProgress
        };

        Self {
            total_steps,
            completed_steps,
            status,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    pub steps: Vec<PlanStep>,
    pub summary: PlanSummary,
    pub version: u64,
    pub updated_at: DateTime<Utc>,
}

impl Default for TaskPlan {
    fn default() -> Self {
        Self {
            explanation: None,
            steps: Vec::new(),
            summary: PlanSummary::default(),
            version: 0,
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdatePlanArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    pub plan: Vec<PlanStep>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanUpdateResult {
    pub success: bool,
    pub message: String,
    pub plan: TaskPlan,
}

impl PlanUpdateResult {
    pub fn success(plan: TaskPlan) -> Self {
        let message = match plan.summary.status {
            PlanCompletionState::Done => PLAN_UPDATE_COMPLETE.to_string(),
            PlanCompletionState::InProgress => PLAN_UPDATE_PROGRESS.to_string(),
            PlanCompletionState::Empty => PLAN_UPDATE_CLEARED.to_string(),
        };
        Self {
            success: true,
            message,
            plan,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlanManager {
    inner: Arc<RwLock<TaskPlan>>,
}

impl Default for PlanManager {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(TaskPlan::default())),
        }
    }
}

impl PlanManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> TaskPlan {
        self.inner.read().clone()
    }

    pub fn update_plan(&self, update: UpdatePlanArgs) -> Result<TaskPlan> {
        validate_plan(&update)?;

        let sanitized_explanation = update
            .explanation
            .as_ref()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());

        let mut in_progress_count = 0usize;
        let mut sanitized_steps: Vec<PlanStep> = Vec::with_capacity(update.plan.len());
        for (index, mut step) in update.plan.into_iter().enumerate() {
            let trimmed = step.step.trim();
            if trimmed.is_empty() {
                return Err(anyhow!("Plan step {} cannot be empty", index + 1));
            }
            if matches!(step.status, StepStatus::InProgress) {
                in_progress_count += 1;
            }
            step.step = trimmed.to_string();
            sanitized_steps.push(step);
        }

        ensure!(
            in_progress_count <= 1,
            "At most one plan step can be in_progress"
        );

        let mut guard = self.inner.write();
        let version = guard.version.saturating_add(1);
        let summary = PlanSummary::from_steps(&sanitized_steps);
        let updated_plan = TaskPlan {
            explanation: sanitized_explanation,
            steps: sanitized_steps,
            summary,
            version,
            updated_at: Utc::now(),
        };
        *guard = updated_plan.clone();
        Ok(updated_plan)
    }
}

fn validate_plan(update: &UpdatePlanArgs) -> Result<()> {
    let step_count = update.plan.len();
    ensure!(
        step_count >= MIN_PLAN_STEPS,
        "Plan must contain at least {} step(s)",
        MIN_PLAN_STEPS
    );
    ensure!(
        step_count <= MAX_PLAN_STEPS,
        "Plan must not exceed {} steps",
        MAX_PLAN_STEPS
    );

    for (index, step) in update.plan.iter().enumerate() {
        ensure!(
            !step.step.trim().is_empty(),
            "Plan step {} cannot be empty",
            index + 1
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_with_default_state() {
        let manager = PlanManager::new();
        let snapshot = manager.snapshot();
        assert_eq!(snapshot.steps.len(), 0);
        assert_eq!(snapshot.version, 0);
        assert_eq!(snapshot.summary.status, PlanCompletionState::Empty);
        assert_eq!(snapshot.summary.total_steps, 0);
    }

    #[test]
    fn rejects_empty_plan() {
        let manager = PlanManager::new();
        let args = UpdatePlanArgs {
            explanation: None,
            plan: Vec::new(),
        };
        assert!(manager.update_plan(args).is_err());
    }

    #[test]
    fn rejects_multiple_in_progress_steps() {
        let manager = PlanManager::new();
        let args = UpdatePlanArgs {
            explanation: None,
            plan: vec![
                PlanStep {
                    step: "Step one".to_string(),
                    status: StepStatus::InProgress,
                },
                PlanStep {
                    step: "Step two".to_string(),
                    status: StepStatus::InProgress,
                },
            ],
        };
        assert!(manager.update_plan(args).is_err());
    }

    #[test]
    fn updates_plan_successfully() {
        let manager = PlanManager::new();
        let args = UpdatePlanArgs {
            explanation: Some("Focus on API layer".to_string()),
            plan: vec![
                PlanStep {
                    step: "Audit handlers".to_string(),
                    status: StepStatus::Pending,
                },
                PlanStep {
                    step: "Add tests".to_string(),
                    status: StepStatus::Pending,
                },
            ],
        };
        let result = manager.update_plan(args).expect("plan should update");
        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.version, 1);
        assert_eq!(result.steps[0].status, StepStatus::Pending);
        assert_eq!(result.summary.total_steps, 2);
        assert_eq!(result.summary.completed_steps, 0);
        assert_eq!(result.summary.status, PlanCompletionState::InProgress);
    }

    #[test]
    fn marks_plan_done_when_all_completed() {
        let manager = PlanManager::new();
        let args = UpdatePlanArgs {
            explanation: None,
            plan: vec![PlanStep {
                step: "Finalize deployment".to_string(),
                status: StepStatus::Completed,
            }],
        };
        let result = manager.update_plan(args).expect("plan should update");
        assert_eq!(result.summary.total_steps, 1);
        assert_eq!(result.summary.completed_steps, 1);
        assert_eq!(result.summary.status, PlanCompletionState::Done);
    }

    #[test]
    fn defaults_missing_status_to_pending() {
        let args_json = serde_json::json!({
            "plan": [
                { "step": "Outline solution" },
                { "step": "Implement fix", "status": "in_progress" }
            ]
        });

        let args: UpdatePlanArgs =
            serde_json::from_value(args_json).expect("args should deserialize");
        assert_eq!(args.plan[0].status, StepStatus::Pending);
        assert_eq!(args.plan[1].status, StepStatus::InProgress);

        let manager = PlanManager::new();
        let result = manager.update_plan(args).expect("plan should update");
        assert_eq!(result.steps[0].status, StepStatus::Pending);
    }

    #[test]
    fn accepts_string_plan_steps() {
        let args_json = serde_json::json!({
            "plan": [
                "Scope code changes",
                { "step": "Write tests", "status": "completed" }
            ]
        });

        let args: UpdatePlanArgs =
            serde_json::from_value(args_json).expect("args should deserialize");

        assert_eq!(args.plan[0].step, "Scope code changes");
        assert_eq!(args.plan[0].status, StepStatus::Pending);
        assert_eq!(args.plan[1].status, StepStatus::Completed);

        let manager = PlanManager::new();
        let result = manager.update_plan(args).expect("plan should update");
        assert_eq!(result.summary.total_steps, 2);
    }
}
