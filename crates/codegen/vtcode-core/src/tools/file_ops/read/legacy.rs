use super::FileOpsTool;
use super::is_image_path;
use crate::tools::error_helpers::with_file_context;
use crate::tools::types::Input;
use crate::utils::image_processing::read_image_file_any_path;
use anyhow::{Result, anyhow};
use base64::Engine;
use serde_json::{Value, json};
use std::path::Path;

impl FileOpsTool {
    pub(super) async fn read_file_legacy(
        &self,
        file_path: &Path,
        input: &Input,
    ) -> Result<(String, Value, bool)> {
        let file_metadata = with_file_context(
            tokio::fs::metadata(file_path).await,
            "read metadata for",
            file_path,
        )?;

        if !file_metadata.is_file() {
            return Err(anyhow!("Path is not a file: {}", file_path.display()));
        }

        if is_image_path(file_path) {
            let image_data = read_image_file_any_path::<&Path>(file_path).await?;
            let metadata = json!({
                "size_bytes": image_data.size,
                "content_kind": "image",
                "encoding": "base64",
                "mime_type": image_data.mime_type,
            });
            return Ok((image_data.base64_data.clone(), metadata, false));
        }

        if let Some(encoding) = input.encoding.as_deref()
            && encoding.eq_ignore_ascii_case("base64")
        {
            let bytes =
                with_file_context(tokio::fs::read(file_path).await, "read file", file_path)?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let metadata = json!({
                "size_bytes": bytes.len(),
                "size_lines": 0,
                "is_truncated": false,
                "type": "file",
                "content_kind": "binary",
                "encoding": "base64",
            });
            return Ok((encoded, metadata, false));
        }

        if input.max_tokens.is_some() || input.max_lines.is_some() || input.chunk_lines.is_some() {
            return self.read_file_chunked(file_path, input, file_metadata.len()).await;
        }

        if let Some(max_bytes) = input.max_bytes {
            let mut bytes =
                with_file_context(tokio::fs::read(file_path).await, "read file", file_path)?;
            let truncated = bytes.len() > max_bytes;
            if truncated {
                bytes.truncate(max_bytes);
            }
            let content = String::from_utf8_lossy(&bytes).into_owned();
            let metadata = json!({
                "size_bytes": file_metadata.len(),
                "size_lines": content.lines().count(),
                "is_truncated": truncated,
                "type": "file",
                "content_kind": "text",
                "encoding": "utf8",
                "applied_max_bytes": max_bytes,
            });
            return Ok((content, metadata, truncated));
        }

        // Absolute line cap: even the legacy full-read path must not dump an
        // unbounded file into context. This mirrors the cap enforced on the new
        // handler path in `ReadFileHandler::handle_detailed`.
        //
        // The file is read as a streaming split on newlines rather than loaded
        // whole: only the first `cap` lines are retained, so a multi-GB file is
        // bounded in memory (the excessive tail is scanned and discarded, never
        // materialized). `size_lines` stays accurate because every line is still
        // counted.
        let cap = crate::tools::read_limits::absolute_line_cap();
        let (capped_content, total_lines, is_truncated) = read_bounded_text(file_path, cap).await?;

        let metadata = json!({
            "size_bytes": file_metadata.len(),
            "size_lines": total_lines,
            "is_truncated": is_truncated,
            "type": "file",
            "content_kind": "text",
            "encoding": "utf8",
            "applied_max_lines": cap,
        });

        Ok((capped_content, metadata, is_truncated))
    }

    pub(super) async fn read_file_chunked(
        &self,
        file_path: &Path,
        input: &Input,
        file_size: u64,
    ) -> Result<(String, Value, bool)> {
        const TOKENS_PER_LINE: usize = 15;

        let bytes = with_file_context(tokio::fs::read(file_path).await, "read file", file_path)?;
        let content = String::from_utf8_lossy(&bytes).into_owned();
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let mut max_lines = input.max_lines.unwrap_or(total_lines);
        if let Some(max_tokens) = input.max_tokens {
            let token_limit_lines = (max_tokens / TOKENS_PER_LINE).max(1);
            max_lines = max_lines.min(token_limit_lines);
        }

        if max_lines == 0 {
            return Err(anyhow!("max_lines must be greater than 0"));
        }

        if total_lines <= max_lines {
            let metadata = json!({
                "size_bytes": file_size,
                "size_lines": total_lines,
                "is_truncated": false,
                "type": "file",
                "content_kind": "text",
                "encoding": "utf8",
                "applied_max_lines": input.max_lines,
                "applied_max_tokens": input.max_tokens,
            });
            return Ok((content, metadata, false));
        }

        let mut head_lines = input.chunk_lines.unwrap_or(max_lines / 2);
        if head_lines == 0 {
            head_lines = 1;
        }
        head_lines = head_lines.min(max_lines).min(total_lines);

        let mut tail_lines = input.chunk_lines.unwrap_or(head_lines);
        let remaining = max_lines.saturating_sub(head_lines);
        tail_lines = tail_lines.min(remaining).min(total_lines.saturating_sub(head_lines));

        let omitted = total_lines.saturating_sub(head_lines + tail_lines);
        let mut final_content = String::new();

        if head_lines > 0 {
            final_content.push_str(&lines[..head_lines].join("\n"));
        }

        if omitted > 0 {
            if !final_content.is_empty() {
                final_content.push('\n');
            }
            final_content.push_str(&format!("... {omitted} lines omitted ..."));
        }

        if tail_lines > 0 {
            if !final_content.is_empty() {
                final_content.push('\n');
            }
            let start = total_lines - tail_lines;
            final_content.push_str(&lines[start..].join("\n"));
        }

        let metadata = json!({
            "size_bytes": file_size,
            "size_lines": total_lines,
            "is_truncated": true,
            "type": "file",
            "content_kind": "text",
            "encoding": "utf8",
            "omitted_line_count": omitted,
            "applied_max_lines": input.max_lines,
            "applied_max_tokens": input.max_tokens,
            "chunk_lines": input.chunk_lines,
        });

        self.log_chunking_operation(file_path, true, Some(total_lines)).await?;

        Ok((final_content, metadata, true))
    }
}

/// Stream a file and return at most `cap` lines of text, the true total line
/// count, and whether the file exceeded the cap.
///
/// Reads line-by-line via a newline split so the file is never fully
/// materialized: only the first `cap` lines are retained; the remainder is
/// scanned and dropped. Line counting matches `str::lines` (a trailing newline
/// does not introduce an extra empty line). Invalid UTF-8 is handled lossily,
/// identical to the previous `from_utf8_lossy` behavior.
async fn read_bounded_text(file_path: &Path, cap: usize) -> Result<(String, usize, bool)> {
    use tokio::io::AsyncBufReadExt;

    let file = with_file_context(tokio::fs::File::open(file_path).await, "open file", file_path)?;
    let reader = tokio::io::BufReader::new(file);
    let mut split = reader.split(b'\n');

    let mut total_lines = 0usize;
    let mut collected: Vec<String> = Vec::with_capacity(cap);
    while let Some(chunk) = with_file_context(split.next_segment().await, "read file", file_path)? {
        total_lines += 1;
        if total_lines <= cap {
            collected.push(String::from_utf8_lossy(&chunk).into_owned());
        }
    }

    let is_truncated = total_lines > cap;
    Ok((collected.join("\n"), total_lines, is_truncated))
}
