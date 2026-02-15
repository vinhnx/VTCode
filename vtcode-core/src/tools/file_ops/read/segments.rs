use super::FileOpsTool;
use crate::tools::builder::ToolResponseBuilder;
use crate::tools::types::Input;
use crate::utils::file_utils::read_file_with_context;
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

impl FileOpsTool {
    pub(super) async fn read_file_paged(
        &self,
        file_path: &Path,
        input: &Input,
    ) -> Result<(String, Value, bool)> {
        // Get file metadata to verify file exists and get size
        let file_metadata = tokio::fs::metadata(file_path).await.with_context(|| {
            format!("Failed to read metadata for file: {}", file_path.display())
        })?;

        if !file_metadata.is_file() {
            return Err(anyhow!("Path is not a file: {}", file_path.display()));
        }

        let file_size = file_metadata.len();

        // Calculate the final content based on whether we're using byte or line-based paging
        let (final_content, is_truncated) =
            if input.offset_lines.is_some() || input.page_size_lines.is_some() {
                // Line-based paging
                self.read_file_by_lines(file_path, input, file_size as usize)
                    .await?
            } else {
                // Byte-based paging (default)
                self.read_file_by_bytes(file_path, input, file_size).await?
            };

        // Create builder and metadata object
        let mut builder = ToolResponseBuilder::new("read_file")
            .success()
            .message(format!(
                "Successfully read file {} (paged)",
                self.workspace_relative_display(file_path)
            ))
            .content(final_content.clone())
            .data("size_bytes", json!(file_size))
            .data("size_lines", json!(final_content.lines().count()))
            .data("is_truncated", json!(is_truncated))
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

        Ok((
            final_content,
            builder.build_json()["metadata"].clone(),
            is_truncated,
        ))
    }

    /// Read file content by lines with offset and page size
    async fn read_file_by_lines(
        &self,
        file_path: &Path,
        input: &Input,
        _file_size: usize,
    ) -> Result<(String, bool)> {
        // Validate and extract parameters
        let offset_lines = input.offset_lines.unwrap_or(0);
        let page_size_lines = input.page_size_lines.unwrap_or(1000); // Reasonable default: 1000 lines

        // Validate offset and page size
        if offset_lines > usize::MAX / 2 {
            return Err(anyhow!("Offset too large: {}", offset_lines));
        }
        if page_size_lines == 0 {
            return Err(anyhow!("Page size must be greater than 0"));
        }

        // Check for overflow before adding
        if offset_lines > usize::MAX - page_size_lines {
            return Err(anyhow!(
                "Offset_lines + page_size_lines would overflow: {} + {}",
                offset_lines,
                page_size_lines
            ));
        }

        let content = read_file_with_context(file_path, "file content")
            .await
            .with_context(|| format!("Failed to read file content: {}", file_path.display()))?;

        let all_lines: Vec<&str> = content.lines().collect();
        let total_lines = all_lines.len();

        // Handle empty file or offset beyond bounds
        if total_lines == 0 || offset_lines >= total_lines {
            return Ok((String::new(), false));
        }

        // Calculate end position (safe because we validated overflow above)
        let end_pos = std::cmp::min(offset_lines + page_size_lines, total_lines);
        let selected_lines = &all_lines[offset_lines..end_pos];

        let final_content = selected_lines.join("\n");
        let is_truncated = end_pos < total_lines;

        Ok((final_content, is_truncated))
    }

    /// Read file content by bytes with offset and page size
    async fn read_file_by_bytes(
        &self,
        file_path: &Path,
        input: &Input,
        file_size: u64,
    ) -> Result<(String, bool)> {
        // Validate and extract parameters
        let offset_bytes = input.offset_bytes.unwrap_or(0);
        let page_size_bytes = input.page_size_bytes.unwrap_or(8192); // Reasonable default: 8KB

        // Validate offset and page size
        if offset_bytes >= file_size {
            return Ok((String::new(), false));
        }
        if page_size_bytes == 0 {
            return Err(anyhow!("Page size must be greater than 0"));
        }

        // Check for overflow before adding (safe cast since page_size_bytes < file_size)
        let page_size_u64 = page_size_bytes as u64;
        if offset_bytes > u64::MAX - page_size_u64 {
            return Err(anyhow!(
                "Offset_bytes + page_size_bytes would overflow: {} + {}",
                offset_bytes,
                page_size_bytes
            ));
        }

        // Open the file and seek to the offset
        let mut file = tokio::fs::File::open(file_path)
            .await
            .with_context(|| format!("Failed to open file: {}", file_path.display()))?;

        // Calculate the end position (don't exceed file boundaries)
        let end_pos = std::cmp::min(offset_bytes + page_size_u64, file_size);
        let actual_read_size = (end_pos - offset_bytes) as usize;

        // Seek to the offset position
        file.seek(std::io::SeekFrom::Start(offset_bytes))
            .await
            .with_context(|| {
                format!(
                    "Failed to seek to offset {} in file: {}",
                    offset_bytes,
                    file_path.display()
                )
            })?;

        // Read the specified number of bytes
        let mut buffer = vec![0; actual_read_size];
        let mut bytes_read = 0;

        if actual_read_size > 0 {
            bytes_read = file.read_exact(&mut buffer).await.with_context(|| {
                format!(
                    "Failed to read {} bytes from offset {} in file: {}",
                    actual_read_size,
                    offset_bytes,
                    file_path.display()
                )
            })?;
        }

        // Convert to string, handling potential UTF-8 errors gracefully
        let final_content = String::from_utf8_lossy(&buffer[..bytes_read]).into_owned();
        let is_truncated = end_pos < file_size;

        Ok((final_content, is_truncated))
    }
}
