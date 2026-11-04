//! One traversal shape shared by the list modes.
//!
//! `list`, `recursive`, and `tree` each used to hand-roll the same walker loop
//! and carry its output as a positional tuple. Naming the shape keeps the
//! fields explicit and lets a mode hand the entry's own `FileType` to path
//! rendering and ignore matching instead of re-deriving it.

use anyhow::{Context, Result};
use ignore::Walk;
use std::fs::FileType;
use std::path::{Path, PathBuf};
use tokio::task::spawn_blocking;
use vtcode_commons::walk::build_default_walker;

/// Depth bound for discovery walks.
///
/// A whole-repo walk at unbounded depth serializes the entire workspace and is
/// then truncated mid-structure; ten levels covers real layouts while keeping
/// the result usable.
pub(super) const WALK_MAX_DEPTH: usize = 10;

/// A walked entry reduced to the facts every list mode needs.
pub(super) struct WalkEntry {
    pub(super) path: PathBuf,
    /// The entry's own type as reported by the walker (`None` when unknown).
    ///
    /// Because the shared walker does not follow links, this is the type of the
    /// link itself for symlinks, which is what path rendering must see.
    pub(super) file_type: Option<FileType>,
    /// Resolved once at collection so callers may move `path` out without
    /// re-reading the type (a partially moved struct cannot re-derive it).
    pub(super) is_dir: bool,
    pub(super) depth: usize,
}

impl WalkEntry {
    fn from_dir_entry(entry: &ignore::DirEntry) -> Self {
        let file_type = entry.file_type();
        Self {
            path: entry.path().to_path_buf(),
            file_type,
            is_dir: file_type.is_some_and(|file_type| file_type.is_dir()),
            depth: entry.depth(),
        }
    }
}

/// The single discovery walk every list mode shares.
fn discovery_walk(root: &Path) -> Walk {
    build_default_walker(root).max_depth(Some(WALK_MAX_DEPTH)).build()
}

/// Walk `root` on the blocking pool.
///
/// Directory traversal is blocking I/O; keeping it off the async runtime is the
/// reason all three list modes previously cloned this function inline.
pub(super) async fn walk_entries(root: PathBuf) -> Result<Vec<WalkEntry>> {
    spawn_blocking(move || {
        discovery_walk(&root)
            .filter_map(std::result::Result::ok)
            .map(|e| WalkEntry::from_dir_entry(&e))
            .collect()
    })
    .await
    .context("Failed to spawn walker task")
}

/// Walk `root` and stop at the first entry `wants` accepts.
///
/// `find_name` answers with a single path, so continuing the walk after a hit
/// only burns I/O: a root-level match in a large repo would otherwise walk the
/// whole tree. First-match order is preserved because the walker is consumed
/// sequentially and `wants` carries both the name and exclusion predicates —
/// exclusion is an independent per-entry check, so folding it in cannot change
/// which entry survives.
pub(super) async fn walk_first_matching(
    root: PathBuf,
    wants: impl Fn(&ignore::DirEntry) -> bool + Send + 'static,
) -> Result<Option<WalkEntry>> {
    spawn_blocking(move || {
        discovery_walk(&root)
            .filter_map(std::result::Result::ok)
            .find(|entry| wants(entry))
            .map(|entry| WalkEntry::from_dir_entry(&entry))
    })
    .await
    .context("Failed to spawn walker task")
}
