use super::FileOpsTool;
use super::is_image_path;
use crate::tools::error_helpers::with_file_context;
use crate::tools::types::Input;
use crate::utils::image_processing::{
    UNSUPPORTED_BINARY_IMAGE_EXTENSIONS, has_supported_image_extension, read_image_file_any_path,
};
use anyhow::{Result, anyhow};
use base64::Engine;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::Path;

impl FileOpsTool {
    pub(super) async fn read_file_legacy(&self, file_path: &Path, input: &Input) -> Result<(String, Value, bool)> {
        let file_metadata = with_file_context(tokio::fs::metadata(file_path).await, "read metadata for", file_path)?;

        if !file_metadata.is_file() {
            return Err(anyhow!("Path is not a file: {}", file_path.display()));
        }

        // Only provider-supported raster formats become base64 image parts.
        // SVG is XML text and flows through the text path below so the model
        // can read it; binary formats no vision API accepts are rejected
        // explicitly instead of producing mojibake or a provider 400.
        if has_supported_image_extension(file_path) {
            let image_data = read_image_file_any_path::<&Path>(file_path).await?;
            let metadata = json!({
                "size_bytes": image_data.size,
                "content_kind": "image",
                "encoding": "base64",
                "mime_type": image_data.mime_type,
            });
            return Ok((image_data.base64_data.clone(), metadata, false));
        }
        if is_image_path(file_path)
            && file_path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| UNSUPPORTED_BINARY_IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        {
            return Err(anyhow!(
                "Unsupported image format for LLM vision: {} (use PNG, JPEG, GIF, or WebP)",
                file_path.display()
            ));
        }

        if let Some(encoding) = input.encoding.as_deref()
            && encoding.eq_ignore_ascii_case("base64")
        {
            let bytes = with_file_context(tokio::fs::read(file_path).await, "read file", file_path)?;
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
            let mut bytes = with_file_context(tokio::fs::read(file_path).await, "read file", file_path)?;
            let truncated = bytes.len() > max_bytes;
            if truncated {
                bytes.truncate(max_bytes);
            }
            let content = String::from_utf8_lossy(&bytes).into_owned();
            let metadata = json!({
                "size_bytes": file_metadata.len(),
                "size_lines": content.lines().count(),
                "is_truncated": truncated,
                "line_truncated": false,
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
        // The file is read through the bounded line reader rather than loaded
        // whole: only the first `cap` lines are retained, so a multi-GB file is
        // bounded in memory (the excessive tail is scanned and discarded, never
        // materialized). `size_lines` stays accurate because every line is still
        // counted.
        let cap = crate::tools::read_limits::absolute_line_cap();
        let (capped_content, total_lines, is_truncated, line_truncated) = read_bounded_text(file_path, cap).await?;

        let metadata = json!({
            "size_bytes": file_metadata.len(),
            "size_lines": total_lines,
            "is_truncated": is_truncated,
            "line_truncated": line_truncated,
            "type": "file",
            "content_kind": "text",
            "encoding": "utf8",
            "applied_max_lines": cap,
        });

        Ok((capped_content, metadata, is_truncated))
    }

    async fn read_file_chunked(
        &self,
        file_path: &Path,
        input: &Input,
        file_size: u64,
    ) -> Result<(String, Value, bool)> {
        const TOKENS_PER_LINE: usize = 15;

        let token_limit_lines = input.max_tokens.map(|max_tokens| (max_tokens / TOKENS_PER_LINE).max(1));
        let absolute_line_cap = crate::tools::read_limits::absolute_line_cap();
        let requested_max_lines = input.max_lines.unwrap_or(absolute_line_cap);
        let max_lines = token_limit_lines
            .map_or(requested_max_lines, |token_limit| requested_max_lines.min(token_limit))
            .min(absolute_line_cap);

        if max_lines == 0 {
            return Err(anyhow!("max_lines must be greater than 0"));
        }

        let mut head_lines = input.chunk_lines.unwrap_or(max_lines / 2);
        if head_lines == 0 {
            head_lines = 1;
        }
        head_lines = head_lines.min(max_lines);

        let mut tail_lines = input.chunk_lines.unwrap_or(head_lines);
        let remaining = max_lines.saturating_sub(head_lines);
        tail_lines = tail_lines.min(remaining);

        let file = with_file_context(tokio::fs::File::open(file_path).await, "open file", file_path)?;
        let mut reader = tokio::io::BufReader::new(file);
        let mut buffer = Vec::new();
        let mut retained_lines = Vec::with_capacity(max_lines);
        let mut retained_tail = VecDeque::with_capacity(tail_lines);
        let mut total_lines = 0usize;
        let mut last_line_had_newline = false;
        let mut is_truncated = false;
        let mut line_was_truncated = false;

        while let Some(truncated) = super::super::read_bounded_line(&mut reader, &mut buffer).await? {
            line_was_truncated |= truncated;
            total_lines = total_lines.saturating_add(1);
            let (line, has_newline) = decode_bounded_line(&buffer);
            last_line_had_newline = has_newline;

            if !is_truncated {
                if retained_lines.len() < max_lines {
                    retained_lines.push(line);
                    continue;
                }

                is_truncated = true;
                let tail_start = retained_lines.len().saturating_sub(tail_lines);
                retained_tail.extend(retained_lines.split_off(tail_start));
                retained_lines.truncate(head_lines);
            }

            if tail_lines > 0 {
                if retained_tail.len() == tail_lines {
                    retained_tail.pop_front();
                }
                retained_tail.push_back(line);
            }
        }

        if !is_truncated {
            let mut content = retained_lines.join("\n");
            if total_lines > 0 && last_line_had_newline {
                content.push('\n');
            }
            let metadata = json!({
                "size_bytes": file_size,
                "size_lines": total_lines,
                "is_truncated": line_was_truncated,
                "line_truncated": line_was_truncated,
                "type": "file",
                "content_kind": "text",
                "encoding": "utf8",
                "applied_max_lines": input.max_lines,
                "applied_max_tokens": input.max_tokens,
            });
            return Ok((content, metadata, line_was_truncated));
        }

        let omitted = total_lines.saturating_sub(head_lines + tail_lines);
        let mut final_content = String::new();
        let mut has_output_line = false;

        for line in &retained_lines {
            append_normalized_line(&mut final_content, &mut has_output_line, line);
        }

        if omitted > 0 {
            if has_output_line {
                final_content.push('\n');
            }
            final_content.push_str(&format!("... {omitted} lines omitted ..."));
            has_output_line = true;
        }

        for line in &retained_tail {
            append_normalized_line(&mut final_content, &mut has_output_line, line);
        }

        let metadata = json!({
            "size_bytes": file_size,
            "size_lines": total_lines,
            "is_truncated": true,
            "line_truncated": line_was_truncated,
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

fn decode_bounded_line(buffer: &[u8]) -> (String, bool) {
    let has_newline = buffer.last() == Some(&b'\n');
    let line = String::from_utf8_lossy(buffer);
    let line = line.strip_suffix('\n').unwrap_or(line.as_ref());
    (line.to_owned(), has_newline)
}

fn append_normalized_line(output: &mut String, has_output_line: &mut bool, line: &str) {
    if *has_output_line {
        output.push('\n');
    }
    output.push_str(line.strip_suffix('\r').unwrap_or(line));
    *has_output_line = true;
}

/// Stream a file and return at most `cap` lines of text, the true total line
/// count, and whether the file exceeded the cap.
///
/// Reads line-by-line through the shared bounded reader so the file is never
/// fully materialized and oversized physical lines cannot grow the buffer
/// without bound. Only the first `cap` lines are retained; the remainder is
/// scanned and dropped. Invalid UTF-8 is handled lossily, identical to the
/// previous `from_utf8_lossy` behavior.
async fn read_bounded_text(file_path: &Path, cap: usize) -> Result<(String, usize, bool, bool)> {
    let file = with_file_context(tokio::fs::File::open(file_path).await, "open file", file_path)?;
    let mut reader = tokio::io::BufReader::new(file);
    let mut buffer = Vec::new();

    let mut total_lines = 0usize;
    let mut last_line_had_newline = false;
    let mut collected: Vec<String> = Vec::with_capacity(cap);
    let mut line_was_truncated = false;
    while let Some(truncated) = super::super::read_bounded_line(&mut reader, &mut buffer).await? {
        line_was_truncated |= truncated;
        total_lines += 1;
        if total_lines <= cap {
            let (line, has_newline) = decode_bounded_line(&buffer);
            last_line_had_newline = has_newline;
            collected.push(line);
        }
    }

    let is_truncated = total_lines > cap || line_was_truncated;
    let mut content = collected.join("\n");
    if total_lines <= cap && total_lines > 0 && last_line_had_newline {
        content.push('\n');
    }
    Ok((content, total_lines, is_truncated, line_was_truncated))
}
