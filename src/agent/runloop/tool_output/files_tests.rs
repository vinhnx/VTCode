use super::*;
use unicode_width::UnicodeWidthStr;

#[tokio::test]
async fn apply_patch_preview_keeps_full_hunk_range_counts() {
    // Regression for the reported diff header: a pure deletion hunk such as
    // `@@ -65,19 +65,0 @@` must render its counts instead of the lossy
    // start-only `@@ -65 +65 @@`, which reads as a one-line change.
    use crate::agent::runloop::tool_output::collect_inline_output;
    use serde_json::json;
    use vtcode_core::ui::InlineHandle;
    use vtcode_core::utils::ansi::AnsiRenderer;

    let payload = json!({
        "diff": [{
            "path": "crates/codegen/vtcode-ui/src/tui/core_tui/style.rs",
            "operation": "updated",
            "additions": 0,
            "deletions": 19,
            "content": "@@ -65,19 +65,0 @@\n-removed_one\n-removed_two\n",
        }]
    });
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    render_apply_patch_diff_preview(&mut renderer, &payload, &GitStyles::new(), &LsStyles::from_env()).expect("render");
    let output = collect_inline_output(&mut receiver);

    assert!(output.contains("@@ -65,19 +65,0 @@"), "hunk header must keep range counts, got: {output:?}");
    assert!(!output.contains("@@ -65 +65 @@"), "lossy start-only hunk header must not render, got: {output:?}");
}

#[test]
fn formats_unified_diff_with_hunk_headers() {
    let diff = "\
diff --git a/file1.txt b/file1.txt
index 0000000..1111111 100644
--- a/file1.txt
+++ b/file1.txt
@@ -1,2 +1,2 @@
-old
+new
";
    let lines = format_diff_content_lines_with_numbers(diff);
    assert_eq!(lines[0], "diff --git a/file1.txt b/file1.txt");
    assert!(lines.iter().any(|line| line == "@@ -1,2 +1,2 @@"));
    // No "• Diff" summary line generated
    assert!(!lines.iter().any(|l| l.starts_with("• Diff ")));
}

#[test]
fn formats_diff_without_git_header() {
    let diff = "\
--- a/file2.txt
+++ b/file2.txt
@@ -2,3 +2,3 @@
-before
+after
";
    let lines = format_diff_content_lines_with_numbers(diff);
    assert!(lines.iter().any(|line| line.starts_with("+++ ")));
    assert!(lines.iter().any(|line| line == "@@ -2,3 +2,3 @@"));
    // No "• Diff" summary line generated
    assert!(!lines.iter().any(|l| l.starts_with("• Diff ")));
}

#[test]
fn formats_diff_with_numbered_hunk_lines() {
    let diff = "\
diff --git a/file1.txt b/file1.txt
index 0000000..1111111 100644
--- a/file1.txt
+++ b/file1.txt
@@ -10,2 +10,2 @@
-old
+new
 context
";
    let lines = format_diff_content_lines_with_numbers(diff);
    assert!(lines.iter().any(|line| line == "@@ -10,2 +10,2 @@"));
    assert!(lines.iter().any(|line| line.starts_with("-   10 │ old")));
    assert!(lines.iter().any(|line| line.starts_with("+   10 │ new")));
    assert!(lines.iter().any(|line| line.starts_with("    11 │ context")));
}

#[test]
fn shorten_path_preserves_short() {
    assert_eq!(shorten_path("/src/main.rs", 60), "/src/main.rs");
}

#[test]
fn shorten_path_truncates_long() {
    let long = "/very/long/deeply/nested/path/to/some/file.rs";
    let short = shorten_path(long, 30);
    assert!(UnicodeWidthStr::width(short.as_str()) <= 30);
    assert!(short.contains("file.rs"));
}

#[test]
fn pad_to_display_width_handles_wide_chars() {
    let padded = preview::pad_to_display_width("表", 4, ' ');
    assert_eq!(UnicodeWidthStr::width(padded.as_str()), 4);
    assert!(padded.starts_with("表"));
}

#[test]
fn truncate_text_safe_respects_display_width() {
    let value = "表表表"; // width = 6
    let truncated = preview::truncate_to_display_width(value, 5);
    assert_eq!(UnicodeWidthStr::width(truncated), 4);
    assert_eq!(truncated, "表表");
}

#[test]
fn shorten_path_handles_unicode_segments() {
    let long = "/đường/dẫn/rất/dài/để/kiểm/tra/ファイル/file.rs";
    let short = shorten_path(long, 20);
    assert!(short.contains("file.rs"));
    assert!(UnicodeWidthStr::width(short.as_str()) <= 20);
}

#[test]
fn formats_diff_with_function_signature_change() {
    // Test case for function signature change - no summary line generated
    let diff = "\
diff --git a/ask.rs b/ask.rs
index 0000000..1111111 100644
--- a/ask.rs
+++ b/ask.rs
@@ -172,7 +172,7 @@
         blocks
     }

-    fn select_best_code_block<'a>(blocks: &'a [CodeFenceBlock]) -> Option<&'a CodeFenceBlock> {
+    fn select_best_code_block(blocks: &[CodeFenceBlock]) -> Option<&CodeFenceBlock> {
         let mut best = None;
         let mut best_score = (0usize, 0u8);
         for block in blocks {
";
    let lines = format_diff_content_lines_with_numbers(diff);

    // No "• Diff" summary line generated
    assert!(!lines.iter().any(|l| l.starts_with("• Diff ")));
    assert!(lines.iter().any(|l| l.contains("diff --git")));
    assert!(lines.iter().any(|l| l == "@@ -172,7 +172,7 @@"));
}

#[test]
fn edit_preview_uses_standard_numbered_diff_formatting() {
    let diff = "\
diff --git a/crates/codegen/vtcode-config/src/loader/config.rs b/crates/codegen/vtcode-config/src/loader/config.rs
index 0000000..1111111 100644
--- a/crates/codegen/vtcode-config/src/loader/config.rs
+++ b/crates/codegen/vtcode-config/src/loader/config.rs
@@ -536,4 +536,4 @@
 # Suppress notifications while terminal is focused
-suppress_when_focused = false
+suppress_when_focused = true

@@ -545,4 +545,4 @@
 # Success notifications for tool call results
-tool_success = true
+tool_success = false
 ";

    let lines = format_diff_content_lines_with_numbers(diff);
    assert_eq!(
        lines[0],
        "diff --git a/crates/codegen/vtcode-config/src/loader/config.rs b/crates/codegen/vtcode-config/src/loader/config.rs"
    );
    assert!(lines.iter().any(|line| line == "@@ -536,4 +536,4 @@"));
    assert!(lines.iter().any(|line| line == "@@ -545,4 +545,4 @@"));
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("-  537 │ suppress_when_focused = false"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("+  537 │ suppress_when_focused = true"))
    );
    assert!(lines.iter().any(|line| line.starts_with("-  546 │ tool_success = true")));
    assert!(lines.iter().any(|line| line.starts_with("+  546 │ tool_success = false")));
}

#[test]
fn standard_diff_formatter_preserves_non_diff_lines() {
    let lines = format_diff_content_lines_with_numbers("plain text output");
    assert_eq!(lines, vec!["plain text output".to_string()]);
}

#[test]
fn standard_diff_formatter_handles_diff_without_diff_git_header() {
    let diff = "\
--- a/file2.txt
+++ b/file2.txt
@@ -2,3 +2,3 @@
-before
+after
";

    let lines = format_diff_content_lines_with_numbers(diff);
    assert_eq!(lines[0], "--- a/file2.txt");
    assert_eq!(lines[1], "+++ b/file2.txt");
    assert!(lines.iter().any(|line| line.starts_with("-    2 │ before")));
    assert!(lines.iter().any(|line| line.starts_with("+    2 │ after")));
}

#[test]
fn strip_redundant_file_headers_keeps_hunks_and_bodies() {
    let content = "diff --git a/src/main.rs b/src/main.rs\nindex 1111111..2222222 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n";
    let stripped = strip_redundant_file_headers(content);
    assert!(!stripped.contains("diff --git"), "git header must be removed: {stripped:?}");
    assert!(!stripped.contains("--- a/src/main.rs"), "old marker must be removed: {stripped:?}");
    assert!(!stripped.contains("+++ b/src/main.rs"), "new marker must be removed: {stripped:?}");
    assert!(stripped.contains("@@ -1 +1 @@"), "hunk must be kept: {stripped:?}");
    assert!(stripped.contains("-old"), "deletion must be kept: {stripped:?}");
    assert!(stripped.contains("+new"), "addition must be kept: {stripped:?}");
}

#[test]
fn strip_redundant_file_headers_falls_back_when_only_headers() {
    let content = "--- a/src/main.rs\n+++ b/src/main.rs\n";
    let stripped = strip_redundant_file_headers(content);
    assert!(stripped.trim().is_empty(), "header-only preview strips to blank: {stripped:?}");
}

#[test]
fn styled_diff_heading_keeps_path_and_counts_as_plain_text() {
    use serde_json::json;
    use vtcode_commons::ansi::strip_ansi;

    let diff = json!({
        "path": "src/main.rs",
        "operation": "updated",
        "additions": 2,
        "deletions": 1
    });
    let styled = styled_diff_heading(&diff, &GitStyles::new(), true);
    let plain = strip_ansi(&styled);
    assert!(plain.contains("Edited src/main.rs"), "heading must keep verb + path: {plain:?}");
    assert!(plain.contains("+2"), "heading must keep additions: {plain:?}");
    assert!(plain.contains("-1"), "heading must keep deletions: {plain:?}");
}

#[test]
fn styled_headings_stay_plain_without_color() {
    let diff = serde_json::json!({
        "path": "src/main.rs",
        "operation": "updated",
        "additions": 2,
        "deletions": 1
    });
    let styled = styled_diff_heading(&diff, &GitStyles::new(), false);
    assert!(!styled.contains('\x1b'), "no-color headings must not leak ANSI: {styled:?}");
    assert!(styled.contains("Edited src/main.rs (+2 -1)"));
}

#[test]
fn strip_redundant_file_headers_keeps_context_and_body_markers() {
    let content = "@@ -1,2 +1,2 @@\n index = 0\n-old\n+new\n";
    let stripped = strip_redundant_file_headers(content);
    assert!(stripped.contains("index = 0"), "context line must survive: {stripped:?}");

    let body = "@@ -1 +1 @@\n--- foo\n+++ bar\n";
    let stripped_body = strip_redundant_file_headers(body);
    assert!(stripped_body.contains("--- foo"), "body marker must survive after hunk: {stripped_body:?}");
    assert!(stripped_body.contains("+++ bar"), "body marker must survive after hunk: {stripped_body:?}");
}

#[test]
fn strip_redundant_file_headers_strips_ansi_colored_headers() {
    let content = "\u{1b}[36mdiff --git a/src/main.rs b/src/main.rs\u{1b}[0m\n\u{1b}[36m--- a/src/main.rs\u{1b}[0m\n\u{1b}[36m+++ b/src/main.rs\u{1b}[0m\n@@ -1 +1 @@\n-old\n+new\n";
    let stripped = strip_redundant_file_headers(content);
    assert!(!stripped.contains("diff --git"), "ANSI git header must be removed: {stripped:?}");
    assert!(!stripped.contains("--- a/src/main.rs"), "ANSI old marker must be removed: {stripped:?}");
    assert!(stripped.contains("@@ -1 +1 @@"), "hunk must be kept: {stripped:?}");
}

#[test]
fn strip_redundant_file_headers_strips_without_path_match() {
    // Headings already identify the file, so headers strip even when the
    // marker path does not match (absolute/quoted/diverged paths).
    let content = "diff --git a/other.rs b/other.rs\n--- a/other.rs\n+++ b/other.rs\n@@ -1 +1 @@\n-old\n+new\n";
    let stripped = strip_redundant_file_headers(content);
    assert!(!stripped.contains("diff --git"), "mismatched header must still strip: {stripped:?}");
    assert!(!stripped.contains("--- a/other.rs"), "mismatched marker must still strip: {stripped:?}");
    assert!(stripped.contains("+new"), "body must survive: {stripped:?}");
}

#[test]
fn strip_redundant_file_headers_strips_modes_and_apply_patch_markers() {
    let content = "new file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/src/new.rs\n*** Update File: src/new.rs\n@@ -0,0 +1 @@\n+hello\n";
    let stripped = strip_redundant_file_headers(content);
    assert!(!stripped.contains("new file mode"), "mode must be removed: {stripped:?}");
    assert!(!stripped.contains("*** Update File:"), "apply-patch marker must be removed: {stripped:?}");
    assert!(!stripped.contains("--- /dev/null"), "/dev/null marker must be removed: {stripped:?}");
    assert!(stripped.contains("+hello"), "body must survive: {stripped:?}");
}

#[test]
fn styled_aggregate_summary_keeps_totals_as_plain_text() {
    use serde_json::json;
    use vtcode_commons::ansi::strip_ansi;

    let diffs = vec![
        json!({"path": "src/a.rs", "operation": "updated", "additions": 2, "deletions": 1}),
        json!({"path": "src/b.rs", "operation": "updated", "additions": 1, "deletions": 2}),
    ];
    let styled = styled_aggregate_summary(&diffs, &GitStyles::new(), true);
    let plain = strip_ansi(&styled);
    assert!(plain.contains("Edited 2 files"), "aggregate must keep file count: {plain:?}");
    assert!(plain.contains("+3"), "aggregate must sum additions: {plain:?}");
    assert!(plain.contains("-3"), "aggregate must sum deletions: {plain:?}");
}

#[test]
fn list_dir_render_survives_an_extreme_page_value() {
    // `page`/`per_page` arrive from the tool payload as raw JSON numbers. The
    // summary line computes `(page - 1) * per_page + 1`, which overflows — and
    // release keeps `overflow-checks = true`, so this used to abort the CLI.
    use crate::agent::runloop::tool_output::collect_inline_output;
    use serde_json::json;
    use vtcode_core::ui::InlineHandle;
    use vtcode_core::utils::ansi::AnsiRenderer;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "count": 2,
        "total": 10,
        "page": 9_223_372_036_854_775_808_u64,
        "per_page": 20,
        "path": "src",
        "items": [{"name": "a.rs", "type": "file"}, {"name": "b.rs", "type": "file"}]
    });

    render_list_dir_output(&mut renderer, &payload, &LsStyles::from_env()).expect("extreme page must render");
    let output = collect_inline_output(&mut receiver);
    assert!(output.contains("Showing 2 of 10 items"), "got: {output:?}");
}
