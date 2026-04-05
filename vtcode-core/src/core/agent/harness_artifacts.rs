use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

const TASKS_DIR: &str = ".vtcode/tasks";
const CURRENT_TASK_FILE: &str = "current_task.md";
const CURRENT_SPEC_FILE: &str = "current_spec.md";
const CURRENT_CONTRACT_FILE: &str = "current_contract.md";
const CURRENT_EVALUATION_FILE: &str = "current_evaluation.md";
const SUMMARY_PREVIEW_CHARS: usize = 280;

pub fn current_task_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(TASKS_DIR).join(CURRENT_TASK_FILE)
}

pub fn current_spec_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(TASKS_DIR).join(CURRENT_SPEC_FILE)
}

pub fn current_contract_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(TASKS_DIR).join(CURRENT_CONTRACT_FILE)
}

pub fn current_evaluation_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(TASKS_DIR).join(CURRENT_EVALUATION_FILE)
}

pub fn existing_harness_artifact_paths(workspace_root: &Path) -> Vec<PathBuf> {
    [
        current_spec_path(workspace_root),
        current_contract_path(workspace_root),
        current_evaluation_path(workspace_root),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .collect()
}

pub fn read_spec_summary(workspace_root: &Path) -> Option<String> {
    read_markdown_summary(&current_spec_path(workspace_root), "Spec")
}

pub fn read_contract_summary(workspace_root: &Path) -> Option<String> {
    read_markdown_summary(&current_contract_path(workspace_root), "Contract")
}

pub fn read_evaluation_summary(workspace_root: &Path) -> Option<String> {
    read_markdown_summary(&current_evaluation_path(workspace_root), "Evaluation")
}

pub async fn write_spec(workspace_root: &Path, content: &str) -> Result<PathBuf> {
    let path = current_spec_path(workspace_root);
    write_artifact(path.as_path(), content, "current spec").await?;
    Ok(path)
}

pub async fn write_evaluation(workspace_root: &Path, content: &str) -> Result<PathBuf> {
    let path = current_evaluation_path(workspace_root);
    write_artifact(path.as_path(), content, "current evaluation").await?;
    Ok(path)
}

pub async fn write_contract(workspace_root: &Path, content: &str) -> Result<PathBuf> {
    let path = current_contract_path(workspace_root);
    write_artifact(path.as_path(), content, "current contract").await?;
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
    if text.chars().count() <= SUMMARY_PREVIEW_CHARS {
        return text.to_string();
    }

    let truncated = text
        .chars()
        .take(SUMMARY_PREVIEW_CHARS.saturating_sub(3))
        .collect::<String>();
    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn writes_and_summarizes_spec_and_evaluation_artifacts() {
        let temp = tempdir().expect("tempdir");

        write_spec(
            temp.path(),
            "# Spec\n\nBuild a stronger exec harness.\n\nKeep it resumable.\n",
        )
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
}
