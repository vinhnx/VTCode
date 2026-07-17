use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

const TASKS_DIR: &str = ".vtcode/tasks";
const CURRENT_TASK_FILE: &str = "current_task.md";
const CURRENT_SPEC_FILE: &str = "current_spec.md";
const CURRENT_CONTRACT_FILE: &str = "current_contract.md";
const CURRENT_EVALUATION_FILE: &str = "current_evaluation.md";
const CURRENT_SPRINT_CONTRACT_FILE: &str = "current_sprint_contract.md";
const CURRENT_OUTCOME_VERIFICATION_FILE: &str = "current_outcome_verification.md";
const CURRENT_FEATURE_LIST_FILE: &str = "current_feature_list.md";
const SUMMARY_PREVIEW_CHARS: usize = 280;

/// Return the path to the current task tracker file.
pub fn current_task_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(TASKS_DIR).join(CURRENT_TASK_FILE)
}

/// Return the path to the current context reset manifest file.
pub fn current_context_reset_path(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(TASKS_DIR)
        .join(crate::core::agent::context_reset::CONTEXT_RESET_FILE)
}

/// Return the path to the current spec artifact file.
pub fn current_spec_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(TASKS_DIR).join(CURRENT_SPEC_FILE)
}

/// Return the path to the current contract artifact file.
pub fn current_contract_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(TASKS_DIR).join(CURRENT_CONTRACT_FILE)
}

/// Return the path to the current evaluation artifact file.
pub fn current_evaluation_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(TASKS_DIR).join(CURRENT_EVALUATION_FILE)
}

/// Return the path to the current sprint contract artifact file.
///
/// The sprint contract is the pre-sprint negotiation artifact: the generator
/// and evaluator agree on scope, acceptance criteria, and out-of-scope items
/// before implementation begins. This follows the long-running harness pattern
/// where "vague user stories become testable contracts."
pub fn current_sprint_contract_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(TASKS_DIR).join(CURRENT_SPRINT_CONTRACT_FILE)
}

/// Return the path to the current outcome verification artifact file.
///
/// The outcome verification records what commands were run to verify, what the
/// actual output was, and whether tests/build passed. This enforces "evaluate
/// outcomes, not claims" -- the agent cannot declare success without showing
/// actual verification output.
pub fn current_outcome_verification_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(TASKS_DIR).join(CURRENT_OUTCOME_VERIFICATION_FILE)
}

/// Return the paths of all harness artifacts that currently exist on disk.
pub fn existing_harness_artifact_paths(workspace_root: &Path) -> Vec<PathBuf> {
    [
        current_spec_path(workspace_root),
        current_contract_path(workspace_root),
        current_evaluation_path(workspace_root),
        current_sprint_contract_path(workspace_root),
        current_outcome_verification_path(workspace_root),
        current_feature_list_path(workspace_root),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .collect()
}

/// Read a short summary of the current spec artifact, or `None` if unavailable.
pub fn read_spec_summary(workspace_root: &Path) -> Option<String> {
    read_markdown_summary(&current_spec_path(workspace_root), "Spec")
}

/// Read a short summary of the current contract artifact, or `None` if unavailable.
pub fn read_contract_summary(workspace_root: &Path) -> Option<String> {
    read_markdown_summary(&current_contract_path(workspace_root), "Contract")
}

/// Read a short summary of the current evaluation artifact, or `None` if unavailable.
pub fn read_evaluation_summary(workspace_root: &Path) -> Option<String> {
    read_markdown_summary(&current_evaluation_path(workspace_root), "Evaluation")
}

/// Write the spec artifact content to disk and return the path.
pub async fn write_spec(workspace_root: &Path, content: &str) -> Result<PathBuf> {
    let path = current_spec_path(workspace_root);
    write_artifact(path.as_path(), content, "current spec").await?;
    Ok(path)
}

/// Write the evaluation artifact content to disk and return the path.
pub async fn write_evaluation(workspace_root: &Path, content: &str) -> Result<PathBuf> {
    let path = current_evaluation_path(workspace_root);
    write_artifact(path.as_path(), content, "current evaluation").await?;
    Ok(path)
}

/// Write the contract artifact content to disk and return the path.
pub async fn write_contract(workspace_root: &Path, content: &str) -> Result<PathBuf> {
    let path = current_contract_path(workspace_root);
    write_artifact(path.as_path(), content, "current contract").await?;
    Ok(path)
}

/// Read a short summary of the sprint contract artifact, or `None` if unavailable.
pub fn read_sprint_contract_summary(workspace_root: &Path) -> Option<String> {
    read_markdown_summary(&current_sprint_contract_path(workspace_root), "SprintContract")
}

/// Write the sprint contract artifact content to disk and return the path.
///
/// The sprint contract is the pre-sprint negotiation artifact where generator
/// and evaluator agree on scope and acceptance criteria before code is written.
pub async fn write_sprint_contract(workspace_root: &Path, content: &str) -> Result<PathBuf> {
    let path = current_sprint_contract_path(workspace_root);
    write_artifact(path.as_path(), content, "sprint contract").await?;
    Ok(path)
}

/// Read a short summary of the outcome verification artifact, or `None` if unavailable.
pub fn read_outcome_verification_summary(workspace_root: &Path) -> Option<String> {
    read_markdown_summary(&current_outcome_verification_path(workspace_root), "OutcomeVerification")
}

/// Return the path to the current feature list artifact file.
///
/// The feature list is a persistent artifact the planner creates and the
/// evaluator modifies during feedback-driven replanning. It lists the
/// project's features with their acceptance criteria, so each agent session
/// can pick up an incremental unit of work. Following the long-running
/// harness pattern: "the planner can achieve replanning by modifying external
/// files: feature_list, sprint_contract, known_issues, next_actions."
pub fn current_feature_list_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(TASKS_DIR).join(CURRENT_FEATURE_LIST_FILE)
}

/// Read a short summary of the feature list artifact, or `None` if unavailable.
pub fn read_feature_list_summary(workspace_root: &Path) -> Option<String> {
    read_markdown_summary(&current_feature_list_path(workspace_root), "FeatureList")
}

/// Write the feature list artifact content to disk and return the path.
pub async fn write_feature_list(workspace_root: &Path, content: &str) -> Result<PathBuf> {
    let path = current_feature_list_path(workspace_root);
    write_artifact(path.as_path(), content, "feature list").await?;
    Ok(path)
}

/// Write the outcome verification artifact content to disk and return the path.
///
/// This records actual verification commands and their output, enforcing
/// "evaluate outcomes, not claims" -- the agent must show proof of verification.
pub async fn write_outcome_verification(workspace_root: &Path, content: &str) -> Result<PathBuf> {
    let path = current_outcome_verification_path(workspace_root);
    write_artifact(path.as_path(), content, "outcome verification").await?;
    Ok(path)
}

async fn write_artifact(path: &Path, content: &str, label: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create {} directory {}", label, parent.display()))?;
    }

    tokio::fs::write(path, content)
        .await
        .with_context(|| format!("write {} {}", label, path.display()))?;
    Ok(())
}

fn read_markdown_summary(path: &Path, label: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .take(4)
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }

    let joined = lines.join(" | ");
    Some(format!("{label}: {}", truncate_summary(&joined)))
}

fn truncate_summary(text: &str) -> String {
    vtcode_commons::formatting::truncate_within(text, SUMMARY_PREVIEW_CHARS, "...")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn writes_and_summarizes_spec_and_evaluation_artifacts() {
        let temp = tempdir().expect("tempdir");

        write_spec(temp.path(), "# Spec\n\nBuild a stronger exec harness.\n\nKeep it resumable.\n")
            .await
            .expect("write spec");
        write_contract(
            temp.path(),
            "# Contract\n\n- Deliver the requested change.\n- Verify with cargo check.\n",
        )
        .await
        .expect("write contract");
        write_evaluation(
            temp.path(),
            "# Evaluation\n\nVerdict: fail\n\nNeed another revision round.\n",
        )
        .await
        .expect("write evaluation");

        let paths = existing_harness_artifact_paths(temp.path());
        assert_eq!(paths.len(), 3);
        assert_eq!(
            read_spec_summary(temp.path()),
            Some("Spec: Build a stronger exec harness. | Keep it resumable.".to_string())
        );
        assert_eq!(
            read_contract_summary(temp.path()),
            Some(
                "Contract: - Deliver the requested change. | - Verify with cargo check."
                    .to_string()
            )
        );
        assert_eq!(
            read_evaluation_summary(temp.path()),
            Some("Evaluation: Verdict: fail | Need another revision round.".to_string())
        );
    }

    #[tokio::test]
    async fn writes_and_summarizes_sprint_contract() {
        let temp = tempdir().expect("tempdir");

        write_sprint_contract(
            temp.path(),
            "# Sprint Contract\n\nScope: implement login endpoint.\nAcceptance: POST /login returns JWT.\n",
        )
        .await
        .expect("write sprint contract");

        let paths = existing_harness_artifact_paths(temp.path());
        assert_eq!(paths.len(), 1);
        assert_eq!(
            read_sprint_contract_summary(temp.path()),
            Some(
                "SprintContract: Scope: implement login endpoint. | Acceptance: POST /login returns JWT."
                    .to_string()
            )
        );
    }

    #[tokio::test]
    async fn writes_and_summarizes_outcome_verification() {
        let temp = tempdir().expect("tempdir");

        write_outcome_verification(
            temp.path(),
            "# Outcome Verification\n\nCommand: cargo nextest run\nResult: 12 passed, 0 failed\nBuild: cargo check PASSED\n",
        )
        .await
        .expect("write outcome verification");

        let paths = existing_harness_artifact_paths(temp.path());
        assert_eq!(paths.len(), 1);
        assert_eq!(
            read_outcome_verification_summary(temp.path()),
            Some(
                "OutcomeVerification: Command: cargo nextest run | Result: 12 passed, 0 failed | Build: cargo check PASSED"
                    .to_string()
            )
        );
    }

    #[tokio::test]
    async fn writes_and_summarizes_feature_list() {
        let temp = tempdir().expect("tempdir");

        write_feature_list(
            temp.path(),
            "# Feature List\n\n- [ ] Auth: login endpoint returns JWT\n- [x] API: health check endpoint\n",
        )
        .await
        .expect("write feature list");

        let paths = existing_harness_artifact_paths(temp.path());
        assert_eq!(paths.len(), 1);
        assert_eq!(
            read_feature_list_summary(temp.path()),
            Some(
                "FeatureList: - [ ] Auth: login endpoint returns JWT | - [x] API: health check endpoint"
                    .to_string()
            )
        );
    }

    #[tokio::test]
    async fn all_artifacts_counted_in_existing_paths() {
        let temp = tempdir().expect("tempdir");

        write_spec(temp.path(), "# Spec\ncontent\n").await.unwrap();
        write_contract(temp.path(), "# Contract\ncontent\n").await.unwrap();
        write_evaluation(temp.path(), "# Evaluation\ncontent\n").await.unwrap();
        write_sprint_contract(temp.path(), "# Sprint\ncontent\n").await.unwrap();
        write_outcome_verification(temp.path(), "# Outcome\ncontent\n").await.unwrap();
        write_feature_list(temp.path(), "# Features\ncontent\n").await.unwrap();

        let paths = existing_harness_artifact_paths(temp.path());
        assert_eq!(paths.len(), 6);
    }
}
