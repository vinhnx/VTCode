use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use tokio::fs::File;
use tokio::io::BufReader;

use crate::tools::file_ops::read_bounded_line;
use crate::tools::file_ops::read_byte_range;

use crate::tools::error_helpers::deserialize_tool_args;
use crate::tools::traits::Tool;
use crate::utils::serde_helpers::{deserialize_maybe_quoted, deserialize_opt_maybe_quoted};

mod batch;
mod lines;

pub use batch::{BatchProgress, BatchReadArgs, BatchReadRequest, BatchReadResult, RangeResult};
use lines::{condense_collected_lines, format_line, range_result_from_lines, trim_empty_lines};

pub struct ReadFileHandler;

const MAX_LINE_LENGTH: usize = 500;
const TAB_WIDTH: usize = 4;
const COMMENT_PREFIXES: &[&str] = &["#", "//", "--"];
const MIN_BATCH_LIMIT: usize = 200;
const DEFAULT_MAX_CONCURRENCY: usize = 8;
const BATCH_CONDENSED_THRESHOLD: usize = 100;
const DEFAULT_BYTE_CHUNK_SIZE: usize = 8192;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReadFileOutcome {
    pub content: String,
    pub lines_read: usize,
    pub has_more: bool,
    /// True when a physical source line exceeded the bounded reader limit.
    pub line_truncated: bool,
    /// True when the requested line count exceeded the absolute cap and was clamped.
    pub capped_by_limit: bool,
    /// The line count actually applied after clamping (0 for byte-range reads).
    pub applied_limit: usize,
}

/// JSON arguments accepted by the `read_file` tool handler.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ReadFileArgs {
    /// Absolute path to the file that will be read.
    #[serde(alias = "path", alias = "filepath", alias = "target_path", alias = "file")]
    pub file_path: String,
    /// 1-indexed line number to start reading from; defaults to 1.
    #[serde(default = "defaults::offset", deserialize_with = "deserialize_maybe_quoted")]
    pub offset: usize,
    /// Maximum number of lines to return; defaults to the configured `max_read_lines` cap.
    #[serde(default = "defaults::limit", deserialize_with = "deserialize_maybe_quoted")]
    pub limit: usize,
    /// Determines whether the handler reads a simple slice or indentation-aware block.
    #[serde(default, deserialize_with = "deserialize_read_mode")]
    pub mode: ReadMode,
    /// Optional indentation configuration used when `mode` is `Indentation`.
    #[serde(default, deserialize_with = "deserialize_indentation")]
    pub indentation: Option<IndentationArgs>,
    /// Optional token limit for response
    #[serde(default, deserialize_with = "deserialize_opt_maybe_quoted")]
    pub max_tokens: Option<usize>,
    /// Whether to condense long outputs to head/tail.
    #[serde(default = "defaults::condense", deserialize_with = "deserialize_maybe_quoted")]
    pub condense: bool,
    /// Byte offset (0-indexed) to start reading from. When present, enables byte-range read mode.
    #[serde(default, deserialize_with = "deserialize_opt_maybe_quoted")]
    pub offset_bytes: Option<u64>,
    /// Number of bytes to read. Accepts alias `length`. When present with `offset_bytes`, enables byte-range read mode.
    #[serde(default, alias = "length", deserialize_with = "deserialize_opt_maybe_quoted")]
    pub page_size_bytes: Option<usize>,
}

impl ReadFileArgs {
    /// Effective line request before the absolute cap is applied.
    pub(crate) fn effective_line_limit(&self) -> usize {
        if matches!(self.mode, ReadMode::Slice) && self.max_tokens.is_none() {
            self.limit.max(MIN_BATCH_LIMIT)
        } else {
            self.limit
        }
    }
}

/// A range specification for reading.
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct ReadRange {
    /// 1-indexed line number to start reading from; defaults to 1.
    #[serde(default = "defaults::offset", deserialize_with = "deserialize_maybe_quoted")]
    pub offset: usize,
    /// Maximum number of lines to return; defaults to 500 for batch.
    #[serde(default = "defaults::batch_limit", deserialize_with = "deserialize_maybe_quoted")]
    pub limit: usize,
    /// Read mode: slice or indentation.
    #[serde(default, deserialize_with = "deserialize_read_mode")]
    pub mode: ReadMode,
    /// Indentation options when mode is indentation.
    #[serde(default, deserialize_with = "deserialize_indentation")]
    pub indentation: Option<IndentationArgs>,
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
    #[serde(default = "defaults::max_levels", deserialize_with = "deserialize_maybe_quoted")]
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

fn deserialize_read_mode<'de, D>(deserializer: D) -> Result<ReadMode, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(ReadMode::Slice),
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("slice") {
                Ok(ReadMode::Slice)
            } else if trimmed.eq_ignore_ascii_case("indentation") {
                Ok(ReadMode::Indentation)
            } else {
                Err(serde::de::Error::custom(format!("invalid read mode: {trimmed}")))
            }
        }
        other => Err(serde::de::Error::custom(format!("invalid read mode type: {other}"))),
    }
}

fn deserialize_indentation<'de, D>(deserializer: D) -> Result<Option<IndentationArgs>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(None),
        Value::Bool(true) => Ok(Some(IndentationArgs::default())),
        Value::Bool(false) => Ok(None),
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("false") {
                Ok(None)
            } else if trimmed.eq_ignore_ascii_case("true") {
                Ok(Some(IndentationArgs::default()))
            } else {
                Err(serde::de::Error::custom(format!("invalid indentation value: {trimmed}")))
            }
        }
        Value::Object(_) => {
            let args = IndentationArgs::deserialize(value).map_err(serde::de::Error::custom)?;
            Ok(Some(args))
        }
        other => Err(serde::de::Error::custom(format!("invalid indentation type: {other}"))),
    }
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
        COMMENT_PREFIXES.iter().any(|prefix| self.raw.trim().starts_with(prefix))
    }
}

impl ReadFileHandler {
    /// Execute a batch read of multiple files/ranges in parallel.
    pub async fn handle_batch(&self, args: BatchReadArgs) -> Result<Value> {
        batch::execute(self, args).await
    }

    /// Read a single range from a file.
    async fn read_range(&self, path: &Path, range: &ReadRange) -> Result<RangeResult> {
        let offset = range.offset.max(1);
        let limit = Self::effective_line_limit(range.limit);

        let (collected, line_truncated) = match range.mode {
            ReadMode::Slice => {
                let result = slice::read(path, offset, limit).await?;
                (result.lines, result.line_truncated)
            }
            ReadMode::Indentation => {
                let indentation = range.indentation.clone().unwrap_or_default();
                let result = indentation::read_block(path, offset, limit, indentation).await?;
                (result.lines, result.line_truncated)
            }
        };

        Ok(range_result_from_lines(offset, collected, line_truncated))
    }

    /// Clamp a requested line count to the absolute per-call ceiling.
    ///
    /// Returns `(applied_limit, capped_by_limit)`. `capped_by_limit` is true
    /// only when the caller asked for strictly more lines than `absolute_max`
    /// permits, so a read whose `requested == absolute_max` is *not* flagged as
    /// clamped (it merely sits at the ceiling). This keeps the pagination chain
    /// alive on every page that reaches the ceiling without misreporting a
    /// clamp on the exact-cap follow-up pages.
    fn clamp_to_absolute_cap(requested: usize, absolute_max: usize) -> (usize, bool) {
        let capped = requested > absolute_max;
        (requested.min(absolute_max), capped)
    }

    fn effective_line_limit(requested: usize) -> usize {
        let absolute_max = crate::tools::read_limits::absolute_line_cap();
        Self::clamp_to_absolute_cap(requested.max(1), absolute_max).0
    }

    pub(crate) async fn handle_detailed(&self, args: ReadFileArgs) -> Result<ReadFileOutcome> {
        let effective_limit = args.effective_line_limit();
        let ReadFileArgs {
            file_path,
            offset,
            limit,
            mode,
            indentation,
            max_tokens: _,
            condense,
            offset_bytes,
            page_size_bytes,
        } = args;

        let path = PathBuf::from(&file_path);
        anyhow::ensure!(path.is_absolute(), "file_path must be an absolute path");

        // Byte-range read path: when offset_bytes or page_size_bytes is present
        if offset_bytes.is_some() || page_size_bytes.is_some() {
            let byte_offset = offset_bytes.unwrap_or(0);
            let byte_length = page_size_bytes.unwrap_or(DEFAULT_BYTE_CHUNK_SIZE);
            anyhow::ensure!(byte_length > 0, "page_size_bytes must be greater than 0");
            return self.read_byte_range(&path, byte_offset, byte_length).await;
        }

        anyhow::ensure!(offset > 0, "offset must be a 1-indexed line number");
        anyhow::ensure!(limit > 0, "limit must be greater than zero");

        let absolute_max = crate::tools::read_limits::absolute_line_cap();

        // Absolute hard cap: no single line-based read may return more than
        // `absolute_max` lines, even when the caller requests a larger `limit`.
        // Byte-range reads are unaffected (they use `page_size_bytes`).
        let (applied_limit, capped_by_limit) = Self::clamp_to_absolute_cap(effective_limit, absolute_max);

        let (mut collected, has_more, line_truncated) = match mode {
            ReadMode::Slice => {
                let result = slice::read(&path, offset, applied_limit).await?;
                (result.lines, result.has_more, result.line_truncated)
            }
            ReadMode::Indentation => {
                let indentation = indentation.unwrap_or_default();
                let result = indentation::read_block(&path, offset, applied_limit, indentation).await?;
                (result.lines, false, result.line_truncated)
            }
        };
        let lines_read = collected.len();

        if condense {
            // Skip condensation for paginated reads (caller advanced past
            // line 1). When the caller passes an explicit offset > 1, they are
            // following an omit-hint to retrieve a specific slice. Re-condensing
            // that slice would produce another omit-hint, trapping the agent
            // in an infinite pagination regress (checkpoint turn_613).
            if offset <= 1 {
                condense_collected_lines(&mut collected);
            }
        }

        Ok(ReadFileOutcome {
            content: collected.join("\n"),
            lines_read,
            has_more,
            line_truncated,
            capped_by_limit,
            applied_limit,
        })
    }

    /// Read a byte range from a file using seek-based access.
    ///
    /// Delegates to the shared `read_byte_range` function in `file_ops`.
    /// Returns content with line numbers prefixed (e.g., "42: content").
    async fn read_byte_range(
        &self,
        file_path: &Path,
        offset_bytes: u64,
        page_size_bytes: usize,
    ) -> Result<ReadFileOutcome> {
        let result = read_byte_range(file_path, offset_bytes, page_size_bytes, true).await?;
        Ok(ReadFileOutcome {
            content: result.content,
            lines_read: result.lines_read,
            has_more: result.has_more,
            line_truncated: false,
            capped_by_limit: false,
            applied_limit: 0,
        })
    }

    /// Legacy handle method for backward compatibility with file_ops.rs
    pub async fn handle(&self, args: ReadFileArgs) -> Result<String> {
        Ok(self.handle_detailed(args).await?.content)
    }
}

#[async_trait]
impl Tool for ReadFileHandler {
    async fn execute(&self, args: Value) -> Result<Value> {
        // Try batch mode first (has "reads" field)
        if args.get("reads").is_some() {
            let batch_args: BatchReadArgs = deserialize_tool_args(&args, "read_file")?;
            return self.handle_batch(batch_args).await;
        }

        // Legacy single-file mode
        let args: ReadFileArgs = deserialize_tool_args(&args, "read_file")?;

        let file_path = args.file_path.clone();
        let outcome = self.handle_detailed(args).await?;

        let mut response = json!({
            "content": outcome.content,
            "file_path": file_path,
            "path": file_path,
            "success": true,
            "no_spool": true
        });

        if outcome.has_more {
            response["has_more"] = json!(true);
        }
        if outcome.line_truncated {
            response["line_truncated"] = json!(true);
            response["truncated"] = json!(true);
            response["truncation_reason"] = json!("oversized_line");
        }

        Ok(response)
    }

    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
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
                    "description": "Maximum lines to return. Requests above the configurable absolute cap (`max_read_lines`) are clamped, and the response carries `next_read_args` to continue reading.",
                    "default": crate::tools::read_limits::read_limit_lines(),
                    "minimum": 1
                },
                "mode": {
                    "type": "string",
                    "enum": ["slice", "indentation"],
                    "description": "Read mode: slice for simple range, indentation for block",
                    "default": "slice"
                },
                "indentation": {
                    "description": "Indentation settings when mode=indentation",
                    "anyOf": [
                        {"type": "boolean"},
                        {
                            "type": "object",
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
                        }
                    ]
                },
                "max_tokens": {
                    "type": "integer",
                    "description": "Optional token limit for response (approximate)"
                },
                "condense": {
                    "type": "boolean",
                    "description": "Condense long outputs to head/tail (default: true)",
                    "default": true
                },
                "offset_bytes": {
                    "type": "integer",
                    "description": "Byte offset (0-indexed) to start reading from. Enables byte-range read mode.",
                    "minimum": 0
                },
                "page_size_bytes": {
                    "type": "integer",
                    "description": "Number of bytes to read. Accepts alias `length`. Default: 8192.",
                    "alias": "length",
                    "minimum": 1
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

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(super) struct SliceReadResult {
        pub lines: Vec<String>,
        pub has_more: bool,
        pub line_truncated: bool,
    }

    pub(super) struct SliceReadRanges {
        pub results: Vec<Option<Result<SliceReadResult>>>,
        pub error: Option<anyhow::Error>,
    }

    pub async fn read(path: &Path, offset: usize, limit: usize) -> Result<SliceReadResult> {
        let file = File::open(path)
            .await
            .context(format!("failed to open file: {}", path.display()))?;

        let mut reader = BufReader::new(file);
        let mut collected = Vec::new();
        let mut seen = 0usize;
        let mut buffer = Vec::new();
        let mut reached_eof = false;
        let mut line_truncated = false;

        loop {
            let Some((line, truncated)) = read_formatted_line(&mut reader, &mut buffer).await? else {
                reached_eof = true;
                break;
            };
            line_truncated |= truncated;

            seen = seen.saturating_add(1);

            if seen < offset {
                continue;
            }

            if collected.len() >= limit {
                break;
            }

            collected.push(line);
        }

        // A successful empty read is necessary to inspect an existing empty
        // file before its first edit. Higher offsets remain out of bounds.
        if seen < offset && !(seen == 0 && offset == 1) {
            anyhow::bail!("offset exceeds file length");
        }

        Ok(SliceReadResult {
            lines: collected,
            has_more: !reached_eof,
            line_truncated,
        })
    }

    pub(super) async fn read_ranges(path: &Path, ranges: &[ReadRange]) -> Result<SliceReadRanges> {
        let file = File::open(path)
            .await
            .context(format!("failed to open file: {}", path.display()))?;

        let normalized_ranges: Vec<(usize, usize)> = ranges
            .iter()
            .map(|range| {
                let offset = range.offset.max(1);
                let limit = ReadFileHandler::effective_line_limit(range.limit);
                (offset, offset.saturating_add(limit.saturating_sub(1)))
            })
            .collect();
        let max_line = normalized_ranges.iter().map(|(_, end)| *end).max().unwrap_or(0);
        let mut collected: Vec<Vec<String>> = normalized_ranges.iter().map(|_| Vec::new()).collect();
        let mut reader = BufReader::new(file);
        let mut buffer = Vec::new();
        let mut line_number = 0usize;
        let mut has_lookahead = false;
        let mut line_truncated = false;
        let mut scan_error = None;

        while line_number < max_line {
            match read_formatted_line(&mut reader, &mut buffer).await {
                Ok(Some((line, truncated))) => {
                    line_truncated |= truncated;
                    line_number += 1;
                    for (index, (offset, end)) in normalized_ranges.iter().enumerate() {
                        if (*offset..=*end).contains(&line_number) {
                            collected[index].push(line.clone());
                        }
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    scan_error = Some(error);
                    break;
                }
            }
        }

        if scan_error.is_none() && line_number == max_line {
            match read_formatted_line(&mut reader, &mut buffer).await {
                Ok(Some((_, truncated))) => {
                    line_truncated |= truncated;
                    has_lookahead = true;
                }
                Ok(None) => {}
                Err(error) => scan_error = Some(error),
            }
        }

        let results = normalized_ranges
            .into_iter()
            .zip(collected)
            .map(|((offset, end), lines)| {
                if line_number < offset && !(line_number == 0 && offset == 1) {
                    scan_error
                        .is_none()
                        .then_some(Err(anyhow::anyhow!("offset exceeds file length")))
                } else if scan_error.is_some() && line_number < end {
                    None
                } else {
                    Some(Ok(SliceReadResult {
                        lines,
                        has_more: line_number > end || (line_number == end && has_lookahead),
                        line_truncated,
                    }))
                }
            })
            .collect();

        Ok(SliceReadRanges { results, error: scan_error })
    }
    async fn read_formatted_line(reader: &mut BufReader<File>, buffer: &mut Vec<u8>) -> Result<Option<(String, bool)>> {
        let Some(truncated) = read_bounded_line(reader, buffer).await.context("failed to read file")? else {
            return Ok(None);
        };

        if buffer.last() == Some(&b'\n') {
            buffer.pop();
            if buffer.last() == Some(&b'\r') {
                buffer.pop();
            }
        }

        Ok(Some((format_line(buffer), truncated)))
    }
}

mod indentation {
    use super::*;

    pub(super) struct IndentationReadResult {
        pub lines: Vec<String>,
        pub line_truncated: bool,
    }

    pub async fn read_block(
        path: &Path,
        offset: usize,
        limit: usize,
        options: IndentationArgs,
    ) -> Result<IndentationReadResult> {
        let anchor_line = options.anchor_line.unwrap_or(offset);
        anyhow::ensure!(anchor_line > 0, "anchor_line must be a 1-indexed line number");

        let guard_limit = options.max_lines.unwrap_or(limit);
        anyhow::ensure!(guard_limit > 0, "max_lines must be greater than zero");

        // Bound the collection to the window the block expansion can ever use:
        // the anchor plus the downward guard limit. The algorithm expands at
        // most `final_limit` lines down from the anchor and needs the full
        // prefix above it, so `anchor + guard_limit` is always sufficient and
        // prevents an unbounded full-file read of gigantic files.
        let collect_cap = anchor_line.saturating_add(guard_limit);
        let (collected, line_truncated) = collect_file_lines(path, collect_cap).await?;
        anyhow::ensure!(!collected.is_empty() && anchor_line <= collected.len(), "anchor_line exceeds file length");

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
            return Ok(IndentationReadResult {
                lines: vec![format!(
                    "{}: {}",
                    collected[anchor_index].number, collected[anchor_index].display
                )],
                line_truncated,
            });
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
                #[allow(
                    clippy::cast_sign_loss,
                    reason = "Intentional compatibility, platform, or test-only suppression."
                )]
                let iu = i as usize;
                if effective_indents[iu] >= min_indent {
                    out.push_front(&collected[iu]);
                    progressed += 1;
                    i -= 1;

                    // Control sibling inclusion
                    if effective_indents[iu] == min_indent && !options.include_siblings {
                        let allow_header_comment = options.include_header && collected[iu].is_comment();
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

        Ok(IndentationReadResult {
            lines: out
                .into_iter()
                .map(|record| format!("{}: {}", record.number, record.display))
                .collect(),
            line_truncated,
        })
    }

    async fn collect_file_lines(path: &Path, max_lines: usize) -> Result<(Vec<LineRecord>, bool)> {
        let file = File::open(path)
            .await
            .context(format!("failed to open file: {}", path.display()))?;

        let mut reader = BufReader::new(file);
        let mut buffer = Vec::new();
        let mut lines = Vec::new();
        let mut number = 0usize;
        let mut line_truncated = false;

        while lines.len() < max_lines {
            let Some(truncated) = read_bounded_line(&mut reader, &mut buffer)
                .await
                .context("failed to read file")?
            else {
                break;
            };
            line_truncated |= truncated;

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
            lines.push(LineRecord { number, raw, display, indent });
        }

        Ok((lines, line_truncated))
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

mod defaults {
    pub fn offset() -> usize {
        1
    }

    pub fn limit() -> usize {
        crate::tools::read_limits::read_limit_lines()
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

    pub fn condense() -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    #[expect(
        unused_imports,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    use super::indentation::*;
    use super::lines::condense_for_batch;
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

        let lines = read(temp.path(), 2, 2).await?.lines;
        assert_eq!(lines, vec!["beta".to_string(), "gamma".to_string()]);
        Ok(())
    }

    #[test]
    fn read_file_args_accepts_boolean_indentation() {
        let args = json!({
            "file_path": "/tmp/example.txt",
            "mode": "slice",
            "indentation": false
        });

        let parsed: ReadFileArgs = serde_json::from_value(args).unwrap();
        assert!(matches!(parsed.mode, ReadMode::Slice));
        assert!(parsed.indentation.is_none());
    }

    #[test]
    fn read_file_args_accepts_true_indentation() {
        let args = json!({
            "file_path": "/tmp/example.txt",
            "mode": "indentation",
            "indentation": true
        });

        let parsed: ReadFileArgs = serde_json::from_value(args).unwrap();
        assert!(matches!(parsed.mode, ReadMode::Indentation));
        assert!(parsed.indentation.is_some());
    }

    #[test]
    fn read_file_args_accepts_empty_mode() {
        let args = json!({
            "file_path": "/tmp/example.txt",
            "mode": ""
        });

        let parsed: ReadFileArgs = serde_json::from_value(args).unwrap();
        assert!(matches!(parsed.mode, ReadMode::Slice));
    }

    #[tokio::test]
    async fn read_file_handler_skips_condense_when_disabled() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        for idx in 0..60 {
            writeln!(temp, "line-{idx}")?;
        }

        let args = ReadFileArgs {
            file_path: temp.path().to_string_lossy().to_string(),
            offset: 1,
            limit: 2000,
            mode: ReadMode::Slice,
            indentation: None,
            max_tokens: None,
            condense: false,
            offset_bytes: None,
            page_size_bytes: None,
        };
        let handler = ReadFileHandler;
        let content = handler.handle(args).await?;

        assert!(!content.contains("lines omitted"));
        assert_eq!(content.lines().count(), 60);
        Ok(())
    }

    #[tokio::test]
    async fn read_file_handler_skips_condense_for_paginated_read() -> Result<()> {
        // Regression: when the caller follows an omit-hint and reads a
        // specific slice (offset > 1), the handler must NOT re-condense the
        // result. Re-condensing would emit another omit-hint and trap the
        // agent in an infinite pagination regress (checkpoint turn_613).
        let mut temp = NamedTempFile::new()?;
        for idx in 1..=300 {
            writeln!(temp, "line-{idx}")?;
        }

        let args = ReadFileArgs {
            file_path: temp.path().to_string_lossy().to_string(),
            offset: 81,
            limit: 229,
            mode: ReadMode::Slice,
            indentation: None,
            max_tokens: None,
            condense: true,
            offset_bytes: None,
            page_size_bytes: None,
        };
        let handler = ReadFileHandler;
        let content = handler.handle(args).await?;

        // No re-condensation: the requested slice is returned verbatim.
        assert!(!content.contains("lines omitted"), "paginated read must not be re-condensed; got: {content}");
        // The slice starts at line 81 (1-indexed), so line-81 must be present.
        assert!(
            content.contains("line-81"),
            "paginated read must include the requested offset; got first line: {:?}",
            content.lines().next()
        );
        Ok(())
    }

    #[tokio::test]
    async fn read_file_handler_still_condenses_default_overview_read() -> Result<()> {
        // The default overview path (offset == 1) must still condense large
        // outputs so the catalog stays bounded. This guards against the fix
        // over-correcting and never condensing.
        let mut temp = NamedTempFile::new()?;
        for idx in 1..=300 {
            writeln!(temp, "line-{idx}")?;
        }

        let args = ReadFileArgs {
            file_path: temp.path().to_string_lossy().to_string(),
            offset: 1,
            limit: 2000,
            mode: ReadMode::Slice,
            indentation: None,
            max_tokens: None,
            condense: true,
            offset_bytes: None,
            page_size_bytes: None,
        };
        let handler = ReadFileHandler;
        let content = handler.handle(args).await?;

        assert!(
            content.contains("lines omitted"),
            "default overview read must condense large output; got {} lines",
            content.lines().count()
        );
        Ok(())
    }

    #[tokio::test]
    async fn empty_file_read_succeeds_at_start_and_rejects_higher_offsets() -> Result<()> {
        let temp = NamedTempFile::new()?;
        let result = read(temp.path(), 1, 2).await?;
        assert!(result.lines.is_empty());
        assert!(!result.has_more);
        assert!(read(temp.path(), 2, 2).await.is_err());
        let ranges = vec![
            ReadRange {
                offset: 1,
                limit: 2,
                mode: ReadMode::Slice,
                indentation: None,
            },
            ReadRange {
                offset: 2,
                limit: 2,
                mode: ReadMode::Slice,
                indentation: None,
            },
        ];
        let result = read_ranges(temp.path(), &ranges).await?;
        assert!(result.results[0].as_ref().unwrap().as_ref().unwrap().lines.is_empty());
        assert!(result.results[1].as_ref().unwrap().is_err());
        Ok(())
    }

    #[tokio::test]
    async fn errors_when_offset_exceeds_length() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "only").unwrap();

        let err = read(temp.path(), 3, 1).await;
        err.unwrap_err();
    }

    #[tokio::test]
    async fn reads_non_utf8_lines() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        temp.as_file_mut().write_all(b"\xff\xfe\nplain\n")?;

        let lines = read(temp.path(), 1, 2).await?.lines;
        let expected_first = format!("{}{}", '\u{FFFD}', '\u{FFFD}');
        assert_eq!(lines, vec![expected_first, "plain".to_string()]);
        Ok(())
    }

    #[tokio::test]
    async fn reports_oversized_physical_lines_as_truncated() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        let long_line = "x".repeat(crate::tools::file_ops::MAX_LINE_READ_BYTES + 1);
        writeln!(temp, "{long_line}")?;
        writeln!(temp, "plain")?;

        let outcome = ReadFileHandler
            .handle_detailed(ReadFileArgs {
                file_path: temp.path().to_string_lossy().into_owned(),
                offset: 1,
                limit: 2,
                mode: ReadMode::Slice,
                indentation: None,
                max_tokens: None,
                condense: false,
                offset_bytes: None,
                page_size_bytes: None,
            })
            .await?;

        assert!(outcome.line_truncated);
        assert!(outcome.content.contains("plain"));
        Ok(())
    }

    #[tokio::test]
    async fn trims_crlf_endings() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        write!(temp, "one\r\ntwo\r\n")?;

        let lines = read(temp.path(), 1, 2).await?.lines;
        assert_eq!(lines, vec!["one".to_string(), "two".to_string()]);
        Ok(())
    }

    #[tokio::test]
    async fn respects_limit_even_with_more_lines() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        writeln!(temp, "first")?;
        writeln!(temp, "second")?;
        writeln!(temp, "third")?;

        let result = read(temp.path(), 1, 2).await?;
        assert_eq!(result.lines, vec!["first".to_string(), "second".to_string()]);
        assert!(result.has_more);
        Ok(())
    }

    #[tokio::test]
    async fn reads_exact_limit_without_continuation_at_eof() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        writeln!(temp, "first")?;
        writeln!(temp, "second")?;

        let result = read(temp.path(), 1, 2).await?;
        assert_eq!(result.lines, vec!["first".to_string(), "second".to_string()]);
        assert!(!result.has_more);
        Ok(())
    }

    #[test]
    fn clamp_to_absolute_cap_marks_only_true_overages() {
        // A request above the ceiling is clamped and flagged.
        assert_eq!(ReadFileHandler::clamp_to_absolute_cap(1000, 400), (400, true));
        // A request exactly at the ceiling sits at the cap but is not "clamped"
        // (this is what keeps the pagination chain alive on follow-up pages).
        assert_eq!(ReadFileHandler::clamp_to_absolute_cap(400, 400), (400, false));
        // A request below the ceiling is unchanged and not flagged.
        assert_eq!(ReadFileHandler::clamp_to_absolute_cap(50, 400), (50, false));
    }

    #[tokio::test]
    async fn absolute_cap_clamps_large_request() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        for idx in 1..=1000 {
            writeln!(temp, "line-{idx}")?;
        }

        let handler = ReadFileHandler;
        let args = ReadFileArgs {
            file_path: temp.path().to_string_lossy().to_string(),
            offset: 1,
            limit: 2000,
            mode: ReadMode::Slice,
            indentation: None,
            max_tokens: None,
            condense: false,
            offset_bytes: None,
            page_size_bytes: None,
        };
        let outcome = handler.handle_detailed(args).await?;

        let cap = crate::tools::read_limits::read_limit_lines();
        assert!(outcome.capped_by_limit, "request larger than the cap must be clamped");
        assert_eq!(outcome.applied_limit, cap);
        assert!(outcome.has_more);
        assert!(outcome.lines_read <= cap);
        Ok(())
    }

    #[tokio::test]
    async fn explicit_limit_under_cap_is_preserved() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        for idx in 1..=100 {
            writeln!(temp, "line-{idx}")?;
        }

        let handler = ReadFileHandler;
        let args = ReadFileArgs {
            file_path: temp.path().to_string_lossy().to_string(),
            offset: 1,
            limit: 50,
            mode: ReadMode::Slice,
            indentation: None,
            max_tokens: None,
            condense: false,
            offset_bytes: None,
            page_size_bytes: None,
        };
        let outcome = handler.handle_detailed(args).await?;

        assert!(!outcome.capped_by_limit);
        // Small limits are expanded to MIN_BATCH_LIMIT (200) by the batch path,
        // so the whole 100-line file is returned, but it is not cap-clamped.
        assert_eq!(outcome.lines_read, 100);
        Ok(())
    }

    #[tokio::test]
    async fn truncates_lines_longer_than_max_length() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        let long_line = "x".repeat(MAX_LINE_LENGTH + 50);
        writeln!(temp, "{long_line}")?;

        let lines = read(temp.path(), 1, 1).await?.lines;
        let expected = "x".repeat(MAX_LINE_LENGTH);
        assert_eq!(lines, vec![expected]);
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
    async fn batch_rejects_zero_max_concurrency() -> Result<()> {
        let handler = ReadFileHandler;
        let args = BatchReadArgs {
            reads: vec![BatchReadRequest {
                file_path: "/tmp/example.txt".to_string(),
                range: None,
                ranges: None,
            }],
            max_concurrency: 0,
            ui_progress: false,
        };

        let error = handler.handle_batch(args).await.expect_err("zero must be rejected");
        assert!(error.to_string().contains("max_concurrency must be greater than zero"));
        Ok(())
    }

    #[tokio::test]
    async fn batch_caps_excessive_max_concurrency() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        writeln!(temp, "line")?;

        let handler = ReadFileHandler;
        let args = BatchReadArgs {
            reads: vec![BatchReadRequest {
                file_path: temp.path().to_string_lossy().to_string(),
                range: None,
                ranges: None,
            }],
            max_concurrency: usize::MAX,
            ui_progress: false,
        };

        let result = handler.handle_batch(args).await?;
        assert_eq!(result["success"], true);
        assert_eq!(result["files_succeeded"], 1);
        Ok(())
    }

    #[tokio::test]
    async fn batch_preserves_input_order_with_duplicate_requests() -> Result<()> {
        let mut first = NamedTempFile::new()?;
        writeln!(first, "first")?;
        let mut second = NamedTempFile::new()?;
        writeln!(second, "second")?;

        let first_path = first.path().to_string_lossy().to_string();
        let second_path = second.path().to_string_lossy().to_string();
        let handler = ReadFileHandler;
        let args = BatchReadArgs {
            reads: vec![
                BatchReadRequest {
                    file_path: second_path.clone(),
                    range: None,
                    ranges: None,
                },
                BatchReadRequest {
                    file_path: first_path.clone(),
                    range: None,
                    ranges: None,
                },
                BatchReadRequest { file_path: second_path, range: None, ranges: None },
            ],
            max_concurrency: usize::MAX,
            ui_progress: false,
        };

        let result = handler.handle_batch(args).await?;
        let items = result["items"].as_array().context("batch items must be an array")?;
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["ranges"][0]["content"], "second");
        assert_eq!(items[1]["ranges"][0]["content"], "first");
        assert_eq!(items[2]["ranges"][0]["content"], "second");
        let content = result["content"].as_str().context("batch content must be a string")?;
        assert!(content.find("second") < content.find("first"));
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
        assert_eq!(ranges[0]["content"], "line1\nline2\nline3");
        assert_eq!(ranges[1]["content"], "line10\nline11\nline12");
        Ok(())
    }

    #[tokio::test]
    async fn batch_range_is_capped_by_absolute_line_limit() -> Result<()> {
        let cap = crate::tools::read_limits::absolute_line_cap();
        let mut temp = NamedTempFile::new()?;
        for i in 1..=cap + 1 {
            writeln!(temp, "line{i}")?;
        }

        let handler = ReadFileHandler;
        let result = handler
            .handle_batch(BatchReadArgs {
                reads: vec![BatchReadRequest {
                    file_path: temp.path().to_string_lossy().to_string(),
                    range: Some(ReadRange {
                        offset: 1,
                        limit: cap + 1,
                        mode: ReadMode::Slice,
                        indentation: None,
                    }),
                    ranges: None,
                }],
                max_concurrency: 1,
                ui_progress: false,
            })
            .await?;

        let range = &result["items"][0]["ranges"][0];
        assert_eq!(range["lines_read"], cap);

        let content = range["content"].as_str().context("batch range content must be a string")?;
        assert!(content.ends_with(&format!("line{cap}")));
        assert!(!content.contains(&format!("line{}", cap + 1)));
        Ok(())
    }

    #[tokio::test]
    async fn batch_preserves_completed_ranges_before_a_later_error() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        writeln!(temp, "line1")?;
        writeln!(temp, "line2")?;

        let handler = ReadFileHandler;
        let args = BatchReadArgs {
            reads: vec![BatchReadRequest {
                file_path: temp.path().to_string_lossy().to_string(),
                range: None,
                ranges: Some(vec![
                    ReadRange {
                        offset: 1,
                        limit: 1,
                        mode: ReadMode::Slice,
                        indentation: None,
                    },
                    ReadRange {
                        offset: 10,
                        limit: 1,
                        mode: ReadMode::Slice,
                        indentation: None,
                    },
                ]),
            }],
            max_concurrency: 1,
            ui_progress: false,
        };

        let result = handler.handle_batch(args).await?;
        assert_eq!(result["success"], false);
        assert_eq!(result["items"][0]["ranges"].as_array().map(Vec::len), Some(1));
        assert_eq!(result["items"][0]["ranges"][0]["content"], "line1");
        assert!(result["items"][0]["error"].as_str().is_some());
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
        let mut lines: Vec<String> = (1..=200).map(|i| format!("line{i}")).collect();
        let (condensed, omitted) = condense_for_batch(&mut lines);
        assert!(condensed);
        assert!(omitted > 0);
        assert!(lines.len() < 200);
        assert!(lines.iter().any(|l| l.contains("omitted")));
    }

    #[test]
    fn condense_for_batch_does_not_treat_plus_minus_text_as_diff() {
        let mut lines: Vec<String> = (1..=150)
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

    // --- Byte-range read tests ---

    #[tokio::test]
    async fn byte_range_reads_first_bytes() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        temp.as_file_mut().write_all(b"Hello, World!")?;

        let handler = ReadFileHandler;
        let args = ReadFileArgs {
            file_path: temp.path().to_string_lossy().to_string(),
            offset: 1,
            limit: 2000,
            mode: ReadMode::Slice,
            indentation: None,
            max_tokens: None,
            condense: false,
            offset_bytes: Some(0),
            page_size_bytes: Some(5),
        };
        let outcome = handler.handle_detailed(args).await?;
        // Content is line-numbered: "1: Hello"
        assert_eq!(outcome.content, "1: Hello");
        assert!(outcome.has_more);
        Ok(())
    }

    #[tokio::test]
    async fn byte_range_reads_with_offset() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        writeln!(temp, "Hello, World!")?;
        writeln!(temp, "Second line")?;

        let handler = ReadFileHandler;
        // Seek past "Hello, World!\n" (14 bytes) to start of second line
        let args = ReadFileArgs {
            file_path: temp.path().to_string_lossy().to_string(),
            offset: 1,
            limit: 2000,
            mode: ReadMode::Slice,
            indentation: None,
            max_tokens: None,
            condense: false,
            offset_bytes: Some(14),
            page_size_bytes: Some(50),
        };
        let outcome = handler.handle_detailed(args).await?;
        // Should start at line 2
        assert!(outcome.content.contains("2: Second line"));
        assert!(!outcome.has_more);
        Ok(())
    }

    #[tokio::test]
    async fn byte_range_beyond_eof_returns_empty() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        temp.as_file_mut().write_all(b"short")?;

        let handler = ReadFileHandler;
        let args = ReadFileArgs {
            file_path: temp.path().to_string_lossy().to_string(),
            offset: 1,
            limit: 2000,
            mode: ReadMode::Slice,
            indentation: None,
            max_tokens: None,
            condense: false,
            offset_bytes: Some(100),
            page_size_bytes: Some(10),
        };
        let outcome = handler.handle_detailed(args).await?;
        assert_eq!(outcome.content, "");
        assert!(!outcome.has_more);
        Ok(())
    }

    #[tokio::test]
    async fn byte_range_clamps_to_file_size() -> Result<()> {
        // Multi-line file so we can test partial-line skipping
        let mut temp = NamedTempFile::new()?;
        writeln!(temp, "first line")?;
        writeln!(temp, "second line")?;
        writeln!(temp, "third line")?;

        let handler = ReadFileHandler;
        let args = ReadFileArgs {
            file_path: temp.path().to_string_lossy().to_string(),
            offset: 1,
            limit: 2000,
            mode: ReadMode::Slice,
            indentation: None,
            max_tokens: None,
            condense: false,
            offset_bytes: Some(0),
            page_size_bytes: Some(100), // Request more than available
        };
        let outcome = handler.handle_detailed(args).await?;
        // Should have line numbers for all three lines
        assert!(outcome.content.contains("1: first line"));
        assert!(outcome.content.contains("2: second line"));
        assert!(outcome.content.contains("3: third line"));
        assert!(!outcome.has_more);
        Ok(())
    }

    #[tokio::test]
    async fn byte_range_reads_non_utf8_lossy() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        temp.as_file_mut().write_all(b"\xff\xfeplain")?;

        let handler = ReadFileHandler;
        let args = ReadFileArgs {
            file_path: temp.path().to_string_lossy().to_string(),
            offset: 1,
            limit: 2000,
            mode: ReadMode::Slice,
            indentation: None,
            max_tokens: None,
            condense: false,
            offset_bytes: Some(0),
            page_size_bytes: Some(7), // Read all 7 bytes
        };
        let outcome = handler.handle_detailed(args).await?;
        // Lossy conversion replaces invalid UTF-8 with U+FFFD
        assert!(outcome.content.contains('\u{FFFD}'));
        assert!(outcome.content.contains("plain"));
        Ok(())
    }

    #[tokio::test]
    async fn byte_range_has_more_when_not_at_eof() -> Result<()> {
        let mut temp = NamedTempFile::new()?;
        writeln!(temp, "line one")?;
        writeln!(temp, "line two")?;
        writeln!(temp, "line three")?;

        let handler = ReadFileHandler;
        // Read only first 10 bytes - should get first line only
        let args = ReadFileArgs {
            file_path: temp.path().to_string_lossy().to_string(),
            offset: 1,
            limit: 2000,
            mode: ReadMode::Slice,
            indentation: None,
            max_tokens: None,
            condense: false,
            offset_bytes: Some(0),
            page_size_bytes: Some(10),
        };
        let outcome = handler.handle_detailed(args).await?;
        // Should have line number
        assert!(outcome.content.contains("1: line one"));
        assert!(outcome.has_more);
        Ok(())
    }

    #[tokio::test]
    async fn byte_range_defaults_to_8192_when_length_omitted() -> Result<()> {
        // Create a file large enough that 8192 bytes doesn't cover everything
        let mut temp = NamedTempFile::new()?;
        for i in 1..=500 {
            writeln!(temp, "line-{i}: {}", "x".repeat(20))?;
        }

        let handler = ReadFileHandler;
        let args = ReadFileArgs {
            file_path: temp.path().to_string_lossy().to_string(),
            offset: 1,
            limit: 2000,
            mode: ReadMode::Slice,
            indentation: None,
            max_tokens: None,
            condense: false,
            offset_bytes: Some(0),
            page_size_bytes: None, // Should default to 8192
        };
        let outcome = handler.handle_detailed(args).await?;
        // Content should have line numbers
        assert!(outcome.content.contains("1: line-1"));
        assert!(outcome.has_more);
        Ok(())
    }

    #[test]
    fn byte_range_args_from_json() {
        let args = json!({
            "file_path": "/tmp/test.bin",
            "offset_bytes": 1024,
            "page_size_bytes": 4096
        });
        let parsed: ReadFileArgs = serde_json::from_value(args).unwrap();
        assert_eq!(parsed.offset_bytes, Some(1024));
        assert_eq!(parsed.page_size_bytes, Some(4096));
    }

    #[test]
    fn byte_range_args_length_alias() {
        let args = json!({
            "file_path": "/tmp/test.bin",
            "offset_bytes": 0,
            "length": 2048
        });
        let parsed: ReadFileArgs = serde_json::from_value(args).unwrap();
        assert_eq!(parsed.offset_bytes, Some(0));
        assert_eq!(parsed.page_size_bytes, Some(2048));
    }
}
