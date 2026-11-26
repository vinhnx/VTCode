use super::workspace_detection::infer_default_verify_commands;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use vtcode_config::core::agent::{ContextResetMode, ContinuationPolicy};

use crate::core::agent::progress_monitor::{ProgressMonitor, milestone_status_from_str};
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::task::Task;
use crate::tools::Tool;
use crate::tools::handlers::TaskTrackerTool;
use crate::tools::handlers::planning_workflow::PlanningWorkflowState;
use vtcode_memory::Milestone;

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
    planning_active: bool,
    review_like: bool,
    inferred_verify_commands: Vec<String>,
    manages_internal_scaffold: bool,
    /// Optional durable progress ledger mirroring the task tracker, persisted
    /// across turns/compaction so long-horizon work can be resumed and stalled
    /// runs detected. `None` when progress tracking is disabled for the session.
    progress: Option<ProgressMonitor>,
    /// Workspace root for writing context reset manifests.
    workspace_root: PathBuf,
    /// Context reset mode from harness config.
    context_reset_mode: ContextResetMode,
    /// Stall threshold for context reset.
    context_reset_stall_threshold: u32,
    transition_thread_id: Option<String>,
    transition_turn_id: Option<String>,
    /// Fingerprint of the last verification failure shape, used to detect
    /// consecutive repeats of the same bug within this run.
    last_failure_fingerprint: Option<String>,
    /// Consecutive `after_verification` failures sharing one fingerprint.
    fingerprint_streak: u8,
}

impl ContinuationController {
    pub(super) fn new(
        workspace_root: PathBuf,
        planning_workflow_state: PlanningWorkflowState,
        continuation_policy: ContinuationPolicy,
        full_auto_active: bool,
        planning_active: bool,
        review_like: bool,
        context_reset_mode: ContextResetMode,
        context_reset_stall_threshold: u32,
    ) -> Self {
        let inferred_verify_commands = infer_default_verify_commands(workspace_root.as_path());
        Self {
            tracker_tool: TaskTrackerTool::new(workspace_root.clone(), planning_workflow_state),
            continuation_policy,
            full_auto_active,
            planning_active,
            review_like,
            inferred_verify_commands,
            manages_internal_scaffold: false,
            progress: None,
            workspace_root,
            context_reset_mode,
            context_reset_stall_threshold,
            transition_thread_id: None,
            transition_turn_id: None,
            last_failure_fingerprint: None,
            fingerprint_streak: 0,
        }
    }

    pub(super) fn with_transition_identity(mut self, thread_id: String, turn_id: String) -> Self {
        self.transition_thread_id = Some(thread_id);
        self.transition_turn_id = Some(turn_id);
        self
    }

    /// Attach a durable progress monitor. Once set, the controller keeps the
    /// monitor's ledger in sync with the task tracker and records advance/stall
    /// signals on each assessment.
    pub(super) fn with_progress_monitor(mut self, monitor: ProgressMonitor) -> Self {
        self.progress = Some(monitor);
        self
    }

    /// Borrow the progress monitor, if one is attached.
    #[must_use]
    // surfaced to the runloop via the persisted ledger; kept for diagnostics
    #[allow(
        dead_code,
        reason = "Accessor kept on the live runner continuation state; clippy-verified unused in-tree."
    )]
    pub(super) fn progress_monitor(&self) -> Option<&ProgressMonitor> {
        self.progress.as_ref()
    }

    pub(super) async fn prepare(&mut self, task: &Task) -> Result<()> {
        if !self.continuation_enabled() {
            return Ok(());
        }

        if self.load_tracker().await?.is_some() {
            // Existing workspace state is not owned by this fresh task, even
            // when it was an internal scaffold in another run.
            return Ok(());
        }

        self.create_internal_scaffold(task).await?;
        self.manages_internal_scaffold = true;
        Ok(())
    }

    /// Read-only incomplete-step labels from an existing tracker.
    ///
    /// Never creates the internal scaffold and never mutates checklist state.
    /// Returns an empty list when no tracker exists or every step is completed.
    pub(super) async fn incomplete_tracker_labels(&self, tracker_adopted: bool) -> Result<Vec<String>> {
        if !self.manages_internal_scaffold && !tracker_adopted {
            return Ok(Vec::new());
        }
        let Some(checklist) = self.load_tracker().await? else {
            return Ok(Vec::new());
        };
        Ok(checklist
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
            .collect())
    }

    pub(super) async fn assess_completion(
        &mut self,
        task: &Task,
        session_state: &AgentSessionState,
        tracker_adopted: bool,
    ) -> Result<CompletionAssessment> {
        if !self.continuation_enabled() {
            return Ok(CompletionAssessment::SkipAccept {
                reason: continuation_skip_reason(
                    &self.continuation_policy,
                    self.full_auto_active,
                    self.planning_active,
                    self.review_like,
                ),
            });
        }

        if !self.manages_internal_scaffold && !tracker_adopted {
            return Ok(CompletionAssessment::SkipAccept {
                reason: "The current task has not adopted the existing workspace checklist.".to_string(),
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
                self.note_progress_stall().await;
                return Ok(CompletionAssessment::Continue {
                    reason: "Task tracker could not be loaded.".to_string(),
                    prompt: "The harness task tracker could not be loaded, so completion cannot be checked yet. \
                             Continue with the task, or say what is blocking it."
                        .to_string(),
                });
            }
        };

        if self.manages_internal_scaffold && is_internal_scaffold(&checklist) {
            self.sync_internal_scaffold_before_completion(session_state, &checklist).await?;
            checklist = self
                .load_tracker()
                .await?
                .context("Internal scaffold should exist after sync")?;
        } else {
            self.manages_internal_scaffold = false;
        }

        self.sync_progress_milestones(&checklist);
        self.checkpoint_progress();

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
            let joined = incomplete_items.join(", ");
            self.note_progress_advance();
            return Ok(CompletionAssessment::Continue {
                reason: format!("Task tracker is incomplete: {joined}."),
                prompt: tracker_incomplete_continue_prompt(&joined),
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
            self.note_progress_advance();
            return Ok(CompletionAssessment::Accept);
        }

        if self.manages_internal_scaffold {
            self.update_internal_step(3, "in_progress", None, None, Some(commands.clone()))
                .await?;
        }

        self.note_progress_advance();
        Ok(CompletionAssessment::Verify { commands })
    }

    pub(super) async fn after_verification(&mut self, results: &[VerificationResult]) -> Result<CompletionAssessment> {
        if results.is_empty() {
            // No verdict observed: leave the streak untouched rather than
            // treating a harness anomaly as a pass or a new shape.
            return Ok(CompletionAssessment::Continue {
                reason: "Unverified: no verification results were observed.".to_string(),
                prompt: "Run a verifier and observe its terminal exit before completing changed code.".to_string(),
            });
        }
        let first_failure = results.iter().find(|result| !result.success || result.exit_code != Some(0));
        if let Some(failure) = first_failure {
            let summary = build_verification_failure_summary(failure);
            let streak = self.note_failure_shape(&failure.output);
            if self.manages_internal_scaffold {
                self.update_internal_step(2, "in_progress", None, None, None).await?;
                self.update_internal_step(
                    3,
                    "blocked",
                    None,
                    Some(summary.clone()),
                    Some(vec![failure.command.clone()]),
                )
                .await?;
            }

            self.note_progress_stall().await;
            let mut prompt = build_verification_failure_prompt(failure);
            prompt.push_str(&vtcode_commons::verification_signal::repeated_shape_note(streak));
            return Ok(CompletionAssessment::Continue { reason: summary, prompt });
        }

        if self.manages_internal_scaffold {
            let summary = format!("Verification passed: {}", format_command_list(results));
            self.update_internal_step(3, "completed", None, Some(summary), None).await?;
        }

        self.last_failure_fingerprint = None;
        self.fingerprint_streak = 0;
        self.note_progress_advance();
        Ok(CompletionAssessment::Accept)
    }

    /// Record one failure shape and return its consecutive-repeat streak.
    /// Unrecognized shapes carry no fingerprint, so they start a fresh
    /// streak of 1 instead of extending the previous one.
    fn note_failure_shape(&mut self, output: &str) -> u8 {
        let shape = vtcode_commons::verification_signal::extract_verification_failure_shape(output);
        let same_shape = self
            .last_failure_fingerprint
            .as_deref()
            .zip(shape.fingerprint.as_deref())
            .is_some_and(|(previous, current)| previous == current);
        let streak = if same_shape {
            self.fingerprint_streak.saturating_add(1)
        } else {
            1
        };
        self.last_failure_fingerprint = shape.fingerprint;
        self.fingerprint_streak = streak;
        streak
    }

    fn continuation_enabled(&self) -> bool {
        if self.planning_active || self.review_like {
            return false;
        }

        match self.continuation_policy {
            ContinuationPolicy::Off => false,
            ContinuationPolicy::ExecOnly => self.full_auto_active,
            ContinuationPolicy::All => true,
        }
    }

    /// Mirror the live task tracker into the durable progress ledger so
    /// completion ratio, milestone status, and resume state stay accurate.
    /// Pure state sync — persistence side effects are delegated to the monitor's
    /// injected sink; the human-readable memory checkpoint is a separate,
    /// explicit step ([`Self::checkpoint_progress`]).
    fn sync_progress_milestones(&mut self, checklist: &TrackerChecklist) {
        if let Some(monitor) = &mut self.progress {
            let milestones = checklist
                .items
                .iter()
                .map(|item| Milestone {
                    id: item.index.map(|i| i.to_string()).unwrap_or_else(|| item.description.clone()),
                    description: item.description.clone(),
                    status: milestone_status_from_str(&item.status),
                })
                .collect();
            monitor.set_milestones(milestones);
        }
    }

    /// Proactively ground progress into durable memory (via the monitor's sink)
    /// so a resumed/forked session can re-ground without waiting for compaction.
    fn checkpoint_progress(&self) {
        if let Some(monitor) = &self.progress {
            monitor.checkpoint();
        }
    }

    /// Record forward progress (no-op without an attached monitor).
    fn note_progress_advance(&mut self) {
        if let Some(monitor) = &mut self.progress {
            monitor.record_advance();
        }
    }

    /// Record a setback/stall (no-op without an attached monitor).
    /// After recording, checks whether a context reset should be triggered
    /// based on the configured stall threshold and context reset mode.
    async fn note_progress_stall(&mut self) {
        let stall_count = {
            let Some(monitor) = &mut self.progress else {
                return;
            };
            monitor.record_stall();
            monitor.consecutive_stalls()
        };
        let workspace_root = self.workspace_root.clone();
        let reset_mode = self.context_reset_mode.as_str().to_owned();
        let threshold = self.context_reset_stall_threshold;

        // Check if the consecutive stall count has crossed the context reset
        // threshold. If so, write a reset manifest without blocking the
        // executor so the next session starts from a clean context.
        if let Err(error) = crate::core::agent::context_reset::maybe_write_reset_on_stall_with_context_async(
            &workspace_root,
            stall_count,
            &reset_mode,
            threshold,
            self.transition_thread_id.clone(),
            self.transition_turn_id.clone(),
        )
        .await
        {
            tracing::warn!(error = %error, "Failed to write context reset manifest after stall");
        }
    }

    async fn load_tracker(&self) -> Result<Option<TrackerChecklist>> {
        let payload = self
            .tracker_tool
            .execute(json!({ "action": "list" }))
            .await
            .context("load task tracker")?;
        let response: TrackerListResponse = serde_json::from_value(payload).context("decode task tracker response")?;
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
            .with_context(|| format!("update internal scaffold step {index}"))?;
        Ok(())
    }
}

fn continuation_skip_reason(
    policy: &ContinuationPolicy,
    full_auto_active: bool,
    planning_active: bool,
    review_like: bool,
) -> String {
    if planning_active {
        return "Continuation disabled in Planning workflow.".to_string();
    }
    if review_like {
        return "Continuation disabled for read-only review runs.".to_string();
    }
    match policy {
        ContinuationPolicy::Off => "Continuation policy is off.".to_string(),
        ContinuationPolicy::ExecOnly if !full_auto_active => {
            "Continuation policy only applies to exec/full-auto runs.".to_string()
        }
        // Dead arm: `continuation_enabled()` returns true for these
        // policies, so this function is never called with them.
        ContinuationPolicy::ExecOnly | ContinuationPolicy::All => "Continuation disabled.".to_string(),
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
    checklist.items.iter().flat_map(|item| item.verify.iter().cloned()).collect()
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
        Some(code) => format!("Verification failed: {} (exit code {}).", failure.command, code),
        None => format!("Unverified: {} produced no terminal verdict.", failure.command),
    }
}

/// Content-free localization appended to failure payloads and prompts.
fn verification_failure_shape_note(failure: &VerificationResult) -> String {
    let shape = vtcode_commons::verification_signal::extract_verification_failure_shape(&failure.output);
    format!(" Failure shape: {}.", shape.describe())
}

/// Compose the harness event payload for a failed verification command.
///
/// Reuses [`build_verification_failure_summary`] for the headline, appends a
/// content-free failure shape, then the trimmed command output when present,
/// matching the layout expected by the `VerificationFailed` event.
pub(super) fn build_verification_failure_payload(failure: &VerificationResult) -> String {
    let mut payload = build_verification_failure_summary(failure);
    payload.push_str(&verification_failure_shape_note(failure));
    if !failure.output.trim().is_empty() {
        payload.push('\n');
        payload.push_str(failure.output.trim());
    }
    payload
}

fn build_verification_failure_prompt(failure: &VerificationResult) -> String {
    let mut prompt = build_verification_failure_summary(failure);
    prompt.push_str(&verification_failure_shape_note(failure));
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
        lower.contains("review command") && lower.contains("read-only")
    })
}

/// Model-facing follow-up injected when the run would end while tracker steps
/// are still open. Shared by completion assessment and the text-only status
/// force-continue path in `execute.rs` so both nudges stay identical.
pub(super) fn tracker_incomplete_continue_prompt(open_steps: &str) -> String {
    format!(
        "Task tracker steps still open: {open_steps}. Continue with the next one in this run \
         instead of asking the user to resume, or say what is blocking it."
    )
}

/// Pure eligibility gate for AgentRunner tracker status continuation.
///
/// Honors the `[agent.harness.continuation].auto_continue_tracker` kill-switch,
/// the idle-turn hard limit exception, and genuine user-question handoffs.
/// Only Continue assessments whose reason names incomplete tracker work force
/// another turn.
pub(super) fn tracker_status_force_continue_eligible(
    auto_continue_tracker: bool,
    idle_limit_hit: bool,
    asks_user: bool,
    assessment_reason: &str,
) -> bool {
    if !auto_continue_tracker || idle_limit_hit || asks_user {
        return false;
    }
    let reason_lower = assessment_reason.to_ascii_lowercase();
    // Only genuine incomplete-tracker signals force another turn. Do not
    // treat scaffold-missing / load-failure reasons as force-continue here;
    // the AgentRunner status path uses a read-only probe and never invents work.
    reason_lower.contains("task tracker is incomplete") || reason_lower.contains("task tracker still has incomplete")
}

#[cfg(test)]
mod tests;
