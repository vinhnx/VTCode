use super::canonicalize::{canonicalize, canonicalize_async};
use super::normalize::normalize_path;
use anyhow::{Context, Result, anyhow, bail};
use std::path::{Component, Path, PathBuf};
use tracing::warn;

/// Validate every existing path component and reject escaping symlinks.
///
/// Platform aliases such as macOS `/tmp -> /private/tmp` are accepted when a
/// symlink resolves beneath its containing directory. Missing trailing
/// components are allowed when `allow_missing_tail` is set so callers can
/// create them safely afterwards. A symlinked leaf always fails: callers
/// must decide explicitly whether to follow, replace, or reject it.
///
/// # Errors
///
/// Returns an error when the path contains `..`, when any existing
/// component is a symlink escaping its containing directory (including a
/// symlinked leaf), when a non-leaf component is not a directory, or when
/// a component is missing and `allow_missing_tail` is false.
pub fn validate_no_escaping_symlink_ancestors(path: &Path, allow_missing_tail: bool) -> std::io::Result<()> {
    use std::fs;

    let components = path.components().collect::<Vec<_>>();
    let mut current = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        if matches!(component, Component::ParentDir) {
            return Err(std::io::Error::other(format!("path contains traversal: {}", path.display())));
        }
        if matches!(component, Component::CurDir) {
            continue;
        }
        current.push(component.as_os_str());
        let is_leaf = index + 1 == components.len();
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if allow_missing_tail && error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        };
        if metadata.file_type().is_symlink() {
            if is_leaf {
                return Err(std::io::Error::other(format!("refusing symlink path {}", current.display())));
            }
            let parent = current
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let canonical_parent = canonicalize(parent)?;
            let canonical_target = canonicalize(&current)?;
            if !canonical_target.starts_with(&canonical_parent) {
                return Err(std::io::Error::other(format!(
                    "path component {} escapes its containing directory",
                    current.display()
                )));
            }
            if !fs::metadata(&current)?.is_dir() {
                return Err(std::io::Error::other(format!("path component {} is not a directory", current.display())));
            }
        } else if !is_leaf && !metadata.is_dir() {
            return Err(std::io::Error::other(format!("path component {} is not a directory", current.display())));
        }
    }
    Ok(())
}

/// Render a path relative to the workspace when it belongs to that workspace.
///
/// Paths outside the workspace remain absolute (or otherwise unchanged) so a
/// diagnostic never hides that it refers to an external location. The
/// filesystem-resolved comparison handles workspace aliases and symlink
/// escapes without requiring callers to canonicalize their candidate first.
pub fn workspace_relative_display(workspace_root: &Path, path: &Path) -> String {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    };

    if let Ok(canonical_workspace) = canonicalize(workspace_root) {
        // A successful canonicalization is authoritative: a path that
        // resolves outside the workspace must not fall back to lexical
        // containment through an escaping symlink.
        match canonicalize_for_display(&candidate) {
            DisplayResolution::Resolved(canonical_candidate) => {
                return canonical_candidate
                    .strip_prefix(&canonical_workspace)
                    .map(|relative| relative.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| path.to_string_lossy().into_owned());
            }
            DisplayResolution::Unresolved => return path.to_string_lossy().into_owned(),
        }
    }

    // If the candidate cannot be resolved yet (for example, a new file with a
    // missing tail), retain the cheap lexical behavior for paths that are
    // clearly under the workspace.
    let normalized_candidate = normalize_path(&candidate);
    let normalized_workspace = normalize_path(workspace_root);
    if let Ok(relative) = normalized_candidate.strip_prefix(normalized_workspace) {
        return relative.to_string_lossy().into_owned();
    }
    path.to_string_lossy().into_owned()
}

enum DisplayResolution {
    Resolved(PathBuf),
    Unresolved,
}

fn canonicalize_for_display(path: &Path) -> DisplayResolution {
    if let Ok(canonical) = canonicalize(path) {
        return DisplayResolution::Resolved(canonical);
    }

    let mut missing_tail = Vec::new();
    let mut existing_prefix = path;
    loop {
        match std::fs::symlink_metadata(existing_prefix) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(file_name) = existing_prefix.file_name() else {
                    return DisplayResolution::Unresolved;
                };
                let Some(parent) = existing_prefix.parent() else {
                    return DisplayResolution::Unresolved;
                };
                missing_tail.push(PathBuf::from(file_name));
                existing_prefix = parent;
            }
            Err(_) => return DisplayResolution::Unresolved,
        }
    }

    let Ok(mut canonical) = canonicalize(existing_prefix) else {
        // This includes dangling symlinks and inaccessible existing paths.
        // Do not fall back to lexical containment for either case.
        return DisplayResolution::Unresolved;
    };
    for component in missing_tail.into_iter().rev() {
        canonical.push(component);
    }
    DisplayResolution::Resolved(canonical)
}

/// Resolve a path relative to a workspace root and ensure it stays within it.
pub fn resolve_workspace_path(workspace_root: &Path, user_path: &Path) -> Result<PathBuf> {
    let candidate = if user_path.is_absolute() {
        user_path.to_path_buf()
    } else {
        workspace_root.join(user_path)
    };

    let canonical =
        canonicalize(&candidate).with_context(|| format!("Failed to canonicalize path {}", candidate.display()))?;

    let workspace_canonical = canonicalize(workspace_root)
        .with_context(|| format!("Failed to canonicalize workspace root {}", workspace_root.display()))?;

    if !canonical.starts_with(&workspace_canonical) {
        return Err(anyhow!("Path {} escapes workspace root {}", canonical.display(), workspace_canonical.display()));
    }

    Ok(canonical)
}

/// Return a canonicalised absolute path that is guaranteed to reside inside the
/// provided `workspace_root`.  If the path is outside the workspace an error is
/// returned.
pub fn secure_path(workspace_root: &Path, user_path: &Path) -> Result<PathBuf> {
    // Resolve relative paths against the workspace root.
    resolve_workspace_path(workspace_root, user_path)
}

/// Ensure a candidate path is inside the workspace root after lexical
/// normalization.
///
/// This is the cheap, filesystem-free tier of workspace containment: it
/// resolves `.`/`..` components lexically but does not follow symlinks. Use
/// [`ensure_path_within_workspace_resolved`] when the candidate may traverse
/// symlinks that point outside the workspace.
///
/// Returns the normalized candidate path on success.
pub fn ensure_path_within_workspace(candidate: &Path, workspace_root: &Path) -> Result<PathBuf> {
    let normalized_candidate = normalize_path(candidate);
    let normalized_workspace = normalize_path(workspace_root);

    if !normalized_candidate.starts_with(&normalized_workspace) {
        bail!("Path '{}' escapes workspace '{}'", candidate.display(), workspace_root.display());
    }

    Ok(normalized_candidate)
}

/// Ensure a candidate path is inside the workspace root, resolving symlinks
/// component by component.
///
/// This is the strict, filesystem-aware tier of workspace containment. On top
/// of the lexical check performed by [`ensure_path_within_workspace`], it
/// walks each component of the candidate below the workspace root and:
///
/// - canonicalizes every existing component and verifies the resolved path
///   still starts with the canonical workspace root (catches symlinks that
///   point outside the workspace);
/// - tolerates nonexistent tail components (paths about to be created);
/// - rejects traversal through a file component (e.g. `file.txt/child`).
///
/// The candidate must already be lexically inside `workspace_root` (both
/// sides are normalized before comparison).
///
/// Returns the normalized candidate path on success.
pub async fn ensure_path_within_workspace_resolved(candidate: &Path, workspace_root: &Path) -> Result<PathBuf> {
    let normalized_root = normalize_path(workspace_root);
    let normalized_candidate = normalize_path(candidate);

    let canonical_root = match canonicalize_async(&normalized_root).await {
        Ok(resolved) => resolved,
        Err(error) => {
            warn!(
                path = %normalized_root.display(),
                %error,
                "Failed to canonicalize workspace root; falling back to provided path"
            );
            normalized_root.clone()
        }
    };

    if normalized_root == normalized_candidate {
        return Ok(normalized_candidate);
    }

    let relative = normalized_candidate
        .strip_prefix(&normalized_root)
        .map_err(|_error| anyhow!("path '{}' escapes the workspace root", candidate.display()))?
        .to_path_buf();

    let mut prefix = normalized_root.clone();
    let mut components = relative.components().peekable();

    while let Some(component) = components.next() {
        prefix.push(component.as_os_str());

        let metadata = match tokio::fs::symlink_metadata(&prefix).await {
            Ok(metadata) => metadata,
            Err(error) => {
                if error.kind() == std::io::ErrorKind::NotFound {
                    break;
                }
                return Err(error).with_context(|| format!("failed to inspect path component '{}'", prefix.display()));
            }
        };

        let resolved = canonicalize_async(&prefix)
            .await
            .with_context(|| format!("failed to canonicalize path component '{}'", prefix.display()))?;

        if metadata.file_type().is_symlink() {
            if !resolved.starts_with(&canonical_root) {
                return Err(anyhow!(
                    "path '{}' escapes the workspace root via symlink '{}'",
                    candidate.display(),
                    prefix.display()
                ));
            }
        } else {
            if !resolved.starts_with(&canonical_root) {
                return Err(anyhow!(
                    "path '{}' escapes the workspace root via component '{}'",
                    candidate.display(),
                    prefix.display()
                ));
            }

            if metadata.is_file() && components.peek().is_some() {
                return Err(anyhow!(
                    "path '{}' traverses through file component '{}'",
                    candidate.display(),
                    prefix.display()
                ));
            }
        }
    }

    Ok(normalized_candidate)
}

#[cfg(test)]
mod tests {
    use super::super::canonicalize::canonicalize;
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn workspace_relative_display_uses_workspace_relative_paths() {
        let workspace = Path::new("/workspace");
        let path = Path::new("/workspace/src/main.rs");

        assert_eq!(workspace_relative_display(workspace, path), "src/main.rs");
    }

    #[test]
    fn workspace_relative_display_preserves_external_paths() {
        let workspace = Path::new("/workspace");
        let path = Path::new("/tmp/external.txt");

        assert_eq!(workspace_relative_display(workspace, path), "/tmp/external.txt");
    }

    #[test]
    fn workspace_relative_display_handles_canonical_workspace_paths() {
        let workspace = tempfile::tempdir().unwrap();
        let workspace_path = canonicalize(workspace.path()).unwrap();
        let path = workspace_path.join("docs").join("guide.md");

        assert_eq!(workspace_relative_display(workspace.path(), &path), "docs/guide.md");
    }

    #[cfg(unix)]
    #[test]
    fn workspace_relative_display_resolves_workspace_aliases() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        let alias = temp.path().join("workspace-alias");
        symlink(&workspace, &alias).unwrap();

        let path = alias.join("src").join("main.rs");
        assert_eq!(workspace_relative_display(&workspace, &path), "src/main.rs");
    }

    #[cfg(unix)]
    #[test]
    fn workspace_relative_display_preserves_symlink_escape() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();
        symlink(&outside, workspace.join("linked-outside")).unwrap();

        let path = workspace.join("linked-outside").join("secret.txt");
        assert_eq!(workspace_relative_display(&workspace, &path), path.to_string_lossy());
    }

    #[cfg(unix)]
    #[test]
    fn workspace_relative_display_fails_closed_for_dangling_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let path = workspace.join("dangling-link");
        symlink(temp.path().join("missing-target"), &path).unwrap();

        assert_eq!(workspace_relative_display(&workspace, &path), path.to_string_lossy());
    }

    #[test]
    fn ensures_path_within_workspace_accepts_nested_path() {
        let workspace = Path::new("/tmp/project");
        let candidate = Path::new("/tmp/project/src/../src/lib.rs");
        let normalized = ensure_path_within_workspace(candidate, workspace).unwrap();
        assert_eq!(normalized, PathBuf::from("/tmp/project/src/lib.rs"));
    }

    #[test]
    fn ensures_path_within_workspace_rejects_escape() {
        let workspace = Path::new("/tmp/project");
        let candidate = Path::new("/tmp/project/../../etc/passwd");
        assert!(ensure_path_within_workspace(candidate, workspace).is_err());
    }

    #[tokio::test]
    async fn resolved_check_accepts_nested_existing_path() {
        let workspace = tempfile::tempdir().unwrap();
        let root = canonicalize(workspace.path()).unwrap();
        let nested = root.join("src");
        tokio::fs::create_dir_all(&nested).await.unwrap();
        let file = nested.join("lib.rs");
        tokio::fs::write(&file, b"test").await.unwrap();

        let result = ensure_path_within_workspace_resolved(&file, &root).await;
        assert_eq!(result.unwrap(), file);
    }

    #[tokio::test]
    async fn resolved_check_accepts_missing_tail_components() {
        let workspace = tempfile::tempdir().unwrap();
        let root = canonicalize(workspace.path()).unwrap();
        let missing = root.join("new_dir/new_file.txt");

        let result = ensure_path_within_workspace_resolved(&missing, &root).await;
        assert_eq!(result.unwrap(), missing);
    }

    #[tokio::test]
    async fn resolved_check_rejects_lexical_escape() {
        let workspace = tempfile::tempdir().unwrap();
        let root = canonicalize(workspace.path()).unwrap();
        let escape = root.join("../outside.txt");

        assert!(ensure_path_within_workspace_resolved(&escape, &root).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn resolved_check_rejects_symlink_escape() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = canonicalize(workspace.path()).unwrap();
        let outside_dir = canonicalize(outside.path()).unwrap();

        let link = root.join("escape");
        tokio::fs::symlink(&outside_dir, &link).await.unwrap();

        let candidate = link.join("secret.txt");
        assert!(ensure_path_within_workspace_resolved(&candidate, &root).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn resolved_check_accepts_symlink_within_workspace() {
        let workspace = tempfile::tempdir().unwrap();
        let root = canonicalize(workspace.path()).unwrap();
        let target = root.join("real");
        tokio::fs::create_dir_all(&target).await.unwrap();
        let link = root.join("alias");
        tokio::fs::symlink(&target, &link).await.unwrap();

        let candidate = link.join("file.txt");
        assert!(ensure_path_within_workspace_resolved(&candidate, &root).await.is_ok());
    }

    #[tokio::test]
    async fn resolved_check_rejects_traversal_through_file() {
        let workspace = tempfile::tempdir().unwrap();
        let root = canonicalize(workspace.path()).unwrap();
        let file = root.join("data.txt");
        tokio::fs::write(&file, b"test").await.unwrap();

        let candidate = file.join("child.txt");
        assert!(ensure_path_within_workspace_resolved(&candidate, &root).await.is_err());
    }

    #[test]
    fn symlink_ancestor_validation_rejects_traversal_and_missing_tail() {
        let temp = tempfile::tempdir().expect("tempdir");
        let missing = temp.path().join("absent").join("child");
        assert!(validate_no_escaping_symlink_ancestors(&missing, true).is_ok());
        assert!(validate_no_escaping_symlink_ancestors(&missing, false).is_err());
        assert!(validate_no_escaping_symlink_ancestors(Path::new("a/../b"), false).is_err());
        assert!(validate_no_escaping_symlink_ancestors(temp.path(), false).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_ancestor_validation_rejects_escaping_links_but_allows_in_parent_aliases() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        let escape = temp.path().join("escape");
        symlink(outside.path(), &escape).expect("symlink");
        let error = validate_no_escaping_symlink_ancestors(&escape.join("child"), false)
            .expect_err("escaping ancestor must fail");
        assert!(error.to_string().contains("escapes its containing directory"), "{error}");
        let leaf_error = validate_no_escaping_symlink_ancestors(&escape, false).expect_err("symlinked leaf must fail");
        assert!(leaf_error.to_string().contains("refusing symlink path"), "{leaf_error}");

        let inner = temp.path().join("inner");
        std::fs::create_dir(&inner).expect("mkdir");
        symlink(&inner, temp.path().join("alias")).expect("symlink");
        // An in-parent alias as a non-leaf ancestor is tolerated (this is
        // how platform aliases like /tmp survive validation); as a leaf it
        // still fails closed.
        assert!(validate_no_escaping_symlink_ancestors(&temp.path().join("alias").join("child"), true).is_ok());
        assert!(validate_no_escaping_symlink_ancestors(&temp.path().join("alias"), false).is_err());
    }
}
