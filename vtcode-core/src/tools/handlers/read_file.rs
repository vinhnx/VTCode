use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Semaphore;
use vtcode_commons::diff_paths::looks_like_diff_content;

use crate::tools::traits::Tool;
use crate::utils::serde_helpers::{deserialize_maybe_quoted, deserialize_opt_maybe_quoted};

pub struct ReadFileHandler;

const MAX_LINE_LENGTH: usize = 500;
const TAB_WIDTH: usize = 4;
const COMMENT_PREFIXES: &[&str] = &["#", "//", "--"];
const MIN_BATCH_LIMIT: usize = 200;
const DEFAULT_MAX_CONCURRENCY: usize = 8;
const BATCH_CONDENSED_THRESHOLD: usize = 30;

/// JSON arguments accepted by the `read_file` tool handler.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ReadFileArgs {
    /// Absolute path to the file that will be read.
    pub file_path: String,
    /// 1-indexed line number to start reading from; defaults to 1.
    #[serde(
        default = "defaults::offset",
        deserialize_with = "deserialize_maybe_quoted"
    )]
    pub offset: usize,
    /// Maximum number of lines to return; defaults to 2000.
    #[serde(
        default = "defaults::limit",
        deserialize_with = "deserialize_maybe_quoted"
    )]
    pub limit: usize,
    /// Determines whether the handler reads a simple slice or indentation-aware block.
    #[serde(default)]
    pub mode: ReadMode,
    /// Optional indentation configuration used when `mode` is `Indentation`.
    #[serde(default)]
    pub indentation: Option<IndentationArgs>,
    /// Optional token limit for response
    #[serde(default, deserialize_with = "deserialize_opt_maybe_quoted")]
    pub max_tokens: Option<usize>,
}

/// Batch read request for reading multiple files or ranges in parallel.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct BatchReadArgs {
    /// List of read requests to execute in parallel.
    pub reads: Vec<BatchReadRequest>,
    /// Maximum concurrent file reads (default: 8).
    #[serde(default = "defaults::max_concurrency")]
    pub max_concurrency: usize,
    /// Whether to show progress in UI (default: true).
    #[serde(default = "defaults::ui_progress")]
    pub ui_progress: bool,
}

/// A single file read request within a batch.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct BatchReadRequest {
    /// Absolute path to the file to read.
    pub file_path: String,
    /// Single range to read (mutually exclusive with `ranges`).
    #[serde(flatten)]
    pub range: Option<ReadRange>,
    /// Multiple ranges to read from the same file.
    #[serde(default)]
    pub ranges: Option<Vec<ReadRange>>,
}

/// A range specification for reading.
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct ReadRange {
    /// 1-indexed line number to start reading from; defaults to 1.
    #[serde(
        default = "defaults::offset",
        deserialize_with = "deserialize_maybe_quoted"
    )]
    pub offset: usize,
    /// Maximum number of lines to return; defaults to 500 for batch.
    #[serde(
        default = "defaults::batch_limit",
        deserialize_with = "deserialize_maybe_quoted"
    )]
    pub limit: usize,
    /// Read mode: slice or indentation.
    #[serde(default)]
    pub mode: ReadMode,
    /// Indentation options when mode is indentation.
    #[serde(default)]
    pub indentation: Option<IndentationArgs>,
}

/// Result for a single file read in batch mode.
#[derive(Serialize, Clone, Debug)]
pub struct BatchReadResult {
    /// The file path that was read.
    pub file_path: String,
    /// Results for each range read.
    pub ranges: Vec<RangeResult>,
    /// Error if the entire file read failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Result for a single range read.
#[derive(Serialize, Clone, Debug)]
pub struct RangeResult {
    /// Starting line offset.
    pub offset: usize,
    /// Lines actually read.
    pub lines_read: usize,
    /// Whether content was condensed.
    pub condensed: bool,
    /// Number of lines omitted if condensed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted_lines: Option<usize>,
    /// The content read.
    pub content: String,
}

/// Progress tracking for batch reads.
#[derive(Clone)]
pub struct BatchProgress {
    /// Total number of files to read.
    pub total_files: Arc<AtomicUsize>,
    /// Number of files completed.
    pub completed_files: Arc<AtomicUsize>,
    /// Current file being read.
    pub current_file: Arc<tokio::sync::RwLock<String>>,
    /// Total bytes to read (estimated).
    pub total_bytes: Arc<AtomicU64>,
    /// Bytes read so far.
    pub bytes_read: Arc<AtomicU64>,
}

impl BatchProgress {
    pub fn new(total_files: usize) -> Self {
        Self {
            total_files: Arc::new(AtomicUsize::new(total_files)),
            completed_files: Arc::new(AtomicUsize::new(0)),
            current_file: Arc::new(tokio::sync::RwLock::new(String::new())),
            total_bytes: Arc::new(AtomicU64::new(0)),
            bytes_read: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn file_started(&self, file_path: &str) {
        let mut current = self.current_file.blocking_write();
        *current = file_path.to_string();
    }

    pub fn file_completed(&self) {
        self.completed_files.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_bytes(&self, bytes: u64) {
        self.bytes_read.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn progress_percent(&self) -> f64 {
        let completed = self.completed_files.load(Ordering::Relaxed);
        let total = self.total_files.load(Ordering::Relaxed);
        if total == 0 {
            100.0
        } else {
            (completed as f64 / total as f64) * 100.0
        }
    }

    pub fn status_line(&self) -> (String, String) {
        let completed = self.completed_files.load(Ordering::Relaxed);
        let total = self.total_files.load(Ordering::Relaxed);
        let current = self.current_file.blocking_read();
        let file_name = PathBuf::from(current.as_str())
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| current.clone());

        let left = format!("Reading {}/{}: {}", completed + 1, total, file_name);
        let right = format!("{:.0}%", self.progress_percent());
        (left, right)
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReadMode {
    #[default]
    Slice,
    Indentation,
}

/// Additional configuration for indentation-aware reads.
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct IndentationArgs {
    /// Optional explicit anchor line; defaults to `offset` when omitted.
    #[serde(default, deserialize_with = "deserialize_opt_maybe_quoted")]
    pub anchor_line: Option<usize>,
    /// Maximum indentation depth to collect; `0` means unlimited.
    #[serde(
        default = "defaults::max_levels",
        deserialize_with = "deserialize_maybe_quoted"
    )]
    pub max_levels: usize,
    /// Whether to include sibling blocks at the same indentation level.
    #[serde(default = "defaults::include_siblings")]
    pub include_siblings: bool,
    /// Whether to include header lines above the anchor block.
    #[serde(default = "defaults::include_header")]
    pub include_header: bool,
    /// Optional hard cap on returned lines; defaults to the global `limit`.
    #[serde(default, deserialize_with = "deserialize_opt_maybe_quoted")]
    pub max_lines: Option<usize>,
}

#[derive(Clone, Debug)]
struct LineRecord {
    number: usize,
    raw: String,
    display: String,
    indent: usize,
}

impl LineRecord {
    fn trimmed(&self) -> &str {
        self.raw.trim_start()
    }

    fn is_blank(&self) -> bool {
        self.trimmed().is_empty()
    }

    fn is_comment(&self) -> bool {
        COMMENT_PREFIXES
            .iter()
            .any(|prefix| self.raw.trim().starts_with(prefix))
    }
}

impl ReadFileHandler {
    /// Execute a batch read of multiple files/ranges in parallel.
    pub async fn handle_batch(&self, args: BatchReadArgs) -> Result<Value> {
        if args.reads.is_empty() {
            return Ok(json!({
                "success": false,
                "error": "No read requests provided"
            }));
        }

        let progress = BatchProgress::new(args.reads.len());
        let semaphore = Arc::new(Semaphore::new(args.max_concurrency.min(args.reads.len())));

        let results: Vec<BatchReadResult> = stream::iter(args.reads)
            .map(|req| {
                let sem = semaphore.clone();
                let prog = progress.clone();
                async move {
                    let _permit = sem.acquire().await.ok();
                    prog.file_started(&req.file_path);
                    let result = self.read_single_batch_request(&req).await;
                    prog.file_completed();
                    result
                }
            })
            .buffer_unordered(args.max_concurrency)
            .collect()
            .await;

        // Build concatenated content for token-efficient response
        let mut content_parts = Vec::new();
        for result in &results {
            if let Some(ref error) = result.error {
                content_parts.push(format!("== {} (ERROR)\n{}", result.file_path, error));
            } else {
                for range in &result.ranges {
                    let end_line = range.offset + range.lines_read.saturating_sub(1);
                    content_parts.push(format!(
                        "== {} (L{}..L{})\n{}",
                        result.file_path, range.offset, end_line, range.content
                    ));
                }
            }
        }

        let all_success = results.iter().all(|r| r.error.is_none());
        Ok(json!({
            "success": all_success,
            "content": content_parts.join("\n\n"),
            "items": results,
            "files_read": results.len(),
            "files_succeeded": results.iter().filter(|r| r.error.is_none()).count(),
            "no_spool": true
        }))
    }

    /// Read a single batch request (one file, possibly multiple ranges).
    async fn read_single_batch_request(&self, req: &BatchReadRequest) -> BatchReadResult {
        let path = PathBuf::from(&req.file_path);

        // Validate path
        if !path.is_absolute() {
            return BatchReadResult {
                file_path: req.file_path.clone(),
                ranges: vec![],
                error: Some("file_path must be an absolute path".to_string()),
            };
        }

        // Determine ranges to read
        let ranges_to_read: Vec<ReadRange> = if let Some(ref ranges) = req.ranges {
            ranges.clone()
        } else if let Some(ref range) = req.range {
            vec![range.clone()]
        } else {
            vec![ReadRange::default()]
        };

        let mut range_results = Vec::new();
        for range in ranges_to_read {
            match self.read_range(&path, &range).await {
                Ok(result) => range_results.push(result),
                Err(e) => {
                    return BatchReadResult {
                        file_path: req.file_path.clone(),
                        ranges: range_results,
                        error: Some(e.to_string()),
                    };
                }
            }
        }

        BatchReadResult {
            file_path: req.file_path.clone(),
            ranges: range_results,
            error: None,
        }
    }

    /// Read a single range from a file.
    async fn read_range(&self, path: &Path, range: &ReadRange) -> Result<RangeResult> {
        let offset = range.offset.max(1);
        let limit = range.limit.max(1);

        let mut collected = match range.mode {
            ReadMode::Slice => slice::read(path, offset, limit).await?,
            ReadMode::Indentation => {
                let indentation = range.indentation.clone().unwrap_or_default();
                indentation::read_block(path, offset, limit, indentation).await?
            }
        };

        let original_len = collected.len();
        let (condensed, omitted) = condense_for_batch(&mut collected);

        Ok(RangeResult {
            offset,
            lines_read: original_len,
            condensed,
            omitted_lines: if omitted > 0 { Some(omitted) } else { None },
            content: collected.join("\n"),
        })
    }

    /// Legacy handle method for backward compatibility with file_ops.rs
    pub async fn handle(&self, args: ReadFileArgs) -> Result<String> {
        let ReadFileArgs {
            file_path,
            offset,
            limit,
            mode,
            indentation,
            max_tokens,
        } = args;

        anyhow::ensure!(offset > 0, "offset must be a 1-indexed line number");
        anyhow::ensure!(limit > 0, "limit must be greater than zero");

        let path = PathBuf::from(&file_path);
        anyhow::ensure!(path.is_absolute(), "file_path must be an absolute path");

        let effective_limit =
            if matches!(mode, ReadMode::Slice) && max_tokens.is_none() && limit < MIN_BATCH_LIMIT {
                MIN_BATCH_LIMIT
            } else {
                limit
            };

        let mut collected = match mode {
            ReadMode::Slice => slice::read(&path, offset, effective_limit).await?,
            ReadMode::Indentation => {
                let indentation = indentation.unwrap_or_default();
                indentation::read_block(&path, offset, limit, indentation).await?
            }
        };

        // Condense large outputs (>100 lines) to head + tail
        condense_collected_lines(&mut collected);

        Ok(collected.join("\n"))
    }
}

#[async_trait]
impl Tool for ReadFileHandler {
    async fn execute(&self, args: Value) -> Result<Value> {
        // Try batch mode first (has "reads" field)
        if args.get("reads").is_some() {
            let batch_args: BatchReadArgs =
                serde_json::from_value(args).context("failed to parse batch read arguments")?;
            return self.handle_batch(batch_args).await;
        }

        // Legacy single-file mode
        let args: ReadFileArgs =
            serde_json::from_value(args).context("failed to parse read_file arguments")?;

        let file_path = args.file_path.clone();
        let content = self.handle(args).await?;

        Ok(json!({
            "content": content,
            "file_path": file_path,
            "path": file_path,
            "success": true,
            "no_spool": true
        }))
    }

    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Read file contents with optional line range, indentation-aware block selection, or batch multiple files"
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to read (for single-file mode)"
                },
                "offset": {
                    "type": "integer",
                    "description": "1-indexed line number to start from (default: 1)",
                    "default": 1,
                    "minimum": 1
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum lines to return (default: 2000)",
                    "default": 2000,
                    "minimum": 1
                },
                "mode": {
                    "type": "string",
                    "enum": ["slice", "indentation"],
                    "description": "Read mode: slice for simple range, indentation for block",
                    "default": "slice"
                },
                "indentation": {
                    "type": "object",
                    "description": "Indentation settings when mode=indentation",
                    "properties": {
                        "anchor_line": {
                            "type": "integer",
                            "description": "Line number to anchor on (defaults to offset)"
                        },
                        "max_levels": {
                            "type": "integer",
                            "description": "Max indentation depth (0=unlimited)",
                            "default": 0
                        },
                        "include_siblings": {
                            "type": "boolean",
                            "description": "Include sibling blocks",
                            "default": false
                        },
                        "include_header": {
                            "type": "boolean",
                            "description": "Include header lines above anchor",
                            "default": true
                        },
                        "max_lines": {
                            "type": "integer",
                            "description": "Hard cap on returned lines"
                        }
                    }
                },
                "max_tokens": {
                    "type": "integer",
                    "description": "Optional token limit for response (approximate)"
                },
                "reads": {
                    "type": "array",
                    "description": "Batch mode: array of file read requests to execute in parallel",
                    "items": {
                        "type": "object",
                        "properties": {
                            "file_path": {
                                "type": "string",
                                "description": "Absolute path to the file"
                            },
                            "offset": {
                                "type": "integer",
                                "description": "1-indexed start line (default: 1)"
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Max lines to return (default: 500 for batch)"
                            },
                            "ranges": {
                                "type": "array",
                                "description": "Multiple ranges from the same file",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "offset": { "type": "integer" },
                                        "limit": { "type": "integer" },
                                        "mode": { "type": "string", "enum": ["slice", "indentation"] }
                                    }
                                }
                            }
                        },
                        "required": ["file_path"]
                    }
                },
                "max_concurrency": {
                    "type": "integer",
                    "description": "Batch mode: max concurrent file reads (default: 8)",
                    "default": 8
                }
            }
        }))
    }
}

mod slice {
    use super::*;

    pub async fn read(path: &std::path::Path, offset: usize, limit: usize) -> Result<Vec<String>> {
        let file = File::open(path)
            .await
            .context(format!("failed to open file: {}", path.display()))?;

        let mut reader = BufReader::new(file);
        let mut collected = Vec::new();
        let mut seen = 0usize;
        let mut buffer = Vec::new();

        loop {
            buffer.clear();
            let bytes_read = reader
                .read_until(b'\n', &mut buffer)
                .await
                .context("failed to read file")?;

            if bytes_read == 0 {
                break;
            }

            // Strip newline characters
            if buffer.last() == Some(&b'\n') {
                buffer.pop();
                if buffer.last() == Some(&b'\r') {
                    buffer.pop();
                }
            }

            seen += 1;

            if seen < offset {
                continue;
            }

            if collected.len() >= limit {
                break;
            }

            let formatted = format_line(&buffer);
            collected.push(formatted);
        }

        if seen < offset {
            anyhow::bail!("offset exceeds file length");
        }

        Ok(collected)
    }
}

mod indentation {
    use super::*;

    pub async fn read_block(
        path: &std::path::Path,
        offset: usize,
        limit: usize,
        options: IndentationArgs,
    ) -> Result<Vec<String>> {
        let anchor_line = options.anchor_line.unwrap_or(offset);
        anyhow::ensure!(
            anchor_line > 0,
            "anchor_line must be a 1-indexed line number"
        );

        let guard_limit = options.max_lines.unwrap_or(limit);
        anyhow::ensure!(guard_limit > 0, "max_lines must be greater than zero");

        let collected = collect_file_lines(path).await?;
        anyhow::ensure!(
            !collected.is_empty() && anchor_line <= collected.len(),
            "anchor_line exceeds file length"
        );

        let anchor_index = anchor_line - 1;
        let effective_indents = compute_effective_indents(&collected);
        let anchor_indent = effective_indents[anchor_index];

        // Compute the min indent
        let min_indent = if options.max_levels == 0 {
            0
        } else {
            anchor_indent.saturating_sub(options.max_levels * TAB_WIDTH)
        };

        // Cap requested lines by guard_limit and file length
        let final_limit = limit.min(guard_limit).min(collected.len());

        if final_limit == 1 {
            return Ok(vec![format!(
                "{}: {}",
                collected[anchor_index].number, collected[anchor_index].display
            )]);
        }

        // Bidirectional cursors
        let mut i: isize = anchor_index as isize - 1; // up
        let mut j: usize = anchor_index + 1; // down
        let mut i_counter_min_indent = 0;
        let mut j_counter_min_indent = 0;

        let mut out = VecDeque::with_capacity(limit);
        out.push_back(&collected[anchor_index]);

        while out.len() < final_limit {
            let mut progressed = 0;

            // Expand upward
            if i >= 0 {
                let iu = i as usize;
                if effective_indents[iu] >= min_indent {
                    out.push_front(&collected[iu]);
                    progressed += 1;
                    i -= 1;

                    // Control sibling inclusion
                    if effective_indents[iu] == min_indent && !options.include_siblings {
                        let allow_header_comment =
                            options.include_header && collected[iu].is_comment();
                        let can_take_line = allow_header_comment || i_counter_min_indent == 0;

                        if can_take_line {
                            i_counter_min_indent += 1;
                        } else {
                            out.pop_front();
                            progressed -= 1;
                            i = -1;
                        }
                    }

                    if out.len() >= final_limit {
                        break;
                    }
                } else {
                    i = -1;
                }
            }

            // Expand downward
            if j < collected.len() {
                let ju = j;
                if effective_indents[ju] >= min_indent {
                    out.push_back(&collected[ju]);
                    progressed += 1;
                    j += 1;

                    // Control sibling inclusion
                    if effective_indents[ju] == min_indent && !options.include_siblings {
                        if j_counter_min_indent > 0 {
                            out.pop_back();
                            progressed -= 1;
                            j = collected.len();
                        }
                        j_counter_min_indent += 1;
                    }
                } else {
                    j = collected.len();
                }
            }

            if progressed == 0 {
                break;
            }
        }

        trim_empty_lines(&mut out);

        Ok(out
            .into_iter()
            .map(|record| format!("{}: {}", record.number, record.display))
            .collect())
    }

    async fn collect_file_lines(path: &std::path::Path) -> Result<Vec<LineRecord>> {
        let file = File::open(path)
            .await
            .context(format!("failed to open file: {}", path.display()))?;

        let mut reader = BufReader::new(file);
        let mut buffer = Vec::new();
        let mut lines = Vec::new();
        let mut number = 0usize;

        loop {
            buffer.clear();
            let bytes_read = reader
                .read_until(b'\n', &mut buffer)
                .await
                .context("failed to read file")?;

            if bytes_read == 0 {
                break;
            }

            if buffer.last() == Some(&b'\n') {
                buffer.pop();
                if buffer.last() == Some(&b'\r') {
                    buffer.pop();
                }
            }

            number += 1;
            let raw = String::from_utf8_lossy(&buffer).into_owned();
            let indent = measure_indent(&raw);
            let display = format_line(&buffer);
            lines.push(LineRecord {
                number,
                raw,
                display,
                indent,
            });
        }

        Ok(lines)
    }

    fn compute_effective_indents(records: &[LineRecord]) -> Vec<usize> {
        let mut effective = Vec::with_capacity(records.len());
        let mut previous_indent = 0usize;
        for record in records {
            if record.is_blank() {
                effective.push(previous_indent);
            } else {
                previous_indent = record.indent;
                effective.push(previous_indent);
            }
        }
        effective
    }

    fn measure_indent(line: &str) -> usize {
        line.chars()
            .take_while(|c| matches!(c, ' ' | '\t'))
            .map(|c| if c == '\t' { TAB_WIDTH } else { 1 })
            .sum()
    }
}

fn format_line(bytes: &[u8]) -> String {
    let decoded = String::from_utf8_lossy(bytes);
    if decoded.len() > MAX_LINE_LENGTH {
        take_bytes_at_char_boundary(&decoded, MAX_LINE_LENGTH).to_string()
    } else {
        decoded.into_owned()
    }
}

fn take_bytes_at_char_boundary(s: &str, limit: usize) -> &str {
    if limit >= s.len() {
        return s;
    }
    let mut i = limit;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    &s[..i]
}

fn trim_empty_lines(out: &mut VecDeque<&LineRecord>) {
    while matches!(out.front(), Some(line) if line.raw.trim().is_empty()) {
        out.pop_front();
    }
    while matches!(out.back(), Some(line) if line.raw.trim().is_empty()) {
        out.pop_back();
    }
}

fn condense_collected_lines(lines: &mut Vec<String>) {
    if looks_like_diff_lines(lines) {
        return;
    }
    const CONDENSED_THRESHOLD: usize = 50;
    const HEAD_LINES: usize = 20;
    const TAIL_LINES: usize = 10;

    // If under threshold, return as-is
    if lines.len() <= CONDENSED_THRESHOLD {
        return;
    }

    // Build condensed output: head + omission indicator + tail
    let head_count = HEAD_LINES.min(lines.len());
    let tail_count = TAIL_LINES.min(lines.len() - head_count);
    let omitted_count = lines.len() - head_count - tail_count;

    // Take head lines
    let mut condensed: Vec<String> = lines[..head_count].to_vec();

    // Add omission indicator
    condensed.push(format!(
        "… [+{} lines omitted; use read_file with offset/limit (1-indexed line numbers) for full content]",
        omitted_count
    ));

    // Add tail lines
    let tail_start = lines.len() - tail_count;
    condensed.extend_from_slice(&lines[tail_start..]);

    // Replace original with condensed
    *lines = condensed;
}

/// Condense lines for batch mode with stricter threshold.
/// Returns (was_condensed, omitted_count).
fn condense_for_batch(lines: &mut Vec<String>) -> (bool, usize) {
    if looks_like_diff_lines(lines) {
        return (false, 0);
    }
    const HEAD_LINES: usize = 15;
    const TAIL_LINES: usize = 5;

    if lines.len() <= BATCH_CONDENSED_THRESHOLD {
        return (false, 0);
    }

    let head_count = HEAD_LINES.min(lines.len());
    let tail_count = TAIL_LINES.min(lines.len() - head_count);
    let omitted_count = lines.len() - head_count - tail_count;

    let mut condensed: Vec<String> = lines[..head_count].to_vec();
    condensed.push(format!(
        "… [+{} lines omitted; use read_file with offset/limit for full content]",
        omitted_count
    ));

    let tail_start = lines.len() - tail_count;
    condensed.extend_from_slice(&lines[tail_start..]);

    *lines = condensed;
    (true, omitted_count)
}

fn looks_like_diff_lines(lines: &[String]) -> bool {
    let joined = lines.join("\n");
    looks_like_diff_content(&joined)
}

mod defaults {
    pub fn offset() -> usize {
        1
    }

    pub fn limit() -> usize {
        2000
    }

    pub fn batch_limit() -> usize {
        500
    }

    pub fn max_concurrency() -> usize {
        super::DEFAULT_MAX_CONCURRENCY
    }

    pub fn ui_progress() -> bool {
        true
    }

    pub fn max_levels() -> usize {
        0
    }

    pub fn include_siblings() -> bool {
        false
    }

    pub fn include_header() -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::indentation::*;
    use super::slice::*;
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn reads_requested_range() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        writeln!(temp, "alpha")?;
        writeln!(temp, "beta")?;
        writeln!(temp, "gamma")?;

        let lines = read(temp.path(), 2, 2).await?;
        assert_eq!(lines, vec!["L2: beta".to_string(), "L3: gamma".to_string()]);
        Ok(())
    }

    #[tokio::test]
    async fn errors_when_offset_exceeds_length() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "only").unwrap();

        let err = read(temp.path(), 3, 1).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn reads_non_utf8_lines() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        temp.as_file_mut().write_all(b"\xff\xfe\nplain\n")?;

        let lines = read(temp.path(), 1, 2).await?;
        let expected_first = format!("L1: {}{}", '\u{FFFD}', '\u{FFFD}');
        assert_eq!(lines, vec![expected_first, "L2: plain".to_string()]);
        Ok(())
    }

    #[tokio::test]
    async fn trims_crlf_endings() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        write!(temp, "one\r\ntwo\r\n")?;

        let lines = read(temp.path(), 1, 2).await?;
        assert_eq!(lines, vec!["L1: one".to_string(), "L2: two".to_string()]);
        Ok(())
    }

    #[tokio::test]
    async fn respects_limit_even_with_more_lines() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        writeln!(temp, "first")?;
        writeln!(temp, "second")?;
        writeln!(temp, "third")?;

        let lines = read(temp.path(), 1, 2).await?;
        assert_eq!(
            lines,
            vec!["L1: first".to_string(), "L2: second".to_string()]
        );
        Ok(())
    }

    #[tokio::test]
    async fn truncates_lines_longer_than_max_length() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        let long_line = "x".repeat(MAX_LINE_LENGTH + 50);
        writeln!(temp, "{long_line}")?;

        let lines = read(temp.path(), 1, 1).await?;
        let expected = "x".repeat(MAX_LINE_LENGTH);
        assert_eq!(lines, vec![format!("L1: {expected}")]);
        Ok(())
    }

    #[tokio::test]
    async fn batch_reads_multiple_files() -> Result<()> {
        let mut temp1 = NamedTempFile::new()?;
        writeln!(temp1, "file1_line1")?;
        writeln!(temp1, "file1_line2")?;

        let mut temp2 = NamedTempFile::new()?;
        writeln!(temp2, "file2_line1")?;
        writeln!(temp2, "file2_line2")?;

        let handler = ReadFileHandler;
        let args = BatchReadArgs {
            reads: vec![
                BatchReadRequest {
                    file_path: temp1.path().to_string_lossy().to_string(),
                    range: None,
                    ranges: None,
                },
                BatchReadRequest {
                    file_path: temp2.path().to_string_lossy().to_string(),
                    range: None,
                    ranges: None,
                },
            ],
            max_concurrency: 2,
            ui_progress: false,
        };

        let result = handler.handle_batch(args).await?;
        assert_eq!(result["success"], true);
        assert_eq!(result["files_read"], 2);
        assert_eq!(result["files_succeeded"], 2);

        let content = result["content"].as_str().unwrap();
        assert!(content.contains("file1_line1"));
        assert!(content.contains("file2_line1"));
        Ok(())
    }

    #[tokio::test]
    async fn batch_reads_multiple_ranges_from_same_file() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        for i in 1..=20 {
            writeln!(temp, "line{i}")?;
        }

        let handler = ReadFileHandler;
        let args = BatchReadArgs {
            reads: vec![BatchReadRequest {
                file_path: temp.path().to_string_lossy().to_string(),
                range: None,
                ranges: Some(vec![
                    ReadRange {
                        offset: 1,
                        limit: 3,
                        mode: ReadMode::Slice,
                        indentation: None,
                    },
                    ReadRange {
                        offset: 10,
                        limit: 3,
                        mode: ReadMode::Slice,
                        indentation: None,
                    },
                ]),
            }],
            max_concurrency: 4,
            ui_progress: false,
        };

        let result = handler.handle_batch(args).await?;
        assert_eq!(result["success"], true);

        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);

        let ranges = items[0]["ranges"].as_array().unwrap();
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0]["offset"], 1);
        assert_eq!(ranges[1]["offset"], 10);
        Ok(())
    }

    #[tokio::test]
    async fn batch_handles_missing_file_gracefully() -> Result<()> {
        let handler = ReadFileHandler;
        let args = BatchReadArgs {
            reads: vec![BatchReadRequest {
                file_path: "/nonexistent/path/file.txt".to_string(),
                range: None,
                ranges: None,
            }],
            max_concurrency: 1,
            ui_progress: false,
        };

        let result = handler.handle_batch(args).await?;
        assert_eq!(result["success"], false);

        let items = result["items"].as_array().unwrap();
        assert!(items[0]["error"].as_str().is_some());
        Ok(())
    }

    #[test]
    fn condense_for_batch_preserves_small_outputs() {
        let mut lines: Vec<String> = (1..=20).map(|i| format!("line{i}")).collect();
        let (condensed, omitted) = condense_for_batch(&mut lines);
        assert!(!condensed);
        assert_eq!(omitted, 0);
        assert_eq!(lines.len(), 20);
    }

    #[test]
    fn condense_for_batch_condenses_large_outputs() {
        let mut lines: Vec<String> = (1..=100).map(|i| format!("line{i}")).collect();
        let (condensed, omitted) = condense_for_batch(&mut lines);
        assert!(condensed);
        assert!(omitted > 0);
        assert!(lines.len() < 100);
        assert!(lines.iter().any(|l| l.contains("omitted")));
    }

    #[test]
    fn condense_for_batch_does_not_treat_plus_minus_text_as_diff() {
        let mut lines: Vec<String> = (1..=60)
            .map(|i| {
                if i % 2 == 0 {
                    format!("+ normal status line {i}")
                } else {
                    format!("- normal status line {i}")
                }
            })
            .collect();
        let (condensed, omitted) = condense_for_batch(&mut lines);
        assert!(condensed);
        assert!(omitted > 0);
    }

    #[test]
    fn condense_for_batch_preserves_actual_diff_output() {
        let mut lines = vec![
            "diff --git a/src/main.rs b/src/main.rs".to_string(),
            "index 1111111..2222222 100644".to_string(),
            "--- a/src/main.rs".to_string(),
            "+++ b/src/main.rs".to_string(),
            "@@ -1 +1 @@".to_string(),
            "-old".to_string(),
            "+new".to_string(),
        ];
        let (condensed, omitted) = condense_for_batch(&mut lines);
        assert!(!condensed);
        assert_eq!(omitted, 0);
    }
}
