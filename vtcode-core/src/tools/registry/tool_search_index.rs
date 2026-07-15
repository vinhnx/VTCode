//! Client-side tool search engine.
//!
//! This module is intentionally isolated from the session catalog and registry
//! plumbing in `tool_catalog_facade`. It owns exactly one concern: ranking
//! tool definitions against a query string.
//!
//! The [`ToolSearchEngine`] trait is the strict interface guardrail. Callers
//! (the session catalog, executors) depend on the trait, never on the concrete
//! scoring implementation, so the current BM25 lexical engine
//! ([`ToolEmbeddingIndex`]) can be replaced by an embedding-based engine in a
//! future generation without touching any consumer.

use std::collections::HashMap;

use crate::llm::provider::ToolDefinition;

/// Strict interface for a client-side tool search engine.
///
/// This is the boundary between "how tools are ranked" and "how ranked tools
/// are consumed". Any future engine (embedding vectors, hybrid, provider API)
/// only needs to implement this trait to be swapped in.
pub trait ToolSearchEngine {
    /// Rank tools against `query`, returning at most `max_results` hits ordered
    /// by descending relevance. An empty or unmatched query yields an empty
    /// result set.
    fn search(&self, query: &str, max_results: usize) -> Vec<ToolSearchResult>;

    /// The catalog epoch this engine was built for. Consumers use it to decide
    /// whether a cached engine is still valid.
    fn epoch(&self) -> u64;
}

/// A single tool search result with a relevance score.
#[derive(Debug, Clone)]
pub struct ToolSearchResult {
    pub name: String,
    pub description: String,
    pub score: f64,
}

/// BM25-style term-frequency tool search engine.
///
/// A pre-computed index for client-side tool search. Instead of sending all
/// tool definitions to the provider for server-side search, this index ranks
/// tools by lexical relevance locally.
///
/// The current implementation uses BM25 term-frequency scoring as a baseline.
/// A future enhancement could replace this with a proper embedding model
/// (e.g. a lightweight local model or the provider's embedding API) behind the
/// same [`ToolSearchEngine`] interface.
#[derive(Debug, Clone)]
pub struct ToolEmbeddingIndex {
    /// One entry per indexed tool with pre-computed term frequencies.
    entries: Vec<ToolIndexEntry>,
    /// Global document frequency: how many entries contain each term.
    doc_freq: HashMap<String, u32>,
    /// Average document length (term count) across all entries, precomputed at
    /// build time so `search` never rescans the corpus per query.
    avg_dl: f64,
    /// Epoch at which this index was built. Invalidated on catalog refresh.
    epoch: u64,
}

#[derive(Debug, Clone)]
struct ToolIndexEntry {
    name: String,
    description: String,
    /// Pre-computed term frequencies for the lowercased terms extracted from
    /// name + description. Replaces a per-query linear scan of a `Vec<String>`
    /// with an O(1) lookup.
    term_freq: HashMap<String, u32>,
    /// Total term count (document length) for BM25 length normalization.
    term_count: usize,
}

/// BM25 term-frequency saturation parameter.
const BM25_K1: f64 = 1.5;
/// BM25 length-normalization parameter.
const BM25_B: f64 = 0.75;
/// Multiplier applied when a query term also appears in the tool name.
const NAME_MATCH_BOOST: f64 = 2.0;

/// Extract, lowercase, and trim query/document tokens using a single shared
/// rule so the index and query are tokenized identically (DRY).
fn tokenize(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split_whitespace().filter_map(|raw| {
        let normalized = raw
            .to_lowercase()
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_string();
        (!normalized.is_empty() && normalized.len() > 1).then_some(normalized)
    })
}

impl ToolEmbeddingIndex {
    /// Build an index from a set of tool definitions.
    #[must_use]
    pub fn build(tools: &[ToolDefinition], epoch: u64) -> Self {
        let entries: Vec<ToolIndexEntry> = tools
            .iter()
            .filter_map(|tool| {
                let func = tool.function.as_ref()?;
                let name = func.name.clone();
                let description = func.description.clone();
                // Fold the tool's namespace name/description into the indexed
                // terms (but not the returned `description`) so a query for
                // a server name ranks that server's tools higher.
                let combined = match tool.namespace.as_ref() {
                    Some(namespace) => format!(
                        "{name} {description} {} {}",
                        namespace.name, namespace.description
                    ),
                    None => format!("{name} {description}"),
                };
                let mut term_freq: HashMap<String, u32> = HashMap::new();
                let mut term_count = 0usize;
                for term in tokenize(&combined) {
                    *term_freq.entry(term).or_insert(0) += 1;
                    term_count += 1;
                }
                Some(ToolIndexEntry {
                    name,
                    description,
                    term_freq,
                    term_count,
                })
            })
            .collect();

        // Compute document frequency (unique terms per entry) and the average
        // document length once, so per-query scoring stays allocation-free.
        let mut doc_freq: HashMap<String, u32> = HashMap::new();
        let mut total_dl = 0usize;
        for entry in &entries {
            total_dl += entry.term_count;
            for term in entry.term_freq.keys() {
                *doc_freq.entry(term.clone()).or_insert(0) += 1;
            }
        }
        let avg_dl = if entries.is_empty() {
            0.0
        } else {
            total_dl as f64 / entries.len() as f64
        };

        Self {
            entries,
            doc_freq,
            avg_dl,
            epoch,
        }
    }
}

impl ToolSearchEngine for ToolEmbeddingIndex {
    /// Search the index with a query string. Returns tools ranked by relevance.
    ///
    /// Uses BM25-inspired scoring:
    /// - Term frequency: how often the query term appears in the tool's text
    /// - Inverse document frequency: rare terms score higher
    /// - Field boost: name matches score higher than description matches
    fn search(&self, query: &str, max_results: usize) -> Vec<ToolSearchResult> {
        let query_terms: Vec<String> = tokenize(query).collect();

        if query_terms.is_empty() || self.entries.is_empty() {
            return Vec::new();
        }

        let n = self.entries.len() as f64;
        let avg_dl = self.avg_dl;

        let mut results: Vec<ToolSearchResult> = self
            .entries
            .iter()
            .map(|entry| {
                let mut score = 0.0;
                let dl = entry.term_count as f64;

                for query_term in &query_terms {
                    // Precomputed term frequency lookup (was an O(terms) scan)
                    let tf = entry.term_freq.get(query_term).copied().unwrap_or(0) as f64;

                    // Look up document frequency by term string, not positional index
                    let df = self.doc_freq.get(query_term).copied().unwrap_or(0) as f64;

                    if df == 0.0 {
                        continue;
                    }

                    // IDF component
                    let idf = ((n - df + 0.5) / (df + 0.5)).max(0.01);

                    // TF-IDF with BM25 length normalization
                    let tf_component = (tf * (BM25_K1 + 1.0))
                        / (tf + BM25_K1 * (1.0 - BM25_B + BM25_B * dl / avg_dl));

                    // Boost for name matches (name is more important than description)
                    let name_match_boost = if entry.name.to_lowercase().contains(query_term) {
                        NAME_MATCH_BOOST
                    } else {
                        1.0
                    };

                    score += idf * tf_component * name_match_boost;
                }

                ToolSearchResult {
                    name: entry.name.clone(),
                    description: entry.description.clone(),
                    score,
                }
            })
            .filter(|r| r.score > 0.0)
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(max_results);
        results
    }

    fn epoch(&self) -> u64 {
        self.epoch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_with_desc(name: &str, desc: &str) -> ToolDefinition {
        ToolDefinition::function(name.to_string(), desc.to_string(), serde_json::json!({}))
    }

    #[test]
    fn embedding_index_search_returns_ranked_results() {
        let tools = vec![
            tool_with_desc("read_file", "Read the contents of a file from disk"),
            tool_with_desc("write_file", "Write content to a file on disk"),
            tool_with_desc(
                "search_code",
                "Search for code patterns using regex or structural search",
            ),
            tool_with_desc(
                "run_command",
                "Execute a shell command and return its output",
            ),
        ];

        let index = ToolEmbeddingIndex::build(&tools, 0);

        // Search for "file read" — should rank read_file highest
        let results = index.search("file read", 3);
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "read_file");

        // Search for "shell execute" — should rank run_command highest
        let results = index.search("shell execute", 3);
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "run_command");

        // Search for "regex pattern" — should rank search_code highest
        let results = index.search("regex pattern", 3);
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "search_code");
    }

    #[test]
    fn embedding_index_empty_query_returns_empty() {
        let tools = vec![tool_with_desc("read_file", "Read a file")];
        let index = ToolEmbeddingIndex::build(&tools, 0);
        let results = index.search("", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn embedding_index_respects_max_results() {
        let tools = vec![
            tool_with_desc("tool_a", "tool a description"),
            tool_with_desc("tool_b", "tool b description"),
            tool_with_desc("tool_c", "tool c description"),
        ];
        let index = ToolEmbeddingIndex::build(&tools, 0);
        let results = index.search("tool", 2);
        assert!(results.len() <= 2);
    }

    #[test]
    fn embedding_index_epoch_is_preserved() {
        let tools = vec![tool_with_desc("read_file", "Read a file")];
        let index = ToolEmbeddingIndex::build(&tools, 42);
        assert_eq!(index.epoch(), 42);
    }
}
