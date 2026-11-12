use super::*;

#[test]
fn test_markdown_heading_renders_prefixes() {
    let markdown = "# Heading\n\n## Subheading\n";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    assert!(text_lines.iter().any(|line| line == "Heading"));
    assert!(text_lines.iter().any(|line| line == "Subheading"));
}
#[test]
fn test_markdown_blockquote_prefix() {
    let markdown = "> Quote line\n> Second line\n";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    assert!(text_lines.iter().any(|line| line.contains("Quote line")));
    assert!(text_lines.iter().any(|line| line.contains("Second line")));
    assert!(
        !text_lines.iter().any(|line| line.contains('│') || line.contains('|')),
        "blockquote must not render a leading bar, got {text_lines:?}"
    );
    for line in &lines {
        for segment in &line.segments {
            if segment.text.contains("Quote line") || segment.text.contains("Second line") {
                assert!(
                    segment.style.get_effects().contains(anstyle::Effects::ITALIC),
                    "blockquote text must be italic, got {segment:?}"
                );
            }
        }
    }
}
#[test]
fn test_markdown_blockquote_end_emits_separator_before_paragraph() {
    let markdown = "> Quote line\n\nNext paragraph\n";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let quote = text_lines.iter().position(|line| line.contains("Quote line")).expect("quote");
    let para = text_lines
        .iter()
        .position(|line| line.contains("Next paragraph"))
        .expect("paragraph");
    assert_eq!(para, quote + 2, "expected exactly one blank separator row, got {text_lines:?}");
    assert!(text_lines[quote + 1].trim().is_empty(), "separator should be blank, got {text_lines:?}");
}
#[test]
fn test_markdown_table_first_response_has_no_leading_blank() {
    let markdown = "| Header 1 | Header 2 |\n|----------|----------|\n| Cell 1   | Cell 2   |\n";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    assert!(!text_lines.is_empty(), "table should render lines");
    assert!(
        !text_lines.first().is_some_and(|line| line.trim().is_empty()),
        "table-first response must not start with a blank row, got {text_lines:?}"
    );
}
#[test]
fn test_markdown_inline_code_strips_backticks() {
    let markdown = "Use `code` here.";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    assert!(text_lines.iter().any(|line| line.contains("Use code here.")));
}
#[test]
fn test_markdown_soft_break_renders_line_break() {
    let markdown = "first line\nsecond line";
    let lines = render_markdown(markdown);
    let text_lines: Vec<String> = lines_to_text(&lines).into_iter().filter(|line| !line.is_empty()).collect();
    assert_eq!(text_lines, vec!["first line".to_string(), "second line".to_string()]);
}
#[test]
fn test_markdown_unordered_list_bullets() {
    let markdown = r#"
- Item 1
- Item 2
  - Nested 1
  - Nested 2
- Item 3
"#;

    let lines = render_markdown(markdown);
    let output: String = lines
        .iter()
        .map(|line| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    // Check for bullet characters (• for depth 0, ◦ for depth 1, etc.)
    assert!(
        output.contains("•") || output.contains("◦") || output.contains("▪"),
        "Should use Unicode bullet characters instead of dashes"
    );
}
#[test]
fn tight_unordered_list_renders_items_without_interior_blanks() {
    let lines = render_markdown("- Alpha\n- Beta\n- Gamma\n");
    let text_lines = lines_to_text(&lines);
    assert_eq!(
        text_lines,
        vec!["• Alpha".to_string(), "• Beta".to_string(), "• Gamma".to_string()],
        "tight items must render on consecutive rows with no blank gap, got {text_lines:?}"
    );
}

#[test]
fn tight_ordered_list_renders_items_without_interior_blanks() {
    let lines = render_markdown("1. First\n2. Second\n");
    let text_lines = lines_to_text(&lines);
    assert_eq!(
        text_lines,
        vec!["1. First".to_string(), "2. Second".to_string()],
        "ordered items must render on consecutive rows with no blank gap, got {text_lines:?}"
    );
}

#[test]
fn loose_list_keeps_exactly_one_blank_between_items() {
    let lines = render_markdown("- Alpha\n\n- Beta\n");
    let text_lines = lines_to_text(&lines);
    assert_eq!(
        text_lines,
        vec!["• Alpha".to_string(), String::new(), "• Beta".to_string()],
        "loose items must keep exactly one blank row (no stacking), got {text_lines:?}"
    );
}

#[test]
fn single_item_list_has_no_interior_blank() {
    let lines = render_markdown("- Solo\n");
    let text_lines = lines_to_text(&lines);
    assert_eq!(text_lines, vec!["• Solo".to_string()], "got {text_lines:?}");
}

#[test]
fn empty_items_keep_bullet_row() {
    // Asymmetric: empty item first vs empty item last.
    let lines = render_markdown("- \n- Beta\n");
    assert_eq!(
        lines_to_text(&lines),
        vec!["• ".to_string(), "• Beta".to_string()],
        "got {:?}",
        lines_to_text(&lines)
    );
    let lines = render_markdown("- Alpha\n-\n");
    assert_eq!(
        lines_to_text(&lines),
        vec!["• Alpha".to_string(), "• ".to_string()],
        "got {:?}",
        lines_to_text(&lines)
    );
}

#[test]
fn empty_outer_item_keeps_bullet_before_nested_list() {
    let lines = render_markdown("- \n  - Inner\n");
    let text_lines = lines_to_text(&lines);
    assert_eq!(
        text_lines,
        vec!["• ".to_string(), "  ◦ Inner".to_string()],
        "outer bullet must not be dropped before a nested list, got {text_lines:?}"
    );
}

#[test]
fn nested_list_separators_do_not_stack() {
    let lines = render_markdown("- Outer one\n  - Inner one\n  - Inner two\n- Outer two\n");
    let text_lines = lines_to_text(&lines);
    assert_eq!(
        text_lines,
        vec![
            "• Outer one".to_string(),
            "  ◦ Inner one".to_string(),
            "  ◦ Inner two".to_string(),
            "• Outer two".to_string(),
        ],
        "tight nested items must render on consecutive rows, got {text_lines:?}"
    );
}

#[test]
fn list_item_with_code_block_keeps_single_separator() {
    let lines = render_markdown("- Before\n```\ncode\n```\n- After\n");
    let text_lines = lines_to_text(&lines);
    let before = text_lines.iter().position(|line| line.contains("Before")).expect("before item");
    let after = text_lines.iter().position(|line| line.contains("After")).expect("after item");
    assert!(
        before + 1 < after,
        "expected a blank separator between the item and the code block, got {text_lines:?}"
    );
    for window in text_lines.windows(2) {
        assert!(
            !(window[0].trim().is_empty() && window[1].trim().is_empty()),
            "blank rows must never stack, got {text_lines:?}"
        );
    }
}

#[test]
fn paragraph_list_paragraph_keeps_single_blank_rhythm() {
    let lines = render_markdown("Intro\n\n- Alpha\n- Beta\n\nOutro\n");
    let text_lines = lines_to_text(&lines);
    assert_eq!(
        text_lines,
        vec![
            "Intro".to_string(),
            String::new(),
            "• Alpha".to_string(),
            "• Beta".to_string(),
            String::new(),
            "Outro".to_string(),
        ],
        "got {text_lines:?}"
    );
}

#[test]
fn test_propose_plan_wrappers_are_stripped_and_bullets_normalized() {
    let markdown = "<proposed_plan>\n## Summary\nFix scrolling.\n\n• Down reaches the final item.\n</proposed_plan>";
    let lines = render_markdown(markdown);
    let output: String = lines
        .iter()
        .map(|line| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!output.contains("<proposed_plan>"), "plan wrappers must not leak: {output}");
    assert!(!output.contains("</proposed_plan>"), "plan wrappers must not leak: {output}");
    assert!(output.contains("Fix scrolling."), "headings/body must survive: {output}");
    assert!(output.contains("•"), "bullets must render as lists: {output}");
}
#[test]
fn test_legacy_plan_wrapper_is_stripped() {
    let markdown = "<plan>\n- Step 1\n</plan>";
    let lines = render_markdown(markdown);
    let output: String = lines
        .iter()
        .map(|line| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!output.contains("<plan>"), "legacy plan tags must not leak: {output}");
    assert!(output.contains("Step 1"), "plan body must survive: {output}");
}
#[test]
fn test_code_block_content_is_not_rewritten_by_plan_cleanup() {
    let markdown = "```text\n• literal <plan>\n```";
    let lines = render_markdown(markdown);
    let output: String = lines
        .iter()
        .map(|line| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(output.contains("• literal <plan>"), "code block content must remain lossless: {output}");
}
#[test]
fn test_inline_code_content_is_not_rewritten_by_plan_cleanup() {
    let markdown = "Use é `<plan>` or `<proposed_plan>` as wrapper names.";
    let lines = render_markdown(markdown);
    let output = lines_to_text(&lines).join("\n");

    assert!(output.contains("<plan>"), "inline plan tag must remain lossless: {output}");
    assert!(output.contains("<proposed_plan>"), "inline plan tag must remain lossless: {output}");
}
#[test]
fn plan_cleanup_keeps_inline_delimiters_across_ordinary_lines() {
    let mut ticks = None;
    assert_eq!(strip_plan_markup_tags("before ``literal", &mut ticks), "before ``literal");
    assert_eq!(ticks, Some(2));
    for text in ["ordinary café 東京", "", "left > right"] {
        assert_eq!(strip_plan_markup_tags(text, &mut ticks), text);
        assert_eq!(ticks, Some(2));
    }
    assert_eq!(strip_plan_markup_tags("` <plan>inside</plan>", &mut ticks), "` <plan>inside</plan>");
    assert_eq!(ticks, Some(2), "a single backtick must not close a double delimiter");
    assert_eq!(strip_plan_markup_tags("end`` <PLAN>outside</PLAN>", &mut ticks), "end`` outside");
    assert_eq!(ticks, None);
    assert_eq!(strip_plan_markup_tags("plain after code", &mut ticks), "plain after code");
    assert_eq!(ticks, None);
}
#[test]
fn plan_cleanup_preserves_fenced_code_and_normalizes_crlf() {
    let cases = [
        ("line café\r\nsecond 東京\r\n", "line café\nsecond 東京\n"),
        (
            "before ``literal\r\nordinary café 東京\r\nend`` after\r\n",
            "before ``literal\nordinary café 東京\nend`` after\n",
        ),
        ("ordinary\r\n\r\n", "ordinary\n"),
        (
            "```text\n<plan>literal</plan>\n```\n<PLAN>after</PLAN>\n",
            "```text\n<plan>literal</plan>\n```\nafter\n",
        ),
        (
            "<PlAn>\r\nbefore ``literal\r\nordinary café 東京\r\n<plan>inside</plan>\r\nend`` after\r\n</pLaN>\r\n",
            "\nbefore ``literal\nordinary café 東京\n<plan>inside</plan>\nend`` after\n",
        ),
        (
            "<proposed_plan>\n```text\n<plan>literal</plan>\n```\n~~~md\n<proposed_plan>other literal</proposed_plan>\n~~~\n</proposed_plan>",
            "\n```text\n<plan>literal</plan>\n```\n~~~md\n<proposed_plan>other literal</proposed_plan>\n~~~\n",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(preprocess_plan_wrappers(source), expected);
    }
}
#[test]
fn multiline_inline_plan_tags_remain_literal_in_rendered_output() {
    let markdown = "<plan>\nOrdinary before ``literal\nborrowed café 東京\n<plan>kept</plan>\nend`` after.\n</plan>";
    let output = lines_to_text(&render_markdown(markdown)).join("\n");
    assert_eq!(output.matches("<plan>").count(), 1, "only the inline code tag must remain");
    assert_eq!(output.matches("</plan>").count(), 1);
    assert!(output.contains("literal borrowed café 東京 <plan>kept</plan> end"), "{output}");
    assert!(output.starts_with("Ordinary before "), "{output}");
    assert!(output.ends_with(" after."), "{output}");
}
