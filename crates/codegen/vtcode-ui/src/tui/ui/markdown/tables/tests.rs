use super::*;

fn ml(text: &str) -> MarkdownLine {
    let mut line = MarkdownLine::default();
    line.push_segment(Style::default(), text);
    line
}

fn text_lines(lines: &[MarkdownLine]) -> Vec<String> {
    lines
        .iter()
        .map(|line| line.segments.iter().map(|segment| segment.text.as_str()).collect())
        .collect()
}

fn table(headers: &[&str], rows: &[&[&str]]) -> TableBuffer {
    TableBuffer {
        headers: headers.iter().map(|text| ml(text)).collect(),
        rows: rows.iter().map(|row| row.iter().map(|text| ml(text)).collect()).collect(),
        ..TableBuffer::default()
    }
}

#[test]
fn total_table_width_includes_padding_and_gaps() {
    assert_eq!(total_table_width(&[10, 10]), 26);
    assert_eq!(total_table_width(&[5, 5, 5]), 25);
}

#[test]
fn wide_table_uses_padded_rows_and_horizontal_separators() {
    let lines =
        render_table(&table(&["Name", "Status"], &[&["core", "active"], &["ui", "paused"]]), Style::default(), None);
    let text = text_lines(&lines);

    assert_eq!(text[0], " Name    Status");
    assert!(text[1].contains('━'));
    assert!(text[2].contains("core"));
    assert!(text[3].contains('─'));
    assert!(!text.iter().any(|line| line.contains('│')));
}

#[test]
fn ambiguous_wide_terminal_renders_ascii_table_rules() {
    // Box-drawing rules break on terminals that render the
    // East-Asian-ambiguous set double-width; ASCII glyphs are the only
    // safe chrome there (nextest isolates the one-time verdict override).
    vtcode_commons::terminal_capabilities::set_ambiguous_width(vtcode_commons::ui_protocol::AmbiguousWidthMode::Wide);
    let lines =
        render_table(&table(&["Name", "Status"], &[&["core", "active"], &["ui", "paused"]]), Style::default(), None);
    let text = text_lines(&lines);

    assert!(text[1].contains('='));
    assert!(text[3].contains('-'));
    assert!(!text.iter().any(|line| line.contains('━') || line.contains('─')));
}

#[test]
fn table_keeps_intrinsic_width_at_boundary_and_records_below_it() {
    let fixture = table(&["A", "B"], &[&["1", "2"]]);
    let wide = text_lines(&render_table(&fixture, Style::default(), Some(12)));
    let narrow = text_lines(&render_table(&fixture, Style::default(), Some(11)));

    assert!(wide.iter().any(|line| line.contains('━')));
    assert!(narrow.iter().any(|line| line.contains("A")));
    assert!(narrow.iter().any(|line| line.contains("B")));
    assert!(!narrow.iter().any(|line| line.contains('━')));
}

#[test]
fn headerless_tables_preserve_grid_layout_when_scaled() {
    let mut fixture = table(&[], &[&["headerless", "value"]]);
    fixture.alignments = vec![Alignment::None, Alignment::None];
    let lines = render_table(&fixture, Style::default(), Some(8));
    let text = text_lines(&lines);

    assert!(text[0].contains("  "));
    assert!(lines.iter().all(|line| line.width() <= 8));
}

#[test]
fn headerless_tables_fallback_before_padding_overflows_width() {
    let fixture = table(&[], &[&["left", "right"]]);
    let lines = render_table(&fixture, Style::default(), Some(6));

    assert!(lines.iter().all(|line| line.width() <= 6));
    assert!(text_lines(&lines).iter().any(|line| line.contains("left")));
}

#[test]
fn wrapped_grid_preserves_columns_before_record_fallback() {
    let fixture = table(&["ID", "Description"], &[&["1", "a short readable description"]]);
    let lines = render_table(&fixture, Style::default(), Some(22));
    let text = text_lines(&lines);

    assert!(text.iter().any(|line| line.contains('━')));
    assert!(!text.iter().any(|line| line.contains("ID:")));
    assert!(lines.iter().all(|line| line.width() <= 22));
}

#[test]
fn narrow_table_uses_aligned_records_with_indented_continuations() {
    let fixture = table(
        &["Claim", "Current state"],
        &[&["Threshold", "Partial: beta/context edit/trigger/pause already exists"]],
    );
    let lines = render_table(&fixture, Style::default(), Some(25));
    let text = text_lines(&lines);

    assert!(text[0].contains("Claim"));
    assert!(text[0].contains("Threshold"));
    assert!(text.iter().any(|line| line.starts_with("          ")));
    assert!(lines.iter().all(|line| line.width() <= 25));
}

#[test]
fn very_narrow_records_stack_value_below_label() {
    let fixture = table(&["Label", "Value"], &[&["one", "a long value"]]);
    let lines = render_table(&fixture, Style::default(), Some(10));
    let text = text_lines(&lines);

    assert!(text.iter().any(|line| line.trim() == "Label"));
    assert!(text.iter().any(|line| line.contains("one")));
    assert!(text.iter().any(|line| line.contains("Value")));
    assert!(lines.iter().all(|line| line.width() <= 10));
}

#[test]
fn markdown_alignment_is_applied_inside_cells() {
    let mut fixture = table(&["L", "C", "R"], &[&["x", "x", "x"]]);
    fixture.alignments = vec![Alignment::Left, Alignment::Center, Alignment::Right];
    let lines = render_table(&fixture, Style::default(), None);
    let header = text_lines(&lines)[0].clone();
    let row = text_lines(&lines)[2].clone();

    assert!(header.contains(" L "));
    assert!(row.contains("  x "));
    assert!(row.ends_with(" x"));
}

#[test]
fn fallback_preserves_value_style_and_link() {
    let mut value = MarkdownLine::default();
    value.push_segment_with_link(Style::default().underline(), "documentation", Some("https://example.com".to_owned()));
    let mut fixture = table(&["Reference"], &[]);
    fixture.rows = vec![vec![value]];

    let lines = render_table(&fixture, Style::default(), Some(5));
    let linked_segments = lines
        .iter()
        .flat_map(|line| line.segments.iter())
        .filter(|segment| segment.link_target.as_deref() == Some("https://example.com"))
        .collect::<Vec<_>>();
    assert_eq!(linked_segments.iter().map(|segment| segment.text.as_str()).collect::<String>(), "documentation");
    assert!(
        linked_segments
            .iter()
            .all(|segment| segment.style.get_effects().contains(anstyle::Effects::UNDERLINE))
    );
}

#[test]
fn fallback_wraps_unicode_values_to_display_width() {
    let fixture = table(&["項目"], &[&["表の説明"]]);
    let lines = render_table(&fixture, Style::default(), Some(7));
    assert!(lines.iter().all(|line| line.width() <= 7), "fallback line exceeded width: {lines:?}");
}

#[test]
fn wrapping_handles_words_and_long_tokens() {
    let line = ml("hello world foo bar");
    let wrapped = wrap_markdown_line(&line, 12);
    assert_eq!(text_lines(&wrapped), ["hello world", "foo bar"]);

    let line = ml("abcdefghij");
    let wrapped = wrap_markdown_line(&line, 5);
    assert_eq!(text_lines(&wrapped), ["abcde", "fghij"]);
}
