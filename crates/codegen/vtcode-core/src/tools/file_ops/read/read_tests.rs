use super::*;
use crate::tools::grep_file::GrepSearchManager;
use crate::tools::types::Input;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn history_jsonl_detection() {
    assert!(is_history_jsonl(Path::new("/tmp/.vtcode/history/test.jsonl")));
    assert!(!is_history_jsonl(Path::new("/tmp/.vtcode/history/test.txt")));
    assert!(!is_history_jsonl(Path::new("/tmp/history/test.jsonl")));
    assert!(!is_history_jsonl(Path::new("/tmp/.vtcode_evil/history/test.jsonl")));
    assert!(!is_history_jsonl(Path::new("/tmp/.vtcode/history_evil/test.jsonl")));
}

#[test]
fn tool_output_spool_path_detection() {
    assert!(is_tool_output_spool_path(Path::new(".vtcode/context/tool_outputs/run-123.txt")));
    assert!(is_tool_output_spool_path(Path::new("/tmp/work/.vtcode/context/tool_outputs/run-123.txt")));
    assert!(!is_tool_output_spool_path(Path::new(".vtcode/history/session.jsonl")));
    assert!(!is_tool_output_spool_path(Path::new(".vtcode_evil/context/tool_outputs/run-123.txt")));
    assert!(!is_tool_output_spool_path(Path::new(".vtcode/context/tool_outputs_evil/run-123.txt")));
    assert!(!is_tool_output_spool_path(Path::new("/tmp/.vtcode/context/tool_outputs_malicious/run-123.txt")));
}

#[test]
fn pty_session_id_detection_from_tool_output_path() {
    assert_eq!(
        pty_session_id_from_tool_output_path(Path::new(".vtcode/context/tool_outputs/run-658ceef2.txt")),
        Some("run-658ceef2".to_string())
    );
    assert_eq!(
        pty_session_id_from_tool_output_path(Path::new(".vtcode/context/tool_outputs/command_session_123.txt")),
        None
    );
}

#[test]
fn build_read_handler_args_normalizes_zero_bounds() {
    let canonical = Path::new("/tmp/example.txt");
    let args = json!({
        "path": "example.txt",
        "offset": 0,
        "limit": 0
    });

    let built = build_read_handler_args(&args, canonical);

    assert_eq!(built["file_path"], json!("/tmp/example.txt"));
    assert_eq!(built["offset"], json!(1));
    assert!(built.get("limit").is_none());
}

#[test]
fn build_read_handler_args_avoids_duplicate_path_aliases() {
    let canonical = Path::new("/tmp/example.txt");
    let args = json!({
        "path": "example.txt"
    });

    let built = build_read_handler_args(&args, canonical);
    let parsed: ReadFileArgs = serde_json::from_value(built).unwrap();

    assert_eq!(parsed.file_path, "/tmp/example.txt");
    assert_eq!(parsed.offset, 1);
}

#[test]
fn build_read_handler_args_normalizes_indentation() {
    let canonical = Path::new("/tmp/example.txt");

    let args_true = json!({
        "path": "example.txt",
        "indentation": true
    });
    let built_true = build_read_handler_args(&args_true, canonical);
    assert_eq!(built_true["mode"], json!("indentation"));
    assert_eq!(built_true["indentation"], json!({}));

    let args_false = json!({
        "path": "example.txt",
        "indentation": false,
        "mode": "slice"
    });
    let built_false = build_read_handler_args(&args_false, canonical);
    assert!(built_false.get("indentation").is_none());
    assert_eq!(built_false["mode"], json!("slice"));

    let args_false_indent_mode = json!({
        "path": "example.txt",
        "indentation": false,
        "mode": "indentation"
    });
    let built_false_indent_mode = build_read_handler_args(&args_false_indent_mode, canonical);
    assert!(built_false_indent_mode.get("indentation").is_none());
    assert_eq!(built_false_indent_mode["mode"], json!("slice"));

    let args_obj = json!({
        "path": "example.txt",
        "indentation": { "max_levels": 2 }
    });
    let built_obj = build_read_handler_args(&args_obj, canonical);
    assert_eq!(built_obj["mode"], json!("indentation"));
    assert_eq!(built_obj["indentation"]["max_levels"], json!(2));

    let args_true_str = json!({
        "path": "example.txt",
        "indentation": "true"
    });
    let built_true_str = build_read_handler_args(&args_true_str, canonical);
    assert_eq!(built_true_str["mode"], json!("indentation"));
    assert_eq!(built_true_str["indentation"], json!({}));

    let args_false_str = json!({
        "path": "example.txt",
        "indentation": "false"
    });
    let built_false_str = build_read_handler_args(&args_false_str, canonical);
    assert!(built_false_str.get("indentation").is_none());
}

#[test]
fn build_read_handler_args_normalizes_mode() {
    let canonical = Path::new("/tmp/example.txt");

    let args_empty = json!({
        "path": "example.txt",
        "mode": ""
    });
    let built_empty = build_read_handler_args(&args_empty, canonical);
    assert_eq!(built_empty["mode"], json!("slice"));

    let args_whitespace = json!({
        "path": "example.txt",
        "mode": "   "
    });
    let built_whitespace = build_read_handler_args(&args_whitespace, canonical);
    assert_eq!(built_whitespace["mode"], json!("slice"));

    let args_unknown = json!({
        "path": "example.txt",
        "mode": "unknown"
    });
    let built_unknown = build_read_handler_args(&args_unknown, canonical);
    assert_eq!(built_unknown["mode"], json!("slice"));

    let args_indent = json!({
        "path": "example.txt",
        "mode": "Indentation"
    });
    let built_indent = build_read_handler_args(&args_indent, canonical);
    assert_eq!(built_indent["mode"], json!("indentation"));
}

#[test]
fn build_read_handler_args_maps_start_end_to_offset_limit() {
    let canonical = Path::new("/tmp/example.txt");

    // start_line/end_line -> offset/limit
    let args = json!({
        "path": "example.txt",
        "start_line": 550,
        "end_line": 590
    });
    let built = build_read_handler_args(&args, canonical);
    assert_eq!(built["offset"], json!(550));
    assert_eq!(built["limit"], json!(41));

    // start_line only -> offset set, limit unchanged
    let args_start_only = json!({
        "path": "example.txt",
        "start_line": 100
    });
    let built_start_only = build_read_handler_args(&args_start_only, canonical);
    assert_eq!(built_start_only["offset"], json!(100));
    assert!(built_start_only.get("limit").is_none());

    // explicit offset takes precedence over start_line
    let args_explicit = json!({
        "path": "example.txt",
        "offset": 10,
        "start_line": 550,
        "end_line": 590
    });
    let built_explicit = build_read_handler_args(&args_explicit, canonical);
    assert_eq!(built_explicit["offset"], json!(10));
    assert_eq!(built_explicit["limit"], json!(41));
}

#[test]
fn history_cache_key_varies_with_offset() {
    let temp_dir = TempDir::new().unwrap();
    let history_dir = temp_dir.path().join(".vtcode/history");
    fs::create_dir_all(&history_dir).unwrap();
    let file_path = history_dir.join("session_0001.jsonl");
    fs::write(&file_path, "line1\nline2\n").unwrap();

    let metadata = fs::metadata(&file_path).unwrap();
    let key_a =
        build_history_cache_key(&file_path, &metadata, &json!({"offset_lines": 0, "page_size_lines": 1})).unwrap();
    let key_b =
        build_history_cache_key(&file_path, &metadata, &json!({"offset_lines": 1, "page_size_lines": 1})).unwrap();

    assert_ne!(key_a, key_b);
}

#[tokio::test]
async fn test_read_file_paging_lines() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let test_file = workspace_root.join("test_file.txt");

    // Create test content with 10 lines
    let test_content = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10\n";
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
async fn test_read_file_paging_lines_clamps_page_size_to_absolute_cap() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let test_file = workspace_root.join("over_cap.txt");
    let absolute_cap = crate::tools::read_limits::absolute_line_cap();
    let test_content = (1..=absolute_cap + 1)
        .map(|line| format!("line{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&test_file, test_content).unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    let result = file_ops
        .read_file(json!({
            "path": test_file.to_string_lossy().into_owned(),
            "offset_lines": 0,
            "page_size_lines": absolute_cap + 1
        }))
        .await
        .unwrap();

    assert_eq!(result["content"].as_str().unwrap().lines().count(), absolute_cap);
    assert_eq!(result["truncated"], true);
}

#[tokio::test]
async fn test_read_file_paging_lines_preserves_lossy_utf8_behavior() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let test_file = workspace_root.join("invalid_utf8.txt");
    fs::write(&test_file, b"before\n\xffafter\n").unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    let result = file_ops
        .read_file(json!({
            "path": test_file.to_string_lossy().into_owned(),
            "offset_lines": 1,
            "page_size_lines": 1
        }))
        .await
        .unwrap();

    assert_eq!(result["content"].as_str().unwrap(), "\u{fffd}after");
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

    // Test byte-based paging: skip first 6 bytes ("line1\n") and read next bytes
    // The new handler aligns to line boundaries and adds line numbers
    let args = json!({
        "path": test_file.to_string_lossy().into_owned(),
        "offset_bytes": 6,
        "page_size_bytes": 6
    });

    let result = file_ops.read_file(args).await.unwrap();
    assert!(result["success"].as_bool().unwrap());
    // New handler returns line-numbered content aligned to line boundaries
    let content = result["content"].as_str().unwrap();
    assert!(content.contains("2: line2"));
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
async fn test_legacy_full_read_respects_absolute_cap() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let test_file = workspace_root.join("big_legacy.txt");
    let content = (1..=1000).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
    fs::write(&test_file, content).unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    // Bare legacy request (no max_* keys) would otherwise dump the whole file.
    let input: Input = serde_json::from_value(json!({ "path": test_file.to_string_lossy() })).unwrap();
    let (read_content, metadata, truncated) = file_ops.read_file_legacy(&test_file, &input).await.unwrap();

    let cap = crate::tools::read_limits::read_limit_lines();
    assert_eq!(read_content.lines().count(), cap);
    assert!(truncated);
    assert_eq!(metadata["is_truncated"], true);
    assert_eq!(metadata["applied_max_lines"], cap);
}

#[tokio::test]
async fn test_legacy_full_read_bounds_oversized_lines() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let test_file = workspace_root.join("giant_line.txt");
    let giant_line = "x".repeat(crate::tools::file_ops::MAX_LINE_READ_BYTES + 10_000);
    fs::write(&test_file, format!("{giant_line}\nlast\n")).unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);
    let input: Input = serde_json::from_value(json!({ "path": test_file.to_string_lossy() })).unwrap();

    let (content, metadata, truncated) = file_ops.read_file_legacy(&test_file, &input).await.unwrap();

    assert!(truncated, "the bounded physical line makes the response incomplete");
    assert_eq!(metadata["size_lines"], 2);
    assert_eq!(metadata["line_truncated"], true);
    assert!(content.len() <= crate::tools::file_ops::MAX_LINE_READ_BYTES + "\nlast\n".len());
    assert!(content.ends_with("\nlast\n"));

    let paged = file_ops
        .read_file(json!({
            "path": test_file.to_string_lossy(),
            "offset_lines": 0,
            "page_size_lines": 2
        }))
        .await
        .unwrap();
    assert_eq!(paged["truncation_reason"], "oversized_line");
}

#[tokio::test]
async fn test_legacy_full_read_preserves_trailing_newline() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let test_file = workspace_root.join("legacy_newline.txt");
    fs::write(&test_file, "hello\n").unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);
    let input: Input = serde_json::from_value(json!({ "path": test_file.to_string_lossy() })).unwrap();

    let (content, _metadata, truncated) = file_ops.read_file_legacy(&test_file, &input).await.unwrap();

    assert_eq!(content, "hello\n");
    assert!(!truncated);
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
async fn test_read_file_full_text_preserves_trailing_newline() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let test_file = workspace_root.join("note.txt");
    fs::write(&test_file, "hello\n").unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    let result = file_ops.read_file(json!({ "path": "note.txt" })).await.unwrap();

    assert_eq!(result["content"].as_str(), Some("hello\n"));
}

#[tokio::test]
async fn test_read_file_legacy_token_chunking() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let test_file = workspace_root.join("test_file.txt");

    // Create test content with 50 lines
    let test_content = (1..=50).map(|i| format!("line-{i}")).collect::<Vec<_>>().join("\n") + "\n";
    fs::write(&test_file, test_content).unwrap();

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
    #[allow(
        clippy::cast_sign_loss,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    let expected_max_tokens = max_tokens as u64; // safe: max_tokens is positive literal
    assert_eq!(result["metadata"]["data"]["applied_max_tokens"].as_u64().unwrap(), expected_max_tokens,);
}

#[tokio::test]
async fn test_read_file_legacy_token_limit_also_clamps_explicit_max_lines() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let test_file = workspace_root.join("explicit_line_limit.txt");
    fs::write(&test_file, (1..=5).map(|i| format!("line-{i}")).collect::<Vec<_>>().join("\n")).unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    let result = file_ops
        .read_file(json!({
            "path": test_file.to_string_lossy().into_owned(),
            "max_lines": 5,
            "max_tokens": 30
        }))
        .await
        .unwrap();

    assert!(result["is_truncated"].as_bool().unwrap());
    assert!(result["content"].as_str().unwrap().contains("... 3 lines omitted ..."));
}

#[tokio::test]
async fn test_read_file_legacy_chunking_bounds_oversized_lines() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let test_file = workspace_root.join("giant_chunked_line.txt");
    let giant_line = "x".repeat(crate::tools::file_ops::MAX_LINE_READ_BYTES + 10_000);
    fs::write(&test_file, format!("{giant_line}\nmiddle\nlast\n")).unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    let result = file_ops
        .read_file(json!({
            "path": test_file.to_string_lossy().into_owned(),
            "max_lines": 2
        }))
        .await
        .unwrap();

    let content = result["content"].as_str().unwrap();
    assert!(result["is_truncated"].as_bool().unwrap());
    assert!(content.lines().next().unwrap().len() <= crate::tools::file_ops::MAX_LINE_READ_BYTES);
    assert!(content.contains("... 1 lines omitted ..."));
    assert!(content.ends_with("last"));
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
    assert!(err.contains("apply_patch"));
    assert!(err.contains("\"patch\":\"...\""));
}

#[tokio::test]
async fn test_missing_spool_file_returns_actionable_error() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    let args = json!({
        "path": ".vtcode/context/tool_outputs/command_session_123.txt"
    });
    let err = file_ops.read_file(args).await.unwrap_err().to_string();

    assert!(err.contains("Spool file not found"));
    assert!(err.contains("Re-run the original tool command"));
}

#[tokio::test]
async fn test_missing_run_session_file_suggests_write_stdin() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    let args = json!({
        "path": ".vtcode/context/tool_outputs/run-123abc.txt"
    });
    let err = file_ops.read_file(args).await.unwrap_err().to_string();

    assert!(err.contains("Session output file not found"));
    assert!(err.contains("write_stdin"));
    assert!(err.contains("run-123abc"));
}

#[tokio::test]
async fn test_missing_file_suggests_similar_workspace_file() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    fs::create_dir_all(workspace_root.join("src")).unwrap();
    fs::write(workspace_root.join("src/tool_exec.rs"), "fn main() {}\n").unwrap();
    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    let args = json!({
        "path": "src/tool_exe.rs"
    });
    let err = file_ops.read_file(args).await.unwrap_err().to_string();

    assert!(err.contains("Did you mean"));
    assert!(err.contains("src/tool_exec.rs"));
}

#[tokio::test]
async fn test_directory_path_returns_actionable_list_guidance() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    fs::create_dir_all(workspace_root.join("src/agent")).unwrap();
    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    let args = json!({
        "path": "src"
    });
    let err = file_ops.read_file(args).await.unwrap_err().to_string();

    assert!(err.contains("is a directory, not a file"));
    assert!(err.contains("exec_command.cmd"));
    assert!(err.contains("find"));
    assert!(err.contains("ls"));
}

#[tokio::test]
async fn test_spool_file_reads_are_chunked_with_next_offset() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let spool_dir = workspace_root.join(".vtcode/context/tool_outputs");
    fs::create_dir_all(&spool_dir).unwrap();
    let spool_file = spool_dir.join("command_session_123.txt");
    let spool_content = (1..=120).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
    fs::write(&spool_file, spool_content).unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    let args = json!({
        "path": ".vtcode/context/tool_outputs/command_session_123.txt"
    });

    let result = file_ops.read_file(args).await.unwrap();
    assert_eq!(result["success"], true);
    assert_eq!(result["spool_chunked"], true);
    assert_eq!(result["lines_returned"], SPOOL_CHUNK_DEFAULT_LIMIT_LINES);
    assert_eq!(result["has_more"], true);
    assert_eq!(
        result["next_read_args"],
        json!({
            "path": ".vtcode/context/tool_outputs/command_session_123.txt",
            "offset": SPOOL_CHUNK_DEFAULT_LIMIT_LINES + 1,
            "limit": SPOOL_CHUNK_DEFAULT_LIMIT_LINES
        })
    );
    assert!(result.get("chunk_limit").is_none());
    assert!(result.get("next_offset").is_none());
    assert!(result.get("preferred_next_action").is_none());
    assert!(result.get("follow_up_prompt").is_none());
}

#[tokio::test]
async fn test_spool_file_last_chunk_omits_continuation_fields() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let spool_dir = workspace_root.join(".vtcode/context/tool_outputs");
    fs::create_dir_all(&spool_dir).unwrap();
    let spool_file = spool_dir.join("command_session_999.txt");
    let spool_content = (1..=5).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
    fs::write(&spool_file, spool_content).unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    let args = json!({
        "path": ".vtcode/context/tool_outputs/command_session_999.txt"
    });

    let result = file_ops.read_file(args).await.unwrap();
    assert_eq!(result["success"], true);
    assert_eq!(result["spool_chunked"], true);
    assert_eq!(result["lines_returned"], 5);
    assert!(result.get("has_more").is_none());
    assert!(result.get("next_read_args").is_none());
    assert!(result.get("follow_up_prompt").is_none());
    assert!(result.get("chunk_limit").is_none());
    assert!(result.get("next_offset").is_none());
    assert!(result.get("preferred_next_action").is_none());
}

#[tokio::test]
async fn test_spool_file_exact_limit_at_eof_omits_continuation_fields() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let spool_dir = workspace_root.join(".vtcode/context/tool_outputs");
    fs::create_dir_all(&spool_dir).unwrap();
    let spool_file = spool_dir.join("command_session_exact.txt");
    let spool_content = (1..=SPOOL_CHUNK_DEFAULT_LIMIT_LINES)
        .map(|i| format!("line{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&spool_file, spool_content).unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    let result = file_ops
        .read_file(json!({
            "path": ".vtcode/context/tool_outputs/command_session_exact.txt"
        }))
        .await
        .unwrap();

    assert_eq!(result["success"], true);
    assert_eq!(result["spool_chunked"], true);
    assert_eq!(result["lines_returned"], SPOOL_CHUNK_DEFAULT_LIMIT_LINES);
    assert!(result.get("has_more").is_none());
    assert!(result.get("next_read_args").is_none());
}

#[tokio::test]
async fn test_read_file_absolute_cap_adds_continuation() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let test_file = workspace_root.join("big.txt");
    let content = (1..=1000).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
    fs::write(&test_file, content).unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    // Request far more than the cap without pagination.
    let result = file_ops
        .read_file(json!({
            "path": "big.txt",
            "limit": 2000
        }))
        .await
        .unwrap();

    assert_eq!(result["success"], true);
    assert_eq!(result["capped_by_limit"], true);
    assert_eq!(result["has_more"], true);
    assert!(result["next_read_args"].is_object());
    assert_eq!(result["next_read_args"]["path"], "big.txt");
    // offset 1 + applied cap (default 400) lines read.
    assert_eq!(result["next_read_args"]["offset"], 401);
    assert_eq!(result["next_read_args"]["limit"], crate::tools::read_limits::read_limit_lines());
}

#[tokio::test]
async fn path_only_read_uses_configured_cap_for_continuation() {
    let original_config = file_read_cache_config();
    let mut configured = original_config.clone();
    configured.max_read_lines = 800;
    crate::tools::cache::configure_file_cache(&configured);

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let test_file = workspace_root.join("configured-cap.txt");
    let content = (1..=1000).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
    fs::write(&test_file, content).unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);
    let result = file_ops.read_file(json!({ "path": "configured-cap.txt" })).await;

    crate::tools::cache::configure_file_cache(&original_config);
    let result = result.unwrap();

    assert_eq!(result["has_more"], true);
    assert_eq!(result["next_read_args"]["offset"], 801);
    assert_eq!(result["next_read_args"]["limit"], 800);
}

#[tokio::test]
async fn test_read_file_cap_continuation_chains_across_pages() {
    // Regresses the bug where the continuation chain died after the first
    // clamped page: follow-up pages pass `applied_limit` back as `limit`,
    // so `capped_by_limit` is false and no `next_read_args` was emitted.
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let test_file = workspace_root.join("big.txt");
    let content = (1..=1000).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
    fs::write(&test_file, content).unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    // Page 1: clamps the oversized request to the cap.
    let page1 = file_ops.read_file(json!({ "path": "big.txt", "limit": 2000 })).await.unwrap();
    assert_eq!(page1["capped_by_limit"], true);
    assert_eq!(page1["has_more"], true);
    assert_eq!(page1["next_read_args"]["offset"], 401);
    let cap = crate::tools::read_limits::read_limit_lines();

    // Page 2: follows the continuation. `capped_by_limit` is now false, but
    // the chain must still surface `next_read_args` because the read hit
    // the ceiling again.
    let page2 = file_ops
        .read_file(json!({
            "path": "big.txt",
            "offset": page1["next_read_args"]["offset"],
            "limit": page1["next_read_args"]["limit"],
        }))
        .await
        .unwrap();
    assert_eq!(page2["capped_by_limit"], false);
    assert_eq!(page2["has_more"], true);
    assert!(page2["next_read_args"].is_object());
    assert_eq!(page2["next_read_args"]["offset"], 401 + cap);
    assert_eq!(page2["next_read_args"]["limit"], cap);

    // Page 3: final partial chunk, no more continuation.
    let page3 = file_ops
        .read_file(json!({
            "path": "big.txt",
            "offset": page2["next_read_args"]["offset"],
            "limit": page2["next_read_args"]["limit"],
        }))
        .await
        .unwrap();
    assert!(page3.get("has_more").is_none());
    assert!(page3.get("next_read_args").is_none());
}

#[tokio::test]
async fn test_read_file_accepts_compact_spool_continuation_args() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let spool_dir = workspace_root.join(".vtcode/context/tool_outputs");
    fs::create_dir_all(&spool_dir).unwrap();
    let spool_file = spool_dir.join("command_session_456.txt");
    let spool_content = (1..=120).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
    fs::write(&spool_file, spool_content).unwrap();

    let grep_manager = std::sync::Arc::new(GrepSearchManager::new(workspace_root.clone()));
    let file_ops = FileOpsTool::new(workspace_root, grep_manager);

    let result = file_ops
        .read_file(json!({
            "p": ".vtcode/context/tool_outputs/command_session_456.txt",
            "o": 81,
            "l": 40
        }))
        .await
        .unwrap();

    assert_eq!(result["success"], true);
    assert_eq!(result["path"], ".vtcode/context/tool_outputs/command_session_456.txt");
    assert_eq!(result["spool_chunked"], true);
    assert_eq!(result["lines_returned"], 40);
    assert_eq!(result["content"].as_str().unwrap().lines().next(), Some("line81"));
    assert!(result.get("has_more").is_none());
    assert!(result.get("next_read_args").is_none());
}

#[tokio::test]
async fn bounded_line_read_truncates_oversized_lines_and_stays_aligned() {
    use tokio::io::BufReader;

    // A physical line longer than MAX_LINE_READ_BYTES must be truncated for
    // display while the reader stays aligned at the next newline.
    let long_line = "x".repeat(crate::tools::file_ops::MAX_LINE_READ_BYTES + 10_000);
    let body = format!("{long_line}\nsecond\nthird\n");
    let cursor = std::io::Cursor::new(body.into_bytes());
    let mut reader = BufReader::new(cursor);
    let mut buffer = Vec::new();

    let first = crate::tools::file_ops::read_bounded_line(&mut reader, &mut buffer)
        .await
        .unwrap()
        .expect("first line present");
    assert!(first, "oversized line must report truncation");
    assert!(buffer.len() <= crate::tools::file_ops::MAX_LINE_READ_BYTES);

    let second = crate::tools::file_ops::read_bounded_line(&mut reader, &mut buffer)
        .await
        .unwrap()
        .expect("second line present");
    assert!(!second, "normal line must not report truncation");
    assert_eq!(buffer, b"second\n");

    let third = crate::tools::file_ops::read_bounded_line(&mut reader, &mut buffer)
        .await
        .unwrap()
        .expect("third line present");
    assert!(!third);
    assert_eq!(buffer, b"third\n");

    assert!(
        crate::tools::file_ops::read_bounded_line(&mut reader, &mut buffer)
            .await
            .unwrap()
            .is_none(),
        "stream must be exhausted after the third line"
    );
}
