#![expect(
    clippy::cast_possible_truncation,
    unused_results,
    reason = "Ignore-pattern counts use the platform's documented compact representation and builder calls are side effects."
)]

//! .vtcodegitignore file pattern matching utilities
//!
//! Uses the `ignore` crate's gitignore parser for correct, battle-tested
//! pattern matching instead of hand-rolled glob conversion.

use anyhow::{Result, anyhow};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::fs::FileType;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;

/// Represents a .vtcodegitignore file with pattern matching capabilities
#[derive(Debug, Clone)]
pub struct VTCodeGitignore {
    /// Root directory where .vtcodegitignore was found
    root_dir: PathBuf,
    /// The same root resolved on disk, when it exists.
    ///
    /// Walkers report resolved paths, so a workspace reached through an alias
    /// (`/var` vs macOS's `/private/var`) yields entries that do not start with
    /// `root_dir`. Keeping both spellings lets matching stay lexical instead of
    /// canonicalizing every entry.
    canonical_root: Option<PathBuf>,
    /// Compiled gitignore matcher
    matcher: Gitignore,
    /// Whether the .vtcodegitignore file exists and was loaded
    loaded: bool,
}

impl VTCodeGitignore {
    /// Create a new VTCodeGitignore instance by looking for .vtcodegitignore in the current directory
    pub async fn new() -> Result<Self> {
        let current_dir = std::env::current_dir().map_err(|e| anyhow!("Failed to get current directory: {e}"))?;

        Self::from_directory(&current_dir).await
    }

    /// Create a VTCodeGitignore instance from a specific directory
    pub async fn from_directory(root_dir: &Path) -> Result<Self> {
        let gitignore_path = root_dir.join(".vtcodegitignore");

        let mut loaded = false;
        let mut builder = GitignoreBuilder::new(root_dir);

        if gitignore_path.exists() {
            match Self::load_patterns(&gitignore_path, &mut builder).await {
                Ok(()) => {
                    loaded = true;
                }
                Err(e) => {
                    // Log warning but don't fail - just treat as no patterns
                    tracing::warn!("Failed to load .vtcodegitignore: {}", e);
                }
            }
        }

        let matcher = builder.build().unwrap_or_else(|_| {
            // Fallback to empty matcher on build error
            Gitignore::empty()
        });

        // Resolved once here so the per-entry match path never canonicalizes.
        let canonical_root = crate::paths::canonicalize(root_dir).ok();

        Ok(Self {
            root_dir: root_dir.to_path_buf(),
            canonical_root,
            matcher,
            loaded,
        })
    }

    /// Load patterns from the .vtcodegitignore file into the builder
    async fn load_patterns(file_path: &Path, builder: &mut GitignoreBuilder) -> Result<()> {
        let content = fs::read_to_string(file_path)
            .await
            .map_err(|e| anyhow!("Failed to read .vtcodegitignore: {e}"))?;

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            builder
                .add_line(None, line)
                .map_err(|e| anyhow!("Invalid pattern on line {}: '{}': {}", line_num + 1, line, e))?;
        }

        Ok(())
    }

    /// Check if a file path should be excluded based on the .vtcodegitignore patterns
    pub fn should_exclude(&self, file_path: &Path) -> bool {
        self.should_exclude_with_is_dir(file_path, file_path.is_dir())
    }

    /// Same as [`Self::should_exclude`] using a file type the caller already has.
    ///
    /// Traversal code holds each entry's `FileType` from `readdir`, so calling
    /// `is_dir()` again costs one `stat` per entry for a fact already in hand.
    ///
    /// A symlink's own type says nothing about its target, and this matcher has
    /// always followed the link (`Path::is_dir`), so symlinks and unknown types
    /// take the `stat` path rather than guessing.
    pub fn should_exclude_with_file_type(&self, file_path: &Path, file_type: Option<FileType>) -> bool {
        match file_type {
            Some(file_type) if !file_type.is_symlink() => {
                self.should_exclude_with_is_dir(file_path, file_type.is_dir())
            }
            _ => self.should_exclude(file_path),
        }
    }

    /// Same as [`Self::should_exclude`] with a caller-supplied directory flag.
    ///
    /// The caller must have resolved the flag from the target, not from a
    /// symlink's own type; prefer [`Self::should_exclude_with_file_type`],
    /// which applies that rule.
    pub fn should_exclude_with_is_dir(&self, file_path: &Path, is_dir: bool) -> bool {
        if !self.loaded {
            return false;
        }

        // Patterns only govern paths below this file's root, and the underlying
        // matcher *asserts* that its input is rootless. Handing it a path that
        // could not be relativized — outside the root, or spelled through a
        // different alias of the same directory (`/var` vs macOS's
        // `/private/var`) — aborts the process instead of excluding anything.
        //
        // Try both spellings of the root. Walkers report resolved paths, so the
        // canonical form is what usually matches; the user-facing form covers
        // paths that arrive as typed. Both are compared lexically, so this stays
        // syscall-free on a per-entry path.
        if let Some(relative) = self.relative_to_root(file_path) {
            return self.matcher.matched_path_or_any_parents(relative, is_dir).is_ignore();
        }

        // Still unrelated lexically: the entry may be reached through yet
        // another alias of the root. Resolving is a syscall, so it happens only
        // after both cheap spellings failed, never per entry in a walk.
        let Some(canonical_root) = self.canonical_root.as_ref() else {
            return false;
        };
        let Ok(canonical_path) = crate::paths::canonicalize(file_path) else {
            return false;
        };
        let Ok(relative) = canonical_path.strip_prefix(canonical_root) else {
            return false;
        };

        self.matcher.matched_path_or_any_parents(relative, is_dir).is_ignore()
    }

    /// Relativize `file_path` against either stored spelling of the root.
    ///
    /// The result borrows `file_path` (both `strip_prefix` cases slice it), so
    /// its lifetime is independent of `self`.
    fn relative_to_root<'a>(&self, file_path: &'a Path) -> Option<&'a Path> {
        file_path
            .strip_prefix(&self.root_dir)
            .ok()
            .or_else(|| self.canonical_root.as_ref().and_then(|root| file_path.strip_prefix(root).ok()))
    }

    /// Filter a list of file paths based on .vtcodegitignore patterns
    pub fn filter_paths(&self, paths: Vec<PathBuf>) -> Vec<PathBuf> {
        if !self.loaded {
            return paths;
        }

        paths.into_iter().filter(|path| !self.should_exclude(path)).collect()
    }

    /// Check if the .vtcodegitignore file was loaded successfully
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Get the number of patterns loaded
    pub fn pattern_count(&self) -> usize {
        self.matcher.num_ignores() as usize
    }

    /// Get the root directory
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }
}

impl Default for VTCodeGitignore {
    fn default() -> Self {
        let root_dir = PathBuf::new();
        let matcher = Gitignore::empty();
        Self {
            root_dir,
            canonical_root: None,
            matcher,
            loaded: false,
        }
    }
}

/// Global .vtcodegitignore instance for easy access.
///
/// Read-mostly: `should_exclude_file`/`filter_paths` are called per file while
/// `reload_vtcode_gitignore` swaps the whole matcher. A `tokio::sync::RwLock`
/// would put atomic contention and an async lock acquisition on the read path;
/// `ArcSwap` keeps reads lock-free (the article's "beware RWLock contention"
/// guidance) while preserving atomic whole-value replacement.
static VTCODE_GITIGNORE: once_cell::sync::Lazy<arc_swap::ArcSwap<VTCodeGitignore>> =
    once_cell::sync::Lazy::new(|| arc_swap::ArcSwap::from_pointee(VTCodeGitignore::default()));

/// Initialize the global .vtcodegitignore instance
pub async fn initialize_vtcode_gitignore() -> Result<()> {
    let gitignore = VTCodeGitignore::new().await?;
    VTCODE_GITIGNORE.store(Arc::new(gitignore));
    Ok(())
}

/// Snapshot the global .vtcodegitignore instance.
pub async fn snapshot_global_vtcode_gitignore() -> Arc<VTCodeGitignore> {
    VTCODE_GITIGNORE.load_full()
}

/// Check if a file should be excluded by the global .vtcodegitignore
pub async fn should_exclude_file(file_path: &Path) -> bool {
    let gitignore = snapshot_global_vtcode_gitignore().await;
    gitignore.should_exclude(file_path)
}

/// Check if a path should be excluded, reusing a known directory flag.
///
/// The caller must supply a target-resolved flag; prefer
/// [`should_exclude_file_with_file_type`] in traversal loops.
pub async fn should_exclude_file_with_is_dir(file_path: &Path, is_dir: bool) -> bool {
    let gitignore = snapshot_global_vtcode_gitignore().await;
    gitignore.should_exclude_with_is_dir(file_path, is_dir)
}

/// Check if a path should be excluded, reusing a `FileType` the walker returned.
///
/// This is the traversal-loop form of [`should_exclude_file`]: it skips the
/// `stat` that learning `is_dir` would otherwise cost, except for symlinks whose
/// target decides the answer.
pub async fn should_exclude_file_with_file_type(file_path: &Path, file_type: Option<FileType>) -> bool {
    let gitignore = snapshot_global_vtcode_gitignore().await;
    gitignore.should_exclude_with_file_type(file_path, file_type)
}

/// Filter paths using the global .vtcodegitignore
pub async fn filter_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let gitignore = snapshot_global_vtcode_gitignore().await;
    gitignore.filter_paths(paths)
}

/// Reload the global .vtcodegitignore from disk
pub async fn reload_vtcode_gitignore() -> Result<()> {
    initialize_vtcode_gitignore().await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a matcher over a temp root with `patterns` written to disk.
    async fn matcher_for(root: &Path, patterns: &str) -> VTCodeGitignore {
        std::fs::write(root.join(".vtcodegitignore"), patterns).expect("write ignore file");
        VTCodeGitignore::from_directory(root).await.expect("build matcher")
    }

    #[tokio::test]
    async fn absolute_path_outside_the_ignore_root_is_not_excluded() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        let gitignore = matcher_for(&root, "target/\n").await;

        let outside = temp.path().join("elsewhere").join("file.rs");
        std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
        std::fs::write(&outside, "x").unwrap();

        // `strip_prefix` cannot relativize this path, and the underlying matcher
        // asserts that whatever it receives is rootless. A path outside the
        // ignore file's scope simply is not covered by its patterns.
        assert!(!gitignore.should_exclude(&outside));
        assert!(!gitignore.should_exclude_with_is_dir(&outside, false));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn entries_reached_through_a_root_alias_still_match_patterns() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let real_root = temp.path().join("workspace");
        std::fs::create_dir_all(real_root.join("skipped")).unwrap();
        let gitignore = matcher_for(&real_root, "skipped/\n").await;

        // An alias of the same directory: the matcher was built from one
        // spelling, but walkers report the other, so matching must survive it.
        let alias_root = temp.path().join("workspace-alias");
        symlink(&real_root, &alias_root).unwrap();
        let alias_child = alias_root.join("skipped");

        assert!(
            gitignore.should_exclude_with_is_dir(&alias_child, true),
            "a directory under an aliased root must still match the pattern"
        );
    }

    #[tokio::test]
    async fn known_file_type_matches_the_stat_based_answer() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir(root.join("skipped")).unwrap();
        std::fs::write(root.join("kept.rs"), "x").unwrap();
        let gitignore = matcher_for(root, "skipped/\n").await;

        let dir_entry = std::fs::read_dir(root)
            .unwrap()
            .flatten()
            .find(|e| e.file_name() == "skipped")
            .unwrap();
        let dir_type = dir_entry.file_type().unwrap();
        let dir_path = dir_entry.path();
        assert!(dir_type.is_dir());

        // Directory ignore patterns must fire from the supplied type alone, and
        // agree with the stat-resolving form.
        assert_eq!(
            gitignore.should_exclude_with_file_type(&dir_path, Some(dir_type)),
            gitignore.should_exclude(&dir_path)
        );
        assert!(gitignore.should_exclude_with_file_type(&dir_path, Some(dir_type)));

        let file_type = std::fs::symlink_metadata(root.join("kept.rs")).unwrap().file_type();
        assert!(!gitignore.should_exclude_with_file_type(&root.join("kept.rs"), Some(file_type)));
    }

    #[test]
    fn unknown_file_type_falls_back_to_the_filesystem() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let gitignore = VTCodeGitignore::default();
        // `loaded == false` short-circuits, so nothing is excluded either way.
        assert!(!gitignore.should_exclude_with_file_type(&root.join("anything"), None));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_entries_defer_to_the_target_type() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir(root.join("real-dir")).unwrap();
        symlink(root.join("real-dir"), root.join("link-dir")).unwrap();
        let gitignore = matcher_for(root, "link-dir/\n").await;

        let link = root.join("link-dir");
        let link_type = std::fs::symlink_metadata(&link).unwrap().file_type();
        assert!(link_type.is_symlink());

        // The link's own type is not a directory, but its target is; the
        // resolution must match what `Path::is_dir` used to decide.
        assert_eq!(gitignore.should_exclude_with_file_type(&link, Some(link_type)), gitignore.should_exclude(&link));
    }
}
