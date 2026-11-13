use super::super::read_byte_range;
use super::FileOpsTool;
use crate::config::constants::tools;
use crate::tools::builder::ToolResponseBuilder;
use crate::tools::types::Input;
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::Path;
use tokio::io::BufReader;

impl FileOpsTool {
    pub(super) async fn read_file_paged(&self, file_path: &Path, input: &Input) -> Result<(String, Value, bool, bool)> {
        // Get file metadata to verify file exists and get size
        let file_metadata = tokio::fs::metadata(file_path)
            .await
            .with_context(|| format!("Failed to read metadata for file: {}", file_path.display()))?;

        if !file_metadata.is_file() {
            return Err(anyhow!("Path is not a file: {}", file_path.display()));
        }

        let file_size = file_metadata.len();

        // Calculate the final content based on whether we're using byte or line-based paging
        let (final_content, is_truncated, line_truncated) =
            if input.offset_lines.is_some() || input.page_size_lines.is_some() {
                // Line-based paging
                self.read_file_by_lines(file_path, input, file_size as usize).await?
            } else {
                // Byte-based paging (default)
                self.read_file_by_bytes(file_path, input, file_size).await?
            };

        // Create builder and metadata object
        let mut builder = ToolResponseBuilder::new(tools::READ_FILE)
            .success()
            .message(format!("Successfully read file {} (paged)", self.workspace_relative_display(file_path)))
            .content(final_content.clone())
            .data("size_bytes", json!(file_size))
            .data("size_lines", json!(final_content.lines().count()))
            .data("is_truncated", json!(is_truncated))
            .data("line_truncated", json!(line_truncated))
            .data("content_kind", json!("text"))
            .data("encoding", json!("utf8"));

        // Copy paging parameters to metadata
        if let Some(offset_bytes) = input.offset_bytes {
            builder = builder.data("offset_bytes", json!(offset_bytes));
        }
        if let Some(page_size_bytes) = input.page_size_bytes {
            builder = builder.data("page_size_bytes", json!(page_size_bytes));
        }
        if let Some(offset_lines) = input.offset_lines {
            builder = builder.data("offset_lines", json!(offset_lines));
        }
        if let Some(page_size_lines) = input.page_size_lines {
            builder = builder.data("page_size_lines", json!(page_size_lines));
        }

        let metadata = builder.build_json()["metadata"]["data"].clone();
        Ok((final_content, metadata, is_truncated, line_truncated))
    }

    /// Read file content by lines with offset and page size
    async fn read_file_by_lines(
        &self,
        file_path: &Path,
        input: &Input,
        _file_size: usize,
    ) -> Result<(String, bool, bool)> {
        // Validate and extract parameters
        let offset_lines = input.offset_lines.unwrap_or(0);
        let page_size_lines = input.page_size_lines.unwrap_or(1000); // Reasonable default: 1000 lines

        // Validate offset and page size
        if offset_lines > usize::MAX / 2 {
            return Err(anyhow!("Offset too large: {offset_lines}"));
        }
        if page_size_lines == 0 {
            return Err(anyhow!("Page size must be greater than 0"));
        }

        // Check for overflow before adding
        if offset_lines > usize::MAX - page_size_lines {
            return Err(anyhow!("Offset_lines + page_size_lines would overflow: {offset_lines} + {page_size_lines}"));
        }

        let page_size_lines = page_size_lines.min(crate::tools::read_limits::absolute_line_cap());
        let file = tokio::fs::File::open(file_path)
            .await
            .with_context(|| format!("Failed to read file content: {}", file_path.display()))?;
        let mut reader = BufReader::new(file);
        let mut buffer = Vec::new();
        let mut line_was_truncated = false;

        for _ in 0..offset_lines {
            let Some(truncated) = super::super::read_bounded_line(&mut reader, &mut buffer).await? else {
                return Ok((String::new(), line_was_truncated, line_was_truncated));
            };
            line_was_truncated |= truncated;
        }

        let mut final_content = String::new();
        for line_index in 0..page_size_lines {
            let Some(truncated) = super::super::read_bounded_line(&mut reader, &mut buffer).await? else {
                return Ok((final_content, line_was_truncated, line_was_truncated));
            };
            line_was_truncated |= truncated;

            let line = String::from_utf8_lossy(&buffer);
            let line = line.strip_suffix('\n').unwrap_or(line.as_ref());
            let line = line.trim_suffix('\r');
            if line_index > 0 {
                final_content.push('\n');
            }
            final_content.push_str(line);
        }

        let has_following_line = super::super::read_bounded_line(&mut reader, &mut buffer).await?;
        let is_truncated = line_was_truncated || has_following_line.is_some();

        Ok((final_content, is_truncated, line_was_truncated))
    }

    /// Read file content by bytes with offset and page size
    async fn read_file_by_bytes(
        &self,
        file_path: &Path,
        input: &Input,
        _file_size: u64,
    ) -> Result<(String, bool, bool)> {
        let offset_bytes = input.offset_bytes.unwrap_or(0);
        let page_size_bytes = input.page_size_bytes.unwrap_or(8192);

        if page_size_bytes == 0 {
            return Err(anyhow!("Page size must be greater than 0"));
        }

        // Legacy path: raw bytes without line numbers
        let result = read_byte_range(file_path, offset_bytes, page_size_bytes, false).await?;
        Ok((result.content, result.has_more, false))
    }
}
