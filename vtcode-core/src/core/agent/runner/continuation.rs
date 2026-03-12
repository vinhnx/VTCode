use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use vtcode_config::core::agent::ContinuationPolicy;

use crate::core::agent::session::AgentSessionState;
use crate::core::agent::task::Task;
use crate::tools::Tool;
use crate::tools::handlers::TaskTrackerTool;
use crate::tools::handlers::plan_mode::PlanModeState;

const INTERNAL_SCAFFOLD_MARKER: &str = "<!-- vtcode:internal_scaffold -->";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CompletionAssessment {
    Accept,
    SkipAccept { reason: String },
    Continue { reason: String, prompt: String },
    Verify { commands: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VerificationResult {
    pub command: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub output: String,
}

#[derive(Debug, Clone, Deserialize)]
struct TrackerListResponse {
    status: String,
    #[serde(default)]
    checklist: Option<TrackerChecklist>,
}

#[derive(Debug, Clone, Deserialize)]
struct TrackerChecklist {
    #[serde(default)]
    items: Vec<TrackerItem>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TrackerItem {
    #[serde(default)]
    index: Option<usize>,
    description: String,
    status: String,
    #[serde(default)]
    outcome: Option<String>,
    #[serde(default)]
    verify: Vec<String>,
}

pub(super) struct ContinuationController {
    tracker_tool: TaskTrackerTool,
    continuation_policy: ContinuationPolicy,
    full_auto_active: bool,
    plan_mode: bool,
    review_like: bool,
    inferred_verify_commands: Vec<String>,
    manages_internal_scaffold: bool,
}

impl ContinuationController {
    pub(super) fn new(
        workspace_root: PathBuf,
        plan_mode_state: PlanModeState,
        continuation_policy: ContinuationPolicy,
        full_auto_active: bool,
        plan_mode: bool,
        review_like: bool,
    ) -> Self {
        let inferred_verify_commands = infer_default_verify_commands(workspace_root.as_path());
        Self {
            tracker_tool: TaskTrackerTool::new(workspace_root, plan_mode_state),
            continuation_policy,
            full_auto_active,
            plan_mode,
            review_like,
            inferred_verify_commands,
            manages_internal_scaffold: false,
        }
    }

    pub(super) async fn prepare(&mut self, task: &Task) -> Result<()> {
        if !self.continuation_enabled() {
            return Ok(());
        }

        if let Some(checklist) = self.load_tracker().await? {
            self.manages_internal_scaffold = is_internal_scaffold(&checklist);
            return Ok(());
        }

        self.create_internal_scaffold(task).await?;
        self.manages_internal_scaffold = true;
        Ok(())
    }

    pub(super) async fn assess_completion(
        &mut self,
        task: &Task,
        session_state: &AgentSessionState,
    ) -> Result<CompletionAssessment> {
        if !self.continuation_enabled() {
            return Ok(CompletionAssessment::SkipAccept {
                reason: continuation_skip_reason(
                    &self.continuation_policy,
                    self.full_auto_active,
                    self.plan_mode,
                    self.review_like,
                ),
            });
        }

        let mut checklist = if let Some(checklist) = self.load_tracker().await? {
            checklist
        } else {
            self.create_internal_scaffold(task).await?;
            self.manages_internal_scaffold = true;
            if let Some(reloaded) = self.load_tracker().await? {
                reloaded
            } else {
                return Ok(CompletionAssessment::Continue {
                    reason: "Task tracker could not be loaded.".to_string(),
                    prompt:
                        "Continue working. The harness task tracker is missing, so do not stop yet."
                            .to_string(),
                });
            }
        };

        if self.manages_internal_scaffold && is_internal_scaffold(&checklist) {
            self.sync_internal_scaffold_before_completion(session_state, &checklist)
                .await?;
            checklist = self
                .load_tracker()
                .await?
                .context("Internal scaffold should exist after sync")?;
        } else {
            self.manages_internal_scaffold = false;
        }

        let incomplete_items = checklist
            .items
            .iter()
            .filter(|item| item.status != "completed")
            .map(|item| {
                let index = item.index.unwrap_or(0);
                if index > 0 {
                    format!("#{} {} ({})", index, item.description, item.status)
                } else {
                    format!("{} ({})", item.description, item.status)
                }
            })
            .collect::<Vec<_>>();

        if !incomplete_items.is_empty() {
            return Ok(CompletionAssessment::Continue {
                reason: format!(
                    "Task tracker is incomplete: {}.",
                    incomplete_items.join(", ")
                ),
                prompt: format!(
                    "Continue working. Do not stop yet. The task tracker still has incomplete steps: {}. Complete the remaining steps before finishing.",
                    incomplete_items.join(", ")
                ),
            });
        }

        let commands = collect_verify_commands(&checklist);
        if commands.is_empty() {
            if self.manages_internal_scaffold {
                self.update_internal_step(
                    3,
                    "completed",
                    None,
                    Some("No verification commands were configured.".to_string()),
                    None,
                )
                .await?;
            }
            return Ok(CompletionAssessment::Accept);
        }

        if self.manages_internal_scaffold {
            self.update_internal_step(3, "in_progress", None, None, Some(commands.clone()))
                .await?;
        }

        Ok(CompletionAssessment::Verify { commands })
    }

    pub(super) async fn after_verification(
        &mut self,
        results: &[VerificationResult],
    ) -> Result<CompletionAssessment> {
        let first_failure = results.iter().find(|result| !result.success);
        if let Some(failure) = first_failure {
            if self.manages_internal_scaffold {
                self.update_internal_step(2, "in_progress", None, None, None)
                    .await?;
                self.update_internal_step(
                    3,
                    "blocked",
                    None,
                    Some(build_verification_failure_summary(failure)),
                    Some(vec![failure.command.clone()]),
                )
                .await?;
            }

            return Ok(CompletionAssessment::Continue {
                reason: build_verification_failure_summary(failure),
                prompt: build_verification_failure_prompt(failure),
            });
        }

        if self.manages_internal_scaffold {
            let summary = if results.is_empty() {
                "Verification passed.".to_string()
            } else {
                format!("Verification passed: {}", format_command_list(results))
            };
            self.update_internal_step(3, "completed", None, Some(summary), None)
                .await?;
        }

        Ok(CompletionAssessment::Accept)
    }

    fn continuation_enabled(&self) -> bool {
        if self.plan_mode || self.review_like {
            return false;
        }

        match self.continuation_policy {
            ContinuationPolicy::Off => false,
            ContinuationPolicy::ExecOnly => self.full_auto_active,
            ContinuationPolicy::All => true,
        }
    }

    async fn load_tracker(&self) -> Result<Option<TrackerChecklist>> {
        let payload = self
            .tracker_tool
            .execute(json!({ "action": "list" }))
            .await
            .context("load task tracker")?;
        let response: TrackerListResponse =
            serde_json::from_value(payload).context("decode task tracker response")?;
        if response.status == "empty" {
            return Ok(None);
        }
        Ok(response.checklist)
    }

    async fn create_internal_scaffold(&self, task: &Task) -> Result<()> {
        let verify = if self.inferred_verify_commands.is_empty() {
            None
        } else {
            Some(self.inferred_verify_commands.clone())
        };
        self.tracker_tool
            .execute(json!({
                "action": "create",
                "title": task.title,
                "items": [
                    {
                        "description": "analyze",
                        "status": "in_progress",
                        "outcome": "Capture the current state and constraints."
                    },
                    {
                        "description": "change",
                        "status": "pending"
                    },
                    {
                        "description": "verify",
                        "status": "pending",
                        "verify": verify
                    }
                ],
                "notes": INTERNAL_SCAFFOLD_MARKER
            }))
            .await
            .context("create internal scaffold")?;
        Ok(())
    }

    async fn sync_internal_scaffold_before_completion(
        &self,
        session_state: &AgentSessionState,
        checklist: &TrackerChecklist,
    ) -> Result<()> {
        if let Some(step) = checklist.items.first()
            && step.status != "completed"
        {
            self.update_internal_step(
                1,
                "completed",
                None,
                Some("Analysis captured in the autonomous run.".to_string()),
                None,
            )
            .await?;
        }

        if let Some(change_step) = checklist.items.get(1) {
            if !session_state.modified_files.is_empty() {
                let change_outcome = change_step
                    .outcome
                    .clone()
                    .or_else(|| Some("Applied workspace changes.".to_string()));
                self.update_internal_step(
                    2,
                    "completed",
                    Some(session_state.modified_files.clone()),
                    change_outcome,
                    None,
                )
                .await?;
            } else if change_step.status != "completed" {
                self.update_internal_step(
                    2,
                    "completed",
                    None,
                    Some("No workspace changes were required.".to_string()),
                    None,
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn update_internal_step(
        &self,
        index: usize,
        status: &str,
        files: Option<Vec<String>>,
        outcome: Option<String>,
        verify: Option<Vec<String>>,
    ) -> Result<()> {
        self.tracker_tool
            .execute(json!({
                "action": "update",
                "index": index,
                "status": status,
                "files": files,
                "outcome": outcome,
                "verify": verify
            }))
            .await
            .with_context(|| format!("update internal scaffold step {}", index))?;
        Ok(())
    }
}

fn continuation_skip_reason(
    policy: &ContinuationPolicy,
    full_auto_active: bool,
    plan_mode: bool,
    review_like: bool,
) -> String {
    if plan_mode {
        return "Continuation disabled in Plan Mode.".to_string();
    }
    if review_like {
        return "Continuation disabled for read-only review runs.".to_string();
    }
    match policy {
        ContinuationPolicy::Off => "Continuation policy is off.".to_string(),
        ContinuationPolicy::ExecOnly if !full_auto_active => {
            "Continuation policy only applies to exec/full-auto runs.".to_string()
        }
        ContinuationPolicy::ExecOnly | ContinuationPolicy::All => {
            "Continuation accepted.".to_string()
        }
    }
}

fn is_internal_scaffold(checklist: &TrackerChecklist) -> bool {
    checklist
        .notes
        .as_deref()
        .is_some_and(|notes| notes.contains(INTERNAL_SCAFFOLD_MARKER))
        && checklist.items.len() == 3
        && checklist.items[0].description == "analyze"
        && checklist.items[1].description == "change"
        && checklist.items[2].description == "verify"
}

fn collect_verify_commands(checklist: &TrackerChecklist) -> Vec<String> {
    checklist
        .items
        .iter()
        .flat_map(|item| item.verify.iter().cloned())
        .collect()
}

fn infer_default_verify_commands(workspace_root: &Path) -> Vec<String> {
    if workspace_root.join("Cargo.toml").exists() {
        return vec!["cargo check".to_string()];
    }
    if workspace_root.join("pytest.ini").exists()
        || workspace_root.join("pyproject.toml").exists()
        || workspace_root.join("setup.py").exists()
    {
        return vec!["pytest".to_string()];
    }
    if workspace_root.join("package.json").exists() {
        return vec!["npm test".to_string()];
    }
    Vec::new()
}

fn format_command_list(results: &[VerificationResult]) -> String {
    results
        .iter()
        .map(|result| result.command.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

fn build_verification_failure_summary(failure: &VerificationResult) -> String {
    match failure.exit_code {
        Some(code) => format!(
            "Verification failed: {} (exit code {}).",
            failure.command, code
        ),
        None => format!("Verification failed: {}.", failure.command),
    }
}

fn build_verification_failure_prompt(failure: &VerificationResult) -> String {
    let mut prompt = build_verification_failure_summary(failure);
    if !failure.output.trim().is_empty() {
        prompt.push_str(" Fix the failure and run verification again. Command output:\n");
        prompt.push_str(failure.output.trim());
    } else {
        prompt.push_str(" Fix the failure and run verification again.");
    }
    prompt
}

pub(super) fn is_review_like_task(task: &Task) -> bool {
    if task.id == "review-task" {
        return true;
    }

    task.instructions.as_deref().is_some_and(|instructions| {
        let lower = instructions.to_ascii_lowercase();
        lower.contains("review mode") && lower.contains("read-only")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_controller(
        temp: &TempDir,
        continuation_policy: ContinuationPolicy,
        review_like: bool,
    ) -> ContinuationController {
        make_controller_with_flags(temp, continuation_policy, true, false, review_like)
    }

    fn make_controller_with_flags(
        temp: &TempDir,
        continuation_policy: ContinuationPolicy,
        full_auto_enabled: bool,
        plan_mode: bool,
        review_like: bool,
    ) -> ContinuationController {
        ContinuationController::new(
            temp.path().to_path_buf(),
            PlanModeState::new(temp.path().to_path_buf()),
            continuation_policy,
            full_auto_enabled,
            plan_mode,
            review_like,
        )
    }

    fn sample_task() -> Task {
        Task {
            id: "exec-task".to_string(),
            title: "Exec Task".to_string(),
            description: "Implement the change".to_string(),
            instructions: None,
        }
    }

    #[tokio::test]
    async fn prepare_creates_internal_scaffold_when_missing() {
        let temp = TempDir::new().expect("tempdir");
        let mut controller = make_controller(&temp, ContinuationPolicy::ExecOnly, false);

        controller.prepare(&sample_task()).await.expect("prepare");

        let checklist = controller
            .load_tracker()
            .await
            .expect("load")
            .expect("checklist");
        assert!(is_internal_scaffold(&checklist));
    }

    #[tokio::test]
    async fn assess_completion_requests_continuation_when_tracker_incomplete() {
        let temp = TempDir::new().expect("tempdir");
        let mut controller = make_controller(&temp, ContinuationPolicy::ExecOnly, false);
        controller.prepare(&sample_task()).await.expect("prepare");

        let session_state = AgentSessionState::new("session".to_string(), 5, 5, 10_000);
        let assessment = controller
            .assess_completion(&sample_task(), &session_state)
            .await
            .expect("assessment");

        assert!(matches!(assessment, CompletionAssessment::Continue { .. }));
    }

    #[tokio::test]
    async fn verification_failure_requests_continuation() {
        let temp = TempDir::new().expect("tempdir");
        let mut controller = make_controller(&temp, ContinuationPolicy::ExecOnly, false);
        controller.prepare(&sample_task()).await.expect("prepare");

        let assessment = controller
            .after_verification(&[VerificationResult {
                command: "cargo check".to_string(),
                success: false,
                exit_code: Some(101),
                output: "error: failed".to_string(),
            }])
            .await
            .expect("verification");

        assert!(matches!(assessment, CompletionAssessment::Continue { .. }));
    }

    #[tokio::test]
    async fn review_like_task_skips_continuation() {
        let temp = TempDir::new().expect("tempdir");
        let mut controller = make_controller(&temp, ContinuationPolicy::All, true);
        controller.prepare(&sample_task()).await.expect("prepare");

        let session_state = AgentSessionState::new("session".to_string(), 5, 5, 10_000);
        let assessment = controller
            .assess_completion(&sample_task(), &session_state)
            .await
            .expect("assessment");

        assert!(matches!(
            assessment,
            CompletionAssessment::SkipAccept { .. }
        ));
    }

    #[tokio::test]
    async fn off_policy_skips_continuation() {
        let temp = TempDir::new().expect("tempdir");
        let mut controller = make_controller(&temp, ContinuationPolicy::Off, false);
        controller.prepare(&sample_task()).await.expect("prepare");

        let session_state = AgentSessionState::new("session".to_string(), 5, 5, 10_000);
        let assessment = controller
            .assess_completion(&sample_task(), &session_state)
            .await
            .expect("assessment");

        assert!(matches!(
            assessment,
            CompletionAssessment::SkipAccept { .. }
        ));
    }

    #[tokio::test]
    async fn exec_only_policy_skips_non_full_auto_sessions() {
        let temp = TempDir::new().expect("tempdir");
        let mut controller =
            make_controller_with_flags(&temp, ContinuationPolicy::ExecOnly, false, false, false);
        controller.prepare(&sample_task()).await.expect("prepare");

        let session_state = AgentSessionState::new("session".to_string(), 5, 5, 10_000);
        let assessment = controller
            .assess_completion(&sample_task(), &session_state)
            .await
            .expect("assessment");

        assert!(matches!(
            assessment,
            CompletionAssessment::SkipAccept { .. }
        ));
    }
}
