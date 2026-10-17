use super::*;

fn write_test_spool(workspace: &Path, content: &str) -> &'static str {
    let path = ".vtcode/context/tool_outputs/fixture.txt";
    std::fs::create_dir_all(workspace.join(".vtcode/context/tool_outputs")).unwrap();
    std::fs::write(workspace.join(path), content).unwrap();
    path
}

#[tokio::test]
async fn spool_guard_preview_limits_unicode_by_bytes_and_retains_distinct_ends() {
    let workspace = tempfile::tempdir().unwrap();
    let content = format!("{}{}{}", "\u{20000}".repeat(3000), "x".repeat(5000), "漢".repeat(5000));
    let path = write_test_spool(workspace.path(), &content);
    let preview = read_spool_preview_for_guard(workspace.path(), path).await.unwrap();
    // Independently derive the boundary: 2048 four-byte head characters,
    // 2730 three-byte tail characters, and 15618 omitted source bytes.
    assert!(preview.len() <= 16 * 1024 + 64);
    let expected = format!("{}\n\n… [15618 bytes omitted] …\n\n{}", "\u{20000}".repeat(2048), "漢".repeat(2730));
    assert_eq!(preview, expected);
}

#[tokio::test]
async fn spool_guard_feedback_identifies_bounded_prefix_without_full_file_claims() {
    let workspace = tempfile::tempdir().unwrap();
    let content = format!("{}{}{}{}", "A".repeat(8192), "B".repeat(16384), "C".repeat(8192), "D".repeat(8192));
    let path = write_test_spool(workspace.path(), &content);
    let payload = build_spool_chunk_guard_error_content(workspace.path(), path, 3).await;
    let payload: Value = serde_json::from_str(&payload).unwrap();
    let note = payload["inline_content_note"].as_str().unwrap();
    assert!(note.contains("Bounded snapshot of the spool prefix"));
    assert!(note.contains("may omit later content"));
    assert!(note.contains("not proof of completion"));
    assert!(!payload.to_string().contains("Full spool content"));
    let preview = payload["inline_content"].as_str().unwrap();
    assert_eq!(preview, format!("{}\n\n… [16384 bytes omitted] …\n\n{}", "A".repeat(8192), "C".repeat(8192)));
    assert!(!preview.contains('D'), "the actual file tail was never read");
    assert!(!preview.contains("full file is"));
}

#[tokio::test]
async fn spool_guard_preview_keeps_exact_budget_and_bounds_one_extra_byte() {
    let workspace = tempfile::tempdir().unwrap();
    let content = format!("{}{}", "H".repeat(8192), "T".repeat(8192));
    let path = write_test_spool(workspace.path(), &content);
    assert_eq!(read_spool_preview_for_guard(workspace.path(), path).await.unwrap(), content);
    let over_budget = format!("{}x{}", "H".repeat(8192), "T".repeat(8192));
    write_test_spool(workspace.path(), &over_budget);
    assert_eq!(
        read_spool_preview_for_guard(workspace.path(), path).await.unwrap(),
        format!("{}\n\n… [1 bytes omitted] …\n\n{}", "H".repeat(8192), "T".repeat(8192))
    );
}

#[test]
fn previous_turn_error_preview_bounds_unicode_by_bytes() {
    let head = format!("Error:{}", "漢".repeat(1000));
    let payload = build_previous_turn_error_spool_content(".vtcode/context/tool_outputs/error.txt", &head);
    let payload: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["inline_content"], format!("Error:{}... [truncated]", "漢".repeat(339)));
}

#[tokio::test]
async fn spool_guard_cap_preserves_bounded_evidence_in_tool_free_recovery() {
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
    let mut backing = TestTurnProcessingBacking::new(8).await;
    let content = format!("{}{}", "H".repeat(8192), "T".repeat(8192));
    let path = write_test_spool(backing.workspace_path(), &content);
    let mut config = vtcode_core::config::VTCodeConfig::default();
    config.tools.max_sequential_spool_chunk_reads = 1;
    backing.set_vt_cfg_for_test(config);
    let mut ctx = backing.turn_processing_context();
    let args = json!({"path":path,"offset":1,"limit":10});
    assert!(
        enforce_spool_chunk_read_guard(&mut ctx, "first", tool_names::READ_FILE, &args)
            .await
            .is_none()
    );
    assert!(matches!(
        enforce_spool_chunk_read_guard(&mut ctx, "second", tool_names::READ_FILE, &args).await,
        Some(ValidationResult::Blocked)
    ));
    assert!(ctx.recovery_is_tool_free());
    let replies = ctx
        .working_history
        .iter()
        .filter(|message| message.content.as_text().contains("inline_content_note"));
    let replies = replies.collect::<Vec<_>>();
    assert_eq!(replies.len(), 1);
    let payload: Value = serde_json::from_str(&replies[0].content.as_text()).unwrap();
    assert_eq!(payload["inline_content"], content);
    assert!(
        payload["inline_content_note"]
            .as_str()
            .unwrap()
            .contains("not proof of completion")
    );
    assert_eq!(payload["recovery_required"], true);
}

#[cfg(unix)]
#[tokio::test]
async fn spool_guard_preview_rejects_symlink_escape() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_path = outside.path().join("outside.txt");
    std::fs::write(&outside_path, "outside fixture must not appear").unwrap();
    let directory = workspace.path().join(".vtcode/context/tool_outputs");
    std::fs::create_dir_all(&directory).unwrap();
    std::os::unix::fs::symlink(&outside_path, directory.join("escape.txt")).unwrap();
    let path = ".vtcode/context/tool_outputs/escape.txt";
    assert!(read_spool_preview_for_guard(workspace.path(), path).await.is_none());
    let payload = build_spool_chunk_guard_error_content(workspace.path(), path, 3).await;
    let parsed: Value = serde_json::from_str(&payload).unwrap();
    assert!(parsed.get("inline_content").is_none());
    assert!(!payload.contains("outside fixture must not appear"));
}

#[tokio::test]
async fn spool_chunk_guard_error_resolves_to_pty_poll_for_run_prefix() {
    let payload =
        build_spool_chunk_guard_error_content(Path::new("/"), ".vtcode/context/tool_outputs/run-1.txt", 3).await;
    let parsed: Value = serde_json::from_str(&payload).expect("spool chunk guard payload should be json");

    assert_eq!(parsed.get("fallback_tool").and_then(Value::as_str), Some(tool_names::WRITE_STDIN));
    assert_eq!(parsed["fallback_tool_args"]["session_id"], "1");
    assert_eq!(parsed["fallback_tool_args"]["chars"], "");
    assert!(parsed.get("next_action").and_then(Value::as_str).is_some());
    assert_eq!(parsed.get("loop_detected").and_then(Value::as_bool), Some(true));
    assert_eq!(parsed.get("recovery_required").and_then(Value::as_bool), Some(true));
}

#[tokio::test]
async fn spool_chunk_guard_error_resolves_to_search_grep_for_search_prefix() {
    let payload = build_spool_chunk_guard_error_content(
        Path::new("/"),
        ".vtcode/context/tool_outputs/unified_search_1782625284532136.txt",
        3,
    )
    .await;
    let parsed: Value = serde_json::from_str(&payload).expect("spool chunk guard payload should be json");

    assert_eq!(parsed.get("fallback_tool").and_then(Value::as_str), Some(tool_names::EXEC_COMMAND));
    assert!(
        parsed["fallback_tool_args"]["cmd"]
            .as_str()
            .is_some_and(|cmd| cmd.contains("unified_search_1782625284532136.txt"))
    );
    assert_eq!(parsed.get("loop_detected").and_then(Value::as_bool), Some(true));
    assert_eq!(parsed.get("recovery_required").and_then(Value::as_bool), Some(true));
}

#[tokio::test]
async fn spool_chunk_guard_error_falls_back_to_warning_error_todo_for_unknown_prefix() {
    let payload =
        build_spool_chunk_guard_error_content(Path::new("/"), ".vtcode/context/tool_outputs/custom_tool_42.txt", 3)
            .await;
    let parsed: Value = serde_json::from_str(&payload).expect("spool chunk guard payload should be json");

    assert_eq!(parsed.get("fallback_tool").and_then(Value::as_str), Some(tool_names::EXEC_COMMAND));
    assert!(
        parsed["fallback_tool_args"]["cmd"]
            .as_str()
            .is_some_and(|cmd| cmd.contains(SPOOL_CHUNK_GREP_PATTERN))
    );
}

#[test]
fn derive_spool_fallback_recognizes_pty_session_id() {
    let (tool, args) = derive_spool_fallback(".vtcode/context/tool_outputs/run-abc123.txt")
        .expect("pty session spool should resolve to a fallback");
    assert_eq!(tool, tool_names::WRITE_STDIN);
    assert_eq!(args["session_id"], "abc123");
    assert_eq!(args["chars"], "");
}
