use super::*;

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
