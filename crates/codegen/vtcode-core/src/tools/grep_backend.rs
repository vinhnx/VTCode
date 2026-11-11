//! Native content-search backend for `grep_file` and `code_search`.
//!
//! This module owns the search execution and the result shapes. The actual
//! scanning lives in [`vtcode_commons::content_search`], which walks the tree
//! with the `ignore` crate and matches lines with the `regex` crate.
//!
//! Two result shapes exist:
//!
//! - [`LiteralSearchOutcome`]: typed candidates used by `code_search`.
//! - [`GrepSearchResult`]: search JSON events used by `grep_file`.
//!
//! The JSON event shape is kept because `grep_file` results are consumed by
//! the transcript renderer and the file-palette path, which index into the
//! `data.path.text` and `type` fields. Producing the same shape natively keeps
//! those consumers unchanged.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use vtcode_commons::content_search::{CaseSensitivity, LineMatch, SearchRequest};
use vtcode_commons::exclusions::DEFAULT_IGNORE_GLOBS;

use super::code_language::CodeLanguage;

/// Maximum number of search results to return - AGENTS.md requires max 5 results
pub(crate) const MAX_SEARCH_RESULTS: NonZeroUsize = NonZeroUsize::new(5).expect("5 is non-zero");

/// Default timeout for blocking grep invocations.
pub(crate) const DEFAULT_SEARCH_TIMEOUT: Duration = Duration::from_secs(5);

/// A literal search hit, in the shape `code_search` consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiteralSearchCandidate {
    /// Path of the file the hit was found in.
    pub path: PathBuf,
    /// One-based line number of the hit.
    pub line: usize,
    /// One-based column of the hit start.
    pub column: usize,
    /// Byte offset of the hit start within the file.
    pub byte_start: usize,
    /// Byte offset of the hit end within the file.
    pub byte_end: usize,
    /// The matched substring.
    pub matched_text: String,
    /// The full text of the matching line.
    pub snippet: String,
    /// Whether the hit covers a whole identifier.
    ///
    /// A hit inside a longer identifier (`Widget` within `WidgetExtra`) is not
    /// word-bounded, so it cannot be a usage of the queried symbol.
    pub word_bounded: bool,
}

/// The outcome of a bounded literal search.
#[derive(Debug, Clone, Default)]
pub(crate) struct LiteralSearchOutcome {
    /// The hits that were found, bounded by the caller's cap.
    pub candidates: Vec<LiteralSearchCandidate>,
    /// Whether the cap was reached before the search finished.
    pub truncated: bool,
}

/// Splits a `|`-separated query into its literal terms.
///
/// An empty query yields no terms. A query without `|` yields exactly one
/// term. Otherwise the query is split on `|`, each part is trimmed, and empty
/// parts are dropped so the alternation is always well-formed.
pub(crate) fn split_alternation(query: &str) -> Vec<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if !trimmed.contains('|') {
        return vec![trimmed.to_string()];
    }
    trimmed
        .split('|')
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_string)
        .collect()
}

/// Builds a search request for a literal-or-alternation query.
///
/// A single-term query is matched verbatim, so regex metacharacters in the
/// query keep no special meaning. A multi-term query becomes a regex
/// alternation of the escaped terms, so `tokio|async-std|runtime` matches any
/// of the three.
fn literal_request(query: &str, search_path: &Path, languages: &[CodeLanguage]) -> SearchRequest {
    let terms = split_alternation(query);
    let mut request = if terms.len() >= 2 {
        let alternation = terms.iter().map(|term| regex::escape(term)).collect::<Vec<_>>().join("|");
        SearchRequest::regex(alternation, search_path.to_path_buf())
    } else {
        SearchRequest::literal(terms.first().cloned().unwrap_or_default(), search_path.to_path_buf())
    };
    request = request.with_case(CaseSensitivity::Smart);
    for language in languages {
        for glob in language.path_globs() {
            request = request.with_language_glob(*glob);
        }
    }
    request
}

/// Runs a bounded literal search over `search_path`.
///
/// The search stops once `candidate_cap` candidates have been collected, so a
/// broad query over a large tree does not scan the whole repository.
pub(crate) async fn search_literal_bounded(
    query: &str,
    search_path: &Path,
    languages: &[CodeLanguage],
    candidate_cap: usize,
) -> Result<LiteralSearchOutcome> {
    if !search_path.exists() {
        anyhow::bail!("literal search failed: {} does not exist", search_path.display());
    }

    let request = literal_request(query, search_path, languages).with_max_results(candidate_cap);
    let matches = vtcode_commons::content_search::search(&request)
        .with_context(|| format!("literal search failed for pattern '{query}'"))?;

    let truncated = matches.len() >= candidate_cap;
    let candidates = matches.into_iter().map(|line_match| literal_candidate(&line_match)).collect();

    Ok(LiteralSearchOutcome { candidates, truncated })
}

/// Converts a native line match into the literal-search candidate shape.
fn literal_candidate(line_match: &LineMatch) -> LiteralSearchCandidate {
    LiteralSearchCandidate {
        path: line_match.path.clone(),
        line: line_match.line,
        column: line_match.column,
        // The offsets are file-absolute, so they can be compared with the
        // declaration ranges the scanner records for the same file.
        byte_start: line_match.byte_start,
        byte_end: line_match.byte_end,
        matched_text: line_match.matched_text.clone(),
        snippet: line_match.line_text.clone(),
        word_bounded: line_match.word_bounded,
    }
}

/// Input parameters for a content search.
///
/// Every option here is read by the engine. Options the engine cannot honour
/// are deliberately absent rather than accepted and ignored, so a caller
/// cannot be misled into believing an unsupported filter was applied.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GrepSearchInput {
    /// The pattern to search for.
    pub pattern: String,
    /// The path to search.
    pub path: String,
    /// Whether matching is case-sensitive. `None` means smart-case.
    pub case_sensitive: Option<bool>,
    /// Whether the pattern is a literal string rather than a regular expression.
    pub literal: Option<bool>,
    /// Restrict the search to files matching this glob.
    pub glob_pattern: Option<String>,
    /// Whether to include hidden files.
    pub include_hidden: Option<bool>,
    /// Maximum number of results to return.
    pub max_results: Option<usize>,
    /// Whether to respect ignore files.
    pub respect_ignore_files: Option<bool>,
    /// Maximum file size to search, in bytes.
    pub max_file_size: Option<usize>,
    /// Whether to report only the files that contain a match.
    pub files_with_matches: Option<bool>,
    /// Optional truncation threshold (bytes).
    pub max_result_bytes: Option<usize>,
    /// Optional timeout for blocking grep.
    pub timeout: Option<Duration>,
    /// Additional ignore globs.
    pub extra_ignore_globs: Option<Vec<String>>,
}

impl GrepSearchInput {
    /// Create a new search input with pattern and path, using sensible defaults
    #[inline]
    pub fn new(pattern: String, path: String) -> Self {
        Self {
            pattern,
            path,
            case_sensitive: None,
            literal: None,
            glob_pattern: None,
            include_hidden: None,
            max_results: None,
            respect_ignore_files: None,
            max_file_size: None,
            files_with_matches: None,
            max_result_bytes: None,
            timeout: None,
            extra_ignore_globs: None,
        }
    }

    /// Create a search input with common defaults for internal grep searches
    #[inline]
    pub fn with_defaults(pattern: String, path: String) -> Self {
        Self {
            pattern,
            path,
            case_sensitive: Some(true),
            literal: Some(false),
            glob_pattern: None,
            include_hidden: Some(false),
            max_results: Some(MAX_SEARCH_RESULTS.get()),
            respect_ignore_files: Some(true),
            max_file_size: None,
            files_with_matches: Some(false),
            max_result_bytes: None,
            timeout: None,
            extra_ignore_globs: None,
        }
    }
}

/// The result of a content search, as JSON events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepSearchResult {
    /// The query that produced this result.
    pub query: String,
    /// The matches, as JSON events.
    pub matches: Vec<Value>,
    /// Whether the result was truncated.
    pub truncated: bool,
    /// Total number of matches found before truncation.
    #[serde(default)]
    pub total_matches: Option<usize>,
}

/// Runs a content search and returns the JSON events.
///
/// The event stream keeps the `begin` / `match` / `end` shape the transcript
/// renderer and the file-palette path already read.
pub(crate) async fn run_search_backend(
    input: &GrepSearchInput,
    cancel_flag: Option<&Arc<AtomicBool>>,
) -> Result<Vec<Value>> {
    let search_path = PathBuf::from(&input.path);
    if !search_path.exists() {
        anyhow::bail!("failed to search for pattern '{}': path does not exist", input.pattern);
    }

    let max_results = input.max_results.unwrap_or(MAX_SEARCH_RESULTS.get());
    let mut request = if input.literal.unwrap_or(false) {
        SearchRequest::literal(input.pattern.clone(), search_path.clone())
    } else {
        SearchRequest::regex(input.pattern.clone(), search_path.clone())
    };

    request = match input.case_sensitive {
        Some(true) => request.with_case(CaseSensitivity::Sensitive),
        Some(false) => request.with_case(CaseSensitivity::Insensitive),
        None => request.with_case(CaseSensitivity::Smart),
    };
    request = request.with_max_results(max_results);
    request = request.with_max_per_file(max_results);
    if let Some(max_file_size) = input.max_file_size {
        request = request.with_max_file_size(max_file_size as u64);
    }
    if input.include_hidden.unwrap_or(false) {
        request = request.with_hidden();
    }
    if input.files_with_matches.unwrap_or(false) {
        request = request.with_files_with_matches();
    }
    if let Some(glob) = &input.glob_pattern {
        request = request.with_include_glob(glob.clone());
    }
    for glob in DEFAULT_IGNORE_GLOBS {
        request = request.with_exclude_glob(*glob);
    }
    if let Some(extra) = &input.extra_ignore_globs {
        for glob in extra {
            request = request.with_exclude_glob(glob);
        }
    }
    if !input.respect_ignore_files.unwrap_or(true) {
        request = request.without_ignore_files();
    }

    // The flag is cloned so the blocking closure owns it: the scan runs on a
    // blocking thread, and the caller's reference does not outlive the await.
    let cancel_flag = cancel_flag.cloned();
    let matches = tokio::task::spawn_blocking(move || {
        if cancel_flag.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
            anyhow::bail!("content search cancelled");
        }
        vtcode_commons::content_search::search(&request).context("failed to search for pattern")
    })
    .await
    .context("content search worker failed")??;

    Ok(matches.into_iter().flat_map(json_events_for_match).collect())
}

/// Renders one native match as the JSON event sequence.
///
/// The shape matches `rg --json`: the submatch offsets are relative to the
/// start of the line, so they index into `lines.text`, and `absolute_offset`
/// marks where that line starts in the file.
fn json_events_for_match(line_match: LineMatch) -> Vec<Value> {
    let path_text = line_match.path.to_string_lossy().into_owned();
    // The offsets are relative to the line, so the file offset is reduced by
    // the start of the line the match was found on.
    let line_relative_start = line_match.byte_start.saturating_sub(line_match.line_start);
    let line_relative_end = line_match.byte_end.saturating_sub(line_match.line_start);
    let mut events = vec![json!({
        "type": "begin",
        "data": {"path": {"text": path_text}}
    })];
    events.push(json!({
        "type": "match",
        "data": {
            "path": {"text": path_text},
            "lines": {"text": line_match.line_text},
            "line_number": line_match.line,
            "absolute_offset": line_match.line_start,
            "submatches": [{
                "start": line_relative_start,
                "end": line_relative_end,
                "match": {"text": line_match.matched_text}
            }]
        }
    }));
    events.push(json!({
        "type": "end",
        "data": {"path": {"text": path_text}}
    }));
    events
}

#[cfg(test)]
mod tests;
