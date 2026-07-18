//! Orient phase for agent sessions.
//!
//! Following the long-running harness pattern: "the agent must orient itself
//! before it can act." The orient phase reads external artifacts (progress
//! ledger, harness artifacts, loop memory, git log) so the agent has an
//! accurate picture of the current state without re-exploring from scratch.
//!
//! This is the just-in-time context injection: references and summaries rather
//! than full content, keeping the context lean while providing grounding.

use std::path::Path;

use super::handoff::HandoffRequest;
use super::harness_artifacts;

/// Structured orientation context gathered at session start.
#[derive(Debug, Clone)]
pub struct OrientationContext {
    /// Summary of the progress ledger (completion ratio, stalled status, milestones).
    pub progress_summary: Option<String>,
    /// Summary of the current spec artifact.
    pub spec_summary: Option<String>,
    /// Summary of the current contract artifact.
    pub contract_summary: Option<String>,
    /// Summary of the sprint contract (pre-sprint negotiation).
    pub sprint_contract_summary: Option<String>,
    /// Summary of the last evaluation.
    pub evaluation_summary: Option<String>,
    /// Summary of the last outcome verification.
    pub outcome_verification_summary: Option<String>,
    /// Recent git log (last N commits).
    pub recent_git_log: Option<String>,
    /// Loop memory notes from previous iterations.
    pub loop_notes: Option<String>,
    /// Loop decisions from previous iterations.
    pub loop_decisions: Option<String>,
    /// Compaction summary from previous sessions.
    pub compaction_summary: Option<String>,
    /// Feature list summary — the persistent artifact the planner creates and
    /// the evaluator modifies during feedback-driven replanning.
    pub feature_list_summary: Option<String>,
    /// Context reset manifest — present when the previous session triggered a
    /// context reset (stall or compaction). Signals that this session starts
    /// from a clean context and should reorient from artifacts only.
    pub context_reset_manifest: Option<String>,
    /// Handoff from a previous agent, if any.
    pub handoff: Option<HandoffRequest>,
}

impl OrientationContext {
    /// Render the orientation context as a section for the system prompt.
    ///
    /// Returns `None` if there is no meaningful context to inject (fresh session).
    pub fn to_prompt_section(&self) -> Option<String> {
        let mut parts = Vec::new();

        if let Some(progress) = &self.progress_summary {
            parts.push(format!("### Progress\n{progress}"));
        }
        if let Some(spec) = &self.spec_summary {
            parts.push(format!("### Spec\n{spec}"));
        }
        if let Some(contract) = &self.contract_summary {
            parts.push(format!("### Contract\n{contract}"));
        }
        if let Some(sprint) = &self.sprint_contract_summary {
            parts.push(format!("### Sprint Contract\n{sprint}"));
        }
        if let Some(eval) = &self.evaluation_summary {
            parts.push(format!("### Last Evaluation\n{eval}"));
        }
        if let Some(outcome) = &self.outcome_verification_summary {
            parts.push(format!("### Outcome Verification\n{outcome}"));
        }
        if let Some(git) = &self.recent_git_log {
            parts.push(format!("### Recent Git Log\n{git}"));
        }
        if let Some(notes) = &self.loop_notes {
            parts.push(format!("### Loop Notes\n{notes}"));
        }
        if let Some(decisions) = &self.loop_decisions {
            parts.push(format!("### Loop Decisions\n{decisions}"));
        }
        if let Some(compaction) = &self.compaction_summary {
            parts.push(format!("### Previous Session Summary\n{compaction}"));
        }
        if let Some(feature_list) = &self.feature_list_summary {
            parts.push(format!("### Feature List\n{feature_list}"));
        }
        if let Some(reset) = &self.context_reset_manifest {
            parts.push(format!(
                "### Context Reset\n\
                 This session starts from a clean context. Previous conversation \
                 history was deliberately discarded to clear noise and bad \
                 assumptions. Reorient from the artifacts below.\n\n{reset}"
            ));
        }
        if let Some(handoff) = &self.handoff {
            parts.push(handoff.to_handoff_prompt());
        }

        if parts.is_empty() {
            return None;
        }

        Some(format!("[Orientation Context]\n\n{}", parts.join("\n\n")))
    }
}

/// Gather orientation context from the workspace.
///
/// Reads all available external artifacts and returns a structured summary
/// that can be injected into the system prompt. Each read is best-effort:
/// a missing artifact is `None`, not an error.
pub fn gather_orientation(workspace_root: &Path, session_id: &str) -> OrientationContext {
    use crate::loop_memory::{LoopMemoryStore, MarkdownLoopMemory};

    let progress_summary = vtcode_session_store::progress::load_progress(workspace_root, session_id)
        .ok()
        .flatten()
        .map(|ledger| {
            format!(
                "Goal: {} | Completion: {:.0}% | Confidence: {:.2} | {}",
                ledger.goal,
                ledger.completion_ratio() * 100.0,
                ledger.confidence,
                if ledger.is_stalled() { "STALLED" } else { "on track" },
            )
        });

    let spec_summary = harness_artifacts::read_spec_summary(workspace_root);
    let contract_summary = harness_artifacts::read_contract_summary(workspace_root);
    let sprint_contract_summary = harness_artifacts::read_sprint_contract_summary(workspace_root);
    let evaluation_summary = harness_artifacts::read_evaluation_summary(workspace_root);
    let outcome_verification_summary = harness_artifacts::read_outcome_verification_summary(workspace_root);
    let feature_list_summary = harness_artifacts::read_feature_list_summary(workspace_root);

    let recent_git_log = gather_recent_git_log(workspace_root);

    let memory = MarkdownLoopMemory::new(workspace_root);
    let loop_notes = memory.read_notes().ok().filter(|s| !s.is_empty());
    let loop_decisions = memory.read_decisions().ok().filter(|s| !s.is_empty());

    // Read compaction summary from persistent artifacts.
    // This follows the context engineering principle: "the summary should be
    // written into a persistent artifact, such as progress.md, so that later
    // sessions can read it."
    let compaction_summary = read_compaction_summary(workspace_root);

    // Read context reset manifest if a reset was triggered by the previous
    // session. This signals that the current session starts from a clean
    // context and should reorient from artifacts only.
    let context_reset_manifest = read_context_reset_manifest(workspace_root);

    OrientationContext {
        progress_summary,
        spec_summary,
        contract_summary,
        sprint_contract_summary,
        evaluation_summary,
        outcome_verification_summary,
        recent_git_log,
        loop_notes,
        loop_decisions,
        compaction_summary,
        feature_list_summary,
        context_reset_manifest,
        handoff: None,
    }
}

/// Gather the last 5 git commits as a compact summary.
fn gather_recent_git_log(workspace_root: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["log", "--oneline", "-5", "--no-decorate"])
        .current_dir(workspace_root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let log = String::from_utf8_lossy(&output.stdout).to_string();
    if log.trim().is_empty() { None } else { Some(log) }
}

/// Read compaction summary from persistent artifacts.
///
/// Reads `memories/compaction_summary.md` written by the compaction checkpoint.
/// This allows later sessions to orient from previous session's compaction summary.
fn read_compaction_summary(workspace_root: &Path) -> Option<String> {
    let path = workspace_root.join("memories").join("compaction_summary.md");
    let content = std::fs::read_to_string(&path).ok()?;
    if content.trim().is_empty() { None } else { Some(content) }
}

/// Read the context reset manifest if a reset was triggered by the previous
/// session. Returns the markdown content of the manifest, or `None` if no
/// reset occurred.
fn read_context_reset_manifest(workspace_root: &Path) -> Option<String> {
    let path = harness_artifacts::current_context_reset_path(workspace_root);
    let content = std::fs::read_to_string(&path).ok()?;
    if content.trim().is_empty() { None } else { Some(content) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_context_returns_none() {
        let ctx = OrientationContext {
            progress_summary: None,
            spec_summary: None,
            contract_summary: None,
            sprint_contract_summary: None,
            evaluation_summary: None,
            outcome_verification_summary: None,
            recent_git_log: None,
            loop_notes: None,
            loop_decisions: None,
            compaction_summary: None,
            feature_list_summary: None,
            context_reset_manifest: None,
            handoff: None,
        };
        assert!(ctx.to_prompt_section().is_none());
    }

    #[test]
    fn single_field_produces_section() {
        let ctx = OrientationContext {
            progress_summary: Some("Goal: test | Completion: 50%".to_string()),
            spec_summary: None,
            contract_summary: None,
            sprint_contract_summary: None,
            evaluation_summary: None,
            outcome_verification_summary: None,
            recent_git_log: None,
            loop_notes: None,
            loop_decisions: None,
            compaction_summary: None,
            feature_list_summary: None,
            context_reset_manifest: None,
            handoff: None,
        };
        let section = ctx.to_prompt_section().expect("should have section");
        assert!(section.contains("[Orientation Context]"));
        assert!(section.contains("### Progress"));
        assert!(section.contains("Goal: test"));
    }

    #[test]
    fn multiple_fields_rendered() {
        let ctx = OrientationContext {
            progress_summary: Some("progress".to_string()),
            spec_summary: Some("spec".to_string()),
            contract_summary: None,
            sprint_contract_summary: None,
            evaluation_summary: None,
            outcome_verification_summary: None,
            recent_git_log: Some("abc123 commit".to_string()),
            loop_notes: None,
            loop_decisions: None,
            compaction_summary: None,
            feature_list_summary: Some("auth, api".to_string()),
            context_reset_manifest: None,
            handoff: None,
        };
        let section = ctx.to_prompt_section().expect("should have section");
        assert!(section.contains("### Progress"));
        assert!(section.contains("### Spec"));
        assert!(section.contains("### Recent Git Log"));
        assert!(section.contains("### Feature List"));
        // contract should be absent
        assert!(!section.contains("### Contract"));
    }

    #[test]
    fn context_reset_manifest_rendered() {
        let ctx = OrientationContext {
            progress_summary: Some("Goal: test | Completion: 50%".to_string()),
            spec_summary: None,
            contract_summary: None,
            sprint_contract_summary: None,
            evaluation_summary: None,
            outcome_verification_summary: None,
            recent_git_log: None,
            loop_notes: None,
            loop_decisions: None,
            compaction_summary: None,
            feature_list_summary: None,
            context_reset_manifest: Some("# Context Reset Manifest\n\n**Trigger:** stall".to_string()),
            handoff: None,
        };
        let section = ctx.to_prompt_section().expect("should have section");
        assert!(section.contains("### Context Reset"));
        assert!(section.contains("clean context"));
        assert!(section.contains("**Trigger:** stall"));
    }
}
