use super::*;

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
    let lines = format_diff_content_lines(diff);
    assert_eq!(lines[0], "diff --git a/file1.txt b/file1.txt");
    assert!(lines.iter().any(|line| line == "@@ -1 +1 @@"));
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
    let lines = format_diff_content_lines(diff);
    assert!(lines.iter().any(|line| line.starts_with("+++ ")));
    assert!(lines.iter().any(|line| line == "@@ -2 +2 @@"));
    // No "• Diff" summary line generated
    assert!(!lines.iter().any(|l| l.starts_with("• Diff ")));
}

#[test]
fn strip_line_number_removes_prefix() {
    assert_eq!(strip_line_number("  42: fn main() {"), "fn main() {");
    assert_eq!(strip_line_number("1:hello"), "hello");
    assert_eq!(strip_line_number("no_number_here"), "no_number_here");
    assert_eq!(strip_line_number("abc: not a number"), "abc: not a number");
}

#[test]
fn shorten_path_preserves_short() {
    assert_eq!(shorten_path("/src/main.rs", 60), "/src/main.rs");
}

#[test]
fn shorten_path_truncates_long() {
    let long = "/very/long/deeply/nested/path/to/some/file.rs";
    let short = shorten_path(long, 30);
    assert!(short.len() <= 45);
    assert!(short.contains("file.rs"));
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
    let lines = format_diff_content_lines(diff);

    // No "• Diff" summary line generated
    assert!(!lines.iter().any(|l| l.starts_with("• Diff ")));
    assert!(lines.iter().any(|l| l.contains("diff --git")));
    assert!(lines.iter().any(|l| l == "@@ -172 +172 @@"));
}

#[test]
fn formats_condensed_edit_preview_with_summary_and_small_context() {
    let diff = "\
diff --git a/vtcode-config/src/loader/config.rs b/vtcode-config/src/loader/config.rs
index 0000000..1111111 100644
--- a/vtcode-config/src/loader/config.rs
+++ b/vtcode-config/src/loader/config.rs
@@ -536,4 +536,4 @@
 # Suppress notifications while terminal is focused
-suppress_when_focused = false
+suppress_when_focused = true
 
@@ -545,4 +545,4 @@
 # Success notifications for tool call results
-tool_success = true
+tool_success = false
 ";

    let lines = format_condensed_edit_diff_lines(diff);
    assert_eq!(
        lines[0],
        "• Edited vtcode-config/src/loader/config.rs (+2 -2)"
    );
    assert!(lines.iter().any(|line| line.contains("- 537")));
    assert!(lines.iter().any(|line| line.contains("+ 537")));
    assert!(lines.iter().any(|line| line.contains("- 546")));
    assert!(lines.iter().any(|line| line.contains("+ 546")));
    assert!(lines.iter().any(|line| line.contains("⋮")));
}

#[test]
fn condensed_edit_preview_falls_back_for_non_diff() {
    let lines = format_condensed_edit_diff_lines("plain text output");
    assert_eq!(lines, vec!["plain text output".to_string()]);
}

#[test]
fn condensed_edit_preview_handles_diff_without_diff_git_header() {
    let diff = "\
--- a/file2.txt
+++ b/file2.txt
@@ -2,3 +2,3 @@
-before
+after
";

    let lines = format_condensed_edit_diff_lines(diff);
    assert_eq!(lines[0], "• Edited file2.txt (+1 -1)");
    assert!(lines.iter().any(|line| line.contains("-   2")));
    assert!(lines.iter().any(|line| line.contains("+   2")));
}
