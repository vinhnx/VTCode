use super::FileOpsTool;
use super::is_image_path;
mod legacy;
use crate::tools::handlers::read_file::{ReadFileArgs, ReadFileHandler};
use crate::tools::traits::FileTool;
use crate::tools::types::Input;
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::Path;
use tokio::io::AsyncSeekExt;
use tracing::info;

impl FileOpsTool {
    pub async fn read_file(&self, args: Value) -> Result<Value> {
        let path_str = args
            .get("path")
            .or_else(|| args.get("file_path"))
            .or_else(|| args.get("filepath"))
            .or_else(|| args.get("target_path"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                let received = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                anyhow!(
                    "Error: Invalid 'read_file' arguments. Missing required path parameter.\n\
                    Received: {}\n\
                    Expected: {{\"path\": \"file/path\"}} or {{\"file_path\": \"file/path\"}}\n\
                    Accepted path parameters: path, file_path, filepath, target_path\n\
                    Optional params: offset_lines, limit, max_bytes, max_tokens",
                    received
                )
            })?;

        // Try to resolve the file path
        let potential_paths = self.resolve_file_path(path_str)?;

        for candidate_path in &potential_paths {
            if !tokio::fs::try_exists(candidate_path).await? {
                continue;
            }

            let canonical = self
                .normalize_and_validate_candidate(candidate_path, path_str)
                .await?;

            if self.should_exclude(&canonical).await {
                continue;
            }

            let metadata = tokio::fs::metadata(&canonical).await?;
            if !metadata.is_file() {
                continue;
            }

            let size_bytes = metadata.len();

            // Heuristic to decide between new handler (line/indentation) and legacy handler (byte-based)
            // If explicit "offset_bytes", "page_size_bytes" are used, stay with legacy.
            // If "mode", "indentation", "offset" (implied line) are used, prefer new handler.
            let is_image = is_image_path(&canonical);
            let is_legacy_request = is_image
                || args.get("offset_bytes").is_some()
                || args.get("page_size_bytes").is_some()
                || args.get("offset_lines").is_some(); // Legacy also used offset_lines, but new one uses 'offset'

            let prefer_new_handler = args.get("mode").is_some()
                || args.get("indentation").is_some()
                || args.get("offset").is_some();

            if !is_legacy_request || prefer_new_handler {
                // Prepare args for new handler
                let mut handler_args_json = args.clone();
                if let Some(obj) = handler_args_json.as_object_mut() {
                    // Inject resolved absolute path
                    obj.insert("file_path".to_string(), json!(canonical.to_string_lossy()));

                    // Map legacy param names if new ones aren't present
                    if !obj.contains_key("offset") && obj.contains_key("offset_lines") {
                        obj["offset"] = obj["offset_lines"].clone();
                    }
                    if !obj.contains_key("limit") && obj.contains_key("page_size_lines") {
                        obj["limit"] = obj["page_size_lines"].clone();
                    }
                    if !obj.contains_key("limit") {
                        // Default limit if not specified (ReadFileArgs defaults to 2000)
                    }
                }

                // Attempt to parse
                match serde_json::from_value::<ReadFileArgs>(handler_args_json) {
                    Ok(read_args) => {
                        let handler = ReadFileHandler;
                        let content = handler.handle(read_args).await?;
                        return Ok(json!({
                           "success": true,
                           "status": "success",
                           "message": format!("Successfully read file {}", self.workspace_relative_display(&canonical)),
                           "content": content,
                           "path": self.workspace_relative_display(&canonical),
                           "metadata": {
                               "size_bytes": size_bytes,
                           }
                        }));
                    }
                    Err(e) => {
                        // If parsing failed (e.g. invalid mode), allow falling back ONLY if it looks strictly like a legacy request,
                        // otherwise wrap the error.
                        if prefer_new_handler {
                            return Err(anyhow!(
                                "Failed to parse arguments for read_file handler: {}. Args: {:?}",
                                e,
                                args
                            ));
                        }
                        // Fall through to legacy
                    }
                }
            }

            // Legacy Fallback
            // We must reconstruct Input from args to use legacy functions
            let input: Input = serde_json::from_value(args.clone())
                .context("Error: Invalid 'read_file' arguments for legacy handler.")?;

            // Check if paging/offset is requested
            let use_paging = input.offset_bytes.is_some()
                || input.page_size_bytes.is_some()
                || input.offset_lines.is_some()
                || input.page_size_lines.is_some();

            let (content, metadata, truncated) = if use_paging {
                self.read_file_paged(&canonical, &input).await?
            } else {
                self.read_file_legacy(&canonical, &input).await?
            };

            let mut result = json!({
                "success": true,
                "status": "success",
                "message": format!("Successfully read {} bytes from {}", size_bytes, self.workspace_relative_display(&canonical)),
                "content": content,
                "path": self.workspace_relative_display(&canonical),
                "metadata": metadata
            });

            // ... (legacy metadata logic) ...
            if let Some(is_truncated) = result
                .get("metadata")
                .and_then(|meta| meta.get("is_truncated"))
                .and_then(Value::as_bool)
            {
                result["is_truncated"] = json!(is_truncated);
            }
            if let Some(encoding) = result
                .get("metadata")
                .and_then(|meta| meta.get("encoding"))
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                result["encoding"] = json!(encoding);
            }
            // ... copy remaining legacy logic ...
            if let Some(content_kind) = result
                .get("metadata")
                .and_then(|meta| meta.get("content_kind"))
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                result["content_kind"] = json!(content_kind);
                if matches!(content_kind.as_str(), "binary" | "image") {
                    result["binary"] = json!(true);
                }
            }
            if let Some(mime_type) = result
                .get("metadata")
                .and_then(|meta| meta.get("mime_type"))
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                result["mime_type"] = json!(mime_type);
            }

            // Add paging information
            if input.offset_bytes.is_some()
                || input.page_size_bytes.is_some()
                || input.offset_lines.is_some()
                || input.page_size_lines.is_some()
            {
                if let Some(offset_bytes) = input.offset_bytes {
                    result["offset_bytes"] = json!(offset_bytes);
                }
                if let Some(page_size_bytes) = input.page_size_bytes {
                    result["page_size_bytes"] = json!(page_size_bytes);
                }
                if let Some(offset_lines) = input.offset_lines {
                    result["offset_lines"] = json!(offset_lines);
                }
                if let Some(page_size_lines) = input.page_size_lines {
                    result["page_size_lines"] = json!(page_size_lines);
                }
                if truncated {
                    result["truncated"] = json!(true);
                    result["truncation_reason"] = json!("reached_end_of_file");
                }
            }

            return Ok(result);
        }

        Err(anyhow!(
            "Error: File not found: {}. Tried paths: {}.",
            path_str,
            potential_paths
                .iter()
                .map(|p| self.workspace_relative_display(p))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    async fn read_file_paged(
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

        // Create metadata object
        let metadata = json!({
            "size_bytes": file_size,
            "size_lines": final_content.lines().count(),
            "is_truncated": is_truncated,
            "type": "file",
            "content_kind": "text",
            "encoding": "utf8",
        });

        Ok((final_content, metadata, is_truncated))
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

        let content = tokio::fs::read_to_string(file_path)
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
        use tokio::io::AsyncReadExt;

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

    /// Log chunking operations for debugging
    async fn log_chunking_operation(
        &self,
        file_path: &Path,
        truncated: bool,
        total_lines: Option<usize>,
    ) -> Result<()> {
        if truncated {
            let log_entry = json!({
                "operation": "read_file_chunked",
                "file_path": file_path.to_string_lossy(),
                "truncated": true,
                "total_lines": total_lines,
                "timestamp": chrono::Utc::now().to_rfc3339()
            });

            info!(
                "File chunking operation: {}",
                serde_json::to_string(&log_entry)?
            );
        }
        Ok(())
    }
}


#[cfg(test)]
mod read_tests {
    use super::*;
    use crate::tools::grep_file::GrepSearchManager;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
        async fn test_read_file_paging_lines() {
            let temp_dir = TempDir::new().unwrap();
            let workspace_root = temp_dir.path().to_path_buf();
            let test_file = workspace_root.join("test_file.txt");

            // Create test content with 10 lines
            let test_content =
                "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10\n";
            fs::write(&test_file, test_content).unwrap();

            let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
            let file_ops = FileOpsTool::new(workspace_root, grep_manager);

            // Test basic paging functionality: offset_lines=2, page_size_lines=3
            // Should return lines 3, 4, 5 (0-indexed: 2, 3, 4)
            let args = json!({
                "path": test_file.to_string_lossy().into_owned(),
                "offset_lines": 2,
                "page_size_lines": 3
            });

            let result = file_ops.read_file(args).await.unwrap();
            assert!(result["success"].as_bool().unwrap());
            assert_eq!(result["content"].as_str().unwrap(), "line3\nline4\nline5");
        }

        #[tokio::test]
        async fn test_read_file_paging_bytes() {
            let temp_dir = TempDir::new().unwrap();
            let workspace_root = temp_dir.path().to_path_buf();
            let test_file = workspace_root.join("test_file.txt");

            let test_content = "line1\nline2\nline3\nline4\nline5\n";
            fs::write(&test_file, test_content).unwrap();

            let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
            let file_ops = FileOpsTool::new(workspace_root, grep_manager);

            // Test byte-based paging: skip first 6 bytes ("line1\n") and read next 6 bytes
            let args = json!({
                "path": test_file.to_string_lossy().into_owned(),
                "offset_bytes": 6,
                "page_size_bytes": 6
            });

            let result = file_ops.read_file(args).await.unwrap();
            assert!(result["success"].as_bool().unwrap());
            assert_eq!(result["content"].as_str().unwrap(), "line2\n");
        }

        #[tokio::test]
        async fn test_read_file_offset_beyond_size() {
            let temp_dir = TempDir::new().unwrap();
            let workspace_root = temp_dir.path().to_path_buf();
            let test_file = workspace_root.join("test_file.txt");

            let test_content = "line1\nline2\nline3\n";
            fs::write(&test_file, test_content).unwrap();

            let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
            let file_ops = FileOpsTool::new(workspace_root, grep_manager);

            // Test when offset is beyond file size
            let args = json!({
                "path": test_file.to_string_lossy().into_owned(),
                "offset_lines": 100,
                "page_size_lines": 10
            });

            let result = file_ops.read_file(args).await.unwrap();
            assert!(result["success"].as_bool().unwrap());
            assert_eq!(result["content"].as_str().unwrap(), "");
        }

        #[tokio::test]
        async fn test_read_file_empty_file() {
            let temp_dir = TempDir::new().unwrap();
            let workspace_root = temp_dir.path().to_path_buf();
            let test_file = workspace_root.join("empty_file.txt");

            fs::write(&test_file, "").unwrap();

            let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
            let file_ops = FileOpsTool::new(workspace_root, grep_manager);

            // Test reading empty file with paging
            let args = json!({
                "path": test_file.to_string_lossy().into_owned(),
                "offset_lines": 0,
                "page_size_lines": 10
            });

            let result = file_ops.read_file(args).await.unwrap();
            assert!(result["success"].as_bool().unwrap());
            assert_eq!(result["content"].as_str().unwrap(), "");
        }

        #[tokio::test]
        async fn test_read_file_legacy_functionality() {
            let temp_dir = TempDir::new().unwrap();
            let workspace_root = temp_dir.path().to_path_buf();
            let test_file = workspace_root.join("test_file.txt");

            let test_content = "line1\nline2\nline3\nline4\nline5\n";
            fs::write(&test_file, test_content).unwrap();

            let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
            let file_ops = FileOpsTool::new(workspace_root, grep_manager);

            // Test legacy functionality with max_bytes
            let args = json!({
                "path": test_file.to_string_lossy().into_owned(),
                "max_bytes": 10
            });

            let result = file_ops.read_file(args).await.unwrap();
            assert!(result["success"].as_bool().unwrap());
            let content = result["content"].as_str().unwrap();
            assert!(content.len() <= 10);
            assert!(content.starts_with("line1"));
        }

        #[tokio::test]
        async fn test_read_file_legacy_token_chunking() {
            let temp_dir = TempDir::new().unwrap();
            let workspace_root = temp_dir.path().to_path_buf();
            let test_file = workspace_root.join("test_file.txt");

            // Create test content with 50 lines
            let test_content = (1..=50)
                .map(|i| format!("line-{}", i))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            std::fs::write(&test_file, test_content).unwrap();

            let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
            let file_ops = FileOpsTool::new(workspace_root, grep_manager);

            // Token budget small enough to keep roughly first+last 5-10 lines
            let max_tokens = 15 * 12; // ~12 lines worth using TOKENS_PER_LINE

            let args = json!({
                "path": test_file.to_string_lossy().into_owned(),
                "max_tokens": max_tokens
            });

            let result = file_ops.read_file(args).await.unwrap();
            assert!(result["success"].as_bool().unwrap());
            let content = result["content"].as_str().unwrap();
            // Should contain first and last lines
            assert!(content.contains("line-1"));
            assert!(content.contains("line-50"));
            // Should indicate truncation
            assert!(result["is_truncated"].as_bool().unwrap());
            // Should report applied token budget
            assert_eq!(
                result["metadata"]["applied_max_tokens"].as_u64().unwrap(),
                max_tokens as u64
            );
        }
    }
