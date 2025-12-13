use std::collections::VecDeque;
use std::path::PathBuf;

use anyhow::{Result, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::tools::traits::Tool;

pub struct ReadFileHandler;

const MAX_LINE_LENGTH: usize = 500;
const TAB_WIDTH: usize = 4;
const COMMENT_PREFIXES: &[&str] = &["#", "//", "--"];

/// JSON arguments accepted by the `read_file` tool handler.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ReadFileArgs {
    /// Absolute path to the file that will be read.
    pub file_path: String,
    /// 1-indexed line number to start reading from; defaults to 1.
    #[serde(default = "defaults::offset")]
    pub offset: usize,
    /// Maximum number of lines to return; defaults to 2000.
    #[serde(default = "defaults::limit")]
    pub limit: usize,
    /// Determines whether the handler reads a simple slice or indentation-aware block.
    #[serde(default)]
    pub mode: ReadMode,
    /// Optional indentation configuration used when `mode` is `Indentation`.
    #[serde(default)]
    pub indentation: Option<IndentationArgs>,
    /// Optional token limit for response
    #[serde(default)]
    pub max_tokens: Option<usize>,
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
    #[serde(default)]
    pub anchor_line: Option<usize>,
    /// Maximum indentation depth to collect; `0` means unlimited.
    #[serde(default = "defaults::max_levels")]
    pub max_levels: usize,
    /// Whether to include sibling blocks at the same indentation level.
    #[serde(default = "defaults::include_siblings")]
    pub include_siblings: bool,
    /// Whether to include header lines above the anchor block.
    #[serde(default = "defaults::include_header")]
    pub include_header: bool,
    /// Optional hard cap on returned lines; defaults to the global `limit`.
    #[serde(default)]
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
    /// Legacy handle method for backward compatibility with file_ops.rs
    pub async fn handle(&self, args: ReadFileArgs) -> Result<String> {
        let ReadFileArgs {
            file_path,
            offset,
            limit,
            mode,
            indentation,
            max_tokens: _,
        } = args;

        anyhow::ensure!(offset > 0, "offset must be a 1-indexed line number");
        anyhow::ensure!(limit > 0, "limit must be greater than zero");

        let path = PathBuf::from(&file_path);
        anyhow::ensure!(path.is_absolute(), "file_path must be an absolute path");

        let collected = match mode {
            ReadMode::Slice => slice::read(&path, offset, limit).await?,
            ReadMode::Indentation => {
                let indentation = indentation.unwrap_or_default();
                indentation::read_block(&path, offset, limit, indentation).await?
            }
        };

        Ok(collected.join("\n"))
    }
}

#[async_trait]
impl Tool for ReadFileHandler {
    async fn execute(&self, args: Value) -> Result<Value> {
        let args: ReadFileArgs =
            serde_json::from_value(args).context("failed to parse read_file arguments")?;

        let content = self.handle(args).await?;

        Ok(json!({
            "content": content,
            "success": true
        }))
    }

    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Read file contents with optional line range and indentation-aware block selection"
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to read"
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
                }
            },
            "required": ["file_path"]
        }))
    }
}

mod slice {
    use super::*;

    pub async fn read(
        path: &std::path::Path,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<String>> {
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
            collected.push(format!("L{seen}: {formatted}"));
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
                "L{}: {}",
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
            .map(|record| format!("L{}: {}", record.number, record.display))
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

mod defaults {
    pub fn offset() -> usize {
        1
    }

    pub fn limit() -> usize {
        2000
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
}
