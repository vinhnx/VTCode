//! Ripgrep execution backend for `grep_file` and `code_search`.
//!
//! Owns everything that actually runs ripgrep and parses its JSON output:
//! the async child-process runner, the bounded streaming literal backend used
//! by `code_search`, and the shared input/result types. Orchestration
//! (debounce, cancellation, cache, finalization) lives in `super::grep_file`.

use crate::tools::ast_grep_language::AstGrepLanguage;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use vtcode_commons::exclusions::DEFAULT_IGNORE_GLOBS;

/// Maximum number of search results to return - AGENTS.md requires max 5 results
pub(crate) const MAX_SEARCH_RESULTS: NonZeroUsize = NonZeroUsize::new(5).expect("5 is non-zero");

/// Optimal number of threads for searching, calculated based on CPU count
static OPTIMAL_SEARCH_THREADS: OnceLock<NonZeroUsize> = OnceLock::new();

/// Calculate optimal number of search threads based on available CPU cores
/// Uses 75% of cores, clamped between 2 and 8 threads
fn optimal_search_threads() -> NonZeroUsize {
    *OPTIMAL_SEARCH_THREADS.get_or_init(|| {
        let cpu_count = num_cpus::get();
        // Use 75% of cores for better parallelism, min 2, max 8
        let threads = (cpu_count * 3 / 4).clamp(2, 8);
        NonZeroUsize::new(threads).unwrap_or(NonZeroUsize::new(2).expect("2 is non-zero"))
    })
}

/// Maximum bytes to keep in a single grep response before truncation.
const DEFAULT_MAX_RESULT_BYTES: usize = 32 * 1024;

/// Default timeout for blocking grep invocations.
pub(crate) const DEFAULT_SEARCH_TIMEOUT: Duration = Duration::from_secs(5);

/// Byte cap for a single streaming literal-search record stream.
pub(crate) const CODE_SEARCH_STREAM_BYTE_CAP: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiteralSearchCandidate {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub matched_text: String,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiteralSearchOutcome {
    pub candidates: Vec<LiteralSearchCandidate>,
    pub truncated: bool,
}

pub(super) async fn kill_and_reap_search_child(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoundedRecordRead {
    Record,
    Eof,
    Exhausted,
}

pub(crate) async fn read_bounded_record<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    record: &mut Vec<u8>,
    bytes_read: &mut usize,
    byte_cap: usize,
) -> std::io::Result<BoundedRecordRead> {
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(if record.is_empty() {
                BoundedRecordRead::Eof
            } else {
                BoundedRecordRead::Record
            });
        }
        if *bytes_read >= byte_cap {
            return Ok(BoundedRecordRead::Exhausted);
        }

        let remaining = byte_cap - *bytes_read;
        let bounded = &available[..available.len().min(remaining)];
        let consumed = bounded
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bounded.len(), |index| index + 1);
        let record_complete = bounded.get(consumed.saturating_sub(1)) == Some(&b'\n');
        record.extend_from_slice(&bounded[..consumed]);
        reader.consume(consumed);
        *bytes_read += consumed;
        if record_complete {
            return Ok(BoundedRecordRead::Record);
        }
    }
}

/// Run a fixed-string smart-case ripgrep stream with request-scoped bounds.
/// Split a `code_search` query on `|` into trimmed, non-empty literal terms.
///
/// Returns an empty vec for an empty/whitespace query, a single-element vec
/// when there is no alternation, and one entry per term otherwise. Terms that
/// are empty after trimming (e.g. `a||b`, `|foo`, `bar|`) are dropped so the
/// alternation is well-formed.
pub(crate) fn split_alternation(query: &str) -> Vec<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if !trimmed.contains('|') {
        return vec![trimmed.to_string()];
    }
    let terms: Vec<String> = trimmed
        .split('|')
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_string)
        .collect();
    terms
}

pub(crate) async fn search_literal_bounded(
    query: &str,
    search_path: &Path,
    languages: &[AstGrepLanguage],
    candidate_cap: usize,
) -> Result<LiteralSearchOutcome> {
    // Support `|`-separated alternation (e.g. "tokio|async-std|runtime"), the
    // most common plan-mode exploration pattern. Without this, alternation
    // queries silently return 0 results because `--fixed-strings` treats the
    // whole query as one literal substring. Single-term queries keep
    // `--fixed-strings` so literal metacharacters stay non-magic; multi-term
    // queries join `regex::escape`-ed terms with `|` as a regex alternation.
    let alternation_terms = split_alternation(query);
    let alternation_regex = (alternation_terms.len() >= 2).then(|| {
        alternation_terms
            .iter()
            .map(|term| regex::escape(term))
            .collect::<Vec<_>>()
            .join("|")
    });
    let (pattern_arg, use_fixed_strings) = match alternation_regex {
        Some(regex_pattern) => (regex_pattern, false),
        None => match alternation_terms.as_slice() {
            [single] => (single.clone(), true),
            // Empty query (or only empty terms): fall back to the trimmed
            // original so ripgrep surfaces a clear "no matches" result.
            _ => (query.trim().to_string(), true),
        },
    };

    let mut command = TokioCommand::new("rg");
    command
        .arg("--json")
        .arg("--smart-case")
        .arg("--sort=path")
        .arg("--hidden")
        .arg("--no-messages")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if use_fixed_strings {
        command.arg("--fixed-strings");
    }
    for pattern in DEFAULT_IGNORE_GLOBS {
        command.arg("--glob").arg(format!("!{pattern}"));
    }
    for language in languages {
        for glob in language.path_globs() {
            command.arg("--iglob").arg(glob);
        }
    }
    // Patterns starting with `-` (e.g. markdown `- [link](...)`) would parse
    // as flags without `-e`; `--` keeps a leading-dash path positional.
    command.arg("-e").arg(&pattern_arg).arg("--").arg(search_path);

    run_literal_child(command, query, candidate_cap).await
}

async fn run_literal_child(
    mut command: TokioCommand,
    query: &str,
    candidate_cap: usize,
) -> Result<LiteralSearchOutcome> {
    // Own cancellation even while waiting for the first JSON record.
    command.kill_on_drop(true);
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to execute ripgrep for literal query '{query}'"))?;
    let Some(stdout) = child.stdout.take() else {
        kill_and_reap_search_child(&mut child).await;
        anyhow::bail!("failed to capture ripgrep output");
    };
    let mut reader = BufReader::new(stdout);
    let mut candidates = Vec::with_capacity(candidate_cap);
    let mut bytes_read = 0usize;
    let mut truncated = false;
    let mut line_buf = Vec::with_capacity(CODE_SEARCH_STREAM_BYTE_CAP);

    loop {
        line_buf.clear();
        let read =
            match read_bounded_record(&mut reader, &mut line_buf, &mut bytes_read, CODE_SEARCH_STREAM_BYTE_CAP).await {
                Ok(read) => read,
                Err(error) => {
                    drop(reader);
                    kill_and_reap_search_child(&mut child).await;
                    return Err(error).context("failed to read ripgrep JSON stream");
                }
            };
        match read {
            BoundedRecordRead::Record => {}
            BoundedRecordRead::Eof => break,
            BoundedRecordRead::Exhausted => {
                truncated = true;
                break;
            }
        }
        let event = match serde_json::from_slice::<Value>(&line_buf) {
            Ok(event) => event,
            Err(error) => {
                drop(reader);
                kill_and_reap_search_child(&mut child).await;
                return Err(error).context("failed to parse ripgrep JSON stream record");
            }
        };
        if event.get("type").and_then(Value::as_str) != Some("match") {
            continue;
        }
        let Some(data) = event.get("data") else {
            continue;
        };
        let Some(path) = data.get("path").and_then(|path| path.get("text")).and_then(Value::as_str) else {
            continue;
        };
        let Some(snippet) = data.get("lines").and_then(|lines| lines.get("text")).and_then(Value::as_str) else {
            continue;
        };
        let line = data
            .get("line_number")
            .and_then(Value::as_u64)
            .and_then(|line| usize::try_from(line).ok())
            .unwrap_or(1);
        let absolute_offset = data
            .get("absolute_offset")
            .and_then(Value::as_u64)
            .and_then(|offset| usize::try_from(offset).ok())
            .unwrap_or(0);
        let Some(submatches) = data.get("submatches").and_then(Value::as_array) else {
            continue;
        };
        for submatch in submatches {
            let Some(start) = submatch
                .get("start")
                .and_then(Value::as_u64)
                .and_then(|offset| usize::try_from(offset).ok())
            else {
                continue;
            };
            let Some(end) = submatch
                .get("end")
                .and_then(Value::as_u64)
                .and_then(|offset| usize::try_from(offset).ok())
            else {
                continue;
            };
            let Some(matched_text) = submatch
                .get("match")
                .and_then(|value| value.get("text"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            candidates.push(LiteralSearchCandidate {
                path: PathBuf::from(path),
                line,
                column: start.saturating_add(1),
                byte_start: absolute_offset.saturating_add(start),
                byte_end: absolute_offset.saturating_add(end),
                matched_text: matched_text.to_string(),
                snippet: snippet.to_string(),
            });
            if candidates.len() >= candidate_cap {
                truncated = true;
                break;
            }
        }
        if truncated {
            break;
        }
    }

    drop(reader);
    if truncated {
        let _ = child.start_kill();
    }
    let status = child.wait().await.context("failed to reap ripgrep process")?;
    if !truncated && !matches!(status.code(), Some(0) | Some(1)) {
        anyhow::bail!("ripgrep literal search failed ({status})");
    }

    Ok(LiteralSearchOutcome { candidates, truncated })
}

/// Input parameters for ripgrep search
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GrepSearchInput {
    pub pattern: String,
    pub path: String,
    pub case_sensitive: Option<bool>,
    pub literal: Option<bool>,
    pub glob_pattern: Option<String>,
    pub context_lines: Option<usize>,
    pub include_hidden: Option<bool>,
    pub max_results: Option<usize>,
    pub respect_ignore_files: Option<bool>, // Whether to respect .gitignore, .ignore files
    pub max_file_size: Option<usize>,       // Maximum file size to search (in bytes)
    pub search_hidden: Option<bool>,        // Whether to search hidden files/directories
    pub search_binary: Option<bool>,        // Whether to search binary files
    pub files_with_matches: Option<bool>,   // Only print filenames with matches
    pub type_pattern: Option<String>,       // Search files of a specific type (e.g., "rust", "python")
    pub invert_match: Option<bool>,         // Invert the matching
    pub word_boundaries: Option<bool>,      // Match only word boundaries (regexp \b)
    pub line_number: Option<bool>,          // Show line numbers
    pub column: Option<bool>,               // Show column numbers
    pub only_matching: Option<bool>,        // Show only matching parts
    pub trim: Option<bool>,                 // Trim whitespace from matches
    pub max_result_bytes: Option<usize>,    // Optional truncation threshold (bytes)
    pub timeout: Option<Duration>,          // Optional timeout for blocking grep
    pub extra_ignore_globs: Option<Vec<String>>, // Additional ignore globs
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
            context_lines: None,
            include_hidden: None,
            max_results: None,
            respect_ignore_files: None,
            max_file_size: None,
            search_hidden: None,
            search_binary: None,
            files_with_matches: None,
            type_pattern: None,
            invert_match: None,
            word_boundaries: None,
            line_number: None,
            column: None,
            only_matching: None,
            trim: None,
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
            context_lines: None,
            include_hidden: Some(false),
            max_results: Some(MAX_SEARCH_RESULTS.get()),
            respect_ignore_files: Some(true),
            max_file_size: None,
            search_hidden: Some(false),
            search_binary: Some(false),
            files_with_matches: Some(false),
            type_pattern: None,
            invert_match: Some(false),
            word_boundaries: Some(false),
            line_number: Some(true),
            column: Some(false),
            only_matching: Some(false),
            trim: Some(false),
            max_result_bytes: Some(DEFAULT_MAX_RESULT_BYTES),
            timeout: Some(DEFAULT_SEARCH_TIMEOUT),
            extra_ignore_globs: None,
        }
    }
}

/// Result of a ripgrep search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepSearchResult {
    pub query: String,
    pub matches: Vec<Value>,
    pub truncated: bool,
    /// Total number of "match" type entries found before truncation.
    /// When `truncated` is true, this tells the agent how many matches exist
    /// vs how many are returned in `matches`.
    #[serde(default)]
    pub total_matches: Option<usize>,
}

/// Run ripgrep as an async child process, streaming its JSON output and
/// reaping it on completion.
///
/// Unlike the old blocking `Command::output()` approach, the child is owned
/// by the future with `kill_on_drop` so a cancelled search (timeout in
/// `perform_search` or user cancellation) actually kills ripgrep instead
/// of leaving it orphaned to keep burning CPU. A `cancel_flag` is polled
/// while streaming so cancellation is noticed even when ripgrep is slow to
/// produce output. Finalization (match counting, truncation) is applied by
/// the caller via `GrepSearchManager::finalize_matches`.
pub(crate) async fn run_ripgrep_backend_async(
    input: &GrepSearchInput,
    cancel_flag: Option<&AtomicBool>,
) -> Result<Vec<Value>> {
    let mut cmd = TokioCommand::new("rg");
    cmd.arg("-j").arg(optimal_search_threads().get().to_string());

    // Add support for respecting ignore files (default is to respect them)
    if !input.respect_ignore_files.unwrap_or(true) {
        cmd.arg("--no-ignore");
    }

    // Add support for searching hidden files (default is not to search hidden)
    if input.search_hidden.unwrap_or(false) {
        cmd.arg("--hidden");
    }

    // Add support for searching binary files
    if input.search_binary.unwrap_or(false) {
        cmd.arg("--binary");
    }

    // Add support for files with matches only
    if input.files_with_matches.unwrap_or(false) {
        cmd.arg("--files-with-matches");
    }

    // Add support for file type filtering
    if let Some(type_pattern) = &input.type_pattern {
        cmd.arg("--type").arg(type_pattern);
    }

    // Add support for max file size
    if let Some(max_file_size) = input.max_file_size {
        cmd.arg("--max-filesize").arg(format!("{max_file_size}B"));
    }

    // Case sensitivity: pick exactly one flag from a single match so
    // ripgrep never sees conflicting `--ignore-case` + `--smart-case`.
    // Previously the cascade could append both when `case_sensitive`
    // defaulted to None but a higher-level wrapper set it to false.
    match input.case_sensitive {
        Some(true) => {
            cmd.arg("--case-sensitive");
        }
        Some(false) => {
            cmd.arg("--ignore-case");
        }
        None => {
            // Default to smart case when the caller didn't specify.
            cmd.arg("--smart-case");
        }
    }

    // Invert match
    if input.invert_match.unwrap_or(false) {
        cmd.arg("--invert-match");
    }

    // Word boundaries
    if input.word_boundaries.unwrap_or(false) {
        cmd.arg("--word-regexp");
    }

    // Line numbers
    if input.line_number.unwrap_or(true) {
        // Default to true to maintain context
        cmd.arg("--line-number");
    } else {
        cmd.arg("--no-line-number");
    }

    // Column numbers
    if input.column.unwrap_or(false) {
        cmd.arg("--column");
    }

    // Only matching parts
    if input.only_matching.unwrap_or(false) {
        cmd.arg("--only-matching");
    }

    // Trim whitespace (handled by not adding the --no-unicode flag, which is default)
    if input.trim.unwrap_or(false) {
        // This is handled in post-processing, not as a flag
    }

    if let Some(literal) = input.literal
        && literal
    {
        cmd.arg("--fixed-strings");
    }

    if let Some(glob_pattern) = &input.glob_pattern {
        cmd.arg("--glob").arg(glob_pattern);
    }

    if input.respect_ignore_files.unwrap_or(true) {
        for pattern in DEFAULT_IGNORE_GLOBS {
            cmd.arg("--glob").arg(format!("!{pattern}"));
        }
        if let Some(extra) = &input.extra_ignore_globs {
            for pattern in extra {
                cmd.arg("--glob").arg(format!("!{pattern}"));
            }
        }
    }

    if let Some(context_lines) = input.context_lines {
        cmd.arg("--context").arg(context_lines.to_string());
    }

    let max_results = input.max_results.unwrap_or(MAX_SEARCH_RESULTS.get());
    cmd.arg("--max-count").arg(max_results.to_string());

    // Use JSON output format for structured results
    cmd.arg("--json");

    // `-e` keeps a leading-dash pattern positional; `--` protects the path.
    cmd.arg("-e").arg(&input.pattern).arg("--").arg(&input.path);

    // Own the child so cancelling the search future kills ripgrep rather
    // than orphaning it.
    cmd.kill_on_drop(true);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to execute ripgrep for pattern '{}'", input.pattern))?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.start_kill();
        let _ = child.wait().await;
        anyhow::bail!("failed to capture ripgrep output");
    };

    let mut reader = BufReader::new(stdout);
    let mut matches: Vec<Value> = Vec::new();
    let mut line_buf = String::new();
    let mut cancel_poll = tokio::time::interval(Duration::from_millis(100));
    cancel_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        line_buf.clear();
        tokio::select! {
            read = reader.read_line(&mut line_buf) => {
                let bytes = read.context("failed to read ripgrep JSON stream")?;
                if bytes == 0 {
                    break;
                }
                if let Ok(event) = serde_json::from_str::<Value>(&line_buf) {
                    matches.push(event);
                }
            }
            _ = cancel_poll.tick() => {
                if cancel_flag.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    anyhow::bail!("ripgrep search cancelled");
                }
            }
        }
    }

    let status = child.wait().await.context("failed to reap ripgrep process")?;
    if !matches!(status.code(), Some(0) | Some(1)) {
        anyhow::bail!("ripgrep search failed");
    }

    Ok(matches)
}

#[cfg(test)]
mod tests;
