#![allow(unused_imports)]
use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use futures::future::select_all;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Notify, RwLock};

use crate::config::VTCodeConfig;
use crate::config::types::ReasoningEffortLevel;
use crate::core::agent::runner::{AgentRunner, RunnerSettings};
use crate::core::agent::task::Task;
use crate::core::threads::{ThreadBootstrap, ThreadId, ThreadRuntimeHandle, ThreadSnapshot};
use crate::hooks::{LifecycleHookEngine, SessionStartTrigger};
use crate::llm::provider::Message;
use crate::tools::exec_session::ExecSessionManager;
use crate::tools::pty::{PtyManager, PtySize};
use crate::utils::session_archive::{SessionArchive, find_session_by_identifier};
use vtcode_config::SubagentSpec;
use vtcode_config::auth::OpenAIChatGptAuthHandle;

use self::background::*;
use self::config::*;
use self::constants::*;
use self::discovery::discover_controller_subagents;
use self::model::*;
use vtcode_config::subagents::SUBAGENT_HARD_CONCURRENCY_LIMIT;

#[allow(unused_imports)]
use super::*;

impl SubagentController {
    /// Verify a proposed change by spawning a read-only verifier sub-agent.
    ///
    /// The verifier re-reads the affected files and either approves or rejects
    /// the change. On rejection, the caller can retry the mutation up to N times.
    ///
    /// This implements the propose/verify separation from Osmani's loop
    /// engineering pattern: the proposer and verifier are independent agents
    /// with no shared context bias.
    pub async fn verify_proposed_change(
        &self,
        diff_description: &str,
        file_paths: &[PathBuf],
    ) -> Result<VerificationResult> {
        let verifier_spec = match self.find_spec("verifier").await {
            Some(s) => Some(s),
            None => {
                // Fall back to a synthetic read-only spec if no verifier agent is defined.
                self.find_spec("explorer").await
            }
        };

        let spec = match verifier_spec {
            Some(s) => s,
            None => {
                // No verifier or explorer agent available. Reject by default
                // (fail-closed) — unverified changes must not be merged.
                tracing::warn!("No verifier agent found; rejecting change without verification");
                return Ok(VerificationResult {
                    approved: false,
                    issues: vec!["No verifier agent available to review this change.".to_string()],
                    reasoning: "No verifier agent available; rejected (fail-closed).".to_string(),
                });
            }
        };

        let files_list = file_paths
            .iter()
            .map(|p| format!("- {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "Verify the following proposed change.\n\n\
             ## Diff Description\n{diff_description}\n\n\
             ## Affected Files\n{files_list}\n\n\
             Read each affected file and check for correctness, safety, and convention adherence.\n\
             Respond with your verification result in the format specified in your instructions."
        );

        let request = SpawnAgentRequest {
            agent_type: Some(spec.name.clone()),
            message: Some(prompt),
            items: Vec::new(),
            fork_context: false,
            model: None,
            reasoning_effort: None,
            background: false,
            max_turns: Some(3),
        };

        let status = self.spawn_custom(spec.clone(), request).await?;
        let entry = self.wait(&[status.id.clone()], Some(60_000)).await?;

        match entry {
            Some(entry) if entry.status == SubagentStatus::Completed => {
                let summary = entry.summary.unwrap_or_default();
                let issues = extract_issues_from_summary(&summary);
                let lower = summary.to_lowercase();

                // Positive approval: verifier must explicitly state approval.
                // Ambiguous or unclear results default to rejected (fail-closed).
                let explicitly_approved = lower.contains("approved")
                    || lower.contains("safe to merge")
                    || lower.contains("no issues found")
                    || lower.contains("looks correct")
                    || lower.contains("verification passed");
                let explicitly_rejected = lower.contains("reject")
                    || lower.contains("denied")
                    || lower.contains("unsafe")
                    || lower.contains("blocked")
                    || lower.contains("dangerous")
                    || lower.contains("malicious")
                    || lower.contains("vulnerability");

                let approved = if explicitly_rejected {
                    false
                } else {
                    explicitly_approved && issues.is_empty()
                };

                Ok(VerificationResult {
                    approved,
                    issues,
                    reasoning: summary,
                })
            }
            Some(entry) => {
                let error = entry.error.unwrap_or_default();
                tracing::warn!(
                    error = %error,
                    "Verifier sub-agent failed; rejecting change (fail-closed)"
                );
                Ok(VerificationResult {
                    approved: false,
                    issues: vec![format!("Verifier agent error: {error}")],
                    reasoning: format!("Verifier failed: {error}"),
                })
            }
            None => {
                tracing::warn!("Verifier sub-agent timed out; rejecting change (fail-closed)");
                Ok(VerificationResult {
                    approved: false,
                    issues: vec!["Verifier agent timed out after 60s.".to_string()],
                    reasoning: "Verifier timed out.".to_string(),
                })
            }
        }
    }

    /// Reconcile a worktree-isolated child: diff → verify → merge → cleanup.
    ///
    /// Uses `WorktreeReconciler::reconcile` in a single `spawn_blocking` call
    /// with the canonical [`HeuristicDiffVerifier`](crate::git::HeuristicDiffVerifier).
    /// This avoids borrowing `&self` across spawn boundaries, which would make
    /// the parent `tokio::spawn` future non-Send due to the recursive spawn
    /// chain through `verify_proposed_change`.
    pub(super) async fn run_worktree_reconciliation(
        &self,
        child_id: &str,
        wt_path: &std::path::Path,
        wt_name: &str,
    ) {
        let ws = self.config.workspace_root.clone();
        let wt_name_owned = wt_name.to_string();
        let wt_path_owned = wt_path.to_path_buf();

        let result = tokio::task::spawn_blocking(move || {
            let reconciler = crate::git::WorktreeReconciler::new(&ws, "main");
            let verifier: Box<dyn crate::git::DiffVerifier + Send + Sync> =
                Box::new(crate::git::HeuristicDiffVerifier);
            reconciler.reconcile(&wt_name_owned, &wt_path_owned, verifier.as_ref())
        })
        .await;

        match result {
            Ok(Ok(rr)) if rr.approved && rr.merged => {
                tracing::info!(
                    child_id,
                    worktree = %wt_name,
                    reasoning = %rr.reasoning,
                    "Worktree reconciled and merged"
                );
            }
            Ok(Ok(rr)) if !rr.approved => {
                tracing::warn!(
                    child_id,
                    worktree = %wt_name,
                    issues = ?rr.issues,
                    "Verifier rejected worktree changes; skipping merge"
                );
            }
            Ok(Ok(rr)) => {
                tracing::info!(
                    child_id,
                    worktree = %wt_name,
                    reasoning = %rr.reasoning,
                    "Worktree reconciliation completed (no merge needed)"
                );
            }
            Ok(Err(err)) => {
                tracing::warn!(
                    child_id,
                    worktree = %wt_name,
                    error = %err,
                    "Worktree reconciliation failed"
                );
            }
            Err(err) => {
                tracing::warn!(
                    child_id,
                    worktree = %wt_name,
                    error = %err,
                    "Reconciliation spawn_blocking panicked"
                );
            }
        }
    }
}
