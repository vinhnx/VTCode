//! File operation tools with composable functionality.
//!
//! This module provides the `FileOpsTool` for file discovery and listing operations,
//! along with supporting utilities for diff previews and path helpers.

mod diff_preview;
mod list;
mod path_policy;
mod read;
pub(crate) use read::{PATCH_READ_CACHE_NONCE, bounded_line_read_path, source_line_offset};
mod tool;
mod write;

use anyhow::{Context, Result};
use std::path::Path;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader};
use vtcode_commons::async_utils::read_exact_uninit;

pub use diff_preview::{
    SUPPRESSED_PREVIEW_REASON, build_diff_preview, canonical_diff_previews, diff_output_has_effective_change,
    diff_preview_error_skip, diff_preview_size_skip, diff_preview_suppressed, diff_preview_user_message,
};
pub use tool::FileOpsTool;
pub use vtcode_commons::fs::is_image_path;

/// Hard cap on the byte size of a single logical line read by any file tool.
///
/// `read_until(b'\n')` grows the caller's buffer unboundedly when a file
/// contains a huge line without a newline (e.g. minified bundles). A line
/// longer than this cap is truncated at the cap for display purposes; the rest
/// of the line is drained so the stream stays aligned at the next newline.
pub(crate) const MAX_LINE_READ_BYTES: usize = 64 * 1024;

/// Read the next line into `buffer`, bounded at `MAX_LINE_READ_BYTES`.
///
/// Returns `Ok(None)` at EOF, otherwise `Ok(Some(true))` when the line was
/// truncated at the cap (the remainder of the physical line was drained), or
/// `Ok(Some(false))` for a fully-read line. The trailing `\n`/`\r\n` is kept in
/// `buffer` for callers that already strip it.
pub(crate) async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buffer: &mut Vec<u8>,
) -> Result<Option<bool>> {
    buffer.clear();
    let mut limited = reader.take(MAX_LINE_READ_BYTES as u64 + 1);
    let n = limited.read_until(b'\n', buffer).await.context("failed to read line")?;
    if n == 0 {
        return Ok(None);
    }

    let truncated = buffer.last() != Some(&b'\n') && buffer.len() > MAX_LINE_READ_BYTES;
    if truncated {
        // Drain the remainder of the oversized physical line so subsequent
        // reads start at the next newline. Read in bounded chunks through a
        // `Take` so the drain never allocates the full remainder AND never
        // over-consumes past the newline (a plain `read` would swallow the
        // next line that BufReader prefetched into the same chunk).
        let mut drain_buf = Vec::with_capacity(4096);
        loop {
            drain_buf.clear();
            let mut chunk = reader.take(4096);
            let n = chunk
                .read_until(b'\n', &mut drain_buf)
                .await
                .context("failed to drain oversized line")?;
            if n == 0 {
                break;
            }
            if drain_buf.last() == Some(&b'\n') {
                break;
            }
        }
        buffer.truncate(MAX_LINE_READ_BYTES);
    }
    Ok(Some(truncated))
}

/// Result of a byte-range read operation.
pub(crate) struct ByteRangeReadResult {
    pub content: String,
    pub has_more: bool,
    /// Number of lines in the returned content.
    pub lines_read: usize,
}

/// Read a byte range from a file using seek-based access.
///
/// When `line_numbers` is true, aligns to line boundaries (skips partial first line)
/// and prefixes each line with its 1-indexed line number (e.g. "42: content").
/// When false, returns raw bytes as a UTF-8 string without line alignment.
pub(crate) async fn read_byte_range(
    file_path: &Path,
    offset_bytes: u64,
    page_size_bytes: usize,
    line_numbers: bool,
) -> Result<ByteRangeReadResult> {
    let metadata = tokio::fs::metadata(file_path)
        .await
        .with_context(|| format!("Failed to read metadata for: {}", file_path.display()))?;

    if !metadata.is_file() {
        anyhow::bail!("Path is not a file: {}", file_path.display());
    }

    let file_size = metadata.len();

    // Offset beyond file boundary: return empty
    if offset_bytes >= file_size {
        return Ok(ByteRangeReadResult {
            content: String::new(),
            has_more: false,
            lines_read: 0,
        });
    }

    // Open file and seek to offset
    let mut file = tokio::fs::File::open(file_path)
        .await
        .with_context(|| format!("Failed to open: {}", file_path.display()))?;

    file.seek(std::io::SeekFrom::Start(offset_bytes))
        .await
        .with_context(|| format!("Failed to seek to offset {} in: {}", offset_bytes, file_path.display()))?;

    // Clamp read size to file bounds, guarding against overflow
    let page_size_u64 = page_size_bytes as u64;
    let end_pos = offset_bytes
        .checked_add(page_size_u64)
        .map(|end| std::cmp::min(end, file_size))
        .unwrap_or(file_size);
    let actual_read_size = (end_pos - offset_bytes) as usize;

    let buffer = read_exact_uninit(&mut file, actual_read_size).await.with_context(|| {
        format!("Failed to read {} bytes from offset {} in: {}", actual_read_size, offset_bytes, file_path.display())
    })?;

    let raw = String::from_utf8_lossy(&buffer).into_owned();
    let has_more = end_pos < file_size;

    if !line_numbers {
        // Raw mode: return content without line numbers or boundary alignment
        return Ok(ByteRangeReadResult { content: raw, has_more, lines_read: 0 });
    }

    // Line-number mode: align to line boundaries and add line numbers
    let start_line = count_lines_before(file_path, offset_bytes).await?;

    // Check if we're at a line boundary by looking at the byte before offset
    let at_line_boundary = if offset_bytes == 0 {
        true
    } else {
        let mut pre = tokio::fs::File::open(file_path)
            .await
            .with_context(|| format!("Failed to open: {}", file_path.display()))?;
        pre.seek(std::io::SeekFrom::Start(offset_bytes - 1))
            .await
            .context("failed to seek for boundary check")?;
        let mut prev_byte = [0u8; 1];
        AsyncReadExt::read_exact(&mut pre, &mut prev_byte)
            .await
            .context("failed to read boundary byte")?;
        prev_byte[0] == b'\n'
    };

    let lines: Vec<&str> = raw.split('\n').collect();
    let mut formatted_lines = Vec::new();
    let mut current_line = start_line;

    for (i, line) in lines.iter().enumerate() {
        // Skip partial first line if we're not at a line boundary
        if i == 0 && !at_line_boundary && !line.is_empty() {
            current_line += 1;
            continue;
        }

        // Skip empty trailing line from split if has_more (incomplete last line)
        if i == lines.len() - 1 && has_more && line.is_empty() {
            continue;
        }

        let trimmed = line.trim_end_matches('\r');
        if !trimmed.is_empty() || i < lines.len() - 1 {
            formatted_lines.push(format!("{current_line}: {trimmed}"));
            current_line += 1;
        }
    }

    let lines_read = formatted_lines.len();
    let content = formatted_lines.join("\n");

    Ok(ByteRangeReadResult { content, has_more, lines_read })
}

/// Count the number of newlines before the given byte offset to determine
/// the 1-indexed line number at that position.
async fn count_lines_before(file_path: &Path, offset_bytes: u64) -> Result<usize> {
    if offset_bytes == 0 {
        return Ok(1);
    }

    let file = tokio::fs::File::open(file_path)
        .await
        .with_context(|| format!("Failed to open: {}", file_path.display()))?;

    let mut reader = BufReader::new(file);
    let mut line_count: usize = 1; // 1-indexed
    let mut bytes_read: u64 = 0;
    let mut buffer = Vec::new();

    while bytes_read < offset_bytes {
        let n = read_bounded_line(&mut reader, &mut buffer)
            .await
            .context("failed to count lines")?;

        let Some(_) = n else { break };

        bytes_read += buffer.len() as u64;
        if bytes_read <= offset_bytes {
            line_count += 1;
        }
    }

    Ok(line_count)
}

/// Restore exact text content by accounting for trailing newline differences.
///
/// When a file's byte size is slightly larger than the content string length,
/// this reconstructs the original content by appending the missing trailing
/// newline(s). Returns `None` if the size mismatch is unexpected.
pub(crate) fn restore_exact_text_content(content: &str, size_bytes: u64) -> Option<String> {
    let content_size = content.len() as u64;
    match size_bytes.checked_sub(content_size) {
        Some(0) => Some(content.to_string()),
        Some(1) => Some(format!("{content}\n")),
        Some(2) if content.contains("\r\n") || !content.contains('\n') => Some(format!("{content}\r\n")),
        _ => None,
    }
}
