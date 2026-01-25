use super::diff_preview_size_skip;
use super::FileOpsTool;
use crate::tools::types::WriteInput;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::path::Path;
use tracing::info;

impl FileOpsTool {
    /// Write large file in chunks for atomicity and memory efficiency
    #[allow(dead_code)]
    pub(super) async fn write_file_chunked(
        &self,
        file_path: &Path,
        input: &WriteInput,
    ) -> Result<Value> {
        // Create parent directories if needed
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let content_bytes = input.content.as_bytes();
        let chunk_size = crate::config::constants::chunking::WRITE_CHUNK_SIZE;
        let total_size = content_bytes.len();

        match input.mode.as_str() {
            "overwrite" => {
                // Write in chunks for large files
                use tokio::io::AsyncWriteExt;
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(file_path)
                    .await?;

                for chunk in content_bytes.chunks(chunk_size) {
                    file.write_all(chunk).await?;
                }
                file.flush().await?;
            }
            "append" => {
                // Append in chunks
                use tokio::io::AsyncWriteExt;
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(file_path)
                    .await?;

                for chunk in content_bytes.chunks(chunk_size) {
                    file.write_all(chunk).await?;
                }
                file.flush().await?;
            }
            "skip_if_exists" => {
                if file_path.exists() {
                    return Ok(json!({
                        "success": true,
                        "skipped": true,
                        "reason": "File already exists"
                    }));
                }
                // Write in chunks for new file
                use tokio::io::AsyncWriteExt;
                let mut file = tokio::fs::File::create(file_path).await?;
                for chunk in content_bytes.chunks(chunk_size) {
                    file.write_all(chunk).await?;
                }
                file.flush().await?;
            }
            _ => {
                return Err(anyhow!(
                    "Error: Unsupported write mode '{}'. Allowed: overwrite, append, skip_if_exists.",
                    input.mode
                ));
            }
        }

        // Log chunked write operation
        self.log_write_operation(file_path, total_size, true)
            .await?;

        Ok(json!({
            "success": true,
            "path": self.workspace_relative_display(file_path),
            "mode": input.mode,
            "bytes_written": total_size,
            "chunked": true,
            "chunk_size": chunk_size,
            "chunks_written": total_size.div_ceil(chunk_size),
            "diff_preview": diff_preview_size_skip()
        }))
    }

    /// Log write operations for debugging
    pub(super) async fn log_write_operation(
        &self,
        file_path: &Path,
        bytes_written: usize,
        chunked: bool,
    ) -> Result<()> {
        let log_entry = json!({
            "operation": if chunked { "write_file_chunked" } else { "write_file" },
            "file_path": file_path.to_string_lossy(),
            "bytes_written": bytes_written,
            "chunked": chunked,
            "chunk_size": if chunked { Some(crate::config::constants::chunking::WRITE_CHUNK_SIZE) } else { None },
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        info!(
            "File write operation: {}",
            serde_json::to_string(&log_entry)?
        );
        Ok(())
    }
}
