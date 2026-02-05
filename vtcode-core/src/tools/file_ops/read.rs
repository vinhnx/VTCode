use super::FileOpsTool;
use super::is_image_path;
mod legacy;
mod logging;
mod segments;
use crate::telemetry::perf::PerfSpan;
use crate::tools::builder::ToolResponseBuilder;
use crate::tools::cache::{FILE_CACHE, file_read_cache_config};
use crate::tools::handlers::read_file::{ReadFileArgs, ReadFileHandler};
use crate::tools::traits::FileTool;
use crate::tools::types::{Input, PathArgs};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::Path;
use std::time::UNIX_EPOCH;

fn is_legacy_read_request(args: &Value, is_image: bool) -> bool {
    if is_image {
        return true;
    }

    let legacy_keys = [
        "offset_bytes",
        "page_size_bytes",
        "offset_lines",
        "page_size_lines",
        "max_bytes",
        "max_lines",
        "chunk_lines",
        "encoding",
        "max_tokens",
    ];

    legacy_keys.iter().any(|key| args.get(*key).is_some())
}

fn is_new_read_request(args: &Value) -> bool {
    let new_keys = ["mode", "indentation", "offset", "limit"];
    new_keys.iter().any(|key| args.get(*key).is_some())
}

fn build_read_handler_args(args: &Value, canonical_path: &std::path::Path) -> Value {
    let mut handler_args_json = args.clone();
    if let Some(obj) = handler_args_json.as_object_mut() {
        obj.insert(
            "file_path".to_string(),
            json!(canonical_path.to_string_lossy()),
        );

        if !obj.contains_key("offset") && obj.contains_key("offset_lines") {
            obj.insert("offset".to_string(), obj["offset_lines"].clone());
        }
        if !obj.contains_key("limit") && obj.contains_key("page_size_lines") {
            obj.insert("limit".to_string(), obj["page_size_lines"].clone());
        }
    }

    handler_args_json
}

fn is_history_jsonl(path: &Path) -> bool {
    if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
        return false;
    }

    let mut saw_vtcode = false;
    for component in path.components() {
        let segment = component.as_os_str().to_string_lossy();
        if segment == ".vtcode" {
            saw_vtcode = true;
            continue;
        }
        if saw_vtcode && segment == "history" {
            return true;
        }
    }
    false
}

fn build_history_cache_key(
    path: &Path,
    metadata: &std::fs::Metadata,
    args: &Value,
) -> Option<String> {
    let modified = metadata.modified().ok()?;
    let mtime = modified.duration_since(UNIX_EPOCH).ok()?.as_millis();
    let size = metadata.len();

    let mode = args.get("mode").and_then(Value::as_str).unwrap_or("legacy");
    let indentation = args
        .get("indentation")
        .and_then(Value::as_str)
        .unwrap_or("");
    let offset = args
        .get("offset")
        .or_else(|| args.get("offset_lines"))
        .or_else(|| args.get("offset_bytes"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let limit = args
        .get("limit")
        .or_else(|| args.get("page_size_lines"))
        .or_else(|| args.get("page_size_bytes"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let max_bytes = args.get("max_bytes").and_then(Value::as_u64).unwrap_or(0);
    let max_tokens = args.get("max_tokens").and_then(Value::as_u64).unwrap_or(0);
    let encoding = args.get("encoding").and_then(Value::as_str).unwrap_or("");

    Some(format!(
        "read_file:history:{}:{}:{}:mode={mode}|indent={indentation}|offset={offset}|limit={limit}|max_bytes={max_bytes}|max_tokens={max_tokens}|encoding={encoding}",
        path.display(),
        size,
        mtime
    ))
}

impl FileOpsTool {
    pub async fn read_file(&self, args: Value) -> Result<Value> {
        let mut perf = PerfSpan::new("vtcode.perf.read_file_ms");

        let path_args: PathArgs = serde_json::from_value(args.clone()).map_err(|_| {
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

        let path_str = &path_args.path;

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
            let history_jsonl = is_history_jsonl(&canonical);
            perf.tag(
                "path_class",
                if history_jsonl {
                    "history_jsonl"
                } else {
                    "other"
                },
            );
            let is_image = is_image_path(&canonical);
            let is_legacy_request = is_legacy_read_request(&args, is_image);
            let is_new_request = is_new_read_request(&args);

            let cache_config = file_read_cache_config();
            let cache_key = if cache_config.enabled
                && history_jsonl
                && size_bytes >= cache_config.min_size_bytes as u64
                && size_bytes <= cache_config.max_size_bytes as u64
            {
                build_history_cache_key(&canonical, &metadata, &args)
            } else {
                None
            };

            if let Some(key) = cache_key.as_ref()
                && let Some(cached) = FILE_CACHE.get_file(key).await
            {
                perf.tag("cache", "hit");
                return Ok(cached);
            }
            perf.tag("cache", if cache_key.is_some() { "miss" } else { "skip" });

            if !is_image && (is_new_request || !is_legacy_request) {
                let handler_args_json = build_read_handler_args(&args, &canonical);
                match serde_json::from_value::<ReadFileArgs>(handler_args_json) {
                    Ok(read_args) => {
                        let handler = ReadFileHandler;
                        let content = handler.handle(read_args).await?;

                        let response = ToolResponseBuilder::new("read_file")
                            .success()
                            .message(format!(
                                "Successfully read file {}",
                                self.workspace_relative_display(&canonical)
                            ))
                            .content(content)
                            .field("path", json!(self.workspace_relative_display(&canonical)))
                            .data("size_bytes", json!(size_bytes))
                            .build_json();

                        if let Some(key) = cache_key.as_ref() {
                            FILE_CACHE.put_file(key.clone(), response.clone()).await;
                        }

                        return Ok(response);
                    }
                    Err(e) => {
                        if is_new_request {
                            return Err(anyhow!(
                                "Failed to parse arguments for read_file handler: {}. Args: {:?}",
                                e,
                                args
                            ));
                        }
                    }
                }
            }

            // Legacy path
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

            let mut builder = ToolResponseBuilder::new("read_file")
                .success()
                .message(format!(
                    "Successfully read {} bytes from {}",
                    size_bytes,
                    self.workspace_relative_display(&canonical)
                ))
                .content(content)
                .field("path", json!(self.workspace_relative_display(&canonical)));

            // Merge legacy metadata
            if let Some(obj) = metadata.as_object() {
                for (k, v) in obj {
                    builder = builder.data(k, v.clone());
                }
            }

            // Special legacy fields at top level
            if let Some(is_truncated) = metadata.get("is_truncated").and_then(Value::as_bool) {
                builder = builder.field("is_truncated", json!(is_truncated));
            }
            if let Some(encoding) = metadata.get("encoding").and_then(Value::as_str) {
                builder = builder.field("encoding", json!(encoding));
            }
            if let Some(content_kind) = metadata.get("content_kind").and_then(Value::as_str) {
                builder = builder.field("content_kind", json!(content_kind));
                if matches!(content_kind, "binary" | "image") {
                    builder = builder.field("binary", json!(true));
                }
            }
            if let Some(mime_type) = metadata.get("mime_type").and_then(Value::as_str) {
                builder = builder.field("mime_type", json!(mime_type));
            }

            // Add paging information
            if use_paging {
                if let Some(offset_bytes) = input.offset_bytes {
                    builder = builder.field("offset_bytes", json!(offset_bytes));
                }
                if let Some(page_size_bytes) = input.page_size_bytes {
                    builder = builder.field("page_size_bytes", json!(page_size_bytes));
                }
                if let Some(offset_lines) = input.offset_lines {
                    builder = builder.field("offset_lines", json!(offset_lines));
                }
                if let Some(page_size_lines) = input.page_size_lines {
                    builder = builder.field("page_size_lines", json!(page_size_lines));
                }
                if truncated {
                    builder = builder.field("truncated", json!(true));
                    builder = builder.field("truncation_reason", json!("reached_end_of_file"));
                }
            }

            let response = builder.build_json();
            if let Some(key) = cache_key.as_ref() {
                FILE_CACHE.put_file(key.clone(), response.clone()).await;
            }

            return Ok(response);
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
}

#[cfg(test)]
mod read_tests {
    use super::*;
    use crate::tools::grep_file::GrepSearchManager;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn history_jsonl_detection() {
        assert!(is_history_jsonl(Path::new(
            "/tmp/.vtcode/history/test.jsonl"
        )));
        assert!(!is_history_jsonl(Path::new(
            "/tmp/.vtcode/history/test.txt"
        )));
        assert!(!is_history_jsonl(Path::new("/tmp/history/test.jsonl")));
    }

    #[test]
    fn history_cache_key_varies_with_offset() {
        let temp_dir = TempDir::new().unwrap();
        let history_dir = temp_dir.path().join(".vtcode/history");
        fs::create_dir_all(&history_dir).unwrap();
        let file_path = history_dir.join("session_0001.jsonl");
        fs::write(&file_path, "line1\nline2\n").unwrap();

        let metadata = fs::metadata(&file_path).unwrap();
        let key_a = build_history_cache_key(
            &file_path,
            &metadata,
            &json!({"offset_lines": 0, "page_size_lines": 1}),
        )
        .unwrap();
        let key_b = build_history_cache_key(
            &file_path,
            &metadata,
            &json!({"offset_lines": 1, "page_size_lines": 1}),
        )
        .unwrap();

        assert_ne!(key_a, key_b);
    }

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
