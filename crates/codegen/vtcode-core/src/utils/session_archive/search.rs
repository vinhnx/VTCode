use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};

use super::list_recent_sessions;
use crate::llm::provider::{MessageContent, MessageRole};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub session_id: String,
    pub session_path: PathBuf,
    pub timestamp: DateTime<Utc>,
    pub message_index: usize,
    pub role: MessageRole,
    pub content_snippet: String,
    pub score: f32, // Simple matching score (e.g., term frequency or just 1.0)
}

/// Search for a query string across recent sessions.
/// Returns a list of `SearchResult` sorted by relevance (or recency).
pub async fn search_sessions(query: &str, session_limit: usize, max_results: usize) -> Result<Vec<SearchResult>> {
    if query.trim().is_empty() || max_results == 0 {
        return Ok(Vec::new());
    }

    let listings = list_recent_sessions(session_limit).await?;
    let matcher = RegexBuilder::new(&regex::escape(query))
        .case_insensitive(true)
        .build()
        .context("failed to compile session search query")?;
    let mut results = Vec::new();

    for listing in listings {
        for (idx, msg) in listing.snapshot.messages.iter().enumerate() {
            let content = match &msg.content {
                MessageContent::Text(t) => t.as_str(),
                MessageContent::Parts(_) => continue,
            };

            if let Some(matched) = matcher.find(content) {
                let snippet = search_result_snippet(content, matched.start(), matched.end());

                results.push(SearchResult {
                    session_id: listing.identifier(),
                    session_path: listing.path.clone(),
                    timestamp: listing.snapshot.started_at,
                    message_index: idx,
                    role: msg.role,
                    content_snippet: snippet,
                    score: 1.0,
                });

                if results.len() >= max_results {
                    break;
                }
            }
        }
        if results.len() >= max_results {
            break;
        }
    }

    Ok(results)
}

fn search_result_snippet(content: &str, match_start: usize, match_end: usize) -> String {
    const CONTEXT_BYTES: usize = 50;

    let start = floor_char_boundary(content, match_start.saturating_sub(CONTEXT_BYTES));
    let end = ceil_char_boundary(content, (match_end + CONTEXT_BYTES).min(content.len()));

    let mut snippet = String::new();
    if start > 0 {
        snippet.push_str("...");
    }
    snippet.push_str(&content[start..end].replace('\n', " "));
    if end < content.len() {
        snippet.push_str("...");
    }
    snippet
}

fn floor_char_boundary(content: &str, mut index: usize) -> usize {
    while index > 0 && !content.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(content: &str, mut index: usize) -> usize {
    while index < content.len() && !content.is_char_boundary(index) {
        index += 1;
    }
    index
}
