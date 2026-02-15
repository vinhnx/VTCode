use super::*;

#[test]
fn formats_unified_diff_with_summary_and_hunk_headers() {
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
    assert_eq!(lines[1], "• Diff file1.txt (+1 -1)");
    assert!(lines.iter().any(|line| line == "@@ -1 +1 @@"));
}

#[test]
fn formats_diff_without_git_header_with_summary_after_plus() {
    let diff = "\
--- a/file2.txt
+++ b/file2.txt
@@ -2,3 +2,3 @@
-before
+after
";
    let lines = format_diff_content_lines(diff);
    let plus_index = lines
        .iter()
        .position(|line| line.starts_with("+++ "))
        .expect("plus header exists");
    assert_eq!(lines[plus_index + 1], "• Diff file2.txt (+1 -1)");
    assert!(lines.iter().any(|line| line == "@@ -2 +2 @@"));
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
