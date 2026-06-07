//! Shared directory walker helpers built on the `ignore` crate.
//!
//! All file traversal in vtcode should go through these builders so that
//! `.gitignore`, `.ignore`, `.git/exclude`, and the centralized exclusion
//! constants are applied consistently.

use ignore::{DirEntry, WalkBuilder};
use std::path::Path;

use crate::exclusions::DEFAULT_EXCLUDED_DIRS;

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

fn apply_defaults(builder: &mut WalkBuilder) {
    // Respect all standard ignore-file mechanisms.
    builder.git_ignore(true);
    builder.git_global(true);
    builder.git_exclude(true);
    builder.ignore(true);
    builder.parents(true);

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
