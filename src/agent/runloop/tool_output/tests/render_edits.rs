use super::*;

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

    let anchors = super::super::collect_inline_diff_review_anchors(&mut receiver);
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

    let anchors = super::super::collect_inline_diff_review_anchors(&mut receiver);
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
