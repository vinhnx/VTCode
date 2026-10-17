#![expect(
    unused_results,
    reason = "WalkBuilder configuration uses fluent setters only for their mutation side effects."
)]

//! Shared directory walker helpers built on the `ignore` crate.
//!
//! All file traversal in vtcode should go through these builders so that
//! `.gitignore`, `.ignore`, `.git/exclude`, and the centralized exclusion
//! constants are applied consistently.

use ignore::{DirEntry, WalkBuilder};
use std::path::Path;

use crate::exclusions::{DEFAULT_EXCLUDED_DIRS, VTCODE_IGNORE_FILE};

/// Build a multi-threaded [`WalkBuilder`] with sensible defaults.
///
/// - Respects `.gitignore`, `.ignore`, `.git/exclude`, and parent ignore files
/// - Does not follow symlinks
/// - Uses the `ignore` crate's default thread pool
///
/// Callers that need to prune additional directories should use
/// [`filter_entry`](WalkBuilder::filter_entry) with [`is_excluded_dir`].
pub fn build_default_walker(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    apply_defaults(&mut builder);
    builder
}

/// Build a single-threaded [`WalkBuilder`] with the same defaults as
/// [`build_default_walker`].
///
/// Use this in synchronous contexts where spawning the `ignore` crate's
/// thread pool would be wasteful (e.g., inside `spawn_blocking` closures
/// that already run on a dedicated thread).
pub fn build_walker_single_threaded(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder.threads(1);
    apply_defaults(&mut builder);
    builder
}

/// Apply standard walker defaults to an existing [`WalkBuilder`].
///
/// Sets gitignore support, hidden file visibility, and symlink policy.
/// Callers that need additional customization (e.g., parallel walkers,
/// symlink following) can call this then override specific settings.
pub fn apply_defaults(builder: &mut WalkBuilder) {
    // Respect all standard ignore-file mechanisms.
    builder.git_ignore(true);
    builder.git_global(true);
    builder.git_exclude(true);
    builder.ignore(true);
    builder.parents(true);

    // `.vtcodegitignore` mirrors `.gitignore` but is scoped to VT Code's own
    // file operations. It has higher precedence than the standard ignore files
    // (including its `!` re-include rules), so a user can whitelist a path that
    // `.gitignore` prunes.
    builder.add_custom_ignore_filename(VTCODE_IGNORE_FILE);

    // Do not follow symlinks by default.
    builder.follow_links(false);

    // Do not skip hidden files by default.  The `ignore` crate skips them
    // by default, but the previous traversal code did not.  Callers that
    // want to hide dotfiles should filter them explicitly.
    builder.hidden(false);
}

/// Returns `true` if `entry` is a directory whose name appears in
/// [`DEFAULT_EXCLUDED_DIRS`].
///
/// Intended for use inside [`WalkBuilder::filter_entry`] closures:
///
/// ```ignore
/// builder.filter_entry(|entry| !vtcode_commons::walk::is_excluded_dir(entry));
/// ```
pub fn is_excluded_dir(entry: &DirEntry) -> bool {
    if !entry.file_type().is_some_and(|ft| ft.is_dir()) {
        return false;
    }

    entry
        .file_name()
        .to_str()
        .is_some_and(|name| DEFAULT_EXCLUDED_DIRS.contains(&name))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn collected_paths(root: &Path) -> Vec<String> {
        let walker = build_default_walker(root).build();
        let mut paths = walker
            .filter_map(Result::ok)
            .filter(|entry| entry.path() != root)
            .map(|entry| {
                entry
                    .path()
                    .strip_prefix(root)
                    .unwrap_or_else(|_| entry.path())
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    #[test]
    fn default_walker_respects_vtcodegitignore() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        fs::write(root.join(".vtcodegitignore"), "ignored_dir/\nignored_file.txt\n").expect("write ignore file");
        fs::create_dir(root.join("ignored_dir")).expect("mkdir ignored_dir");
        fs::write(root.join("ignored_dir/secret.rs"), "x").expect("write secret");
        fs::write(root.join("ignored_file.txt"), "x").expect("write ignored file");
        fs::write(root.join("kept.rs"), "x").expect("write kept file");

        let paths = collected_paths(root);

        assert!(paths.contains(&"kept.rs".to_owned()), "kept file should remain: {paths:?}");
        assert!(!paths.iter().any(|p| p.contains("ignored_file.txt")), "ignored file leaked: {paths:?}");
        assert!(!paths.iter().any(|p| p.contains("ignored_dir")), "ignored dir leaked: {paths:?}");
    }

    #[test]
    fn default_walker_vtcodegitignore_negation_reincludes() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        // Exclude every log, then re-include one. The custom-ignore file's
        // negation must win over its own earlier pattern (mirrors the repo's
        // `!README.md` style allow-list).
        fs::write(root.join(".vtcodegitignore"), "*.log\n!important.log\n").expect("write ignore file");
        fs::write(root.join("important.log"), "x").expect("write important log");
        fs::write(root.join("noise.log"), "x").expect("write noise log");
        fs::write(root.join("kept.rs"), "x").expect("write kept file");

        let paths = collected_paths(root);

        assert!(paths.contains(&"important.log".to_owned()), "negated file should be re-included: {paths:?}");
        assert!(!paths.iter().any(|p| p.ends_with("noise.log")), "non-negated file leaked: {paths:?}");
        assert!(paths.contains(&"kept.rs".to_owned()), "unrelated file should remain: {paths:?}");
    }
}
