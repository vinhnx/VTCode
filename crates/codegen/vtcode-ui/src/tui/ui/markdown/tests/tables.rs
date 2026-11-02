use super::*;

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
        code_block_weight: CodeBlockWeight::Primary,
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
