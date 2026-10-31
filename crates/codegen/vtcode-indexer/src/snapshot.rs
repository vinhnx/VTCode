//! Snapshot loader for the persisted workspace file index.
//!
//! [`crate::MarkdownIndexStorage`] writes the whole index as one
//! human-readable markdown snapshot (`index.md`) via atomic rename. This
//! module parses that exact format so [`crate::SimpleIndexer`] can hydrate
//! its metadata cache from the previous run and skip re-reading unchanged
//! files. Parsing is strict and fail-closed: any deviation from the writer's
//! format discards the whole snapshot, and a full re-index rebuilds it.

use std::fs;
use std::path::Path;

use crate::FileIndex;
use crate::atomic_write;

/// Name of the snapshot record inside the index directory.
const SNAPSHOT_FILE: &str = "index.md";
/// Header line written by the snapshot persistence path.
const SNAPSHOT_HEADER: &str = "# Workspace File Index";
/// Length of a SHA-256 hex digest, the only hash format written.
const HASH_HEX_LEN: usize = 64;

/// Load the persisted snapshot entries.
///
/// Returns an empty vec when the snapshot is missing, unreadable, or
/// malformed; hydration never fails the caller because a full re-index is
/// always the safe fallback.
pub(crate) fn load_snapshot_entries(index_dir: &Path) -> Vec<FileIndex> {
    let Ok(dir) = atomic_write::canonical_existing_storage_dir(index_dir) else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(dir.join(SNAPSHOT_FILE)) else {
        return Vec::new();
    };
    match parse_snapshot(&text) {
        Some(entries) => entries,
        None => {
            tracing::debug!(
                index_dir = %index_dir.display(),
                "index snapshot malformed; falling back to a full re-index"
            );
            Vec::new()
        }
    }
}

/// Strictly parse the snapshot format written by
/// `MarkdownIndexStorage::persist_batch_refs`: a `# Workspace File Index`
/// header, an `- **Entries**: N` count line, then repeated `## {path}`
/// blocks with fixed-order field bullets. Returns `None` on any deviation so
/// a partial load can never happen.
pub(crate) fn parse_snapshot(text: &str) -> Option<Vec<FileIndex>> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());

    if lines.next()? != SNAPSHOT_HEADER {
        return None;
    }
    let expected: usize = bullet_value(lines.next()?, "Entries")?.parse().ok()?;

    let mut entries = Vec::new();
    while let Some(path_line) = lines.next() {
        let path = path_line.strip_prefix("## ")?;
        if path.is_empty() {
            return None;
        }
        if bullet_value(lines.next()?, "Path")? != path {
            return None;
        }
        let hash = bullet_value(lines.next()?, "Hash")?;
        if hash.len() != HASH_HEX_LEN || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let modified: u64 = bullet_value(lines.next()?, "Modified")?.parse().ok()?;
        let size: u64 = bullet_value(lines.next()?, "Size")?.strip_suffix(" bytes")?.parse().ok()?;
        let language = bullet_value(lines.next()?, "Language")?.to_owned();
        let tags = match bullet_value(lines.next()?, "Tags")? {
            "" => Vec::new(),
            joined => joined.split(", ").map(str::to_owned).collect::<Vec<_>>(),
        };

        entries.push(FileIndex::from_parts(path.to_owned(), hash.to_owned(), modified, size, language, tags));
    }

    (entries.len() == expected).then_some(entries)
}

/// Extract the value of a `- **{name}**: value` bullet.
fn bullet_value<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let rest = line.strip_prefix("- **")?;
    let (field_name, value) = rest.split_once("**: ")?;
    (field_name == name).then_some(value)
}
