//! Proactive context gathering for vibe coding support
//!
//! Automatically gathers relevant file snippets and context before LLM calls
//! to minimize token usage while maximizing relevance.

use anyhow::{Context as AnyhowContext, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::entity_resolver::{EntityMatch, FileLocation};
use super::workspace_state::WorkspaceState;
use crate::tools::grep_file::{GrepSearchInput, GrepSearchManager};

/// Maximum number of files to gather
const MAX_CONTEXT_FILES: usize = 3;

/// Maximum snippets per file
const MAX_SNIPPETS_PER_FILE: usize = 20;

/// Maximum tokens to gather
const MAX_CONTEXT_TOKENS: usize = 2000;

/// Lines of context around entity location
const CONTEXT_LINES: usize = 10;

/// A file snippet with metadata
#[derive(Debug, Clone)]
pub struct FileSnippet {
    pub file: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
    pub content: String,
    pub relevance_score: f32,
}

/// Gathered context from proactive gathering
#[derive(Debug, Clone, Default)]
pub struct GatheredContext {
    /// Files included in context
    pub files: Vec<PathBuf>,

    /// Snippets from files
    pub snippets: HashMap<PathBuf, Vec<FileSnippet>>,

    /// Search results (if any)
    pub search_results: Option<serde_json::Value>,

    /// Total estimated tokens
    pub estimated_tokens: usize,
}

impl GatheredContext {
    /// Create new empty context
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a file to context
    pub fn add_file(&mut self, file: PathBuf) {
        if !self.files.contains(&file) {
            self.files.push(file);
        }
    }

    /// Add files to context
    pub fn add_files(&mut self, files: Vec<PathBuf>) {
        for file in files {
            self.add_file(file);
        }
    }

    /// Add a snippet
    pub fn add_snippet(&mut self, file: PathBuf, snippet: FileSnippet) {
        // Estimate tokens (rough: 4 chars per token)
        self.estimated_tokens += snippet.content.len() / 4;

        self.snippets
            .entry(file.clone())
            .or_insert_with(Vec::new)
            .push(snippet);
    }

    /// Add search results
    pub fn add_search_results(&mut self, results: serde_json::Value) {
        // Estimate tokens
        if let Ok(json_str) = serde_json::to_string(&results) {
            self.estimated_tokens += json_str.len() / 4;
        }

        self.search_results = Some(results);
    }

    /// Check if token budget is exceeded
    pub fn is_over_budget(&self) -> bool {
        self.estimated_tokens > MAX_CONTEXT_TOKENS
    }

    /// Format as prompt text
    pub fn to_prompt_text(&self) -> String {
        let mut text = String::from("## Proactively Gathered Context\n\n");

        if !self.files.is_empty() {
            text.push_str("### Relevant Files:\n");
            for file in &self.files {
                text.push_str(&format!("- {}\n", file.display()));
            }
            text.push('\n');
        }

        if !self.snippets.is_empty() {
            text.push_str("### File Snippets:\n\n");
            for (file, snippets) in &self.snippets {
                text.push_str(&format!("**{}**:\n", file.display()));
                for snippet in snippets {
                    text.push_str(&format!(
                        "```\n{} (lines {}-{})\n{}\n```\n\n",
                        file.display(),
                        snippet.line_start,
                        snippet.line_end,
                        snippet.content
                    ));
                }
            }
        }

        text
    }
}

/// Proactive context gatherer
pub struct ProactiveGatherer {
    /// Grep search manager for file searching
    grep_manager: Option<Arc<GrepSearchManager>>,

    /// Workspace state tracker
    workspace_state: Arc<RwLock<WorkspaceState>>,

    /// Workspace root
    workspace_root: PathBuf,
}

impl ProactiveGatherer {
    /// Create new proactive gatherer
    pub fn new(workspace_root: PathBuf, workspace_state: Arc<RwLock<WorkspaceState>>) -> Self {
        Self {
            grep_manager: None,
            workspace_state,
            workspace_root,
        }
    }

    /// Create with grep manager
    pub fn with_grep(
        workspace_root: PathBuf,
        workspace_state: Arc<RwLock<WorkspaceState>>,
        grep_manager: Arc<GrepSearchManager>,
    ) -> Self {
        Self {
            grep_manager: Some(grep_manager),
            workspace_state,
            workspace_root,
        }
    }

    /// Gather context for entity matches
    pub async fn gather_context(&self, entity_matches: &[EntityMatch]) -> Result<GatheredContext> {
        let mut context = GatheredContext::new();

        // Collect files from entity matches
        let mut candidate_files = Vec::new();
        for entity_match in entity_matches {
            for location in &entity_match.locations {
                candidate_files.push((location.path.clone(), entity_match.total_score()));
            }
        }

        // Rank files
        let ranked_files = self.rank_files_for_context(candidate_files).await;

        // Gather snippets from top files
        for (file, _score) in ranked_files.iter().take(MAX_CONTEXT_FILES) {
            if let Some(snippet) = self.read_file_snippet(file, entity_matches).await? {
                context.add_file(file.clone());
                context.add_snippet(file.clone(), snippet);
            }

            // Check token budget
            if context.is_over_budget() {
                break;
            }
        }

        Ok(context)
    }

    /// Rank files by relevance
    async fn rank_files_for_context(&self, files: Vec<(PathBuf, f32)>) -> Vec<(PathBuf, f32)> {
        let state = self.workspace_state.read().await;

        let mut scored_files: Vec<(PathBuf, f32)> = files
            .into_iter()
            .map(|(file, base_score)| {
                let mut score = base_score;

                // Recent access bonus
                if state.was_recently_accessed(&file) {
                    score += 0.3;
                }

                // Hot file bonus
                if state.hot_files().iter().any(|(f, _)| f == &file) {
                    score += 0.2;
                }

                (file, score)
            })
            .collect();

        // Sort by score descending
        scored_files.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        scored_files
    }

    /// Read file snippet around entity locations
    async fn read_file_snippet(
        &self,
        file: &Path,
        entity_matches: &[EntityMatch],
    ) -> Result<Option<FileSnippet>> {
        // Find locations in this file
        let mut locations: Vec<&FileLocation> = Vec::new();
        for entity_match in entity_matches {
            for location in &entity_match.locations {
                if location.path == file {
                    locations.push(location);
                }
            }
        }

        if locations.is_empty() {
            return Ok(None);
        }

        // Use first location (could merge multiple in future)
        let location = locations[0];

        // Read file
        let content = tokio::fs::read_to_string(file)
            .await
            .with_context(|| format!("Failed to read file {:?}", file))?;

        let lines: Vec<&str> = content.lines().collect();

        // Calculate snippet range
        let line_start = location.line_start.saturating_sub(CONTEXT_LINES).max(1);
        let line_end = (location.line_end + CONTEXT_LINES).min(lines.len());

        // Extract snippet
        let snippet_content = lines[line_start.saturating_sub(1)..line_end].join("\n");

        Ok(Some(FileSnippet {
            file: file.to_path_buf(),
            line_start,
            line_end,
            content: snippet_content,
            relevance_score: 1.0,
        }))
    }

    /// Infer search term from context
    pub fn infer_search_term(&self, vague_term: &str) -> Option<String> {
        // For now, just return the term itself
        // Future: could be smarter about expanding terms
        if vague_term.len() >= 3 {
            Some(vague_term.to_string())
        } else {
            None
        }
    }

    /// Perform proactive search
    pub async fn proactive_search(&self, search_term: &str) -> Result<Option<serde_json::Value>> {
        if let Some(grep_manager) = &self.grep_manager {
            let input = GrepSearchInput::with_defaults(
                search_term.to_string(),
                self.workspace_root.to_string_lossy().to_string(),
            );

            match grep_manager.perform_search(input).await {
                Ok(result) => {
                    // Convert to JSON value
                    let json_value = serde_json::to_value(&result.matches)
                        .with_context(|| "Failed to serialize search results")?;
                    Ok(Some(json_value))
                }
                Err(_) => Ok(None),
            }
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gathered_context_new() {
        let context = GatheredContext::new();
        assert_eq!(context.files.len(), 0);
        assert_eq!(context.estimated_tokens, 0);
        assert!(!context.is_over_budget());
    }

    #[test]
    fn test_gathered_context_add_file() {
        let mut context = GatheredContext::new();
        let file = PathBuf::from("src/test.rs");

        context.add_file(file.clone());

        assert_eq!(context.files.len(), 1);
        assert_eq!(context.files[0], file);
    }

    #[test]
    fn test_gathered_context_add_snippet() {
        let mut context = GatheredContext::new();
        let file = PathBuf::from("src/test.rs");

        let snippet = FileSnippet {
            file: file.clone(),
            line_start: 1,
            line_end: 10,
            content: "fn test() {}\n".repeat(10),
            relevance_score: 1.0,
        };

        context.add_snippet(file.clone(), snippet);

        assert_eq!(context.snippets.len(), 1);
        assert!(context.estimated_tokens > 0);
    }

    #[test]
    fn test_gathered_context_to_prompt_text() {
        let mut context = GatheredContext::new();
        let file = PathBuf::from("src/test.rs");

        context.add_file(file.clone());

        let snippet = FileSnippet {
            file: file.clone(),
            line_start: 1,
            line_end: 5,
            content: "fn main() {}\n".to_string(),
            relevance_score: 1.0,
        };

        context.add_snippet(file, snippet);

        let text = context.to_prompt_text();

        assert!(text.contains("Proactively Gathered Context"));
        assert!(text.contains("src/test.rs"));
        assert!(text.contains("fn main()"));
    }

    #[tokio::test]
    async fn test_proactive_gatherer_new() {
        let workspace_root = PathBuf::from("/test");
        let workspace_state = Arc::new(RwLock::new(WorkspaceState::new()));

        let gatherer = ProactiveGatherer::new(workspace_root.clone(), workspace_state);

        assert_eq!(gatherer.workspace_root, workspace_root);
    }
}
