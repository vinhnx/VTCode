//! Memory file reading, parsing, and query matching.
//!
//! This module is the read-only access layer for persistent memory storage.
//! It owns:
//! - Pure parsing of fact lines and topic files (`parse_fact_line`, `parse_topic_file`).
//! - Topic source encoding/decoding (`encode_topic_source`, `decode_topic_source`).
//! - Fact classification by topic (`classify_fact`).
//! - Async file-system readers for topic, rollout, and note files.
//! - File-discovery walkers (recursive `.md` listing, pending-rollout scans).
//! - Query normalization and match collection with deduplication.
//!
//! **Guard rails**: every item is `pub(super)` — visible only to
//! `persistent_memory::mod` and its test submodule. No filesystem paths or
//! parsing internals are exposed beyond this module boundary.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::{
    GroundedFactRecord, MemoryTopic, NOTES_DIRNAME, PersistentMemoryFiles, PersistentMemoryMatch,
    extract_memory_highlights, normalize_whitespace,
};

/// A note file's relative path and its extracted highlight lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MemoryNoteSummary {
    pub relative_path: String,
    pub highlights: Vec<String>,
}

// ---------------------------------------------------------------------------
// Query normalization and matching
// ---------------------------------------------------------------------------

pub(super) fn normalize_memory_query(query: &str) -> Option<String> {
    let normalized = normalize_whitespace(query).to_ascii_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}

pub(super) async fn collect_memory_matches(
    files: &PersistentMemoryFiles,
    normalized_query: &str,
) -> Result<Vec<PersistentMemoryMatch>> {
    Ok(collect_all_memory_matches(files)
        .await?
        .into_iter()
        .filter(|r| {
            let nf = normalize_whitespace(&r.fact).to_ascii_lowercase();
            let ns = normalize_whitespace(&r.source).to_ascii_lowercase();
            nf.contains(normalized_query) || ns.contains(normalized_query)
        })
        .collect())
}

pub(super) async fn collect_all_memory_matches(files: &PersistentMemoryFiles) -> Result<Vec<PersistentMemoryMatch>> {
    let (prefs, repo, rollout, notes) = tokio::try_join!(
        read_topic_records(&files.preferences_file, MemoryTopic::Preferences),
        read_topic_records(&files.repository_facts_file, MemoryTopic::RepositoryFacts),
        read_rollout_records(&files.rollout_summaries_dir),
        read_note_summaries(&files.notes_dir),
    )?;

    let mut matches = Vec::new();
    for r in prefs.into_iter().chain(repo).chain(rollout.0).chain(rollout.1) {
        let (_, src) = decode_topic_source(&r.source);
        matches.push(PersistentMemoryMatch { source: src, fact: r.fact });
    }
    for n in notes {
        for h in n.highlights {
            matches.push(PersistentMemoryMatch { source: n.relative_path.clone(), fact: h });
        }
    }

    // Deduplicate by normalized fact, keeping the *last* occurrence of each
    // fact (move-to-end semantics). The prior implementation re-normalized
    // every prior entry's fact on each iteration — O(n²) String allocations.
    // Iterating in reverse and keeping the first-seen (= last-in-original)
    // entry is O(n) and yields the identical ordering:
    //   [A, B, A, C] -> original removes old A, pushes new A -> [B, A, C]
    //   reverse keeps C, A, B (skips 2nd A) -> reverse -> [B, A, C]
    let len = matches.len();
    let mut seen = HashSet::with_capacity(len);
    let mut deduped: Vec<PersistentMemoryMatch> = Vec::with_capacity(len);
    for r in matches.into_iter().rev() {
        let nf = normalize_whitespace(&r.fact).to_ascii_lowercase();
        if seen.insert(nf) {
            deduped.push(r);
        }
    }
    deduped.reverse();
    Ok(deduped)
}

pub(super) async fn collect_cleanup_candidates(files: &PersistentMemoryFiles) -> Result<Vec<GroundedFactRecord>> {
    let (prefs, repo, rollout) = tokio::try_join!(
        read_topic_records(&files.preferences_file, MemoryTopic::Preferences),
        read_topic_records(&files.repository_facts_file, MemoryTopic::RepositoryFacts),
        read_rollout_records(&files.rollout_summaries_dir),
    )?;
    Ok(prefs.into_iter().chain(repo).chain(rollout.0).chain(rollout.1).collect())
}

// ---------------------------------------------------------------------------
// File discovery (recursive `.md` listing)
// ---------------------------------------------------------------------------

/// List `.md` files under `dir`, optionally filtering by a predicate on the file name.
fn list_md_files(dir: &Path, filter: impl Fn(&str) -> bool) -> Result<Vec<PathBuf>> {
    fn walk(dir: &Path, files: &mut Vec<PathBuf>, filter: &impl Fn(&str) -> bool) -> Result<()> {
        if !dir.exists() {
            return Ok(());
        }
        for entry in std::fs::read_dir(dir).with_context(|| format!("Failed to list {}", dir.display()))? {
            let entry = entry?;
            let path = entry.path();
            // `readdir` already reported the type; `Path::is_dir` would pay a
            // `stat` per entry for the same answer.
            if vtcode_commons::walk::entry_is_dir(&entry) {
                walk(&path, files, filter)?;
            } else if path.extension().and_then(|v| v.to_str()) == Some("md")
                && filter(path.file_name().and_then(|v| v.to_str()).unwrap_or(""))
            {
                files.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, &mut files, &filter)?;
    files.sort();
    Ok(files)
}

fn list_pending_rollout_files(rollout_dir: &Path) -> Result<Vec<PathBuf>> {
    list_md_files(rollout_dir, |n| n.ends_with(".pending.md"))
}

pub(super) async fn list_pending_rollout_files_async(rollout_dir: &Path) -> Result<Vec<PathBuf>> {
    let rollout_dir = rollout_dir.to_path_buf();
    tokio::task::spawn_blocking(move || list_pending_rollout_files(&rollout_dir))
        .await
        .context("Pending rollout scan task panicked")?
}

pub(super) fn list_rollout_markdown_files(rollout_dir: &Path) -> Result<Vec<PathBuf>> {
    list_md_files(rollout_dir, |_| true)
}

pub(super) async fn list_rollout_markdown_files_async(rollout_dir: &Path) -> Result<Vec<PathBuf>> {
    let rollout_dir = rollout_dir.to_path_buf();
    tokio::task::spawn_blocking(move || list_rollout_markdown_files(&rollout_dir))
        .await
        .context("Rollout markdown scan task panicked")?
}

fn list_note_markdown_files(notes_dir: &Path) -> Result<Vec<PathBuf>> {
    list_md_files(notes_dir, |_| true)
}

async fn list_note_markdown_files_async(notes_dir: &Path) -> Result<Vec<PathBuf>> {
    let notes_dir = notes_dir.to_path_buf();
    tokio::task::spawn_blocking(move || list_note_markdown_files(&notes_dir))
        .await
        .context("Note markdown scan task panicked")?
}

pub(super) fn count_pending_rollout_summaries(rollout_dir: &Path) -> Result<usize> {
    Ok(list_md_files(rollout_dir, |n| n.ends_with(".pending.md"))?.len())
}

pub(super) async fn count_pending_rollout_summaries_async(rollout_dir: &Path) -> Result<usize> {
    Ok(list_pending_rollout_files_async(rollout_dir).await?.len())
}

// ---------------------------------------------------------------------------
// Async file readers
// ---------------------------------------------------------------------------

pub(super) async fn read_note_summaries(notes_dir: &Path) -> Result<Vec<MemoryNoteSummary>> {
    let mut notes = Vec::new();
    for path in list_note_markdown_files_async(notes_dir).await? {
        let content = String::from_utf8(vtcode_commons::fs::read_private_file_no_follow(&path).await?)
            .with_context(|| format!("Failed to decode {} as UTF-8", path.display()))?;
        let relative = path
            .strip_prefix(notes_dir)
            .with_context(|| format!("Failed to relativize {}", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        notes.push(MemoryNoteSummary {
            relative_path: format!("{NOTES_DIRNAME}/{relative}"),
            highlights: extract_memory_highlights(&content, 3),
        });
    }
    Ok(notes)
}

pub(super) async fn read_topic_records(path: &Path, topic: MemoryTopic) -> Result<Vec<GroundedFactRecord>> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Ok(Vec::new());
    }
    let bytes = vtcode_commons::fs::read_private_file_no_follow(path)
        .await
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let contents = String::from_utf8(bytes).with_context(|| format!("Failed to decode {} as UTF-8", path.display()))?;
    Ok(parse_topic_file(&contents)
        .into_iter()
        .map(|r| GroundedFactRecord {
            fact: r.fact,
            source: encode_topic_source(topic, &r.source),
        })
        .collect())
}

pub(super) async fn read_rollout_records(
    rollout_dir: &Path,
) -> Result<(Vec<GroundedFactRecord>, Vec<GroundedFactRecord>)> {
    if !tokio::fs::try_exists(rollout_dir).await.unwrap_or(false) {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut prefs = Vec::new();
    let mut repo_facts = Vec::new();
    let mut entries = tokio::fs::read_dir(rollout_dir)
        .await
        .with_context(|| format!("Failed to list {}", rollout_dir.display()))?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("md") {
            continue;
        }
        let contents = String::from_utf8(vtcode_commons::fs::read_private_file_no_follow(&path).await?)
            .with_context(|| format!("Failed to decode {} as UTF-8", path.display()))?;
        for record in parse_topic_file(&contents) {
            let (topic, _) = decode_topic_source(&record.source);
            match topic.unwrap_or_else(|| classify_fact(&record)) {
                MemoryTopic::Preferences => prefs.push(record),
                MemoryTopic::RepositoryFacts => repo_facts.push(record),
            }
        }
    }
    Ok((prefs, repo_facts))
}

// ---------------------------------------------------------------------------
// Pure parsing and source encoding
// ---------------------------------------------------------------------------

pub(super) fn parse_topic_file(contents: &str) -> Vec<GroundedFactRecord> {
    contents
        .lines()
        .filter_map(parse_fact_line)
        .map(|(source, fact)| GroundedFactRecord { source, fact })
        .collect()
}

pub(super) fn parse_fact_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let remainder = trimmed.strip_prefix("- [")?;
    let (source, fact) = remainder.split_once("] ")?;
    let fact = fact.trim();
    if fact.is_empty() {
        return None;
    }
    Some((source.trim().to_string(), fact.to_string()))
}

pub(super) fn classify_fact(fact: &GroundedFactRecord) -> MemoryTopic {
    if fact.source == "user_assertion" {
        MemoryTopic::Preferences
    } else {
        MemoryTopic::RepositoryFacts
    }
}

pub(super) fn encode_topic_source(topic: MemoryTopic, source: &str) -> String {
    format!("{}:{}", topic.slug(), source)
}

pub(super) fn decode_topic_source(source: &str) -> (Option<MemoryTopic>, String) {
    match source.split_once(':') {
        Some((topic, rest)) => (MemoryTopic::from_slug(topic), rest.trim().to_string()),
        None => (None, source.to_string()),
    }
}
