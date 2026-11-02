use super::*;

#[test]
fn test_markdown_task_list_markers() {
    let markdown = "- [x] Done\n- [ ] Todo\n";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    assert!(text_lines.iter().any(|line| line.contains("[x]")));
    assert!(text_lines.iter().any(|line| line.contains("[ ]")));
}
#[test]
fn test_code_indentation_normalization_removes_common_indent() {
    let code_with_indent = "    fn hello() {\n        println!(\"world\");\n    }";
    let expected = "fn hello() {\n    println!(\"world\");\n}";
    let result = normalize_code_indentation(code_with_indent, Some("rust"), false);
    assert_eq!(result, expected);
}
#[test]
fn test_code_indentation_preserves_already_normalized() {
    let code = "fn hello() {\n    println!(\"world\");\n}";
    let result = normalize_code_indentation(code, Some("rust"), false);
    assert_eq!(result, code);
}
#[test]
fn test_code_indentation_without_language_hint() {
    // Without language hint, normalization still happens - common indent is stripped
    let code = "    some code";
    let result = normalize_code_indentation(code, None, false);
    assert_eq!(result, "some code");
}
#[test]
fn test_code_indentation_preserves_relative_indentation() {
    let code = "    line1\n        line2\n    line3";
    let expected = "line1\n    line2\nline3";
    let result = normalize_code_indentation(code, Some("python"), false);
    assert_eq!(result, expected);
}
#[test]
fn test_code_indentation_mixed_whitespace_preserves_indent() {
    // Mixed tabs and spaces - common prefix should be empty if they differ
    let code = "    line1\n\tline2";
    let result = normalize_code_indentation(code, None, false);
    // Should preserve original content rather than stripping incorrectly
    assert_eq!(result, code);
}
#[test]
fn test_code_indentation_common_prefix_mixed() {
    // Common prefix is present ("    ")
    let code = "    line1\n    \tline2";
    let expected = "line1\n\tline2";
    let result = normalize_code_indentation(code, None, false);
    assert_eq!(result, expected);
}
#[test]
fn test_code_indentation_preserve_when_requested() {
    let code = "    line1\n        line2\n    line3\n";
    let result = normalize_code_indentation(code, Some("rust"), true);
    assert_eq!(result, code);
}
#[test]
fn test_diff_summary_counts_function_signature_change() {
    // Test case matching the user's TODO scenario - function signature change
    let diff = "diff --git a/ask.rs b/ask.rs\n\
index 0000000..1111111 100644\n\
--- a/ask.rs\n\
+++ b/ask.rs\n\
@@ -172,7 +172,7 @@\n\
      blocks\n\
  }\n\
 \n\
-    fn select_best_code_block<'a>(blocks: &'a [CodeFenceBlock]) -> Option<&'a CodeFenceBlock> {\n\
+    fn select_best_code_block(blocks: &[CodeFenceBlock]) -> Option<&CodeFenceBlock> {\n\
      let mut best = None;\n\
      let mut best_score = (0usize, 0u8);\n\
      for block in blocks {";

    let lines = normalize_diff_lines(diff);

    // Find the summary line
    let summary_line = lines
        .iter()
        .find(|l| l.starts_with("• Diff "))
        .expect("should have summary line");

    // Should show (+1 -1) not (+0 -0)
    assert_eq!(summary_line, "• Diff ask.rs (+1 -1)");
}
#[test]
fn test_markdown_file_link_hides_destination() {
    let markdown = "[markdown_render.rs:74](/Users/example/code/codex/codex-rs/tui/src/markdown_render.rs:74)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);

    // Should contain the link text but NOT the destination
    assert!(text_lines.iter().any(|line| line.contains("markdown_render.rs:74")));
    assert!(!text_lines.iter().any(|line| line.contains("/Users/example")));
}
#[test]
fn test_markdown_url_link_shows_destination() {
    let markdown = "[docs](https://example.com/docs)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain both the link text and the destination
    assert!(combined.contains("docs"));
    assert!(combined.contains("https://example.com/docs"));
}
#[test]
fn test_markdown_relative_link_hides_destination() {
    let markdown = "[relative](./path/to/file.md)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain the link text but NOT the destination
    assert!(combined.contains("relative"));
    assert!(!combined.contains("./path/to/file.md"));
}
#[test]
fn test_markdown_home_relative_link_hides_destination() {
    let markdown = "[home relative](~/path/to/file.md)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain the link text but NOT the destination
    assert!(combined.contains("home relative"));
    assert!(!combined.contains("~/path/to/file.md"));
}
#[test]
fn test_markdown_parent_relative_link_hides_destination() {
    let markdown = "[parent](../path/to/file.md)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain the link text but NOT the destination
    assert!(combined.contains("parent"));
    assert!(!combined.contains("../path/to/file.md"));
}
#[test]
fn test_markdown_file_url_link_hides_destination() {
    let markdown = "[file url](file:///path/to/file.md)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain the link text but NOT the destination
    assert!(combined.contains("file url"));
    assert!(!combined.contains("file:///path/to/file.md"));
}
#[test]
fn test_markdown_windows_path_link_hides_destination() {
    let markdown = "[windows](C:\\path\\to\\file.md)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain the link text but NOT the destination
    assert!(combined.contains("windows"));
    assert!(!combined.contains("C:\\path\\to\\file.md"));
}
#[test]
fn test_markdown_https_link_shows_destination() {
    let markdown = "[secure](https://secure.example.com)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain both the link text and the destination
    assert!(combined.contains("secure"));
    assert!(combined.contains("https://secure.example.com"));
}
#[test]
fn test_markdown_http_link_shows_destination() {
    let markdown = "[http](http://example.com)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain both the link text and the destination
    assert!(combined.contains("http"));
    assert!(combined.contains("http://example.com"));
}
#[test]
fn test_plain_file_paths_get_link_targets() {
    let markdown = "See src/main.rs and README.md.";
    let lines = render_markdown(markdown);
    let mut targets = Vec::new();
    for line in &lines {
        for seg in &line.segments {
            if let Some(target) = &seg.link_target {
                targets.push((seg.text.clone(), target.clone()));
            }
        }
    }

    assert!(
        targets
            .iter()
            .any(|(text, target)| text == "src/main.rs" && **target == *"src/main.rs")
    );
    assert!(
        targets
            .iter()
            .any(|(text, target)| text == "README.md" && **target == *"README.md")
    );
    assert!(!targets.iter().any(|(text, _)| text.ends_with('.')));
}
#[test]
fn test_plain_urls_are_not_file_links() {
    let markdown = "See https://example.com/docs for info.";
    let lines = render_markdown(markdown);
    let has_link_target = lines
        .iter()
        .flat_map(|line| line.segments.iter())
        .any(|seg| seg.link_target.is_some());
    assert!(!has_link_target);
}
#[test]
fn test_quoted_file_path_with_spaces_gets_link_target() {
    let markdown = "Open \"docs/My Notes.md\" for info.";
    let lines = render_markdown(markdown);
    let has_link_target = lines
        .iter()
        .flat_map(|line| line.segments.iter())
        .any(|seg| seg.text == "docs/My Notes.md" && seg.link_target.as_deref() == Some("docs/My Notes.md"));
    assert!(has_link_target);
}
#[test]
fn test_load_location_suffix_regexes() {
    let _colon = &*COLON_LOCATION_SUFFIX_RE;
    let _hash = &*HASH_LOCATION_SUFFIX_RE;
}
#[test]
fn test_file_link_hides_destination() {
    let markdown =
        "[codex-rs/tui/src/markdown_render.rs](/Users/example/code/codex/codex-rs/tui/src/markdown_render.rs)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain the link text but NOT the destination path
    assert!(combined.contains("codex-rs/tui/src/markdown_render.rs"));
    assert!(!combined.contains("/Users/example"));
}
#[test]
fn test_file_link_appends_line_number_when_label_lacks_it() {
    let markdown = "[markdown_render.rs](/Users/example/code/codex/codex-rs/tui/src/markdown_render.rs:74)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain the filename AND the line number
    assert!(combined.contains("markdown_render.rs"));
    assert!(combined.contains(":74"));
}
#[test]
fn test_file_link_uses_label_for_line_number() {
    let markdown = "[markdown_render.rs:74](/Users/example/code/codex/codex-rs/tui/src/markdown_render.rs:74)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain the label with line number, but not duplicate
    assert!(combined.contains("markdown_render.rs:74"));
    // Should not have duplicate :74
    assert!(!combined.contains(":74:74"));
}
#[test]
fn test_label_suffix_detection_across_segments() {
    let segments = vec![
        MarkdownSegment::new(Style::default(), "markdown_render.rs:"),
        MarkdownSegment::new(Style::default().italic(), "74"),
    ];
    assert!(label_segments_have_location_suffix(&segments));
}
#[test]
fn test_file_link_appends_hash_anchor_when_label_lacks_it() {
    let markdown = "[markdown_render.rs](file:///Users/example/code/codex/codex-rs/tui/src/markdown_render.rs#L74C3)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain the filename AND the converted location
    assert!(combined.contains("markdown_render.rs"));
    assert!(combined.contains(":74:3"));
}
#[test]
fn test_file_link_uses_label_for_hash_anchor() {
    let markdown =
        "[markdown_render.rs#L74C3](file:///Users/example/code/codex/codex-rs/tui/src/markdown_render.rs#L74C3)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain the label with location, but not duplicate
    assert!(combined.contains("markdown_render.rs#L74C3"));
}
#[test]
fn test_file_link_appends_range_when_label_lacks_it() {
    let markdown = "[markdown_render.rs](/Users/example/code/codex/codex-rs/tui/src/markdown_render.rs:74:3-76:9)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain the filename AND the range
    assert!(combined.contains("markdown_render.rs"));
    assert!(combined.contains(":74:3-76:9"));
}
#[test]
fn test_file_link_uses_label_for_range() {
    let markdown =
        "[markdown_render.rs:74:3-76:9](/Users/example/code/codex/codex-rs/tui/src/markdown_render.rs:74:3-76:9)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain the label with range, but not duplicate
    assert!(combined.contains("markdown_render.rs:74:3-76:9"));
    // Should not have duplicate range
    assert!(!combined.contains(":74:3-76:9:74:3-76:9"));
}
#[test]
fn test_file_link_appends_hash_range_when_label_lacks_it() {
    let markdown =
        "[markdown_render.rs](file:///Users/example/code/codex/codex-rs/tui/src/markdown_render.rs#L74C3-L76C9)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain the filename AND the converted range
    assert!(combined.contains("markdown_render.rs"));
    assert!(combined.contains(":74:3-76:9"));
}
#[test]
fn test_file_link_uses_label_for_hash_range() {
    let markdown = "[markdown_render.rs#L74C3-L76C9](file:///Users/example/code/codex/codex-rs/tui/src/markdown_render.rs#L74C3-L76C9)";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let combined = text_lines.join("");

    // Should contain the label with range, but not duplicate
    assert!(combined.contains("markdown_render.rs#L74C3-L76C9"));
}
#[test]
fn test_normalize_hash_location_single() {
    assert_eq!(normalize_hash_location("L74C3"), Some(":74:3".to_string()));
}
#[test]
fn test_normalize_hash_location_range() {
    assert_eq!(normalize_hash_location("L74C3-L76C9"), Some(":74:3-76:9".to_string()));
}
#[test]
fn test_normalize_hash_location_line_only() {
    assert_eq!(normalize_hash_location("L74"), Some(":74".to_string()));
}
#[test]
fn test_normalize_hash_location_range_line_only() {
    assert_eq!(normalize_hash_location("L74-L76"), Some(":74-76".to_string()));
}
#[test]
fn test_label_has_location_suffix_colon() {
    assert!(label_has_location_suffix("file.rs:74"));
    assert!(label_has_location_suffix("file.rs:74:3"));
    assert!(label_has_location_suffix("file.rs:74:3-76:9"));
    assert!(!label_has_location_suffix("file.rs"));
}
#[test]
fn test_label_has_location_suffix_hash() {
    assert!(label_has_location_suffix("file.rs#L74C3"));
    assert!(label_has_location_suffix("file.rs#L74C3-L76C9"));
    assert!(!label_has_location_suffix("file.rs#section"));
}
