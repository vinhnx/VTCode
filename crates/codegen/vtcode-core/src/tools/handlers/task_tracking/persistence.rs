//! Publication of the tracker, its mirror, and embedded plan projection.

use anyhow::{Context, Result, bail};
use hashbrown::HashSet;
use std::path::{Path, PathBuf};

use vtcode_commons::fs::write_private_file_atomic;

pub(crate) struct TrackerArtifact {
    pub path: PathBuf,
    pub content: String,
    pub context: &'static str,
    previous: Option<String>,
}

impl TrackerArtifact {
    pub(crate) async fn new(path: PathBuf, content: String, context: &'static str) -> Result<Self> {
        let previous = read_tracker_artifact(&path, context).await?;
        Ok(Self { path, content, context, previous })
    }

    pub(crate) async fn embedded_plan(path: PathBuf, tracker_markdown: &str) -> Result<Option<Self>> {
        let Some(previous) = read_tracker_artifact(&path, "plan file").await? else {
            return Ok(None);
        };
        let content =
            crate::tools::handlers::planning_workflow::render_plan_with_tracker(&previous, Some(tracker_markdown));
        Ok(Some(Self {
            path,
            content,
            context: "plan file",
            previous: Some(previous),
        }))
    }
}

pub(crate) async fn read_tracker_artifact(path: &Path, context: &str) -> Result<Option<String>> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Failed to read {context}: {}", path.display())),
    }
}

pub(crate) async fn write_tracker_artifact(path: &Path, content: &str, context: &str) -> Result<()> {
    if read_tracker_artifact(path, context).await?.as_deref() == Some(content) {
        return Ok(());
    }
    publish_tracker_artifact(path, content, context).await
}

async fn publish_tracker_artifact(path: &Path, content: &str, context: &str) -> Result<()> {
    write_private_file_atomic(path, content)
        .await
        .with_context(|| format!("Failed to write {context}: {}", path.display()))
}

async fn rollback_artifacts(published: &[TrackerArtifact]) -> Result<()> {
    let mut failures = Vec::new();
    for artifact in published.iter().rev() {
        let restore = async {
            let current = read_tracker_artifact(&artifact.path, artifact.context).await?;
            if current.as_deref() != Some(artifact.content.as_str()) {
                bail!(
                    "{} changed after publication; refusing to overwrite it during rollback",
                    artifact.path.display()
                );
            }
            if let Some(previous) = &artifact.previous {
                publish_tracker_artifact(&artifact.path, previous, artifact.context).await
            } else {
                tokio::fs::remove_file(&artifact.path)
                    .await
                    .with_context(|| format!("Failed to remove new tracker artifact: {}", artifact.path.display()))
            }
        }
        .await;
        if let Err(error) = restore {
            failures.push(format!("{error:#}"));
        }
    }
    if !failures.is_empty() {
        bail!("Failed to roll back tracker artifacts: {}", failures.join("; "));
    }
    Ok(())
}

/// Snapshot every target before publication. Individual files are atomic;
/// a reported publication failure restores prior files in reverse order.
pub(crate) async fn persist_tracker_artifacts(artifacts: Vec<TrackerArtifact>) -> Result<()> {
    let mut targets = HashSet::with_capacity(artifacts.len());
    for artifact in &artifacts {
        if !targets.insert(&artifact.path) {
            bail!("Duplicate tracker artifact target: {}", artifact.path.display());
        }
        if read_tracker_artifact(&artifact.path, artifact.context).await? != artifact.previous {
            bail!("Tracker artifact changed before publication: {}", artifact.path.display());
        }
    }
    drop(targets);
    let snapshots: Vec<_> = artifacts
        .into_iter()
        .filter(|artifact| artifact.previous.as_deref() != Some(artifact.content.as_str()))
        .collect();
    let mut published = 0usize;
    let publish_result: Result<()> = async {
        for artifact in &snapshots {
            if read_tracker_artifact(&artifact.path, artifact.context).await? != artifact.previous {
                bail!("Tracker artifact changed before publication: {}", artifact.path.display());
            }
            publish_tracker_artifact(&artifact.path, &artifact.content, artifact.context).await?;
            published += 1;
        }
        Ok(())
    }
    .await;
    if let Err(error) = publish_result {
        let published_snapshots = snapshots
            .get(..published)
            .context("published tracker artifact count is invalid")?;
        if let Err(rollback) = rollback_artifacts(published_snapshots).await {
            return Err(error.context(format!("{rollback:#}")));
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn artifact(path: &Path, content: &str) -> TrackerArtifact {
        TrackerArtifact::new(path.to_path_buf(), content.to_string(), "test tracker")
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn failed_publish_restores_old_files_and_removes_new_files() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("existing.md");
        let parent_file = dir.path().join("parent-file");
        std::fs::write(&existing, "old checklist").unwrap();
        let error = persist_tracker_artifacts(vec![
            artifact(&existing, "new checklist").await,
            artifact(&parent_file, "blocks its child").await,
            artifact(&parent_file.join("child.md"), "cannot publish").await,
        ])
        .await
        .unwrap_err();
        assert!(format!("{error:#}").contains("child.md"));
        assert_eq!(std::fs::read_to_string(existing).unwrap(), "old checklist");
        assert!(!parent_file.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinked_tracker_parent_is_rejected_and_prior_publication_is_rolled_back() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        let redirected = dir.path().join("redirected");
        let outside_file = outside.path().join("tracker.md");
        std::fs::write(&first, "original first").unwrap();
        std::fs::write(&outside_file, "outside contents").unwrap();
        symlink(outside.path(), &redirected).unwrap();
        let error = persist_tracker_artifacts(vec![
            artifact(&first, "new first").await,
            artifact(&redirected.join("tracker.md"), "must not escape").await,
        ])
        .await
        .unwrap_err();
        let diagnostic = format!("{error:#}");
        assert!(diagnostic.contains("symlink"), "{diagnostic}");
        assert_eq!(std::fs::read_to_string(first).unwrap(), "original first");
        assert_eq!(std::fs::read_to_string(outside_file).unwrap(), "outside contents");
    }

    #[tokio::test]
    async fn preflight_read_failure_leaves_other_artifacts_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("existing.md");
        std::fs::write(&existing, "old checklist").unwrap();
        let first = artifact(&existing, "new checklist").await;
        let error = TrackerArtifact::new(dir.path().to_path_buf(), "invalid target".into(), "test tracker")
            .await
            .err()
            .unwrap();
        assert!(format!("{error:#}").contains("Failed to read"));
        assert_eq!(std::fs::read_to_string(existing).unwrap(), "old checklist");
        assert_eq!(first.previous.as_deref(), Some("old checklist"));
    }

    #[tokio::test]
    async fn rollback_preserves_concurrent_edits_and_restores_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        let concurrent = dir.path().join("concurrent.md");
        std::fs::write(&first, "published first").unwrap();
        std::fs::write(&concurrent, "another writer").unwrap();
        let mut first_artifact = artifact(&first, "published first").await;
        first_artifact.previous = Some("original first".into());
        let mut concurrent_artifact = artifact(&concurrent, "published second").await;
        concurrent_artifact.previous = Some("original second".into());
        let snapshots = [first_artifact, concurrent_artifact];
        let error = rollback_artifacts(&snapshots).await.unwrap_err();
        assert!(format!("{error:#}").contains("refusing to overwrite"));
        assert_eq!(std::fs::read_to_string(first).unwrap(), "original first");
        assert_eq!(std::fs::read_to_string(concurrent).unwrap(), "another writer");
    }

    #[tokio::test]
    async fn stale_snapshot_and_duplicate_targets_are_rejected_before_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tracker.md");
        std::fs::write(&path, "original").unwrap();
        let stale = artifact(&path, "stale replacement").await;
        std::fs::write(&path, "concurrent update").unwrap();
        let error = persist_tracker_artifacts(vec![stale]).await.unwrap_err();
        assert!(error.to_string().contains("changed before publication"));
        let error = persist_tracker_artifacts(vec![
            artifact(&path, "concurrent update").await,
            artifact(&path, "duplicate replacement").await,
        ])
        .await
        .unwrap_err();
        assert!(error.to_string().contains("Duplicate tracker artifact target"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "concurrent update");
    }
}
