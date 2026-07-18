//! Read-only queries across sessions for analytics and long-term learning.

use std::path::Path;

use crate::error::SessionStoreError;
use crate::sessions_root;

/// Lightweight summary of a single session, read from its `manifest.json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct SessionSummary {
    /// Session identifier (directory name).
    pub session_id: String,
    /// Number of completed turns.
    pub turn_count: u64,
    /// Total events recorded.
    pub event_count: u64,
    /// Lifecycle status.
    pub status: String,
    /// RFC3339 last-update timestamp (used for ordering).
    pub updated_at: String,
}

/// A single grounded fact drawn from a session's memory envelope.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct FactRecord {
    /// The fact text.
    pub fact: String,
    /// Session the fact originated from.
    pub session_id: String,
}

/// One result returned from a memory search.
///
/// Mirrors the shape used by the grok-build memory subsystem so that
/// higher-level consumers (tool bridge, context injection) can share
/// formatting logic once a richer backend is available.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct MemorySearchResult {
    /// Stable identifier for this chunk (session_id + fact index).
    pub chunk_id: String,
    /// Source memory file path.
    pub path: String,
    /// 0-based start line in the source file (0 for derived facts).
    pub start_line: usize,
    /// 0-based end line in the source file (0 for derived facts).
    pub end_line: usize,
    /// Relevance score (higher = more relevant).
    pub score: f64,
    /// Text snippet from the chunk.
    pub snippet: String,
    /// Source scope: `"session"` for per-session memory files.
    pub source: String,
    /// Unix timestamp (seconds) when the source memory was created.
    pub created_at: Option<i64>,
}

/// List up to `n` most-recently-updated sessions.
#[must_use]
pub fn recent_sessions(workspace: &Path, n: usize) -> Vec<SessionSummary> {
    let root = sessions_root(workspace);
    if !root.exists() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    for entry in entries.filter_map(Result::ok) {
        let manifest = entry.path().join("manifest.json");
        if let Ok(bytes) = std::fs::read(&manifest)
            && let Ok(s) = serde_json::from_slice::<SessionSummary>(&bytes)
        {
            out.push(s);
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out.truncate(n);
    out
}

/// Cross-session long-term-learning query: collect grounded facts from every
/// session's derived memory envelope. This is how the agent learns across
/// sessions without loading any history into context.
pub fn query_facts(workspace: &Path, limit: usize) -> Result<Vec<FactRecord>, SessionStoreError> {
    let root = sessions_root(workspace);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut facts: Vec<FactRecord> = Vec::new();
    let entries = std::fs::read_dir(&root).map_err(|e| SessionStoreError::io(root.clone(), e))?;
    for entry in entries.filter_map(Result::ok) {
        let memory = entry.path().join(crate::DERIVED_DIR).join("memory.json");
        let Ok(bytes) = std::fs::read(&memory) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let session_id = entry.file_name().to_string_lossy().to_string();
        if let Some(arr) = value.get("grounded_facts").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(fact) = item.get("fact").and_then(|f| f.as_str()) {
                    facts.push(FactRecord {
                        fact: fact.to_string(),
                        session_id: session_id.clone(),
                    });
                }
            }
        }
    }
    facts.truncate(limit);
    Ok(facts)
}

/// Cross-session memory search: scan every session's derived memory envelope
/// for facts matching `query`. Returns up to `max_results` results with
/// score >= `min_score`, sorted by descending relevance.
///
/// Scoring is based on the number of case-insensitive query matches found
/// in each fact. A simple BMH-style substring count keeps this zero-dependency
/// while still surfacing the most-relevant chunks first.
pub fn search_memory(
    workspace: &Path,
    query: &str,
    max_results: usize,
    min_score: f64,
) -> Result<Vec<MemorySearchResult>, SessionStoreError> {
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let root = sessions_root(workspace);
    if !root.exists() {
        return Ok(Vec::new());
    }

    let lowered = query.to_ascii_lowercase();
    let mut results: Vec<MemorySearchResult> = Vec::new();

    let entries = std::fs::read_dir(&root).map_err(|e| SessionStoreError::io(root.clone(), e))?;
    for entry in entries.filter_map(Result::ok) {
        let session_dir = entry.path();
        let memory = session_dir.join(crate::DERIVED_DIR).join("memory.json");
        let Ok(bytes) = std::fs::read(&memory) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let session_id = entry.file_name().to_string_lossy().to_string();
        let created_at = value
            .get("created_at")
            .and_then(|v| v.as_i64())
            .or_else(|| value.get("updated_at").and_then(|v| v.as_i64()));

        if let Some(arr) = value.get("grounded_facts").and_then(|v| v.as_array()) {
            for (idx, item) in arr.iter().enumerate() {
                let Some(fact) = item.get("fact").and_then(|f| f.as_str()) else {
                    continue;
                };
                let score = count_substring_matches(fact, &lowered) as f64;
                if score <= 0.0 || score < min_score {
                    continue;
                }
                results.push(MemorySearchResult {
                    chunk_id: format!("{session_id}:{idx}"),
                    path: memory.to_string_lossy().to_string(),
                    start_line: 0,
                    end_line: 0,
                    score,
                    snippet: fact.to_string(),
                    source: "session".to_string(),
                    created_at,
                });
            }
        }
    }

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(max_results);
    Ok(results)
}

/// Return the configured default for `max_results` in search queries.
pub fn default_search_max_results() -> usize {
    6
}

/// Return the configured default for `min_score` in search queries.
pub fn default_search_min_score() -> f64 {
    0.0
}

fn count_substring_matches(text: &str, lowered_query: &str) -> usize {
    if lowered_query.is_empty() {
        return 0;
    }
    let lowered = text.to_ascii_lowercase();
    let mut count = 0;
    let mut start = 0;
    while let Some(pos) = lowered[start..].find(lowered_query) {
        count += 1;
        start += pos + lowered_query.len();
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn count_substring_matches_counts_overlapping() {
        assert_eq!(count_substring_matches("aaaa", "aa"), 2);
        assert_eq!(count_substring_matches("ababa", "aba"), 1);
        assert_eq!(count_substring_matches("hello world", "ll"), 1);
        assert_eq!(count_substring_matches("", "x"), 0);
    }

    #[test]
    fn search_memory_returns_matching_facts() {
        let dir = TempDir::new().expect("tempdir");
        let sess = crate::session_dir(dir.path(), "s1");
        std::fs::create_dir_all(sess.join(crate::DERIVED_DIR)).expect("mkdir");
        let memory = serde_json::json!({
            "grounded_facts": [
                {"fact": "the widget is blue"},
                {"fact": "the server runs on port 8080"},
                {"fact": "use PostgreSQL for persistence"},
            ]
        });
        std::fs::write(sess.join(crate::DERIVED_DIR).join("memory.json"), serde_json::to_string(&memory).expect("ser"))
            .expect("write");

        let results = search_memory(dir.path(), "blue", 10, 0.0).expect("search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].snippet, "the widget is blue");
        assert_eq!(results[0].chunk_id, "s1:0");
        assert_eq!(results[0].score, 1.0);
    }

    #[test]
    fn search_memory_scores_multiple_matches() {
        let dir = TempDir::new().expect("tempdir");
        let sess = crate::session_dir(dir.path(), "s2");
        std::fs::create_dir_all(sess.join(crate::DERIVED_DIR)).expect("mkdir");
        let memory = serde_json::json!({
            "grounded_facts": [
                {"fact": "rust uses rustc and cargo"},
                {"fact": "cargo is the rust build tool"},
            ]
        });
        std::fs::write(sess.join(crate::DERIVED_DIR).join("memory.json"), serde_json::to_string(&memory).expect("ser"))
            .expect("write");

        let results = search_memory(dir.path(), "cargo", 10, 0.0).expect("search");
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.score == 1.0));
    }

    #[test]
    fn search_memory_respects_min_score() {
        let dir = TempDir::new().expect("tempdir");
        let sess = crate::session_dir(dir.path(), "s3");
        std::fs::create_dir_all(sess.join(crate::DERIVED_DIR)).expect("mkdir");
        let memory = serde_json::json!({
            "grounded_facts": [
                {"fact": "alpha beta gamma"},
            ]
        });
        std::fs::write(sess.join(crate::DERIVED_DIR).join("memory.json"), serde_json::to_string(&memory).expect("ser"))
            .expect("write");

        let results = search_memory(dir.path(), "beta", 10, 2.0).expect("search");
        assert!(results.is_empty());
    }

    #[test]
    fn search_memory_empty_query_returns_empty() {
        let dir = TempDir::new().expect("tempdir");
        let results = search_memory(dir.path(), "", 10, 0.0).expect("search");
        assert!(results.is_empty());
    }

    #[test]
    fn search_memory_sorts_by_score_descending() {
        let dir = TempDir::new().expect("tempdir");
        for i in 0..3 {
            let sess = crate::session_dir(dir.path(), &format!("s{i}"));
            std::fs::create_dir_all(sess.join(crate::DERIVED_DIR)).expect("mkdir");
            let memory = serde_json::json!({
                "grounded_facts": [
                    {"fact": format!("fact {i} appears twice twice")},
                ]
            });
            std::fs::write(
                sess.join(crate::DERIVED_DIR).join("memory.json"),
                serde_json::to_string(&memory).expect("ser"),
            )
            .expect("write");
        }

        let results = search_memory(dir.path(), "twice", 10, 0.0).expect("search");
        assert_eq!(results.len(), 3);
        assert!(results.windows(2).all(|w| w[0].score >= w[1].score));
    }
}
