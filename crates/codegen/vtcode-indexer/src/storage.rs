//! Snapshot persistence for the workspace file index.
//!
//! [`MarkdownIndexStorage`] writes the whole index as one human-readable
//! markdown snapshot (`index.md`) via atomic rename; [`snapshot`](crate::snapshot)
//! parses that exact format back. Legacy per-file `.md` entries are removed on
//! every snapshot write.

use anyhow::Result;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::Path;

use crate::FileIndex;
use crate::atomic_write;
use crate::calculate_hash;

/// Persistence backend for [`SimpleIndexer`](crate::SimpleIndexer).
pub trait IndexStorage: Send + Sync {
    /// Prepare any directories or resources required for persistence.
    fn init(&self, index_dir: &Path) -> Result<()>;

    /// Persist an indexed file entry.
    fn persist(&self, index_dir: &Path, entry: &FileIndex) -> Result<()>;

    /// Whether this backend expects full-snapshot persistence.
    ///
    /// Snapshot-aware backends receive the complete in-memory index on each
    /// update so on-disk state stays consistent across single-file and
    /// directory indexing flows.
    fn prefers_snapshot_persistence(&self) -> bool {
        false
    }

    /// Remove a previously persisted file entry.
    ///
    /// Defaults to a no-op to keep existing custom storage backends compatible.
    fn remove(&self, _index_dir: &Path, _file_path: &Path) -> Result<()> {
        Ok(())
    }

    /// Persist a batch of indexed file entries.
    ///
    /// Defaults to calling [`IndexStorage::persist`] for each entry, keeping
    /// existing custom storage backends compatible.
    fn persist_batch(&self, index_dir: &Path, entries: &[FileIndex]) -> Result<()> {
        for entry in entries {
            self.persist(index_dir, entry)?;
        }
        Ok(())
    }

    /// Persist a batch of indexed file entries borrowed from the in-memory cache.
    ///
    /// Defaults to cloning the borrowed entries and delegating to
    /// [`IndexStorage::persist_batch`] so existing custom storage backends remain
    /// compatible.
    fn persist_batch_refs(&self, index_dir: &Path, entries: &[&FileIndex]) -> Result<()> {
        let owned = entries.iter().map(|entry| (*entry).clone()).collect::<Vec<_>>();
        self.persist_batch(index_dir, &owned)
    }
}

/// Markdown-backed [`IndexStorage`] implementation.
#[derive(Debug, Default, Clone)]
pub struct MarkdownIndexStorage;

impl IndexStorage for MarkdownIndexStorage {
    fn init(&self, index_dir: &Path) -> Result<()> {
        fs::create_dir_all(index_dir)?;
        Ok(())
    }

    fn persist(&self, index_dir: &Path, entry: &FileIndex) -> Result<()> {
        let dir = atomic_write::canonical_storage_dir(index_dir)?;
        let file_name = format!("{}.md", calculate_hash(&entry.path));
        let mut buffer = Vec::new();
        writeln!(buffer, "# File Index: {}", entry.path)?;
        writeln!(buffer)?;
        write_markdown_fields(&mut buffer, entry)?;
        atomic_write::write_atomic(&dir, &file_name, &buffer)
    }

    fn prefers_snapshot_persistence(&self) -> bool {
        true
    }

    fn remove(&self, index_dir: &Path, file_path: &Path) -> Result<()> {
        if !index_dir.exists() {
            return Ok(());
        }
        let dir = atomic_write::canonical_storage_dir(index_dir)?;
        let file_name = format!("{}.md", calculate_hash(file_path.to_string_lossy().as_ref()));
        match fs::remove_file(dir.join(file_name)) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    fn persist_batch(&self, index_dir: &Path, entries: &[FileIndex]) -> Result<()> {
        persist_markdown_snapshot(index_dir, entries.iter())
    }

    fn persist_batch_refs(&self, index_dir: &Path, entries: &[&FileIndex]) -> Result<()> {
        persist_markdown_snapshot(index_dir, entries.iter().copied())
    }
}

fn persist_markdown_snapshot<'a>(index_dir: &Path, entries: impl IntoIterator<Item = &'a FileIndex>) -> Result<()> {
    let entries = entries.into_iter().collect::<Vec<_>>();

    let dir = atomic_write::canonical_storage_dir(index_dir)?;
    let mut buffer = Vec::new();

    writeln!(buffer, "# Workspace File Index")?;
    writeln!(buffer)?;
    writeln!(buffer, "- **Entries**: {}", entries.len())?;
    writeln!(buffer)?;

    for entry in entries {
        write_markdown_entry(&mut buffer, entry)?;
    }

    atomic_write::write_atomic(&dir, "index.md", &buffer)?;
    cleanup_legacy_markdown_entries(&dir)?;
    Ok(())
}

fn write_markdown_entry(writer: &mut impl Write, entry: &FileIndex) -> std::io::Result<()> {
    writeln!(writer, "## {}", entry.path)?;
    writeln!(writer)?;
    write_markdown_fields(writer, entry)?;
    writeln!(writer)?;
    Ok(())
}

fn write_markdown_fields(writer: &mut impl Write, entry: &FileIndex) -> std::io::Result<()> {
    writeln!(writer, "- **Path**: {}", entry.path)?;
    writeln!(writer, "- **Hash**: {}", entry.hash)?;
    writeln!(writer, "- **Modified**: {}", entry.modified)?;
    writeln!(writer, "- **Size**: {} bytes", entry.size)?;
    writeln!(writer, "- **Language**: {}", entry.language)?;
    writeln!(writer, "- **Tags**: {}", entry.tags.join(", "))?;
    Ok(())
}

fn cleanup_legacy_markdown_entries(index_dir: &Path) -> Result<()> {
    for entry in fs::read_dir(index_dir)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if is_legacy_markdown_entry_name(file_name.as_ref()) {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[inline]
fn is_legacy_markdown_entry_name(file_name: &str) -> bool {
    let Some(hash_part) = file_name.strip_suffix(".md") else {
        return false;
    };
    hash_part.len() == 64 && hash_part.bytes().all(|byte| byte.is_ascii_hexdigit())
}
