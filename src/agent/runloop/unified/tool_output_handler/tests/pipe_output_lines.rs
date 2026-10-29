use super::*;

#[test]
fn build_pipe_command_output_lines_labels_stderr_once() {
    let output = serde_json::json!({
        "stdout": "normal output",
        "stderr": "diagnostic output",
        "exit_code": 1
    });

    assert_eq!(
        build_pipe_command_output_lines(
            tools::EXECUTE_CODE,
            &serde_json::json!({"command": "printf test"}),
            &output,
            None,
            ToolDisplayStatus::Failure,
        ),
        vec![
            "• Ran printf test",
            "  stdout:",
            "    normal output",
            "  stderr:",
            "    diagnostic output",
            "    ✗ run error, exit code: 1",
        ]
    );
}
#[test]
fn build_merged_command_output_lines_keeps_complete_capture_and_status_once() {
    let output = serde_json::json!({
        "exit_code": 2,
        "critical_note": "output was retained in the current session"
    });
    let capture = "stdout line\nstderr line\n";

    let lines = build_merged_command_output_lines(
        tools::RUN_PTY_CMD,
        &serde_json::json!({"command": "long command"}),
        capture,
        None,
        &output,
        ToolDisplayStatus::Failure,
    );

    assert_eq!(lines[0], "• Ran long command");
    assert!(lines.contains(&"  └ stdout line".to_string()));
    assert!(lines.contains(&"    stderr line".to_string()));
    assert!(lines.contains(&"    output was retained in the current session".to_string()));
    assert_eq!(lines.iter().filter(|line| line.contains("stderr line")).count(), 1);
    assert_eq!(lines.iter().filter(|line| line.contains("exit code: 2")).count(), 1);
}
#[test]
fn build_merged_command_output_lines_keeps_distinct_stderr_without_capture() {
    let output = serde_json::json!({"stderr": "diagnostic output"});

    let lines = build_merged_command_output_lines(
        tools::RUN_PTY_CMD,
        &serde_json::json!({"command": "long command"}),
        "",
        None,
        &output,
        ToolDisplayStatus::Success,
    );

    assert_eq!(lines, vec!["• Ran long command", "  stderr:", "    diagnostic output",]);
}
#[test]
fn build_merged_command_output_lines_labels_distinct_stderr_with_merged_output() {
    let output = serde_json::json!({
        "output": "normal output",
        "stderr": "diagnostic output"
    });

    let lines = build_merged_command_output_lines(
        tools::RUN_PTY_CMD,
        &serde_json::json!({"command": "long command"}),
        "normal output\ndiagnostic output\n",
        None,
        &output,
        ToolDisplayStatus::Success,
    );

    assert_eq!(
        lines,
        vec![
            "• Ran long command",
            "  └ normal output",
            "  stderr:",
            "    diagnostic output",
        ]
    );
}
#[test]
fn build_merged_command_output_lines_labels_identical_named_streams_without_merged_output() {
    let output = serde_json::json!({
        "stdout": "same output",
        "stderr": "same output"
    });

    let lines = build_merged_command_output_lines(
        tools::RUN_PTY_CMD,
        &serde_json::json!({"command": "long command"}),
        "same output\nsame output",
        None,
        &output,
        ToolDisplayStatus::Success,
    );

    assert_eq!(
        lines,
        vec![
            "• Ran long command",
            "  stdout:",
            "    same output",
            "  stderr:",
            "    same output",
        ]
    );
}
#[tokio::test]
async fn pty_capture_reads_complete_workspace_spool() {
    let workspace = TempDir::new().expect("workspace temp dir");
    let spool_path = workspace.path().join(".vtcode/context/tool_outputs/pty.txt");
    tokio::fs::create_dir_all(spool_path.parent().expect("spool parent"))
        .await
        .expect("create spool parent");
    let complete_output = "first complete line\nsecond complete line\n";
    tokio::fs::write(&spool_path, complete_output).await.expect("write spool");

    let output = serde_json::json!({
        "spool_path": ".vtcode/context/tool_outputs/pty.txt",
        "spooled_bytes": complete_output.len(),
        "spool_sha256": vtcode_commons::utils::calculate_sha256(complete_output.as_bytes()),
        "spool_state": "completed",
        "output": "first preview line"
    });

    assert_eq!(
        load_complete_output(&output, Some(workspace.path())).await.as_deref(),
        Some("first complete line\nsecond complete line\n")
    );
}
#[tokio::test]
async fn pty_capture_rejects_spool_outside_workspace() {
    let workspace = TempDir::new().expect("workspace temp dir");
    let outside = TempDir::new().expect("outside temp dir");
    let spool_path = outside.path().join("pty.txt");
    tokio::fs::write(&spool_path, "secret outside workspace")
        .await
        .expect("write outside spool");

    let output = serde_json::json!({ "spool_path": spool_path });

    assert!(load_complete_output(&output, Some(workspace.path())).await.is_none());
}
#[tokio::test]
async fn pty_capture_rejects_malformed_spool_metadata_without_inline_fallback() {
    let workspace = TempDir::new().expect("workspace temp dir");
    let output = serde_json::json!({
        "spool_path": null,
        "output": "untrusted inline fallback"
    });

    assert!(load_complete_output(&output, Some(workspace.path())).await.is_none());
}
