use super::*;

#[tokio::test]
async fn render_tool_output_does_not_duplicate_spooled_output_hint() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let spool_path = ".vtcode/context/tool_outputs/web.txt";
    let hint = spooled_output_hint(spool_path);
    let payload = json!({
        "output": hint,
        "spool_path": spool_path
    });

    render_tool_output(&mut renderer, Some("custom_tool"), &payload, None)
        .await
        .expect("spooled hint payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert_eq!(inline_output.matches("Large output was spooled to").count(), 1);
    assert!(inline_output.contains("exec_command"));
    assert!(inline_output.contains("cat, sed, or rg"));
}
#[tokio::test]
async fn render_tool_output_read_file_long_preview_keeps_preview_limits() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let content = (1..=100).map(|idx| format!("{idx}: line {idx}")).collect::<Vec<_>>().join("\n");
    let payload = json!({
        "path": "src/main.rs",
        "content": content
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::READ_FILE), &payload, None)
        .await
        .expect("read_file preview payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    // read_file now shows a summary line instead of code preview
    assert!(inline_output.contains("Read 100 lines"));
    assert!(inline_output.contains("└ Read 100 lines"));
    assert!(!inline_output.contains("    Read 100 lines"));
}
#[tokio::test]
async fn render_tool_output_renders_loop_recovery_hint_from_structured_fields() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "loop_detected": true,
        "fallback_tool": vtcode_core::config::constants::tools::CODE_SEARCH
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::CODE_SEARCH), &payload, None)
        .await
        .expect("loop recovery hint payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("Loop detected; fallback is available."));
}
#[tokio::test]
async fn render_tool_output_renders_spooled_loop_recovery_hint() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "loop_detected": true,
        "spool_path": ".vtcode/context/tool_outputs/readme.txt",
        "next_read_args": {
            "path": ".vtcode/context/tool_outputs/readme.txt",
            "offset": 81,
            "limit": 40
        }
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::READ_FILE), &payload, None)
        .await
        .expect("spooled loop recovery hint payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("Loop detected; continue from spooled output."));
}
#[tokio::test]
async fn render_tool_output_does_not_duplicate_loop_recovery_hint() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "loop_detected": true,
        "fallback_tool": vtcode_core::config::constants::tools::CODE_SEARCH,
        "output": "Loop detected; fallback is available."
    });

    render_tool_output(&mut renderer, Some("custom_tool"), &payload, None)
        .await
        .expect("duplicate hint payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert_eq!(inline_output.matches("Loop detected; fallback is available.").count(), 1);
}
#[tokio::test]
async fn render_tool_output_command_session_keeps_exit_127_output_and_guidance() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "command": "pip install pymupdf",
        "output": "bash: pip: command not found",
        "session_id": "run-127",
        "is_exited": true,
        "exit_code": 127,
        "critical_note": "Command `pip` was not found in PATH.",
        "next_action": "Check the command name or install the missing binary, then rerun the command."
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::UNIFIED_EXEC), &payload, None)
        .await
        .expect("exit 127 payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("bash: pip: command not found"));
    assert!(inline_output.contains("not found in PATH."));
    assert!(inline_output.contains("Check the command name or install the missing binary, then rerun the command."));
    assert!(inline_output.contains("✓ exit 127"));
    assert!(!inline_output.contains("Solution:"));
    assert_eq!(
        inline_output
            .matches("Check the command name or install the missing binary, then rerun the command.")
            .count(),
        1
    );
}
#[tokio::test]
async fn render_tool_output_renders_generic_recoverable_failure_guidance() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "error": "Tool preflight validation failed: x",
        "is_recoverable": true,
        "next_action": "Retry with fallback_tool_args."
    });

    render_tool_output(&mut renderer, Some("custom_tool"), &payload, None)
        .await
        .expect("generic recoverable failure should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("Tool preflight validation failed: x"));
    assert!(inline_output.contains("Retry with fallback_tool_args."));
    assert_eq!(inline_output.matches("Retry with fallback_tool_args.").count(), 1);
    assert!(!inline_output.contains("\"error\""));
    assert!(!inline_output.contains("\"next_action\""));
}
#[tokio::test]
async fn render_tool_output_write_file_diff_truncation_does_not_claim_full_review() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "diff_preview": {
            "content": "@@ -1 +1 @@\n-old\n+new\n",
            "truncated": true,
            "omitted_line_count": 5
        }
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::WRITE_FILE), &payload, None)
        .await
        .expect("write file diff payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(
        inline_output.contains("preview excerpt retained") || inline_output.contains("lines omitted"),
        "tool-truncated previews must not claim full-diff review: {inline_output:?}"
    );
    assert!(
        !inline_output.contains("RecordDiffReview"),
        "excerpt payloads must not record expand anchors: {inline_output:?}"
    );
    assert!(!inline_output.contains("review full diff"));
    assert!(!inline_output.contains("use read_file for full view"));
}
