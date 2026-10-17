use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::exec::events::matrix::MatrixSpec;

/// Capture content, names and symlink targets; Git extensions and user aliases
/// cannot affect the tracked-file inventory. Declared inputs are also included.
pub async fn fingerprint(root: &Path, spec: &MatrixSpec) -> Result<String> {
    let root = root.to_path_buf();
    let spec = spec.clone();
    tokio::task::spawn_blocking(move || fingerprint_blocking(&root, &spec))
        .await
        .context("matrix fingerprint task failed")?
}

fn fingerprint_blocking(root: &Path, spec: &MatrixSpec) -> Result<String> {
    let root = vtcode_commons::canonicalize(root).context("canonicalize matrix workspace")?;
    let mut paths = BTreeSet::new();
    let mut digest = Sha256::new();
    inventory_tracked_sources(&root, &root, &mut paths, &mut digest)?;
    for task in &spec.tasks {
        for input in &task.inputs {
            collect_input(&root, &Path::new(&task.workspace).join(input), &mut paths)?;
        }
    }
    for path in paths {
        ensure!(
            !path.is_absolute() && !path.components().any(|c| matches!(c, std::path::Component::ParentDir)),
            "invalid matrix source path"
        );
        hash_source_path(&mut digest, &path);
        let absolute = root.join(&path);
        match std::fs::symlink_metadata(&absolute) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                hash_change_stamp(&mut digest, &metadata)?;
                digest.update(b"symlink");
                let target = std::fs::read_link(&absolute)?;
                let bytes = target.as_os_str().as_encoded_bytes();
                digest.update((bytes.len() as u64).to_le_bytes());
                digest.update(bytes);
            }
            Ok(metadata) if metadata.is_file() => {
                ensure!(vtcode_commons::canonicalize(&absolute)?.starts_with(&root), "matrix source escaped workspace");
                hash_change_stamp(&mut digest, &metadata)?;
                digest.update(b"file");
                digest.update(metadata.len().to_le_bytes());
                digest.update(
                    std::fs::read(&absolute).with_context(|| format!("read matrix source {}", path.display()))?,
                );
            }
            Ok(_) => anyhow::bail!("matrix source is not a file: {}", path.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => digest.update(b"deleted"),
            Err(error) => return Err(error).context("inspect matrix source"),
        }
    }
    let mut fingerprint = String::with_capacity(64);
    for byte in digest.finalize() {
        use std::fmt::Write;
        write!(&mut fingerprint, "{byte:02x}")?;
    }
    Ok(fingerprint)
}

fn hash_source_path(digest: &mut Sha256, path: &Path) {
    let bytes = path.as_os_str().as_encoded_bytes();
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
}

fn inventory_tracked_sources(
    root: &Path,
    repository: &Path,
    paths: &mut BTreeSet<PathBuf>,
    digest: &mut Sha256,
) -> Result<()> {
    let output = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "ls-files", "--stage", "-z"])
        .current_dir(repository)
        .output()
        .context("inventory tracked matrix sources")?;
    ensure!(output.status.success(), "matrix fingerprint requires a Git workspace");
    let prefix = repository.strip_prefix(root).context("submodule escaped matrix workspace")?;
    for bytes in output.stdout.split(|byte| *byte == 0).filter(|bytes| !bytes.is_empty()) {
        let entry = std::str::from_utf8(bytes).context("tracked matrix path is not UTF-8")?;
        let (header, name) = entry.split_once('\t').context("invalid tracked matrix inventory entry")?;
        let path = Path::new(name);
        ensure!(
            !path.is_absolute() && !path.components().any(|c| matches!(c, std::path::Component::ParentDir)),
            "invalid matrix source path"
        );
        let relative = prefix.join(path);
        let mut fields = header.split_whitespace();
        let mode = fields.next().context("missing tracked matrix mode")?;
        let revision = fields.next().context("missing tracked matrix revision")?;
        if mode != "160000" {
            paths.insert(relative);
            continue;
        }
        hash_source_path(digest, &relative);
        digest.update(b"gitlink");
        digest.update((revision.len() as u64).to_le_bytes());
        digest.update(revision.as_bytes());
        let absolute = repository.join(path);
        let metadata = match std::fs::symlink_metadata(&absolute) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                digest.update(b"uninitialized");
                continue;
            }
            Err(error) => return Err(error).context("inspect matrix submodule"),
        };
        ensure!(metadata.is_dir() && !metadata.file_type().is_symlink(), "matrix gitlink is not a directory");
        let submodule = vtcode_commons::canonicalize(&absolute).context("resolve matrix submodule")?;
        ensure!(submodule.starts_with(repository) && submodule != repository, "submodule escaped matrix workspace");
        if !submodule.join(".git").exists() {
            digest.update(b"uninitialized");
            continue;
        }
        // An uninitialized directory can otherwise resolve to the parent Git
        // repository. Verify the worktree before inventorying nested sources.
        let top = Command::new("git")
            .args(["-c", "core.fsmonitor=false", "rev-parse", "--show-toplevel"])
            .current_dir(&submodule)
            .output()
            .context("resolve submodule Git workspace")?;
        ensure!(top.status.success(), "invalid initialized matrix submodule");
        let top = std::str::from_utf8(&top.stdout).context("submodule workspace is not UTF-8")?;
        let top = top.strip_suffix('\n').unwrap_or(top);
        ensure!(
            vtcode_commons::canonicalize(Path::new(top))? == submodule,
            "submodule Git workspace escaped gitlink"
        );
        let head = Command::new("git")
            .args(["-c", "core.fsmonitor=false", "rev-parse", "--verify", "HEAD"])
            .current_dir(&submodule)
            .output()
            .context("read matrix submodule HEAD")?;
        ensure!(head.status.success(), "matrix submodule HEAD unavailable");
        digest.update(b"initialized");
        digest.update((head.stdout.len() as u64).to_le_bytes());
        digest.update(head.stdout);
        inventory_tracked_sources(root, &submodule, paths, digest)?;
    }
    Ok(())
}

// Change stamps conservatively invalidate writes even when content is restored.
fn hash_change_stamp(digest: &mut Sha256, metadata: &std::fs::Metadata) -> Result<()> {
    let modified = metadata.modified()?.duration_since(std::time::UNIX_EPOCH)?;
    digest.update(modified.as_secs().to_le_bytes());
    digest.update(modified.subsec_nanos().to_le_bytes());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        digest.update(metadata.ctime().to_le_bytes());
        digest.update(metadata.ctime_nsec().to_le_bytes());
        digest.update(metadata.ino().to_le_bytes());
    }
    Ok(())
}

fn collect_input(root: &Path, path: &Path, paths: &mut BTreeSet<PathBuf>) -> Result<()> {
    let absolute = root.join(path);
    let resolved = vtcode_commons::canonicalize(&absolute).context("resolve declared matrix input")?;
    ensure!(resolved.starts_with(root), "matrix input escaped workspace");
    let metadata = std::fs::symlink_metadata(&absolute)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        for entry in std::fs::read_dir(absolute)? {
            collect_input(root, &path.join(entry?.file_name()), paths)?;
        }
    } else {
        paths.insert(path.to_path_buf());
        if metadata.file_type().is_symlink() {
            paths.insert(
                resolved
                    .strip_prefix(root)
                    .context("resolve workspace-relative matrix input")?
                    .to_path_buf(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::events::matrix::{MatrixTaskSpec, WorkspaceAccess};
    #[tokio::test]
    async fn fingerprint_tracks_dirty_deleted_and_declared_untracked_content() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .arg("init")
                .current_dir(root.path())
                .output()
                .unwrap()
                .status
                .success()
        );
        std::fs::write(root.path().join("source.rs"), "first").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "source.rs"])
                .current_dir(root.path())
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(root.path().join("fixture"), "asymmetric").unwrap();
        let spec = MatrixSpec {
            id: "m".into(),
            resources: Default::default(),
            tasks: vec![MatrixTaskSpec {
                id: "a".into(),
                instructions: "inspect".into(),
                dependencies: vec![],
                workspace: ".".into(),
                access: WorkspaceAccess::Read,
                checks: vec!["true".into()],
                resources: Default::default(),
                timeout_secs: 10,
                replay_safe: false,
                inputs: vec!["fixture".into()],
            }],
        };
        let first = fingerprint(root.path(), &spec).await.unwrap();
        assert_eq!(first, fingerprint(root.path(), &spec).await.unwrap());
        std::fs::write(root.path().join("source.rs"), "temporary").unwrap();
        std::fs::write(root.path().join("source.rs"), "first").unwrap();
        assert_ne!(
            first,
            fingerprint(root.path(), &spec).await.unwrap(),
            "restoring content must invalidate the previous generation"
        );
        std::fs::write(root.path().join("source.rs"), "second").unwrap();
        let second = fingerprint(root.path(), &spec).await.unwrap();
        assert_ne!(first, second);
        std::fs::remove_file(root.path().join("source.rs")).unwrap();
        let deleted = fingerprint(root.path(), &spec).await.unwrap();
        assert_ne!(second, deleted);
        std::fs::write(root.path().join("fixture"), "changed").unwrap();
        assert_ne!(deleted, fingerprint(root.path(), &spec).await.unwrap());
    }
}
