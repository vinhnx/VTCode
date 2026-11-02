use super::*;
use tempfile::tempdir;
use vtcode_commons::preview::tail_preview_text;

#[tokio::test]
async fn test_spooler_creation() {
    let temp = tempdir().unwrap();
    let spooler = ToolOutputSpooler::new(temp.path());

    assert!(spooler.config.enabled);
    assert_eq!(spooler.config.threshold_bytes, DEFAULT_SPOOL_THRESHOLD_BYTES);
    assert_eq!(spooler.config.max_age_secs, default_max_age_secs());
}

#[tokio::test]
async fn test_should_spool_small_value() {
    let temp = tempdir().unwrap();
    let spooler = ToolOutputSpooler::new(temp.path());

    let small_value = json!({"result": "ok"});
    assert!(!spooler.should_spool(&small_value));
}

#[tokio::test]
async fn test_should_spool_large_value() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig { threshold_bytes: 100, ..Default::default() }; // Low threshold for testing
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);

    let large_content = "x".repeat(200);
    let large_value = json!({"content": large_content});
    assert!(spooler.should_spool(&large_value));
}

#[tokio::test]
async fn test_should_not_spool_when_disabled() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig { threshold_bytes: 100, ..Default::default() }; // Low threshold for testing
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);

    let large_content = "x".repeat(200);
    let large_value = json!({"output": large_content, "no_spool": true});
    assert!(!spooler.should_spool(&large_value));
}

#[tokio::test]
async fn test_spool_output() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);

    let content = "Line 1\nLine 2\nLine 3\n".repeat(10);
    let value = json!({"output": content});

    let result = spooler.spool_output("test_tool", &value, false).await.unwrap();

    assert!(result.file_path.to_string_lossy().contains("test_tool"));
    assert!(result.original_bytes > 0);

    // Verify file was created
    let full_path = temp.path().join(&result.file_path);
    assert!(full_path.exists());
}

#[tokio::test]
async fn test_process_output_small() {
    let temp = tempdir().unwrap();
    let spooler = ToolOutputSpooler::new(temp.path());

    let small_value = json!({"result": "ok"});
    let result = spooler.process_output("test", small_value.clone(), false).await.unwrap();

    // Should return original value unchanged
    assert_eq!(result, small_value);
}

#[tokio::test]
async fn test_process_output_large() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);

    let large_value = json!({"content": "x".repeat(200)});
    let result = spooler.process_output("test", large_value, false).await.unwrap();

    assert!(result.get("spool_path").is_some());
    assert!(result.get("spooled_to_file").is_none());
    assert!(result.get("content").is_some());
    assert!(result.get("spool_path").is_some());
    assert!(result.get("file_path").is_none());
    assert!(result.get("truncated").is_none());
    assert!(result.get("omitted_bytes").is_none());
}

#[test]
fn pending_bounded_read_trims_incomplete_utf8_character() {
    let temp = tempdir().unwrap();
    let output_dir = temp.path().join(TOOL_OUTPUT_DIR);
    std::fs::create_dir_all(&output_dir).unwrap();
    let spool_path = output_dir.join("pending.txt");
    std::fs::write(&spool_path, "prefix🙂suffix").unwrap();

    let reference_value = json!({
        "spool_path": ".vtcode/context/tool_outputs/pending.txt",
        "spool_state": "pending",
    });
    let reference = SpooledOutputReference::from_value(&reference_value).unwrap();

    // `🙂` is four bytes. Stop after its first byte and retain the valid
    // prefix instead of making the entire preview unavailable.
    let preview = reference.read_pending_bounded(temp.path(), "prefix".len() + 1).unwrap();
    assert_eq!(preview, "prefix");
}

#[tokio::test]
async fn completed_spool_reference_detects_tampering_and_legacy_metadata() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);
    let expected = "x".repeat(200);
    let result = spooler
        .process_output("integrity", json!({"content": expected}), false)
        .await
        .expect("spool output");
    let reference = SpooledOutputReference::from_value(&result).expect("typed spool reference");
    let persisted = reference.read_verified_completed(temp.path()).expect("verified read");
    assert!(persisted.contains(&expected));

    let path = temp.path().join(reference.spool_path);
    std::fs::write(&path, "y".repeat(persisted.len())).expect("tamper fixture");
    assert!(reference.read_verified_completed(temp.path()).is_err());

    let legacy = json!({
        "spool_path": reference.spool_path,
        "spool_complete": true,
    });
    let legacy_reference = SpooledOutputReference::from_value(&legacy).expect("legacy reference");
    assert!(legacy_reference.read_verified_completed(temp.path()).is_err());
}

#[test]
fn test_sanitize_tool_name() {
    assert_eq!(sanitize_tool_name("read_file"), "read_file");
    assert_eq!(sanitize_tool_name("mcp/fetch"), "mcp_fetch");
    assert_eq!(sanitize_tool_name("tool-name"), "tool_name");
}

#[tokio::test]
async fn test_read_file_spools_raw_content() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);

    let file_content = "fn main() {\n    println!(\"Hello, world!\");\n}\n// More code here...";

    // Simulate a read_file response with content field
    let read_file_response = json!({
        "success": true,
        "content": file_content,
        "path": "test.rs"
    });

    let result = spooler.process_output("read_file", read_file_response, false).await.unwrap();

    // Should include source_path for read_file
    let source_path = result.get("source_path").and_then(|v| v.as_str()).unwrap();
    assert_eq!(source_path, "test.rs");

    let content_field = result.get("content").and_then(|v| v.as_str()).unwrap();
    assert!(content_field.contains("fn main()"));
    assert!(!content_field.contains("\"success\"")); // Should not show JSON structure

    let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
    let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
    assert_eq!(spooled_content, file_content);
    assert!(!spooled_content.contains("\"success\"")); // Raw content, not JSON
}

#[tokio::test]
async fn test_run_pty_cmd_spools_raw_output() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);

    let command_output = "   Compiling vtcode-core v0.68.1\n   Checking vtcode-core v0.68.1\n    Finished dev [unoptimized + debuginfo] target(s)";

    // Simulate a run_pty_cmd response with output field
    let pty_response = json!({
        "output": command_output,
        "exit_code": 0,
        "wall_time": 1.234,
        "success": true
    });

    let result = spooler.process_output("run_pty_cmd", pty_response, false).await.unwrap();

    // Should return file reference
    assert!(result.get("spool_path").is_some());
    assert!(result.get("spooled_to_file").is_none());

    // Verify spooled file contains raw output, not JSON wrapper
    let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
    let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
    assert_eq!(spooled_content, command_output);
    assert!(!spooled_content.contains("\"output\""));
    assert!(!spooled_content.contains("\"exit_code\""));
}

#[tokio::test]
async fn test_pty_tools_spool_raw_output() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);

    let command_output = "Some command output text\nwith multiple lines\nfor testing";

    let send_input_response = json!({
        "output": command_output,
        "wall_time": 0.123,
        "session_id": "session123"
    });

    let result = spooler
        .process_output("send_pty_input", send_input_response, false)
        .await
        .unwrap();

    assert!(result.get("spool_path").is_some());
    assert!(result.get("spooled_to_file").is_none());
    let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
    let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
    assert_eq!(spooled_content, command_output);
    assert!(!spooled_content.contains("\"output\""));

    let read_session_response = json!({
        "output": command_output,
        "wall_time": 0.456
    });

    let result = spooler
        .process_output("read_pty_session", read_session_response, false)
        .await
        .unwrap();

    assert!(result.get("spool_path").is_some());
    assert!(result.get("spooled_to_file").is_none());
    let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
    let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
    assert_eq!(spooled_content, command_output);
    assert!(!spooled_content.contains("\"output\""));
}

#[tokio::test]
async fn test_forced_pty_spool_keeps_structured_continuation_metadata() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig { threshold_bytes: 999_999, ..Default::default() }; // ensure only force triggers
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);

    let output = "x".repeat(10_000);
    let value = json!({
        "output": output,
        "process_id": "run-abc123",
        "next_continue_args": {
            "session_id": "run-abc123"
        },
        "truncated": true
    });

    let result = spooler
        .process_output_with_force("run_pty_cmd", value, false, true)
        .await
        .unwrap();

    assert_eq!(result.get("process_id").and_then(|v| v.as_str()), Some("run-abc123"));
    assert_eq!(
        result.get("next_continue_args"),
        Some(&json!({
            "session_id": "run-abc123"
        }))
    );
    assert!(result.get("follow_up_prompt").is_none());
    assert_eq!(result.get("truncated").and_then(|v| v.as_bool()), Some(true));
    assert!(result.get("spooled_to_file").is_none());
    assert!(
        result
            .get("output")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.contains("bytes omitted"))
    );
    assert!(result.get("spool_hint").is_none());
}

#[test]
fn spool_line_counts_handle_empty_chunks_and_unterminated_lines() {
    for (chunks, expected) in [
        (vec![""], 0),
        (vec!["abc", "", "def"], 1),
        (vec!["a\n", "\n", ""], 2),
        (vec!["first\r", "\nsecond\n", "last"], 3),
        (vec!["日本語", "\n", "final"], 2),
    ] {
        let mut counter = SpoolLineCounter::default();
        for chunk in chunks {
            counter.append(chunk.as_bytes());
        }
        assert_eq!(counter.line_count(), expected);
    }
}

#[tokio::test]
async fn test_exec_command_spools_raw_output() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);

    let command_output = "   Compiling vtcode-core v0.68.1\n   Checking vtcode-core v0.68.1\n    Finished dev";

    let exec_command_response = json!({
        "output": command_output,
        "exit_code": 0,
        "wall_time": 1.234,
        "success": true
    });

    let result = spooler
        .process_output(tools::EXEC_COMMAND, exec_command_response, false)
        .await
        .unwrap();

    assert!(result.get("spool_path").is_some());
    assert!(result.get("spooled_to_file").is_none());

    let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
    let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
    assert_eq!(spooled_content, command_output);
    assert_eq!(result["spool_line_count"], 3);
    assert!(result["spool_note"].as_str().unwrap().contains("EOF at line 3"));
    assert!(!spooled_content.contains("\"output\""));
    assert!(!spooled_content.contains("\"exit_code\""));
}

#[tokio::test]
async fn test_exec_command_spools_internal_raw_output_over_preview() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig { threshold_bytes: 10, ..Default::default() };
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);

    let raw_output = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6";
    let preview_output = "line 1\nline 2\n[Output truncated]";
    let exec_command_response = json!({
        "output": preview_output,
        "raw_output": raw_output,
        "truncated": true,
        "exit_code": 0,
        "wall_time": 1.234,
        "success": true
    });

    let result = spooler
        .process_output(tools::EXEC_COMMAND, exec_command_response, false)
        .await
        .unwrap();

    let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
    let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
    assert_eq!(spooled_content, raw_output);
    assert_ne!(spooled_content, preview_output);
    let spool_note = result.get("spool_note").and_then(Value::as_str).unwrap();
    assert!(spool_note.contains("exec_command"));
    assert!(!spool_note.contains("unified_file"));
    assert!(!spool_note.contains("unified_search"));
    assert!(!spool_note.contains("unified_exec"));
}

#[tokio::test]
async fn test_double_serialized_pty_output() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);

    let command_output = "   Compiling vtcode-core v0.68.1\n   Checking vtcode-core v0.68.1\n    Finished dev";

    let inner_json = json!({
        "output": command_output,
        "exit_code": 0,
        "wall_time": 1.234,
        "success": true
    });
    let double_serialized = json!(serde_json::to_string(&inner_json).unwrap());

    let result = spooler.process_output("run_pty_cmd", double_serialized, false).await.unwrap();

    assert!(result.get("spool_path").is_some());
    assert!(result.get("spooled_to_file").is_none());

    let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
    let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
    assert_eq!(spooled_content, command_output);
    assert!(!spooled_content.contains("\"output\""));
}

#[tokio::test]
async fn test_bash_and_shell_spool_raw_output() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);

    let command_output = "total 32\ndrwxr-xr-x  10 user  staff   320 Jan  1 12:00 .";

    let bash_response = json!({
        "output": command_output,
        "exit_code": 0,
        "wall_time": 0.1
    });

    let result = spooler.process_output("bash", bash_response, false).await.unwrap();

    assert!(result.get("spool_path").is_some());
    assert!(result.get("spooled_to_file").is_none());
    let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
    let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
    assert_eq!(spooled_content, command_output);

    let shell_response = json!({
        "output": command_output,
        "exit_code": 0,
        "wall_time": 0.2
    });

    let result = spooler.process_output("shell", shell_response, false).await.unwrap();

    assert!(result.get("spool_path").is_some());
    assert!(result.get("spooled_to_file").is_none());
    let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
    let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
    assert_eq!(spooled_content, command_output);
}

#[test]
fn test_condense_content_short() {
    let short = "a".repeat(CONDENSE_HEAD_BYTES + CONDENSE_TAIL_BYTES);
    let result = condense_content(&short);
    assert_eq!(result, short);
}

#[test]
fn test_condense_content_long() {
    let total = 20_000;
    let long_content = "a".repeat(total);
    let result = condense_content(&long_content);
    assert!(result.contains("bytes omitted"));
    assert!(result.len() < total);
    assert!(result.starts_with(&"a".repeat(100)));
    assert!(result.ends_with(&"a".repeat(100)));
}

#[test]
fn test_condense_content_utf8_boundary() {
    let mut content = "a".repeat(CONDENSE_HEAD_BYTES - 1);
    content.push('é'); // 2-byte char at boundary
    content.push_str(&"b".repeat(20_000));
    let result = condense_content(&content);
    assert!(result.contains("bytes omitted"));
    assert!(result.is_char_boundary(0));
}

#[test]
fn test_tail_preview_content_shows_only_tail() {
    let input = (0..200).map(|i| format!("line-{i}")).collect::<Vec<_>>().join("\n");
    let preview = tail_preview_text(&input, 500, 10);
    assert!(preview.contains("omitted"));
    assert!(preview.contains("line-199"));
    assert!(!preview.contains("line-1\n"));
}

#[test]
fn bounded_previews_share_one_budget_and_respect_tiny_limits() {
    // Asymmetric across shapes: inspection keeps head+tail while
    // verification keeps tail only, but both must honor the same
    // explicit byte limit, and both go empty when it is zero. Content
    // exceeds every budget so truncation actually engages. Note the
    // degenerate edge: at budget 1 the multibyte omitted-notice (`…`)
    // itself does not fit, so verification truncates to nothing; that
    // tail-shape edge predates the shared budget helper.
    let content = (0..200)
        .map(|i| format!("line {i:03} with some payload text"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(content.len() > 1024);
    for budget in [1usize, 64, 100, 1024] {
        let inspection = inspection_preview_content(&content, Some(budget));
        let verification = verification_preview_content(&content, Some(budget));
        assert!(inspection.len() <= budget, "inspection must respect {budget}");
        assert!(verification.len() <= budget, "verification must respect {budget}");
    }
    for budget in [64usize, 100, 1024] {
        assert!(!inspection_preview_content(&content, Some(budget)).is_empty());
        assert!(!verification_preview_content(&content, Some(budget)).is_empty());
    }
    assert!(inspection_preview_content(&content, Some(0)).is_empty());
    assert!(verification_preview_content(&content, Some(0)).is_empty());
}

#[tokio::test]
async fn test_estimate_size_content_field() {
    let temp = tempdir().unwrap();
    let spooler = ToolOutputSpooler::new(temp.path());

    let val = json!({"content": "hello world"});
    assert_eq!(spooler.estimate_size(&val), 11);
}

#[tokio::test]
async fn test_estimate_size_output_field() {
    let temp = tempdir().unwrap();
    let spooler = ToolOutputSpooler::new(temp.path());

    let val = json!({"output": "some output"});
    assert_eq!(spooler.estimate_size(&val), 11);
}

#[tokio::test]
async fn test_estimate_size_string_value() {
    let temp = tempdir().unwrap();
    let spooler = ToolOutputSpooler::new(temp.path());

    let val = json!("raw string");
    assert_eq!(spooler.estimate_size(&val), 10);
}

#[tokio::test]
async fn test_estimate_size_fallback() {
    let temp = tempdir().unwrap();
    let spooler = ToolOutputSpooler::new(temp.path());

    let val = json!({"some_key": 42});
    assert!(spooler.estimate_size(&val) > 0);
}

#[tokio::test]
async fn test_cleanup_old_files_respects_configured_max_age() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig {
        threshold_bytes: 1,
        max_age_secs: 0,
        ..Default::default()
    };
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);
    let value = json!({"output": "old output"});

    let result = spooler.spool_output("test_tool", &value, false).await.unwrap();
    let full_path = temp.path().join(&result.file_path);
    assert!(full_path.exists());

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let removed = spooler.cleanup_old_files().await.unwrap();
    assert_eq!(removed, 1);
    assert!(!full_path.exists());
}

#[tokio::test]
async fn test_cleanup_enforces_max_files_across_sessions() {
    // Simulate leftovers from prior sessions: files exist on disk but are
    // not in this instance's tracking vec. max_files must still bound them.
    let temp = tempdir().unwrap();
    let output_dir = temp.path().join(TOOL_OUTPUT_DIR);
    std::fs::create_dir_all(&output_dir).unwrap();
    for index in 0..8 {
        std::fs::write(output_dir.join(format!("stale_{index}.txt")), b"leftover").unwrap();
        // Ensure distinct mtimes so oldest-first eviction is deterministic.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    let config = SpoolerConfig {
        threshold_bytes: 1,
        // Keep everything young so only the count budget applies.
        max_age_secs: 3600,
        max_files: 3,
        ..Default::default()
    };
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);
    let removed = spooler.cleanup_old_files().await.unwrap();
    assert_eq!(removed, 5, "startup prune must drop overflow past max_files");

    let remaining = std::fs::read_dir(&output_dir).unwrap().count();
    assert_eq!(remaining, 3, "on-disk spool count must respect max_files");
}

#[tokio::test]
async fn test_cleanup_skips_pinned_blocked_turn_outputs() {
    let temp = tempdir().unwrap();
    let config = SpoolerConfig {
        threshold_bytes: 1,
        max_age_secs: 0,
        ..Default::default()
    };
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);
    let value = json!({"output": "blocked output"});

    let result = spooler.spool_output("exec_command", &value, false).await.unwrap();
    let full_path = temp.path().join(&result.file_path);
    assert!(full_path.exists());

    spooler.pin_spool_files(std::slice::from_ref(&full_path)).await;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let removed = spooler.cleanup_old_files().await.unwrap();
    assert_eq!(removed, 0);
    assert!(full_path.exists());

    spooler.unpin_spool_files(std::slice::from_ref(&full_path)).await;
    let removed = spooler.cleanup_old_files().await.unwrap();
    assert_eq!(removed, 1);
    assert!(!full_path.exists());
}

#[tokio::test]
async fn test_cleanup_expired_files_never_enforces_count_budget() {
    // Startup pruning runs in a fresh process whose pin set cannot see a
    // concurrent session's young spools, so it must be age-only: young
    // files beyond max_files stay on disk until the in-session periodic
    // cleanup enforces the budget.
    let temp = tempdir().unwrap();
    let output_dir = temp.path().join(".vtcode").join("context").join("tool_outputs");
    std::fs::create_dir_all(&output_dir).unwrap();
    for i in 0..8 {
        std::fs::write(output_dir.join(format!("young-{i}.out")), b"young").unwrap();
    }

    let config = SpoolerConfig {
        threshold_bytes: 1,
        max_age_secs: 3600,
        max_files: 3,
        ..Default::default()
    };
    let spooler = ToolOutputSpooler::with_config(temp.path(), config);

    let removed = spooler.cleanup_expired_files().await.unwrap();
    assert_eq!(removed, 0, "startup prune must not evict young files past max_files");
    assert_eq!(std::fs::read_dir(&output_dir).unwrap().count(), 8, "all young spools survive the startup prune");

    let removed = spooler.cleanup_old_files().await.unwrap();
    assert_eq!(removed, 5, "in-session cleanup still enforces the count budget");
    assert_eq!(std::fs::read_dir(&output_dir).unwrap().count(), 3);
}
