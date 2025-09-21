use std::sync::Arc;

use anyhow::{Result, anyhow, ensure};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

const PLAN_UPDATE_SUCCESS: &str = "Plan updated successfully";
const MAX_PLAN_STEPS: usize = 12;
const MIN_PLAN_STEPS: usize = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
}

impl StepStatus {
    pub fn label(&self) -> &'static str {
        match self {
            StepStatus::Pending => "pending",
            StepStatus::InProgress => "in_progress",
            StepStatus::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanStep {
    pub step: String,
    pub status: StepStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    pub steps: Vec<PlanStep>,
    pub version: u64,
    pub updated_at: DateTime<Utc>,
}

impl Default for TaskPlan {
    fn default() -> Self {
        Self {
            explanation: None,
            steps: Vec::new(),
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
    pub message: &'static str,
    pub plan: TaskPlan,
}

impl PlanUpdateResult {
    pub fn success(plan: TaskPlan) -> Self {
        Self {
            success: true,
            message: PLAN_UPDATE_SUCCESS,
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
        let updated_plan = TaskPlan {
            explanation: sanitized_explanation,
            steps: sanitized_steps,
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
    }
}
