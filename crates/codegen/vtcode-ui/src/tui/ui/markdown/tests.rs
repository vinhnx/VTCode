use super::code_blocks::{
    normalize_code_indentation, normalize_diff_lines, render_diff_code_block_with_context, render_diff_content_segments,
};
use super::links::{
    COLON_LOCATION_SUFFIX_RE, HASH_LOCATION_SUFFIX_RE, label_has_location_suffix, label_segments_have_location_suffix,
    normalize_hash_location,
};
use super::*;
use crate::tui::ui::syntax_highlight::DiffScopeBackgroundRgbs;
use crate::tui::utils::diff_styles::DiffColorPalette;
use crate::tui::utils::diff_styles::{
    DiffColorLevel, DiffLineType, DiffTheme, diff_add_fg, diff_del_fg, diff_render_style_context_for,
    style_content_ansi, style_sign_ansi,
};

fn lines_to_text(lines: &[MarkdownLine]) -> Vec<String> {
    lines
        .iter()
        .map(|line| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>())
        .collect()
}

fn render_markdown_with_table_width(markdown: &str, table_max_width: usize) -> Vec<MarkdownLine> {
    render_markdown_to_lines_with_options(
        markdown,
        Style::default(),
        &theme::active_styles(),
        None,
        RenderMarkdownOptions {
            preserve_code_indentation: false,
            disable_code_block_table_reparse: false,
            table_max_width: Some(table_max_width),
        },
    )
}

fn render_markdown_highlighted(markdown: &str) -> Vec<MarkdownLine> {
    render_markdown_to_lines_with_options(
        markdown,
        Style::default(),
        &theme::active_styles(),
        Some(&SyntaxHighlightingConfig::default()),
        RenderMarkdownOptions::default(),
    )
}

fn diff_backgrounds_enabled() -> bool {
    DiffColorLevel::detect() != DiffColorLevel::Ansi16
}

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

#[test]
fn test_markdown_table_box_drawing() {
    let markdown = r#"
| Header 1 | Header 2 |
|----------|----------|
| Cell 1   | Cell 2   |
| Cell 3   | Cell 4   |
"#;

    let lines = render_markdown(markdown);
    let output: String = lines
        .iter()
        .map(|line| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    // The responsive grid uses padded cells and horizontal rules instead of
    // vertical borders, matching the Codex-inspired table presentation.
    assert!(output.contains('━'), "table should render a header separator: {output}");
    assert!(!output.contains('│'), "table should not render vertical borders: {output}");
}

#[test]
fn test_markdown_table_renders_header_separator_and_rows() {
    let markdown = "\
| File | Line | Function |
|------|------|----------|
| src/main.rs | 10 | main |
| src/lib.rs | 20 | init |
";

    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let non_blank: Vec<&str> = text_lines.iter().map(String::as_str).filter(|l| !l.is_empty()).collect();

    assert!(non_blank.len() >= 5, "expected header + separators + 2 rows, got: {non_blank:?}");
    assert!(
        non_blank[0].contains("File") && non_blank[0].contains("Function"),
        "first line should be the header row: {}",
        non_blank[0]
    );
    assert!(non_blank[1].contains('━'), "second line should be the header separator: {}", non_blank[1]);
    assert!(
        non_blank.iter().any(|line| line.contains("src/main.rs")),
        "first data row is missing: {non_blank:?}"
    );
    assert!(
        non_blank.iter().any(|line| line.contains("src/lib.rs")),
        "second data row is missing: {non_blank:?}"
    );
    assert!(non_blank.iter().any(|line| line.contains('─')), "body separator is missing: {non_blank:?}");
}

#[test]
fn test_narrow_markdown_table_renders_aligned_records() {
    let markdown = "| Was | Now |\n|-----|-----|\n| old | [new](https://example.com/new) |\n| one | two |\n";
    let lines = render_markdown_with_table_width(markdown, 8);
    let output = lines_to_text(&lines).join("\n");

    assert!(output.contains("Was"), "fallback should retain the first header: {output}");
    assert!(output.contains("Now"), "fallback should retain the second header: {output}");
    assert!(!output.contains("│"), "narrow table should not render column separators: {output}");
    assert!(output.contains("─"), "rows should remain visually separated: {output}");

    let was_segment = lines
        .iter()
        .flat_map(|line| line.segments.iter())
        .find(|segment| segment.text == "Was")
        .expect("fallback header segment exists");
    assert!(was_segment.style.get_effects().contains(anstyle::Effects::BOLD));

    let link_segment = lines
        .iter()
        .flat_map(|line| line.segments.iter())
        .find(|segment| segment.text.starts_with("new"))
        .expect("fallback value link exists");
    assert_eq!(link_segment.link_target.as_deref(), Some("https://example.com/new"));
}

#[test]
fn test_markdown_table_boundary_uses_intrinsic_width() {
    let markdown = "| A | B |\n|---|---|\n| 1 | 2 |\n";
    let at_boundary = lines_to_text(&render_markdown_with_table_width(markdown, 12)).join("\n");
    let below_boundary = lines_to_text(&render_markdown_with_table_width(markdown, 11)).join("\n");

    assert!(at_boundary.contains('━'), "exact intrinsic width should keep table layout: {at_boundary}");
    assert!(
        below_boundary.contains('A') && below_boundary.contains('1'),
        "fallback should retain the first field: {below_boundary}"
    );
    assert!(!below_boundary.contains('━'), "fallback should remove grid separators: {below_boundary}");
}

#[test]
fn test_table_inside_markdown_code_block_renders_as_table() {
    let markdown = "```markdown\n\
        | Module | Purpose |\n\
        |--------|----------|\n\
        | core   | Library  |\n\
        ```\n";

    let lines = render_markdown(markdown);
    let output: String = lines
        .iter()
        .map(|line| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        output.contains('━'),
        "Table inside ```markdown code block should render with horizontal table rules, got: {output}"
    );
    // Should NOT contain code-block line numbers
    assert!(!output.contains("  1  "), "Table inside markdown code block should not have line numbers");
}

#[test]
fn test_table_inside_md_code_block_renders_as_table() {
    let markdown = "```md\n\
        | A | B |\n\
        |---|---|\n\
        | 1 | 2 |\n\
        ```\n";

    let lines = render_markdown(markdown);
    let output = lines_to_text(&lines).join("\n");

    assert!(output.contains('━'), "Table inside ```md code block should render as table: {output}");
}

#[test]
fn test_narrow_markdown_and_md_fenced_tables_use_aligned_records() {
    for language in ["markdown", "md"] {
        let markdown = format!(
            "```{language}\n| Name | Description |\n|------|-------------|\n| item | a long description that needs wrapping |\n```\n"
        );
        let output = lines_to_text(&render_markdown_with_table_width(&markdown, 18)).join("\n");

        assert!(output.contains("Name"), "{language} fence should retain the header label: {output}");
        assert!(output.contains("Description"), "{language} fence should retain all header labels: {output}");
        assert!(!output.contains("│"), "narrow {language} table should use aligned records: {output}");
    }
}

#[test]
fn test_non_markdown_fenced_table_remains_code() {
    let markdown = "```text\n| Name | Value |\n|------|-------|\n| item | value |\n```\n";
    let output = lines_to_text(&render_markdown_with_table_width(markdown, 8)).join("\n");

    assert!(output.contains("| Name |"), "non-Markdown fence should retain source pipes: {output}");
    assert!(!output.contains("│"), "non-Markdown fence should not be reparsed as a table: {output}");
}

#[test]
fn test_unlabeled_fenced_table_remains_code() {
    let markdown = "```\n| Name | Value |\n|------|-------|\n| item | value |\n```\n";
    let output = lines_to_text(&render_markdown_with_table_width(markdown, 8)).join("\n");

    assert!(output.contains("| Name |"), "unlabeled fence should retain source pipes: {output}");
    assert!(!output.contains("│"), "unlabeled fence should not be reparsed as a table: {output}");
}

#[test]
fn test_table_code_block_reparse_guard_can_disable_table_reparse() {
    let markdown = "```markdown\n\
        | Module | Purpose |\n\
        |--------|----------|\n\
        | core   | Library  |\n\
        ```\n";
    let options = RenderMarkdownOptions {
        preserve_code_indentation: false,
        disable_code_block_table_reparse: true,
        table_max_width: None,
    };
    let lines =
        render_markdown_to_lines_with_options(markdown, Style::default(), &theme::active_styles(), None, options);
    let output = lines_to_text(&lines).join("\n");

    assert!(
        output.contains("| Module | Purpose |"),
        "Guarded render should keep code-block content literal: {output}"
    );
    assert!(output.contains("  1  "), "Guarded render should keep code-block line numbers: {output}");
}

#[test]
fn test_rust_code_block_with_pipes_not_treated_as_table() {
    let markdown = "```rust\n\
        | Header | Col |\n\
        |--------|-----|\n\
        | a      | b   |\n\
        ```\n";

    let lines = render_markdown(markdown);
    let output = lines_to_text(&lines).join("\n");

    // Rust code blocks should NOT be reinterpreted as tables
    assert!(output.contains("| Header |"), "Rust code block should keep raw pipe characters: {output}");
}

#[test]
fn test_markdown_code_block_with_language_renders_line_numbers() {
    let markdown = "```rust\nfn main() {}\n```\n";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let code_line = text_lines
        .iter()
        .find(|line| line.contains("fn main() {}"))
        .expect("code line exists");
    assert!(code_line.contains("  1  "));
}

#[test]
fn test_markdown_code_block_trailing_newline_has_no_phantom_line_number() {
    let markdown = "```rust\nfn first() {}\nfn second() {}\n```\n";
    let lines = render_markdown_highlighted(markdown);

    let code_lines = lines
        .iter()
        .filter(|line| {
            let text = line.segments.iter().map(|segment| segment.text.as_str()).collect::<String>();
            text.contains("fn first() {}") || text.contains("fn second() {}")
        })
        .count();
    assert_eq!(code_lines, 2);

    assert!(
        lines
            .iter()
            .any(|line| line.segments.iter().any(|segment| segment.text == "  2  ")),
        "second source line should have a gutter"
    );
    assert!(
        lines
            .iter()
            .all(|line| line.segments.iter().all(|segment| segment.text != "  3  ")),
        "trailing newline should not create a phantom gutter"
    );
}

#[test]
fn test_markdown_code_block_leading_whitespace_uses_code_style() {
    let markdown = "```rust\nlet value = 1;\n    return value;\n```\n";
    let lines = render_markdown_highlighted(markdown);

    let code_line = lines
        .iter()
        .find(|line| {
            line.segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<String>()
                .contains("return value")
        })
        .expect("code line exists");
    let gutter_style = code_line.segments.first().expect("gutter segment exists").style;
    let first_code_segment = code_line
        .segments
        .iter()
        .find(|segment| segment.text == "    ")
        .expect("leading whitespace segment exists");

    assert_ne!(first_code_segment.style, gutter_style);
}

#[test]
fn test_markdown_code_block_omitted_line_gutter_uses_source_line_numbers() {
    let markdown = "```rust\n\
line 1\n\
line 2\n\
… [+70 lines omitted; use read_file with offset/limit (1-indexed line numbers) for full content]\n\
tail line\n\
```\n";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);

    let omitted_line = text_lines
        .iter()
        .find(|line| line.contains("lines omitted"))
        .expect("omitted line exists");
    assert!(omitted_line.contains("3-72  "), "omitted line should render source range, got: {omitted_line}");

    let tail_line = text_lines
        .iter()
        .find(|line| line.contains("tail line"))
        .expect("tail line exists");
    assert!(tail_line.contains("73  "), "tail line should continue from omitted range, got: {tail_line}");
}

#[test]
fn test_markdown_code_block_without_language_skips_line_numbers() {
    let markdown = "```\nfn main() {}\n```\n";
    let lines = render_markdown(markdown);
    let text_lines = lines_to_text(&lines);
    let code_line = text_lines
        .iter()
        .find(|line| line.contains("fn main() {}"))
        .expect("code line exists");
    assert!(!code_line.contains("  1  "));
}

#[test]
fn markdown_diff_code_block_stores_line_background() {
    let markdown = "```diff\n@@ -1 +1 @@\n- old\n+ new\n context\n```\n";
    let lines = render_markdown(markdown);

    let joined = |line: &MarkdownLine| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>();
    let added_line = lines
        .iter()
        .find(|line| joined(line).contains("+ new"))
        .expect("added line exists");
    assert_eq!(added_line.line_background.is_some(), diff_backgrounds_enabled());
    assert!(added_line.segments.iter().any(|seg| seg.style.get_fg_color().is_some()));
    assert_eq!(added_line.segments.iter().all(|seg| seg.style.get_bg_color().is_some()), diff_backgrounds_enabled());
    assert_eq!(added_line.segments[0].text, "+");
    assert!(!added_line.segments.iter().any(|seg| seg.text.contains('│')));

    let removed_line = lines
        .iter()
        .find(|line| joined(line).contains("- old"))
        .expect("removed line exists");
    assert_eq!(removed_line.line_background.is_some(), diff_backgrounds_enabled());
    assert!(removed_line.segments.iter().any(|seg| seg.style.get_fg_color().is_some()));
    assert_eq!(
        removed_line.segments.iter().all(|seg| seg.style.get_bg_color().is_some()),
        diff_backgrounds_enabled()
    );
    assert_eq!(removed_line.segments[0].text, "-");

    let context_line = lines
        .iter()
        .find(|line| joined(line).contains("context"))
        .expect("context line exists");
    assert!(context_line.line_background.is_none());
    assert!(context_line.segments.iter().all(|seg| seg.style.get_bg_color().is_none()));
}

#[test]
fn markdown_diff_truecolor_has_row_tint_and_stronger_word_chip() {
    let context =
        diff_render_style_context_for(DiffTheme::Dark, DiffColorLevel::TrueColor, DiffScopeBackgroundRgbs::default());
    let lines = render_diff_code_block_with_context(
        "@@ -1 +1 @@\n-const value = 1;\n+const value = 2;\n context",
        &theme::active_styles(),
        Style::default(),
        &[],
        context,
    );
    let joined = |line: &MarkdownLine| line.segments.iter().map(|segment| segment.text.as_str()).collect::<String>();
    let added = lines.iter().find(|line| joined(line).contains("value = 2")).expect("added row");
    let removed = lines
        .iter()
        .find(|line| joined(line).contains("value = 1"))
        .expect("removed row");
    let context_line = lines.iter().find(|line| joined(line).contains("context")).expect("context row");

    let add_row = anstyle::Color::Rgb(anstyle::RgbColor(20, 58, 45));
    let add_word = anstyle::Color::Rgb(anstyle::RgbColor(36, 100, 70));
    let del_row = anstyle::Color::Rgb(anstyle::RgbColor(70, 38, 42));
    let del_word = anstyle::Color::Rgb(anstyle::RgbColor(140, 52, 58));

    assert_eq!(added.line_background, Some(add_row));
    assert_eq!(removed.line_background, Some(del_row));
    assert!(
        added
            .segments
            .iter()
            .any(|segment| segment.style.get_bg_color() == Some(add_word))
    );
    assert!(
        removed
            .segments
            .iter()
            .any(|segment| segment.style.get_bg_color() == Some(del_word))
    );
    assert!(
        added
            .segments
            .iter()
            .any(|segment| segment.style.get_bg_color() == Some(add_row))
    );
    assert!(
        removed
            .segments
            .iter()
            .any(|segment| segment.style.get_bg_color() == Some(del_row))
    );
    let added_ansi = added
        .segments
        .iter()
        .map(|segment| segment.style.render().to_string())
        .collect::<String>();
    assert!(added_ansi.contains("48;2;20;58;45"), "row tint must render as ANSI background");
    assert!(added_ansi.contains("48;2;36;100;70"), "word tint must render as ANSI background");
    assert!(context_line.line_background.is_none());
    assert!(
        context_line
            .segments
            .iter()
            .all(|segment| segment.style.get_bg_color().is_none())
    );
}

#[test]
fn markdown_diff_ansi16_is_background_free() {
    let context =
        diff_render_style_context_for(DiffTheme::Dark, DiffColorLevel::Ansi16, DiffScopeBackgroundRgbs::default());
    let lines = render_diff_code_block_with_context(
        "@@ -1 +1 @@\n-old\n+new",
        &theme::active_styles(),
        Style::default(),
        &[],
        context,
    );
    assert!(lines.iter().all(|line| line.line_background.is_none()));
    assert!(
        lines
            .iter()
            .flat_map(|line| line.segments.iter())
            .all(|segment| segment.style.get_bg_color().is_none())
    );
}

#[test]
fn markdown_diff_ansi16_keeps_the_diff_foreground_through_syntax_segments() {
    let context =
        diff_render_style_context_for(DiffTheme::Dark, DiffColorLevel::Ansi16, DiffScopeBackgroundRgbs::default());
    let lines = render_diff_code_block_with_context(
        "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n- fn old() {}\n+ fn new() {}",
        &theme::active_styles(),
        Style::default(),
        &[],
        context,
    );
    let added = lines
        .iter()
        .find(|line| lines_to_text(std::slice::from_ref(line))[0].contains("fn new()"))
        .expect("added row");
    assert!(
        added
            .segments
            .iter()
            .skip(1)
            .all(|segment| segment.style.get_fg_color().is_some()),
        "ANSI16 diff bodies must retain a foreground on every syntax segment"
    );
}

#[test]
fn markdown_diff_empty_add_delete_rows_keep_the_capability_fallback() {
    let markdown = "```diff\n@@ -1 +1 @@\n+\n- \n```\n";
    let lines = render_markdown(markdown);
    let joined = |line: &MarkdownLine| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>();
    // Exclude the `• Diff …` summary row: it carries `+N`/`-N` count text but
    // is header metadata, not a tinted add/del body row.
    let diff_rows: Vec<_> = lines
        .iter()
        .filter(|line| {
            let text = joined(line);
            let trimmed = text.trim_start();
            !trimmed.starts_with("• Diff ")
                && (trimmed.contains('+') || trimmed.contains('-') || trimmed.contains("@@"))
        })
        .collect();
    assert!(diff_rows.len() >= 2, "empty + and - rows must render: {diff_rows:?}");
    for line in diff_rows {
        let text = joined(line);
        if text.trim_start().starts_with("@@") {
            assert!(line.line_background.is_none(), "hunk metadata must stay un-tinted");
            assert!(line.segments.iter().all(|seg| seg.style.get_bg_color().is_none()));
        } else {
            assert_eq!(line.line_background.is_some(), diff_backgrounds_enabled());
            assert_eq!(
                line.segments.iter().all(|seg| seg.style.get_bg_color().is_some()),
                diff_backgrounds_enabled(),
                "empty row must have no background holes"
            );
        }
    }
}

#[test]
fn markdown_diff_header_styles_are_classified() {
    let markdown = "```diff\ndiff --git a/main.rs b/main.rs\nindex 1111111..2222222 100644\n--- a/main.rs\n+++ b/main.rs\n@@ -1 +1 @@\n```\n";
    let lines = render_markdown(markdown);

    let text_and_style = |needle: &str| {
        lines
            .iter()
            .flat_map(|line| line.segments.iter())
            .find(|segment| segment.text.contains(needle))
            .map(|segment| segment.style)
            .unwrap_or_else(|| panic!("{needle} segment exists"))
    };
    let metadata = anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlack);
    let cyan = anstyle::Color::Ansi(anstyle::AnsiColor::Cyan);
    let detected_level = DiffColorLevel::detect();
    let detected_theme = DiffTheme::detect();
    let deletion = diff_del_fg(detected_theme, detected_level);
    let insertion = diff_add_fg(detected_theme, detected_level);

    assert_eq!(text_and_style("diff --git").get_fg_color(), Some(metadata));
    assert_eq!(text_and_style("index 1111111").get_fg_color(), Some(metadata));
    assert_eq!(text_and_style("--- a/main.rs").get_fg_color(), Some(deletion));
    assert_eq!(text_and_style("+++ b/main.rs").get_fg_color(), Some(insertion));
    assert_eq!(text_and_style("@@ -1 +1 @@").get_fg_color(), Some(cyan));
    for needle in ["--- a/main.rs", "+++ b/main.rs", "@@ -1 +1 @@"] {
        assert_eq!(text_and_style(needle).get_bg_color(), None, "{needle} must be background-free");
    }
}

#[test]
fn markdown_diff_preserves_hunk_range_counts() {
    // Regression: a partial hunk must keep its counts; `@@ -65,19 +64,0 @@`
    // collapsed to `@@ -65 +64 @@` reads as a one-line change.
    let markdown = "```diff\n@@ -65,19 +64,0 @@\n-removed\n```\n";
    let lines = render_markdown(markdown);
    let text = lines_to_text(&lines);
    assert!(
        text.iter().any(|line| line.trim() == "@@ -65,19 +64,0 @@"),
        "hunk header must keep range counts, got: {text:?}"
    );
}

#[test]
fn markdown_diff_lines_use_row_tints_and_strong_markers() {
    let markdown = "```diff\n-const value = 1;\n+const value = 2;\n```\n";
    let lines = render_markdown(markdown);

    let added_line = lines
        .iter()
        .find(|line| lines_to_text(std::slice::from_ref(line))[0].contains("const value = 2;"))
        .expect("added line exists");
    let removed_line = lines
        .iter()
        .find(|line| lines_to_text(std::slice::from_ref(line))[0].contains("const value = 1;"))
        .expect("removed line exists");

    // Marker-only reference shape: sign + body, without a second gutter.
    assert_eq!(added_line.segments[0].text, "+");
    assert!(!added_line.segments.iter().any(|seg| seg.text.contains('│')));
    assert_eq!(added_line.line_background.is_some(), diff_backgrounds_enabled());
    assert_eq!(removed_line.line_background.is_some(), diff_backgrounds_enabled());
    // The sign stays bold for scannability.
    assert!(added_line.segments[0].style.get_effects().contains(anstyle::Effects::BOLD));
    // Color-capable output paints every segment; ANSI16 remains foreground-only.
    assert_eq!(added_line.segments.iter().all(|seg| seg.style.get_bg_color().is_some()), diff_backgrounds_enabled());
    assert_eq!(
        removed_line.segments.iter().all(|seg| seg.style.get_bg_color().is_some()),
        diff_backgrounds_enabled()
    );
}

#[test]
fn diff_style_helpers_use_row_tint_and_ansi16_fallback() {
    let truecolor = DiffScopeBackgroundRgbs::default();
    for theme in [DiffTheme::Light, DiffTheme::Dark] {
        let context = diff_render_style_context_for(theme, DiffColorLevel::TrueColor, truecolor);
        for kind in [DiffLineType::Insert, DiffLineType::Delete] {
            let style = style_content_ansi(kind, context);
            let marker = style_sign_ansi(kind, context);
            assert!(style.get_bg_color().is_some());
            assert_eq!(marker.get_bg_color(), style.get_bg_color());
            assert!(marker.get_fg_color().is_some());
            assert_eq!(style.get_fg_color(), None);
        }
    }

    let ansi16 = diff_render_style_context_for(DiffTheme::Dark, DiffColorLevel::Ansi16, truecolor);
    assert!(style_content_ansi(DiffLineType::Insert, ansi16).get_bg_color().is_none());
    assert!(style_sign_ansi(DiffLineType::Insert, ansi16).get_bg_color().is_none());
}

#[test]
fn test_markdown_unlabeled_diff_code_block_detects_diff() {
    let markdown = "```\n@@ -1 +1 @@\n- old\n+ new\n```\n";
    let lines = render_markdown_to_lines(markdown, Style::default(), &theme::active_styles(), None);
    let added_line = lines
        .iter()
        .find(|line| {
            line.segments
                .iter()
                .map(|seg| seg.text.as_str())
                .collect::<String>()
                .contains("new")
        })
        .expect("added line exists");
    assert_eq!(added_line.line_background.is_some(), diff_backgrounds_enabled());
    assert!(added_line.segments.iter().any(|seg| seg.style.get_fg_color().is_some()));
    assert_eq!(added_line.segments.iter().all(|seg| seg.style.get_bg_color().is_some()), diff_backgrounds_enabled());
}

#[test]
fn test_diff_blank_line_renders_placeholder_space() {
    let segments = render_diff_content_segments("", Some("toml"), Style::default());

    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].text, " ");
}

#[test]
fn markdown_diff_code_block_styles_additions_deletions_and_hunk_headers() {
    let markdown = "```diff\n--- a/main.rs\n+++ b/main.rs\n@@ -1 +1 @@\n-const value = 1;\n+const value = 2;\n```\n";
    let lines = render_markdown(markdown);

    let joined = |line: &MarkdownLine| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>();
    let added_line = lines
        .iter()
        .find(|line| joined(line).contains("const value = 2;"))
        .expect("added line exists");
    assert_eq!(added_line.line_background.is_some(), diff_backgrounds_enabled());

    let removed_line = lines
        .iter()
        .find(|line| joined(line).contains("const value = 1;"))
        .expect("removed line exists");
    assert_eq!(removed_line.line_background.is_some(), diff_backgrounds_enabled());

    let header_line = lines
        .iter()
        .find(|line| joined(line).contains("@@ -1 +1 @@"))
        .expect("hunk header exists");
    assert_eq!(header_line.segments[0].style.get_fg_color(), Some(anstyle::Color::Ansi(anstyle::AnsiColor::Cyan)));
    assert!(header_line.line_background.is_none());

    let old_header = lines
        .iter()
        .find(|line| joined(line).contains("--- a/main.rs"))
        .expect("old file header exists");
    assert!(old_header.line_background.is_none());

    let new_header = lines
        .iter()
        .find(|line| joined(line).contains("+++ b/main.rs"))
        .expect("new file header exists");
    assert!(new_header.line_background.is_none());
}

#[test]
fn markdown_diff_rows_use_full_backgrounds_without_holes() {
    let markdown = "```diff\n--- a/main.rs\n+++ b/main.rs\n@@ -1 +1 @@\n-const value = 1;\n+const value = 2;\n```\n";
    let lines = render_markdown(markdown);

    for needle in [
        "const value = 2;",
        "const value = 1;",
        "--- a/main.rs",
        "+++ b/main.rs",
        "@@ -1 +1 @@",
    ] {
        let line = lines
            .iter()
            .find(|line| {
                line.segments
                    .iter()
                    .map(|seg| seg.text.as_str())
                    .collect::<String>()
                    .contains(needle)
            })
            .unwrap_or_else(|| panic!("{needle} line exists"));
        let is_metadata = needle.starts_with("--- ") || needle.starts_with("+++ ") || needle.starts_with("@@");
        assert_eq!(
            line.line_background.is_some(),
            !is_metadata && diff_backgrounds_enabled(),
            "row bg state in {needle}"
        );
        assert!(!line.segments.is_empty());
        for segment in &line.segments {
            assert_eq!(
                segment.style.get_bg_color().is_some(),
                !is_metadata && diff_backgrounds_enabled(),
                "bg hole in {needle}"
            );
        }
        assert!(line.segments.iter().any(|seg| seg.style.get_fg_color().is_some()), "missing fg in {needle}");
    }
}

#[test]
fn markdown_diff_body_preserves_syntax_and_row_backgrounds() {
    let markdown = "```diff\ndiff --git a/main.rs b/main.rs\n--- a/main.rs\n+++ b/main.rs\n@@ -1 +1 @@\n-fn old() {}\n+fn new() {}\n```\n";
    let lines = render_markdown(markdown);

    let find_body = |needle: &str| {
        lines
            .iter()
            .find(|line| {
                line.segments
                    .iter()
                    .map(|seg| seg.text.as_str())
                    .collect::<String>()
                    .contains(needle)
            })
            .unwrap_or_else(|| panic!("{needle} line exists"))
    };
    let added_line = find_body("fn new()");
    let removed_line = find_body("fn old()");

    // Marker + body, with no second gutter and no unpainted row holes.
    assert!(added_line.segments.len() >= 2, "added body needs marker + body");
    assert!(removed_line.segments.len() >= 2, "removed body needs marker + body");
    assert!(
        added_line
            .segments
            .iter()
            .skip(1)
            .all(|segment| { segment.style.get_bg_color().is_some() == diff_backgrounds_enabled() })
    );
    assert!(
        removed_line
            .segments
            .iter()
            .skip(1)
            .all(|segment| { segment.style.get_bg_color().is_some() == diff_backgrounds_enabled() })
    );
    assert_eq!(added_line.line_background.is_some(), diff_backgrounds_enabled());
    assert_eq!(removed_line.line_background.is_some(), diff_backgrounds_enabled());
}

#[test]
fn markdown_apply_patch_headers_provide_syntax_language_for_diff_bodies() {
    let context =
        diff_render_style_context_for(DiffTheme::Dark, DiffColorLevel::TrueColor, DiffScopeBackgroundRgbs::default());
    let lines = render_diff_code_block_with_context(
        "*** Begin Patch\n*** Update File: src/main.rs\n@@\n- fn old() {}\n+ fn new() {}\n*** End Patch",
        &theme::active_styles(),
        Style::default(),
        &[],
        context,
    );
    let added = lines
        .iter()
        .find(|line| lines_to_text(std::slice::from_ref(line))[0].contains("fn new()"))
        .expect("added apply_patch body exists");
    assert!(
        added
            .segments
            .iter()
            .skip(1)
            .any(|segment| segment.style.get_fg_color().is_some()),
        "the .rs apply_patch body should retain syntax foregrounds"
    );
}

#[test]
fn markdown_diff_body_without_path_stays_solid() {
    let markdown = "```diff\n@@ -1 +1 @@\n-fn old() {}\n+fn new() {}\n```\n";
    let lines = render_markdown(markdown);

    let added_line = lines
        .iter()
        .find(|line| {
            line.segments
                .iter()
                .map(|seg| seg.text.as_str())
                .collect::<String>()
                .contains("fn new()")
        })
        .expect("added line exists");
    // No file header: marker + body (word chips may split the body).
    assert!(added_line.segments.len() >= 2);
    assert_eq!(added_line.segments[0].text, "+");
}

#[test]
fn markdown_diff_prose_file_body_uses_row_tint_without_syntax_split() {
    let markdown = "```diff\ndiff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-old **bold** text\n+new **bold** text\n```\n";
    let lines = render_markdown(markdown);

    let added_line = lines
        .iter()
        .find(|line| {
            line.segments
                .iter()
                .map(|seg| seg.text.as_str())
                .collect::<String>()
                .contains("new **bold** text")
        })
        .expect("added line exists");
    // Marker + body, with prose kept as one body segment.
    assert!(added_line.segments.len() >= 2, "prose body: marker + body");
    assert_eq!(added_line.line_background.is_some(), diff_backgrounds_enabled());
}

#[test]
fn test_diff_prose_content_skips_syntax_highlighting() {
    let fallback = Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::BrightGreen)));
    for hint in [Some("md"), Some("markdown"), Some("txt")] {
        let segments = render_diff_content_segments("- **Agent-first**: prose `code`", hint, fallback);
        assert_eq!(segments.len(), 1, "prose hint {hint:?} must stay solid");
        assert_eq!(segments[0].text, "- **Agent-first**: prose `code`");
        assert_eq!(segments[0].style, fallback);
    }
}

#[test]
fn test_diff_unknown_language_stays_solid_fallback() {
    let fallback = Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::BrightGreen)));
    for hint in [None, Some(""), Some("xyzunknown123")] {
        let segments = render_diff_content_segments("fn new() {}", hint, fallback);
        assert_eq!(segments.len(), 1, "unknown hint {hint:?} must stay solid");
        assert_eq!(segments[0].style, fallback);
    }
}

#[test]
fn markdown_unlabeled_minimal_hunk_detects_diff() {
    let markdown = "```\n@@\n pub fn demo() {\n  -    old();\n  +    new();\n }\n```\n";
    let lines = render_markdown_to_lines(markdown, Style::default(), &theme::active_styles(), None);
    let header_segment = lines
        .iter()
        .flat_map(|line| line.segments.iter())
        .find(|seg| seg.text.trim() == "@@")
        .expect("hunk header exists");
    assert_eq!(header_segment.style.get_fg_color(), Some(anstyle::Color::Ansi(anstyle::AnsiColor::Cyan)));

    let joined = |line: &MarkdownLine| line.segments.iter().map(|seg| seg.text.as_str()).collect::<String>();
    let removed_segment = lines
        .iter()
        .find(|line| joined(line).contains("old();"))
        .expect("removed line exists");
    assert_eq!(removed_segment.line_background.is_some(), diff_backgrounds_enabled());

    let added_segment = lines
        .iter()
        .find(|line| joined(line).contains("new();"))
        .expect("added line exists");
    assert_eq!(added_segment.line_background.is_some(), diff_backgrounds_enabled());
}

#[test]
fn test_highlight_line_for_diff_strips_background_colors() {
    let segments =
        highlight_line_for_diff("let changed = true;", Some("rust")).expect("highlighting should return segments");
    assert!(segments.iter().all(|(style, _)| style.get_bg_color().is_none()));
}

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
