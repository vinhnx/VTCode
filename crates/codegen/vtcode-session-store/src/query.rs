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
