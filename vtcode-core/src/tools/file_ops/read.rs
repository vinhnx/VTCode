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

const SPOOL_CHUNK_DEFAULT_LIMIT_LINES: usize = 40;
const SPOOL_CHUNK_MAX_LIMIT_LINES: usize = 50;
const SPOOL_CHUNK_SENTINEL_MAX_TOKENS: usize = 4096;

#[derive(Clone, Copy, Debug)]
struct SpoolChunkPlan {
    offset: usize,
    limit: usize,
}

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

fn looks_like_patch_payload(args: &Value) -> bool {
    fn looks_like_patch_text(text: &str) -> bool {
        let trimmed = text.trim_start();
        trimmed.starts_with("*** Begin Patch")
            || trimmed.starts_with("*** Update File:")
            || trimmed.starts_with("*** Add File:")
            || trimmed.starts_with("*** Delete File:")
    }

    args.get("patch")
        .and_then(Value::as_str)
        .is_some_and(looks_like_patch_text)
        || args
            .get("input")
            .and_then(Value::as_str)
            .is_some_and(looks_like_patch_text)
        || args.as_str().is_some_and(looks_like_patch_text)
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

fn parse_usize_value(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .or_else(|| value.as_str().and_then(|s| s.parse::<usize>().ok()))
}

fn has_explicit_limit(args: &Value) -> bool {
    ["limit", "page_size_lines", "max_lines", "chunk_lines"]
        .iter()
        .any(|key| args.get(*key).is_some())
}

fn apply_spool_chunk_defaults(handler_args_json: &mut Value, raw_args: &Value) -> SpoolChunkPlan {
    let mut offset = 1usize;
    let mut limit = SPOOL_CHUNK_DEFAULT_LIMIT_LINES;

    if let Some(obj) = handler_args_json.as_object_mut() {
        offset = obj
            .get("offset")
            .and_then(parse_usize_value)
            .unwrap_or(1)
            .max(1);

        let requested_limit = if has_explicit_limit(raw_args) {
            raw_args
                .get("limit")
                .or_else(|| raw_args.get("page_size_lines"))
                .or_else(|| raw_args.get("max_lines"))
                .or_else(|| raw_args.get("chunk_lines"))
                .and_then(parse_usize_value)
                .unwrap_or(SPOOL_CHUNK_DEFAULT_LIMIT_LINES)
        } else {
            SPOOL_CHUNK_DEFAULT_LIMIT_LINES
        };

        limit = requested_limit.clamp(1, SPOOL_CHUNK_MAX_LIMIT_LINES);

        obj.insert("offset".to_string(), json!(offset));
        obj.insert("limit".to_string(), json!(limit));
        if !obj.contains_key("max_tokens") {
            // Preserve narrow chunking behavior by bypassing MIN_BATCH_LIMIT expansion
            // in ReadFileHandler::handle (which only applies when max_tokens is absent).
            obj.insert(
                "max_tokens".to_string(),
                json!(SPOOL_CHUNK_SENTINEL_MAX_TOKENS),
            );
        }
    }

    SpoolChunkPlan { offset, limit }
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

fn is_tool_output_spool_path(path: &Path) -> bool {
    let mut saw_vtcode = false;
    let mut saw_context = false;

    for component in path.components() {
        let segment = component.as_os_str().to_string_lossy();
        if segment == ".vtcode" {
            saw_vtcode = true;
            saw_context = false;
            continue;
        }
        if saw_vtcode && segment == "context" {
            saw_context = true;
            continue;
        }
        if saw_vtcode && saw_context && segment == "tool_outputs" {
            return true;
        }
    }

    false
}

fn pty_session_id_from_tool_output_path(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_str()?;
    let session_id = file_name.strip_suffix(".txt")?;
    if session_id.starts_with("run-")
        && session_id.len() > "run-".len()
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Some(session_id.to_string())
    } else {
        None
    }
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
            if looks_like_patch_payload(&args) {
                return anyhow!(
                    "Error: Patch content was sent to read_file.\n\
                    Use the patch path instead: unified_file with {{\"action\":\"patch\",\"patch\":\"...\"}} \
                    (or {{\"action\":\"patch\",\"input\":\"...\"}}).\n\
                    read_file requires a path parameter."
                );
            }
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
        let missing_spool_candidate = potential_paths
            .iter()
            .find(|candidate| is_tool_output_spool_path(candidate.as_path()))
            .cloned();

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
            let is_spool_output = is_tool_output_spool_path(&canonical);
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
                let mut handler_args_json = build_read_handler_args(&args, &canonical);
                let spool_plan = if is_spool_output {
                    Some(apply_spool_chunk_defaults(&mut handler_args_json, &args))
                } else {
                    None
                };
                match serde_json::from_value::<ReadFileArgs>(handler_args_json) {
                    Ok(read_args) => {
                        let requested_path = self.workspace_relative_display(&canonical);
                        let handler = ReadFileHandler;
                        let content = handler.handle(read_args).await?;
                        let lines_returned = if content.is_empty() {
                            0usize
                        } else {
                            content.lines().count()
                        };

                        let mut builder = ToolResponseBuilder::new("read_file")
                            .success()
                            .message(format!("Successfully read file {}", requested_path))
                            .content(content)
                            .field("path", json!(requested_path.clone()))
                            .field("no_spool", json!(true))
                            .data("size_bytes", json!(size_bytes));

                        if let Some(plan) = spool_plan {
                            let has_more = lines_returned >= plan.limit;
                            let next_offset = plan.offset.saturating_add(lines_returned);
                            let follow_up_prompt = if has_more {
                                "Use `next_read_args` for the next chunk; use `grep_file` for targeted matches."
                                    .to_string()
                            } else {
                                format!(
                                    "End of spooled output at line {}.",
                                    next_offset.saturating_sub(1)
                                )
                            };

                            builder = builder
                                .field("spool_chunked", json!(true))
                                .field("chunk_limit", json!(plan.limit))
                                .field("lines_returned", json!(lines_returned))
                                .field("has_more", json!(has_more))
                                .field("next_offset", json!(next_offset))
                                .field("follow_up_prompt", json!(follow_up_prompt));
                            if has_more {
                                builder = builder.field(
                                    "next_read_args",
                                    json!({
                                        "path": requested_path.clone(),
                                        "offset": next_offset,
                                        "limit": plan.limit
                                    }),
                                );
                            }
                        }

                        let response = builder.build_json();

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
                .field("path", json!(self.workspace_relative_display(&canonical)))
                .field("no_spool", json!(true));

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

        if let Some(spool_path) = missing_spool_candidate {
            if let Some(session_id) = pty_session_id_from_tool_output_path(&spool_path) {
                return Err(anyhow!(
                    "Error: Session output file not found: {}. This looks like a PTY session id. Use read_pty_session with session_id=\"{}\" instead of read_file.",
                    self.workspace_relative_display(&spool_path),
                    session_id,
                ));
            }
            return Err(anyhow!(
                "Error: Spool file not found (possibly expired): {}. Re-run the original tool command to regenerate this output.",
                self.workspace_relative_display(&spool_path),
            ));
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
    fn tool_output_spool_path_detection() {
        assert!(is_tool_output_spool_path(Path::new(
            ".vtcode/context/tool_outputs/run-123.txt"
        )));
        assert!(is_tool_output_spool_path(Path::new(
            "/tmp/work/.vtcode/context/tool_outputs/run-123.txt"
        )));
        assert!(!is_tool_output_spool_path(Path::new(
            ".vtcode/history/session.jsonl"
        )));
    }

    #[test]
    fn pty_session_id_detection_from_tool_output_path() {
        assert_eq!(
            pty_session_id_from_tool_output_path(Path::new(
                ".vtcode/context/tool_outputs/run-658ceef2.txt"
            )),
            Some("run-658ceef2".to_string())
        );
        assert_eq!(
            pty_session_id_from_tool_output_path(Path::new(
                ".vtcode/context/tool_outputs/unified_exec_123.txt"
            )),
            None
        );
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
            result["metadata"]["data"]["applied_max_tokens"]
                .as_u64()
                .unwrap(),
            max_tokens as u64
        );
    }

    #[tokio::test]
    async fn test_read_file_patch_payload_returns_actionable_error() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();
        let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops = FileOpsTool::new(workspace_root, grep_manager);

        let args = json!({
            "input": "*** Begin Patch\n*** End Patch\n"
        });
        let err = file_ops.read_file(args).await.unwrap_err().to_string();

        assert!(err.contains("Patch content was sent to read_file"));
        assert!(err.contains("\"action\":\"patch\""));
    }

    #[tokio::test]
    async fn test_missing_spool_file_returns_actionable_error() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();
        let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops = FileOpsTool::new(workspace_root, grep_manager);

        let args = json!({
            "path": ".vtcode/context/tool_outputs/unified_exec_123.txt"
        });
        let err = file_ops.read_file(args).await.unwrap_err().to_string();

        assert!(err.contains("Spool file not found"));
        assert!(err.contains("Re-run the original tool command"));
    }

    #[tokio::test]
    async fn test_missing_run_session_file_suggests_read_pty_session() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();
        let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops = FileOpsTool::new(workspace_root, grep_manager);

        let args = json!({
            "path": ".vtcode/context/tool_outputs/run-123abc.txt"
        });
        let err = file_ops.read_file(args).await.unwrap_err().to_string();

        assert!(err.contains("Session output file not found"));
        assert!(err.contains("read_pty_session"));
        assert!(err.contains("run-123abc"));
    }

    #[tokio::test]
    async fn test_spool_file_reads_are_chunked_with_next_offset() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();
        let spool_dir = workspace_root.join(".vtcode/context/tool_outputs");
        std::fs::create_dir_all(&spool_dir).unwrap();
        let spool_file = spool_dir.join("unified_exec_123.txt");
        let spool_content = (1..=120)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&spool_file, spool_content).unwrap();

        let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops = FileOpsTool::new(workspace_root, grep_manager);

        let args = json!({
            "path": ".vtcode/context/tool_outputs/unified_exec_123.txt"
        });

        let result = file_ops.read_file(args).await.unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["spool_chunked"], true);
        assert_eq!(result["chunk_limit"], SPOOL_CHUNK_DEFAULT_LIMIT_LINES);
        assert_eq!(result["lines_returned"], SPOOL_CHUNK_DEFAULT_LIMIT_LINES);
        assert_eq!(result["has_more"], true);
        assert_eq!(result["next_offset"], SPOOL_CHUNK_DEFAULT_LIMIT_LINES + 1);
        assert_eq!(
            result["next_read_args"],
            json!({
                "path": ".vtcode/context/tool_outputs/unified_exec_123.txt",
                "offset": SPOOL_CHUNK_DEFAULT_LIMIT_LINES + 1,
                "limit": SPOOL_CHUNK_DEFAULT_LIMIT_LINES
            })
        );
        assert!(
            result["follow_up_prompt"]
                .as_str()
                .unwrap_or_default()
                .contains("next_read_args")
        );
    }
}
