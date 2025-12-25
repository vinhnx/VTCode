use std::fmt::Write;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, ensure};
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
pub enum PlanPhase {
    Understanding,
    Design,
    Review,
    FinalPlan,
}

impl PlanPhase {
    pub fn label(&self) -> &'static str {
        match self {
            PlanPhase::Understanding => "understanding",
            PlanPhase::Design => "design",
            PlanPhase::Review => "review",
            PlanPhase::FinalPlan => "final_plan",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            PlanPhase::Understanding => "Gathering context and open questions",
            PlanPhase::Design => "Drafting approaches and tradeoffs",
            PlanPhase::Review => "Validating plan aligns with request",
            PlanPhase::FinalPlan => "Ready to execute agreed plan",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    #[default]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<PlanPhase>,
    pub steps: Vec<PlanStep>,
    pub summary: PlanSummary,
    pub version: u64,
    pub updated_at: DateTime<Utc>,
}

impl TaskPlan {
    pub fn current_step(&self) -> Option<&PlanStep> {
        self.steps.iter().find(|s| !s.status.is_complete())
    }
}

impl Default for TaskPlan {
    fn default() -> Self {
        Self {
            explanation: None,
            phase: None,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<PlanPhase>,
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
    plan_file: Option<PathBuf>,
}

impl Default for PlanManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PlanManager {
    pub fn new() -> Self {
        Self::with_plan_file(None)
    }

    pub fn with_plan_file(plan_file: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(TaskPlan::default())),
            plan_file,
        }
    }

    pub fn snapshot(&self) -> TaskPlan {
        self.inner.read().clone()
    }

    pub fn update_plan(&self, update: UpdatePlanArgs) -> Result<TaskPlan> {
        validate_plan(&update)?;
        validate_plan_quality(&update)?;

        let sanitized_explanation = update
            .explanation
            .as_ref()
            .map(|text| text.trim().to_owned())
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
        let phase = update.phase.or_else(|| guard.phase.clone());
        let summary = PlanSummary::from_steps(&sanitized_steps);
        let updated_plan = TaskPlan {
            explanation: sanitized_explanation,
            phase,
            steps: sanitized_steps,
            summary,
            version,
            updated_at: Utc::now(),
        };
        *guard = updated_plan.clone();
        drop(guard);

        self.persist_plan(&updated_plan)?;

        Ok(updated_plan)
    }

    fn persist_plan(&self, plan: &TaskPlan) -> Result<()> {
        let Some(path) = &self.plan_file else {
            return Ok(());
        };

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create plan directory '{}'", parent.display())
            })?;
        }

        let content = render_plan_markdown(plan);
        fs::write(path, content)
            .with_context(|| format!("Failed to write plan file '{}'", path.display()))?;

        Ok(())
    }
}

fn render_plan_markdown(plan: &TaskPlan) -> String {
    let mut output = String::new();

    let _ = writeln!(output, "# Task Plan (v{})", plan.version);
    let _ = writeln!(output, "Updated: {}", plan.updated_at.to_rfc3339());
    let _ = writeln!(
        output,
        "Status: {} ({} of {} completed)",
        plan.summary.status.label(),
        plan.summary.completed_steps,
        plan.summary.total_steps
    );

    if let Some(phase) = &plan.phase {
        let _ = writeln!(
            output,
            "Plan Phase: {} - {}",
            phase.label(),
            phase.description()
        );
    }

    if let Some(explanation) = &plan.explanation
        && !explanation.is_empty()
    {
        let _ = writeln!(output, "\nFocus: {}", explanation);
    }

    let _ = writeln!(output, "\n## Steps");
    for (idx, step) in plan.steps.iter().enumerate() {
        let mut line = String::new();
        let _ = write!(
            line,
            "{}. {} {}",
            idx + 1,
            step.status.checkbox(),
            step.step
        );
        if let Some(note) = step.status.status_note() {
            line.push_str(note);
        }
        let _ = writeln!(output, "{}", line);
    }

    output
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

/// Validates plan quality when entering final_plan phase
/// Provides suggestions for improving plan precision and thoroughness
fn validate_plan_quality(update: &UpdatePlanArgs) -> Result<()> {
    let Some(phase) = &update.phase else {
        return Ok(()); // Only validate when explicitly setting phase
    };

    // Only enforce quality checks for final_plan phase
    if !matches!(phase, PlanPhase::FinalPlan) {
        return Ok(());
    }

    let mut warnings = Vec::new();

    // Check 1: Minimum task breakdown
    if update.plan.len() < 3 {
        warnings.push("Plan should break down into at least 3 concrete steps for better clarity");
    }

    // Check 2: Look for file path patterns in steps
    let has_file_paths = update.plan.iter().any(|step| {
        step.step.contains('/')
            && (step.step.contains(".rs")
                || step.step.contains(".md")
                || step.step.contains(".toml")
                || step.step.contains(':'))
    });

    if !has_file_paths {
        warnings.push("Plan should include specific file paths (e.g., path/to/file.rs:45)");
    }

    // Check 3: Explanation should exist for final plan
    if update.explanation.is_none()
        || update
            .explanation
            .as_ref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
    {
        warnings.push("Plan missing context/explanation of what was explored");
    }

    if !warnings.is_empty() {
        // Return as warning, not error (for now - can be made strict via config)
        tracing::warn!("Plan quality suggestions: {}", warnings.join("; "));
        // TODO: Make this an error if config.agent.strict_plan_validation = true
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use tempfile::tempdir;

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
            phase: None,
            plan: Vec::new(),
        };
        assert!(manager.update_plan(args).is_err());
    }

    #[test]
    fn rejects_multiple_in_progress_steps() {
        let manager = PlanManager::new();
        let args = UpdatePlanArgs {
            explanation: None,
            phase: None,
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
            phase: None,
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
            phase: None,
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

    #[test]
    fn persists_plan_to_disk_with_phase() {
        let dir = tempdir().expect("temp directory");
        let plan_path = dir.path().join("plan.md");
        let manager = PlanManager::with_plan_file(Some(plan_path.clone()));

        let args = UpdatePlanArgs {
            explanation: Some("Planning in read-only mode".to_string()),
            phase: Some(PlanPhase::Design),
            plan: vec![PlanStep {
                step: "Outline API surface".to_string(),
                status: StepStatus::InProgress,
            }],
        };

        let updated = manager.update_plan(args).expect("plan should update");
        assert_eq!(updated.phase, Some(PlanPhase::Design));

        let content = fs::read_to_string(plan_path).expect("plan markdown should exist");
        assert!(content.contains("Plan Phase: design"));
        assert!(content.contains("[ ] Outline API surface"));
    }

    #[test]
    fn retains_previous_phase_when_not_provided() {
        let manager = PlanManager::new();

        let _ = manager
            .update_plan(UpdatePlanArgs {
                explanation: None,
                phase: Some(PlanPhase::Understanding),
                plan: vec![PlanStep {
                    step: "Read requirements".to_string(),
                    status: StepStatus::InProgress,
                }],
            })
            .expect("plan should update");

        let updated = manager
            .update_plan(UpdatePlanArgs {
                explanation: Some("Refine design".to_string()),
                phase: None,
                plan: vec![PlanStep {
                    step: "Draft approach".to_string(),
                    status: StepStatus::Pending,
                }],
            })
            .expect("plan should update");

        assert_eq!(updated.phase, Some(PlanPhase::Understanding));
    }
}
