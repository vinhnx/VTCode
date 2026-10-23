use serde_json::json;
use vtcode_commons::ui_protocol::TaskItemStatus;
use vtcode_core::config::ToolDisplayMode;
use vtcode_core::ui::InlineHandle;
use vtcode_core::utils::ansi::AnsiRenderer;

use super::{
    TRACKER_ROW_DESCRIPTION_MAX_BYTES, TRACKER_TRANSCRIPT_MAX_ROWS, TrackerLine, collect_inline_output,
    humanize_tracker_title, is_tracker_current_row, preferred_follow_up_rendered_body, render_tool_output,
    should_render_command_session_terminal_panel, spooled_output_hint, tracker_current_tree_row,
    tracker_panel_metadata, tracker_panel_rows, tracker_progress_lines, tracker_row_text, tracker_summary_lines,
    tracker_transcript_lines, tracker_tree_body_lines,
};

#[test]
fn tracker_row_text_bounds_long_descriptions_only() {
    let long = "Add `vtcode exec resume` to the Commands section — document the cross-turn exec-session \
                resume contract in the second-tier command table row for `vtcode exec`";
    let bounded = tracker_row_text(long.to_string());

    assert_eq!(bounded, "Add `vtcode exec resume` to the Commands section");
    assert!(
        bounded.len() <= TRACKER_ROW_DESCRIPTION_MAX_BYTES + '…'.len_utf8(),
        "row must stay within the budget: {} bytes",
        bounded.len()
    );

    // Long clauses without a detail separator still truncate with an ellipsis.
    let unbroken =
        "Implement a very long refactor across many modules and services that keeps going without a separator at all";
    let truncated = tracker_row_text(unbroken.to_string());
    assert!(truncated.ends_with('…'), "unbroken row must be bounded: {truncated:?}");

    // Short descriptions stay verbatim so the panel does not add noise.
    assert_eq!(tracker_row_text("Verify with cargo check".to_string()), "Verify with cargo check");
}

#[test]
fn tracker_rows_and_current_row_share_one_short_description() {
    let long = "Update the Everyday recipes block — add a headless resume example next to the existing \
                `vtcode continue --session-id` recipe and align the schedule example with the canonical flag order";
    let payload = json!({
        "checklist": {
            "title": "Refine README",
            "completed": 0,
            "total": 1,
            "items": [{"index": 1, "description": long, "status": "pending"}]
        }
    });

    let rows = tracker_tree_body_lines(&payload);
    assert_eq!(rows.len(), 1, "single item yields a single row: {rows:?}");
    assert_eq!(rows[0], "  └ Update the Everyday recipes block");
    assert!(!rows[0].contains("headless"), "detail tail must not surface: {rows:?}");

    let current = tracker_current_tree_row(&payload).expect("pending row is the current task");
    assert_eq!(current.text, "  » Update the Everyday recipes block");
}

#[test]
fn command_session_terminal_panel_detects_command_payload() {
    let payload = json!({
        "command": "cargo check",
        "output": "Checking vtcode"
    });
    assert!(should_render_command_session_terminal_panel(&payload));
}

#[test]
fn command_session_terminal_panel_detects_session_payload() {
    let payload = json!({
        "session_id": "run-123",
        "is_exited": true
    });
    assert!(should_render_command_session_terminal_panel(&payload));
}

#[test]
fn command_session_terminal_panel_ignores_non_terminal_payload() {
    let payload = json!({
        "sessions": [],
        "success": true
    });
    assert!(!should_render_command_session_terminal_panel(&payload));
}

#[test]
fn command_session_terminal_panel_skips_git_diff_payload() {
    let payload = json!({
        "command": "git diff -- src/main.rs",
        "output": "diff --git a/src/main.rs b/src/main.rs",
        "content_type": "git_diff"
    });
    assert!(!should_render_command_session_terminal_panel(&payload));
}

#[test]
fn preferred_follow_up_rendered_body_prefers_output_over_content() {
    let payload = json!({
        "output": "stdout body",
        "content": "content body"
    });

    assert_eq!(preferred_follow_up_rendered_body(&payload), Some("stdout body"));
}

#[test]
fn preferred_follow_up_rendered_body_falls_back_to_content() {
    let payload = json!({
        "content": "content body"
    });

    assert_eq!(preferred_follow_up_rendered_body(&payload), Some("content body"));
}

#[tokio::test]
async fn render_tool_output_command_session_git_diff_renders_diff_not_command_preview() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "command": "git diff -- src/main.rs",
        "output": "diff --git a/src/main.rs b/src/main.rs\n+added\n-removed\n",
        "content_type": "git_diff",
        "is_exited": true,
        "exit_code": 0
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::UNIFIED_EXEC), &payload, None)
        .await
        .expect("git diff payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("diff --git a/src/main.rs b/src/main.rs"));
    assert!(!inline_output.contains("└ "), "run-command preview prefix should not appear for git diff payload");
}

#[tokio::test]
async fn render_tool_output_command_session_git_diff_stdout_renders_diff_not_command_preview() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "command": "git diff -- src/lib.rs",
        "stdout": "diff --git a/src/lib.rs b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n",
        "content_type": "git_diff",
        "is_exited": true,
        "exit_code": 0
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::UNIFIED_EXEC), &payload, None)
        .await
        .expect("git diff stdout payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("diff --git a/src/lib.rs b/src/lib.rs"));
    assert!(inline_output.contains("@@ -1 +1 @@"));
    assert!(inline_output.contains("new"));
    assert!(!inline_output.contains("└ "), "run-command preview prefix should not appear for git diff payload");
}

#[tokio::test]
async fn render_tool_output_apply_patch_renders_diff_content() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "success": true,
        "diff": [{
            "path": "README.md",
            "content": "diff --git a/README.md b/README.md\n-before\n+after\n",
            "skipped": false
        }]
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::APPLY_PATCH), &payload, None)
        .await
        .expect("apply_patch diff payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("README.md"));
    assert!(inline_output.contains("-before"));
    assert!(inline_output.contains("+after"));
}

#[tokio::test]
async fn render_tool_output_apply_patch_parses_ansi_diff_payloads() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "success": true,
        "diff": [{
            "path": "README.md",
            "operation": "updated",
            "content": "\u{1b}[36mdiff --git a/README.md b/README.md\u{1b}[0m\n\u{1b}[36m@@ -1 +1 @@\u{1b}[0m\n\u{1b}[31m-before\u{1b}[0m\n\u{1b}[32m+after\u{1b}[0m\n",
            "additions": 1,
            "deletions": 1,
            "skipped": false
        }]
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::APPLY_PATCH), &payload, None)
        .await
        .expect("ANSI apply_patch diff payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("-    1 │ before"));
    assert!(inline_output.contains("+    1 │ after"));
}

#[tokio::test]
async fn render_tool_output_command_session_renders_structured_hints() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "command": "cargo check",
        "output": "tail preview",
        "session_id": "run-123",
        "is_exited": false,
        "next_continue_args": {
            "session_id": "run-123"
        },
        "spool_path": ".vtcode/context/tool_outputs/run-123.txt"
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::UNIFIED_EXEC), &payload, None)
        .await
        .expect("structured hint payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("Large output was spooled to"));
    assert!(inline_output.contains("exec_command"));
    assert!(inline_output.contains("cat, sed, or rg"));
    assert!(!inline_output.contains("read_file/grep_file"));
    assert!(inline_output.contains("Still running — check again for more output."));
    assert!(!inline_output.contains("next_continue_args"));
}

#[tokio::test]
async fn render_tool_output_exec_command_renders_terminal_panel_with_output() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_tool_display_mode(ToolDisplayMode::Expanded);
    let payload = json!({
        "command": "cargo check",
        "output": "Compiling vtcode v0.135.9",
        "stdout": "Compiling vtcode v0.135.9",
        "stderr": "",
        "is_exited": true,
        "exit_code": 0
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::EXEC_COMMAND), &payload, None)
        .await
        .expect("exec_command payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(
        inline_output.contains("Compiling vtcode v0.135.9"),
        "exec_command output must be rendered in the terminal panel, got: {inline_output}"
    );
    assert!(
        !inline_output.contains("(no output)"),
        "exec_command output must not fall through to the no-output status renderer"
    );
}

#[tokio::test]
async fn render_tool_output_exec_command_compact_hides_completed_stdout() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "command": "cargo check",
        "stdout": "verbose completed output",
        "stderr": ""
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::EXEC_COMMAND), &payload, None)
        .await
        .expect("exec_command payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(!inline_output.contains("verbose completed output"));
}

#[tokio::test]
async fn render_tool_output_exec_pty_cmd_renders_terminal_panel_with_output() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "command": "ls -la",
        "output": "total 0",
        "session_id": "run-456",
        "is_exited": true,
        "exit_code": 0
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::EXEC_PTY_CMD), &payload, None)
        .await
        .expect("exec_pty_cmd payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(
        inline_output.contains("total") && inline_output.contains("✓ exit 0"),
        "exec_pty_cmd output must be rendered in the terminal panel, got: {inline_output}"
    );
}

#[tokio::test]
async fn render_tool_output_run_pty_completed_spooled_output_is_reference_only() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "command": "cargo check",
        "output": "preview text that should not render inline",
        "session_id": "run-123",
        "is_exited": true,
        "exit_code": 0,
        "spool_path": ".vtcode/context/tool_outputs/run-123.txt"
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::RUN_PTY_CMD), &payload, None)
        .await
        .expect("spooled PTY payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("✓ exit 0"));
    assert!(inline_output.contains("Large output was spooled to"));
    assert!(!inline_output.contains("preview text that should not render inline"));
    assert!(!inline_output.contains("(no output)"));
}

#[tokio::test]
async fn render_tool_output_read_file_renders_spool_hint_on_early_return_path() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "path": "README.md",
        "content": "preview",
        "spool_path": ".vtcode/context/tool_outputs/readme.txt"
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::READ_FILE), &payload, None)
        .await
        .expect("read_file payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("Large output was spooled to"));
    assert!(inline_output.contains("exec_command"));
    assert!(inline_output.contains("cat, sed, or rg"));
    assert!(!inline_output.contains("read_file/grep_file"));
}

#[tokio::test]
async fn render_tool_output_web_fetch_content_fallback_renders_follow_up_hint() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "content": "preview",
        "spool_path": ".vtcode/context/tool_outputs/web.txt"
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::WEB_FETCH), &payload, None)
        .await
        .expect("web_fetch payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("Large output was spooled to"));
    assert!(inline_output.contains("exec_command"));
    assert!(inline_output.contains("cat, sed, or rg"));
    assert!(!inline_output.contains("read_file/grep_file"));
}

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

#[tokio::test]
async fn render_tool_output_write_file_truncated_preview_does_not_record_anchor() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "diff_preview": {
            "content": "@@ -1 +1 @@\n-old\n+new\n",
            "truncated": true,
            "omitted_line_count": 5,
            "path": "src/main.rs"
        }
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::WRITE_FILE), &payload, None)
        .await
        .expect("write file diff payload should render");

    let anchors = super::collect_inline_diff_review_anchors(&mut receiver);
    assert!(
        anchors.is_empty(),
        "tool-level truncated previews are excerpts and must not record full-diff anchors: {anchors:?}"
    );
}

#[tokio::test]
async fn render_tool_output_write_file_untruncated_preview_can_record_expand_from_streams() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_table_max_width(Some(40));
    // Complete unified body (not a registry excerpt) whose rows exceed
    // DIFF_WRAP_SOURCE_MAX_WIDTH so safety-cap truncation advertises expand.
    let body = format!("@@ -1 +1 @@\n-{}\n+{}\n", "old ".repeat(600), "new ".repeat(600));
    let payload = json!({
        "diff_preview": {
            "content": body,
            "truncated": false,
            "path": "src/main.rs"
        }
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::WRITE_FILE), &payload, None)
        .await
        .expect("write file diff payload should render");

    let anchors = super::collect_inline_diff_review_anchors(&mut receiver);
    assert!(!anchors.is_empty(), "safety-capped full body should record an expand anchor");
    assert!(
        anchors
            .iter()
            .any(|a| a.unified.contains("old old") || a.unified.contains("new new"))
    );
}

#[tokio::test]
async fn render_tool_output_write_file_uses_canonical_diff_entries() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "path": "README.md",
        "diff": [{
            "path": "README.md",
            "operation": "updated",
            "content": "@@ -1 +1 @@\n-before\n+after\n",
            "additions": 1,
            "deletions": 1,
            "truncated": false,
            "skipped": false
        }]
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::WRITE_FILE), &payload, None)
        .await
        .expect("canonical write diff should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("• Edited README.md (+1 -1)"));
    assert!(inline_output.contains("-    1 │ before"));
    assert!(inline_output.contains("+    1 │ after"));
}

#[tokio::test]
async fn render_tool_output_groups_multiple_file_edits_in_compact_summary() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "diff": [
            {
                "path": "src/a.rs",
                "operation": "updated",
                "content": "@@ -1 +1 @@\n-old a\n+new a\n",
                "additions": 2,
                "deletions": 3,
                "skipped": false
            },
            {
                "path": "src/b.rs",
                "operation": "updated",
                "content": "@@ -1 +1 @@\n-old b\n+new b\n",
                "summary": {"additions": 2, "deletions": 2},
                "skipped": false
            }
        ]
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::APPLY_PATCH), &payload, None)
        .await
        .expect("multi-file diff payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("• Edited 2 files (+4 -5)"));
    assert!(inline_output.contains("  ├ src/a.rs (+2 -3)"));
    assert!(inline_output.contains("  └ src/b.rs (+2 -2)"));
    assert!(!inline_output.contains("• Edited src/a.rs"));
    assert!(!inline_output.contains("• Edited src/b.rs"));
    assert!(inline_output.find("  ├ src/a.rs").unwrap() < inline_output.find("  └ src/b.rs").unwrap());
}

#[tokio::test]
async fn render_tool_output_apply_patch_strips_duplicated_file_headers() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "diff": [{
            "path": "src/main.rs",
            "operation": "updated",
            "content": "diff --git a/src/main.rs b/src/main.rs\nindex 1111111..2222222 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n",
            "additions": 1,
            "deletions": 1,
            "skipped": false
        }]
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::APPLY_PATCH), &payload, None)
        .await
        .expect("apply_patch diff with headers should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("• Edited src/main.rs (+1 -1)"));
    assert!(inline_output.contains("@@ -1 +1 @@"));
    assert!(inline_output.contains("-    1 │ old"));
    assert!(inline_output.contains("+    1 │ new"));
    assert!(!inline_output.contains("--- a/src/main.rs"), "heading already shows the path: {inline_output}");
    assert!(!inline_output.contains("+++ b/src/main.rs"), "heading already shows the path: {inline_output}");
    assert!(!inline_output.contains("diff --git"), "git header must not duplicate the heading: {inline_output}");
}

#[tokio::test]
async fn render_tool_output_apply_patch_header_only_shows_no_changes() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "diff": [{
            "path": "src/main.rs",
            "operation": "updated",
            "content": "--- a/src/main.rs\n+++ b/src/main.rs\n",
            "skipped": false
        }]
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::APPLY_PATCH), &payload, None)
        .await
        .expect("header-only diff should render a friendly row");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("• Edited src/main.rs"));
    assert!(inline_output.contains("no changes"), "header-only preview must not render blank: {inline_output}");
    assert!(!inline_output.contains("--- a/src/main.rs"));
}

#[tokio::test]
async fn render_tool_output_apply_patch_skipped_shows_user_message_not_reason_code() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "diff": [{
            "path": "src/big.rs",
            "operation": "updated",
            "skipped": true,
            "reason": "too_many_changes",
            "summary": {"additions": 12, "deletions": 3}
        }]
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::APPLY_PATCH), &payload, None)
        .await
        .expect("skipped diff should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(!inline_output.contains("too_many_changes"), "stable reason code must not surface: {inline_output}");
    assert!(inline_output.contains("+12 -3"), "friendly message keeps counts: {inline_output}");
}

#[tokio::test]
async fn render_tool_output_empty_content_keeps_truncation_notice() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "diff": [{
            "path": "src/big.rs",
            "operation": "updated",
            "content": "",
            "truncated": true,
            "omitted_line_count": 9,
            "skipped": false
        }]
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::APPLY_PATCH), &payload, None)
        .await
        .expect("empty truncated diff should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("+9 lines"), "omission metadata must survive empty bodies: {inline_output}");
}

#[test]
fn tracker_summary_lines_hide_successful_tracker_details() {
    let payload = json!({
        "status": "updated",
        "message": "Item 2 status changed: pending -> in_progress",
        "checklist": {
            "total": 4,
            "completed": 1,
            "in_progress": 2,
            "pending": 1,
            "blocked": 0,
            "progress_percent": 25,
            "items": [
                { "index": 1, "description": "A", "status": "completed" },
                { "index": 2, "description": "B", "status": "in_progress" },
                { "index": 3, "description": "C", "status": "in_progress" },
                { "index": 4, "description": "D", "status": "pending" }
            ]
        }
    });

    assert!(tracker_summary_lines(&payload).is_empty());
}

#[test]
fn tracker_summary_lines_still_show_message_without_checklist() {
    let payload = json!({
        "status": "empty",
        "message": "No active checklist."
    });
    let lines = tracker_summary_lines(&payload);
    assert!(lines.iter().any(|line| line == "  Tracker status: empty"));
    assert!(lines.iter().any(|line| line == "  Update: No active checklist."));
}

#[test]
fn tracker_progress_lines_keep_counts_when_tree_body_is_empty() {
    // Explicit completed/total without renderable step titles still answers
    // "how far along is this work?" on the user-facing surface.
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "completed": 2,
            "total": 5,
            "items": []
        }
    });

    let rows = tracker_progress_lines(&payload);

    assert_eq!(rows, vec!["• Release 2/5"]);
    assert!(tracker_tree_body_lines(&payload).is_empty());
}

#[test]
fn tracker_progress_lines_show_title_and_progress_only() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Investigate", "status": "pending" },
                { "index_path": "2", "description": "Implement", "status": "in_progress" },
                { "index_path": "3", "description": "Verify", "status": "completed" },
                { "index_path": "4", "description": "Resolve dependency", "status": "blocked" }
            ]
        }
    });

    let rows = tracker_progress_lines(&payload);

    assert_eq!(rows, vec!["• Release 1/4"]);
    assert!(rows.iter().all(|row| !row.contains("next:")));
    assert!(rows.iter().all(|row| !row.contains("├") && !row.contains("└")));
}

#[test]
fn tracker_tree_body_lines_are_panel_only_compact_tree() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Investigate", "status": "pending" },
                { "index_path": "2", "description": "Implement", "status": "in_progress" },
                { "index_path": "3", "description": "Verify", "status": "completed" },
                { "index_path": "4", "description": "Resolve dependency", "status": "blocked" }
            ]
        }
    });

    let rows = tracker_tree_body_lines(&payload);

    assert_eq!(
        rows,
        vec![
            "  ├ Investigate",
            "  ├ Implement",
            "  ├ Verify",
            "  └ Resolve dependency",
        ]
    );
    assert!(rows.iter().all(|row| !row.starts_with("• ")));
    assert!(
        rows.iter()
            .all(|row| !row.contains("□") && !row.contains("[x]") && !row.contains("[-]")),
        "status surfaces through styling, not glyphs: {rows:?}"
    );
    assert_eq!(
        tracker_panel_rows(&payload),
        (
            vec![
                "  ├ Investigate".to_string(),
                "  ├ Implement".to_string(),
                "  ├ Verify".to_string(),
                "  └ Resolve dependency".to_string(),
            ],
            vec![
                TaskItemStatus::Pending,
                TaskItemStatus::InProgress,
                TaskItemStatus::Completed,
                TaskItemStatus::Blocked,
            ],
            Some(1),
        )
    );
}

#[test]
fn humanize_tracker_title_strips_timestamp_prefix_from_generated_ids() {
    assert_eq!(humanize_tracker_title("1789108823046-kind-lagoon"), "Kind Lagoon");
    assert_eq!(humanize_tracker_title("1789108823046-kind_lagoon"), "Kind Lagoon");
}

#[test]
fn humanize_tracker_title_keeps_user_titles_verbatim() {
    assert_eq!(humanize_tracker_title("Release"), "Release");
    assert_eq!(humanize_tracker_title("2024-report"), "2024-report");
    assert_eq!(humanize_tracker_title("  Task tracker  "), "Task tracker");
}

#[test]
fn tracker_progress_lines_humanize_generated_title_without_raw_slug() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "1789108823046-kind-lagoon",
            "items": [
                { "index_path": "1", "description": "Investigate", "status": "pending" },
            ]
        }
    });

    let rows = tracker_progress_lines(&payload);

    assert_eq!(rows, vec!["• Investigate 0/1"]);
    assert!(!rows[0].contains("1789108823046"));
    assert!(!rows[0].contains("Lagoon"));
    assert!(!rows[0].contains("next:"));
}

#[test]
fn tracker_progress_lines_prefer_explicit_counts_over_derived() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "1789108823046-kind-lagoon",
            "completed": 2,
            "total": 5,
            "items": [
                { "index_path": "1", "description": "Done one", "status": "completed" },
                { "index_path": "2", "description": "Done two", "status": "completed" },
                { "index_path": "3", "description": "Audit heavy-crate linkage", "status": "pending" },
            ]
        }
    });

    let rows = tracker_progress_lines(&payload);

    assert_eq!(rows, vec!["• Done one 2/5"]);
    assert!(!rows[0].contains("1789108823046"));
    assert!(!rows[0].contains("Lagoon"));
    assert!(!rows[0].contains("Audit heavy-crate linkage"));
}

#[test]
fn tracker_header_replaces_humanized_codename_with_first_task() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Jolly Forest",
            "items": [
                { "index_path": "1", "description": "Add vtcode exec resume to the Commands section – document the contract", "status": "pending" },
                { "index_path": "2", "description": "Verify links", "status": "pending" },
            ]
        }
    });

    let rows = tracker_progress_lines(&payload);
    assert_eq!(rows, vec!["• Add vtcode exec resume to the Commands section 0/2"]);
    assert!(!rows[0].contains("Jolly"));

    let metadata = tracker_panel_metadata(&payload).expect("metadata");
    assert_eq!(metadata.title, "Add vtcode exec resume to the Commands section");
}

#[test]
fn tracker_header_keeps_user_title_verbatim() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Investigate", "status": "pending" },
            ]
        }
    });

    let rows = tracker_progress_lines(&payload);
    assert_eq!(rows, vec!["• Release 0/1"]);
}

#[test]
fn tracker_tree_body_lines_strip_metadata_but_keep_task_titles() {
    // Parents summarize their children, so showing their stored leaf status
    // would be misleading. Metadata remains in the structured payload but
    // must not turn into visible detail rows.
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                {
                    "index_path": "1",
                    "level": 0,
                    "description": "Prepare release",
                    "status": "in_progress",
                    "files": ["Cargo.toml"],
                    "outcome": "Version is ready",
                    "verify": ["cargo nextest run -p vtcode"]
                },
                { "index_path": "1.1", "level": 1, "description": "Update version", "status": "completed" },
                { "index_path": "1.2", "level": 1, "description": "Run checks", "status": "in_progress" },
                { "index_path": "2", "level": 0, "description": "Publish", "status": "pending" }
            ]
        }
    });

    let rows = tracker_tree_body_lines(&payload);

    assert_eq!(
        rows,
        vec![
            "  ├ Prepare release",
            "  │ Update version",
            "  │ Run checks",
            "  └ Publish",
        ]
    );
    assert!(
        rows.iter()
            .all(|line| { !line.contains("files:") && !line.contains("outcome:") && !line.contains("verify:") })
    );
    assert_eq!(payload["checklist"]["items"][0]["files"], json!(["Cargo.toml"]));
    assert_eq!(payload["checklist"]["items"][0]["outcome"], "Version is ready");
    assert_eq!(payload["checklist"]["items"][0]["verify"], json!(["cargo nextest run -p vtcode"]));
    let metadata = tracker_panel_metadata(&payload).expect("structured panel metadata");
    assert_eq!(metadata.title, "Release");
    assert_eq!((metadata.completed, metadata.total), (1, 4));
}

#[test]
fn tracker_progress_lines_keep_diagnostics_for_empty_or_malformed_tracker_responses() {
    // Compact rendering applies only to successful structured checklists.
    // Empty and malformed responses must remain diagnosable instead of
    // silently presenting a blank task panel.
    let empty = json!({});
    let malformed = json!({
        "status": "error",
        "message": "Tracker response did not include checklist items.",
        "view": { "lines": "not an array" }
    });
    let malformed_items = json!({
        "status": "error",
        "message": "Tracker response contained invalid checklist items.",
        "checklist": { "items": [{}] }
    });

    assert!(tracker_progress_lines(&empty).is_empty());
    assert_eq!(
        tracker_progress_lines(&malformed),
        vec![
            "• Tasks",
            "  Tracker status: error",
            "  Update: Tracker response did not include checklist items.",
        ]
    );
    assert_eq!(
        tracker_progress_lines(&malformed_items),
        vec![
            "• Tasks",
            "  Tracker status: error",
            "  Update: Tracker response contained invalid checklist items.",
        ]
    );

    let partial_failure = json!({
        "status": "error",
        "message": "Tracker response was only partially applied.",
        "checklist": {
            "items": [
                { "index": 1, "description": "Still present", "status": "completed" }
            ]
        }
    });
    assert_eq!(
        tracker_progress_lines(&partial_failure),
        vec![
            "• Tasks",
            "  Tracker status: error",
            "  Update: Tracker response was only partially applied.",
        ]
    );
    // Failed updates stay diagnosable in the transcript; remaining checklist
    // rows remain available on the panel body path.
    assert_eq!(tracker_tree_body_lines(&partial_failure), vec!["  └ Still present"]);
}

fn transcript_texts(lines: &[TrackerLine]) -> Vec<&str> {
    lines.iter().map(|line| line.text.as_str()).collect()
}

fn transcript_statuses(lines: &[TrackerLine]) -> Vec<Option<TaskItemStatus>> {
    lines.iter().map(|line| line.status).collect()
}

#[test]
fn tracker_transcript_lines_compact_shows_header_plus_current_task() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Investigate cache miss", "status": "completed" },
                { "index_path": "2", "description": "Defer eager setup", "status": "in_progress" },
                { "index_path": "3", "description": "Verify with cargo check", "status": "pending" },
            ]
        }
    });

    let rows = tracker_transcript_lines(&payload, false);

    assert_eq!(transcript_texts(&rows), vec!["• Release 1/3", "  » Defer eager setup"]);
    assert_eq!(transcript_statuses(&rows), vec![None, Some(TaskItemStatus::InProgress)]);
    assert!(rows.iter().all(|row| !row.text.contains("[-]") && !row.text.contains("□")));
}

#[test]
fn tracker_panel_and_transcript_focus_actionable_leaf_before_parent() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Prepare release", "status": "in_progress" },
                { "index_path": "1.1", "description": "Update version", "status": "completed" },
                { "index_path": "1.2", "description": "Run checks", "status": "pending" },
                { "index_path": "2", "description": "Publish", "status": "blocked" }
            ]
        }
    });

    let (texts, statuses, current) = tracker_panel_rows(&payload);
    assert_eq!(current, Some(2));
    assert!(texts[2].ends_with("Run checks"));
    assert_eq!(statuses[2], TaskItemStatus::Pending);
    assert_eq!(
        tracker_current_tree_row(&payload),
        Some(TrackerLine::row("  » Run checks".to_string(), TaskItemStatus::Pending))
    );
}

#[test]
fn tracker_current_tree_row_prefers_in_progress_over_pending_and_blocked() {
    // Asymmetric statuses: pending comes first in document order, but the
    // in-progress leaf later must win as current.
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Verify with cargo check", "status": "pending" },
                { "index_path": "2", "description": "Defer eager setup", "status": "in_progress" },
                { "index_path": "3", "description": "Resolve dependency", "status": "blocked" },
            ]
        }
    });

    assert_eq!(
        tracker_current_tree_row(&payload),
        Some(TrackerLine::row("  » Defer eager setup".to_string(), TaskItemStatus::InProgress))
    );
}

#[test]
fn tracker_current_tree_row_falls_back_to_pending_then_blocked() {
    let pending_only = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Verify with cargo check", "status": "completed" },
                { "index_path": "2", "description": "Defer eager setup", "status": "pending" },
            ]
        }
    });
    assert_eq!(
        tracker_current_tree_row(&pending_only),
        Some(TrackerLine::row("  » Defer eager setup".to_string(), TaskItemStatus::Pending))
    );

    let blocked_only = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Verify with cargo check", "status": "completed" },
                { "index_path": "2", "description": "Resolve dependency", "status": "blocked" },
            ]
        }
    });
    assert_eq!(
        tracker_current_tree_row(&blocked_only),
        Some(TrackerLine::row("  » Resolve dependency".to_string(), TaskItemStatus::Blocked))
    );
}

#[test]
fn tracker_current_tree_row_stays_header_only_when_all_completed_or_empty() {
    let all_done = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Investigate cache miss", "status": "completed" },
                { "index_path": "2", "description": "Defer eager setup", "status": "completed" },
            ]
        }
    });
    assert_eq!(tracker_current_tree_row(&all_done), None);
    assert_eq!(tracker_transcript_lines(&all_done, false), vec![TrackerLine::plain("• Release 2/2".to_string())]);

    let empty = json!({
        "status": "updated",
        "checklist": { "title": "Release", "completed": 0, "total": 0, "items": [] }
    });
    assert_eq!(tracker_current_tree_row(&empty), None);
}

#[test]
fn tracker_current_row_marker_is_detected() {
    assert!(is_tracker_current_row("  » Defer eager setup"));
    assert!(is_tracker_current_row("  » Verify with cargo check"));
    assert!(!is_tracker_current_row("  ├ Defer eager setup"));
    assert!(!is_tracker_current_row("• Release 1/3"));
}

#[test]
fn tracker_glyph_strip_keeps_mid_string_brackets_and_branches() {
    // Only the leading status glyph is stripped; bracket text inside the
    // description and the branch structure survive verbatim.
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Fix [-] flag handling", "status": "pending" },
                { "index_path": "2", "description": "Ship [x] marked docs", "status": "completed" },
            ]
        }
    });

    let rows = tracker_transcript_lines(&payload, true);

    assert_eq!(
        transcript_texts(&rows),
        vec!["• Release 1/2", "  ├ Fix [-] flag handling", "  └ Ship [x] marked docs"]
    );
    assert_eq!(
        transcript_statuses(&rows),
        vec![None, Some(TaskItemStatus::Pending), Some(TaskItemStatus::Completed)]
    );
}

#[test]
fn tracker_transcript_lines_expanded_shows_each_task_item() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Investigate cache miss", "status": "completed" },
                { "index_path": "2", "description": "Defer eager setup", "status": "in_progress" },
                { "index_path": "3", "description": "Verify with cargo check", "status": "pending" },
            ]
        }
    });

    let rows = tracker_transcript_lines(&payload, true);

    assert_eq!(transcript_texts(&rows)[0], "• Release 1/3");
    assert_eq!(
        transcript_texts(&rows)[1..],
        [
            "  ├ Investigate cache miss",
            "  ├ Defer eager setup",
            "  └ Verify with cargo check",
        ]
    );
    assert_eq!(
        transcript_statuses(&rows),
        vec![
            None,
            Some(TaskItemStatus::Completed),
            Some(TaskItemStatus::InProgress),
            Some(TaskItemStatus::Pending),
        ]
    );
}

#[test]
fn tracker_transcript_lines_expanded_truncates_large_checklists() {
    let items: Vec<serde_json::Value> = (1..=(TRACKER_TRANSCRIPT_MAX_ROWS + 5))
        .map(|index| {
            json!({
                "index_path": index.to_string(),
                "description": format!("Distinct task {index}"),
                "status": if index == 1 { "completed" } else { "pending" },
            })
        })
        .collect();
    let payload = json!({
        "status": "updated",
        "checklist": { "title": "Release", "items": items }
    });

    let rows = tracker_transcript_lines(&payload, true);

    assert_eq!(rows.len(), 1 + TRACKER_TRANSCRIPT_MAX_ROWS + 1);
    assert_eq!(rows[0].text, format!("• Release 1/{}", TRACKER_TRANSCRIPT_MAX_ROWS + 5));
    assert_eq!(rows[0].status, None);
    assert!(rows[1].text.contains("Distinct task 1"));
    assert_eq!(rows[1].status, Some(TaskItemStatus::Completed));
    assert!(
        rows[TRACKER_TRANSCRIPT_MAX_ROWS]
            .text
            .contains(format!("Distinct task {TRACKER_TRANSCRIPT_MAX_ROWS}").as_str())
    );
    assert_eq!(rows[TRACKER_TRANSCRIPT_MAX_ROWS + 1].text, "  … 5 more");
    assert_eq!(rows[TRACKER_TRANSCRIPT_MAX_ROWS + 1].status, None);
    assert!(
        rows.iter()
            .all(|row| !row.text.contains("Distinct task 31") || row.text.starts_with("  …"))
    );
}

#[test]
fn tracker_transcript_lines_expanded_keeps_diagnostics_without_tree() {
    let payload = json!({
        "status": "error",
        "message": "Tracker response was only partially applied.",
        "checklist": {
            "items": [
                { "index": 1, "description": "Still present", "status": "completed" }
            ]
        }
    });

    let rows = tracker_transcript_lines(&payload, true);

    assert_eq!(
        transcript_texts(&rows),
        vec![
            "• Tasks",
            "  Tracker status: error",
            "  Update: Tracker response was only partially applied.",
        ]
    );
    assert!(rows.iter().all(|row| row.status.is_none()));
    assert!(rows.iter().all(|row| !row.text.contains("Still present")));
}

#[tokio::test]
async fn render_tool_output_mcp_structured_fallback_and_content_precedence() {
    for (payload, expected, absent) in [
        (json!({"structuredContent": {"answer": 42}}), "answer: 42", "unused"),
        (
            json!({"content": [{"type": "text", "text": " \n"}],
                "structuredContent": {"status": "ready"}}),
            "status: ready",
            "unused",
        ),
        (
            json!({"content": [{"type": "text", "text": ""}],
                   "structuredContent": {"status": "empty text"}}),
            "status: empty text",
            "unused",
        ),
        (
            json!({"content": [{"type": "text", "text":
                r#"{"answer":5,"_meta":"legacy hidden","model":"legacy hidden","isError":false}"#}],
                "structuredContent": {"duplicate": 91}}),
            "answer: 5",
            "legacy hidden",
        ),
        (
            json!({"content": [{"type": "image", "data": "aW1hZ2U=", "mimeType": "image/png"}],
                "structuredContent": {"answer": 91}}),
            "[image content]",
            "answer",
        ),
        (json!({"structuredContent": {}}), "{}", "unused"),
        (json!({"structuredContent": []}), "[]", "unused"),
        (json!({"structuredContent": null}), "null", "unused"),
        (
            json!({"structuredContent": {"_meta": "public application data"}}),
            "public application data",
            "unused",
        ),
        (
            json!({"content": [{"type": "text", "text": "ordinary content"}],
                "structuredContent": {"answer": 91}}),
            "ordinary content",
            "answer",
        ),
    ] {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
            .await
            .unwrap();
        let output = collect_inline_output(&mut receiver);
        assert_eq!(output.matches(expected).count(), 1, "{output:?}");
        assert!(!output.contains(absent), "{output:?}");
    }
}

#[tokio::test]
async fn render_tool_output_mcp_structured_preserves_mixed_application_fields() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({"structuredContent": {
        "answer": 42, "_meta": "public application data", "model": "application model", "isError": true
    }});

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    for expected in [
        "answer: 42",
        "_meta: public application data",
        "model: application model",
        "isError: true",
    ] {
        assert_eq!(output.matches(expected).count(), 1, "{output:?}");
    }
}

#[tokio::test]
async fn render_tool_output_mcp_blank_rows_do_not_hide_structured_results() {
    for (payload, expected) in [
        (json!({"structuredContent": ""}), "\"\""),
        (json!({"structuredContent": "\n\t"}), "\"\\n\\t\""),
        (
            json!({"content": [{"type": "text", "text": "\"\""}],
                "structuredContent": {"answer": 17}}),
            "answer: 17",
        ),
    ] {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

        render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
            .await
            .unwrap();

        let output = collect_inline_output(&mut receiver);
        assert_eq!(output.matches(expected).count(), 1, "{output:?}");
    }
}

#[tokio::test]
async fn render_tool_output_mcp_custom_profile_keeps_precedence() {
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::config::mcp::McpRendererProfile;

    let mut config = VTCodeConfig::default();
    config
        .mcp
        .ui
        .renderers
        .insert("example".to_owned(), McpRendererProfile::SequentialThinking);
    let payload = json!({"summary": "custom summary", "structuredContent": {"answer": 73}});
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, Some(&config))
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    assert_eq!(output.matches("custom summary").count(), 1, "{output:?}");
    assert!(!output.contains("answer"), "{output:?}");
}

#[tokio::test]
async fn render_tool_output_mcp_resources_and_media_preserve_content_order() {
    let payload = json!({"content": [
        {"type": "text", "text": "first public text"},
        {"type": "resource", "resource": {
            "uri": "file:///report.txt", "mimeType": "text/plain", "text": "embedded report body"}},
        {"type": "audio", "mimeType": "audio/wav", "data": "PRIVATE_AUDIO_PAYLOAD"},
        {"type": "resource_link", "uri": "https://example.com/manual", "name": "Public manual",
            "mimeType": "text/html"},
        {"type": "resource", "resource": {
            "uri": "file:///archive.bin", "mimeType": "application/octet-stream", "blob": "PRIVATE_BLOB_PAYLOAD"}},
        {"type": "image", "mimeType": "image/png", "data": "PRIVATE_IMAGE_PAYLOAD"},
        {"type": "text", "text": "last public text"}
    ], "structuredContent": {"duplicate_fallback": true}});
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    let mut previous_end = 0;
    for expected in [
        "first public text",
        "[resource: file:///report.txt]",
        "type: text/plain",
        "embedded report body",
        "[audio content]",
        "type: audio/wav",
        "[resource: https://example.com/manual]",
        "name: Public manual",
        "type: text/html",
        "[resource: file:///archive.bin]",
        "type: application/octet-stream",
        "[image content]",
        "type: image/png",
        "last public text",
    ] {
        assert_eq!(output.matches(expected).count(), 1, "{output:?}");
        let position = output.find(expected).unwrap();
        assert!(position >= previous_end, "out-of-order {expected:?}: {output:?}");
        previous_end = position + expected.len();
    }
    assert!(!output.contains("PRIVATE_"), "{output:?}");
    assert!(!output.contains("duplicate_fallback"), "{output:?}");
}

#[tokio::test]
async fn render_tool_output_mcp_media_only_results_take_precedence_over_structured_fallback() {
    for (content, expected) in [
        (json!({"type": "audio", "mimeType": "audio/ogg", "data": "PRIVATE_AUDIO"}), "[audio content]"),
        (
            json!({"type": "resource_link", "uri": "https://example.com/link", "name": "Link"}),
            "[resource: https://example.com/link]",
        ),
        (
            json!({"type": "resource", "resource": {"uri": "file:///empty.txt", "text": ""}}),
            "[resource: file:///empty.txt]",
        ),
        (json!({"type": "resource", "uri": "file:///legacy.txt"}), "[resource: file:///legacy.txt]"),
    ] {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        let payload = json!({"content": [content], "structuredContent": {"duplicate_fallback": true}});

        render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
            .await
            .unwrap();

        let output = collect_inline_output(&mut receiver);
        assert_eq!(output.matches(expected).count(), 1, "{output:?}");
        assert!(!output.contains("duplicate_fallback"), "{output:?}");
        assert!(!output.contains("PRIVATE_"), "{output:?}");
    }
}

#[tokio::test]
async fn render_tool_output_mcp_resource_text_reuses_json_and_code_formatting() {
    for (text, expected, absent) in [
        (r#"{"count":7}"#, "count: 7", "{\"count\""),
        ("```rust\nlet count = 3;\n```", "let count = 3;", "```"),
    ] {
        let payload = json!({"content": [{"type": "resource", "resource": {
            "uri": "file:///sample.txt", "text": text
        }}]});
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

        render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
            .await
            .unwrap();

        let output = collect_inline_output(&mut receiver);
        assert_eq!(output.matches(expected).count(), 1, "{output:?}");
        assert!(!output.contains(absent), "{output:?}");
        assert!(output.contains("[resource: file:///sample.txt]"), "{output:?}");
    }
}
