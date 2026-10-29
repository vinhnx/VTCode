use super::*;

#[test]
fn per_file_language_tracking_switches_grammars() {
    let git_line = DiffDisplayLine::body(
        DiffDisplayKind::Metadata,
        None,
        None,
        "diff --git a/src/main.rs b/src/main.rs".to_string(),
    );
    assert_eq!(language_hint_for_display_line(&git_line).as_deref(), Some("rs"));
    let marker_line = DiffDisplayLine::body(DiffDisplayKind::Metadata, None, None, "+++ b/app.ts".to_string());
    assert_eq!(language_hint_for_display_line(&marker_line).as_deref(), Some("ts"));
    let hunk = DiffDisplayLine::body(DiffDisplayKind::HunkHeader, None, None, "@@ -1 +1 @@".to_string());
    assert_eq!(language_hint_for_display_line(&hunk), None);
}
#[test]
fn syntax_segments_skip_prose_and_plain_but_keep_code() {
    assert!(syntax_segments_for_diff_body("fn main() {}", Some("rs")).is_some());
    assert!(syntax_segments_for_diff_body("hello", Some("md")).is_none());
    assert!(syntax_segments_for_diff_body("hello", Some("txt")).is_none());
    assert!(syntax_segments_for_diff_body("hello", None).is_none());
    assert!(syntax_segments_for_diff_body("", Some("rs")).is_none());
}
#[test]
fn rust_diff_body_layers_syntax_over_row_tint() {
    let bg = Some(anstyle::Color::Rgb(anstyle::RgbColor(20, 58, 45)));
    let word_bg = Some(anstyle::Color::Rgb(anstyle::RgbColor(36, 100, 70)));
    let content = "fn main() { let value = 2; }";
    let rendered = highlight_diff_body_with_syntax(content, Some("rs"), bg, &[], word_bg, None).expect("syntax body");
    assert!(rendered.contains("48;2;20;58;45"), "row tint must survive syntax: {rendered:?}");
    assert!(rendered.contains("38;2;"), "syntax foreground must layer over tint: {rendered:?}");
    // ANSI16/no-color has no tint, so syntax must fall back to solid handling.
    assert!(highlight_diff_body_with_syntax(content, Some("rs"), None, &[], word_bg, None).is_none());
    assert!(highlight_diff_body_with_syntax(content, Some("md"), bg, &[], word_bg, None).is_none());
}
#[test]
fn word_chips_apply_stronger_bg_on_changed_spans() {
    let content = "let a = 1;";
    let word_bg = Some(anstyle::Color::Rgb(anstyle::RgbColor(36, 100, 70)));
    let line_bg = Some(anstyle::Color::Rgb(anstyle::RgbColor(20, 58, 45)));
    let rendered = highlight_diff_content(content, line_bg, &[(8, 9)], word_bg).expect("rendered");
    assert!(rendered.contains("48;2;36;100;70"));
    assert!(rendered.contains("48;2;20;58;45"));
    // Default fg on both spans: no bright syntax leak.
    assert!(!rendered.contains("38;2;"));
}
#[test]
fn inline_diff_uses_shared_accessible_marker_gutter_and_two_level_palette() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let mut line = test_diff_line(DiffDisplayKind::Addition, None, Some(7), "let value = 2;");
    line.changed = vec![(4, 9)];
    let mut buffer = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax(
        &line,
        git.add,
        3,
        git.add_word.and_then(|style| style.get_bg_color()),
        &git,
        &mut buffer,
    )
    .to_owned();

    assert!(rendered.contains("48;2;20;58;45"), "row tint missing: {rendered:?}");
    assert!(rendered.contains("48;2;36;100;70"), "word tint missing: {rendered:?}");
    assert!(rendered.contains("38;2;85;255;85"), "accessible addition marker missing: {rendered:?}");
    assert!(rendered.contains("38;2;165;175;170"), "accessible gutter missing: {rendered:?}");
}
#[test]
fn inline_diff_row_tint_reaches_the_requested_width() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let line = test_diff_line(DiffDisplayKind::Addition, None, Some(7), "let value = 2;");
    let mut buffer = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax_to_width(
        &line,
        git.add,
        3,
        git.add_word.and_then(|style| style.get_bg_color()),
        &git,
        Some(48),
        true,
        None,
        false,
        &mut buffer,
    );
    let stripped = strip_ansi_codes(rendered);

    assert_eq!(vtcode_commons::preview::display_width(&stripped), 48);
    assert!(rendered.contains("48;2;20;58;45"), "row tint missing: {rendered:?}");
    assert!(rendered.ends_with("\x1b[0m"), "padded row must reset cleanly: {rendered:?}");
}
#[test]
fn compact_diff_row_drops_the_visible_gutter_but_keeps_semantic_styling() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let mut line = test_diff_line(DiffDisplayKind::Addition, None, Some(7), "changed");
    line.changed = vec![(0, line.text.len())];
    let mut buffer = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax_to_width(
        &line,
        git.add,
        5,
        git.add_word.and_then(|style| style.get_bg_color()),
        &git,
        Some(28),
        false,
        None,
        false,
        &mut buffer,
    );
    let stripped = strip_ansi_codes(rendered);

    assert_eq!(vtcode_commons::preview::display_width(&stripped), 28);
    assert!(stripped.starts_with(' '));
    assert!(!stripped.contains('+'));
    assert!(!stripped.contains('│'));
    assert!(!stripped.contains('7'));
    assert!(rendered.contains("48;2;20;58;45"), "compact row tint missing: {rendered:?}");
    assert!(rendered.contains("38;2;85;255;85"), "compact semantic foreground missing: {rendered:?}");
    assert!(rendered.contains("48;2;36;100;70"), "compact word tint missing: {rendered:?}");
}
#[test]
fn compact_diff_row_truncates_before_applying_color() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let line = test_diff_line(DiffDisplayKind::Addition, None, Some(7), &"changed ".repeat(20));
    let mut buffer = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax_to_width(
        &line,
        git.add,
        5,
        git.add_word.and_then(|style| style.get_bg_color()),
        &git,
        Some(28),
        false,
        None,
        false,
        &mut buffer,
    );
    let stripped = strip_ansi_codes(rendered);

    assert_eq!(vtcode_commons::preview::display_width(&stripped), 28);
    assert!(rendered.contains("48;2;20;58;45"), "truncated row tint missing: {rendered:?}");
    assert!(rendered.contains("38;2;85;255;85"), "compact semantic foreground missing: {rendered:?}");
}
#[test]
fn compact_context_content_is_not_reclassified_as_a_deletion() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let ls = LsStyles::from_env();
    let line = test_diff_line(DiffDisplayKind::Context, Some(4), Some(4), "- bullet item");

    assert_eq!(
        select_render_line_style(&line, " - bullet item", Some("run_pty_cmd"), &git, &ls),
        None,
        "compact context rows must keep source-leading markers as content"
    );
}
#[test]
fn foreground_only_diff_rows_keep_their_visible_gutter_when_narrow() {
    assert!(should_show_diff_gutter(true, false, Some(1), 5));
    assert!(!should_show_diff_gutter(true, true, Some(28), 5));
    assert!(should_show_diff_gutter(true, true, Some(29), 5));
}
#[test]
fn narrow_inline_diff_rows_emit_full_logical_bodies_for_reflow() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_diff_preview_mode(vtcode_commons::ui_protocol::DiffPreviewMode::Inline);
    renderer.set_table_max_width(Some(28));
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let marker = "old ".repeat(30);
    let diff = format!("@@ -1 +1 @@\n-{}\n+{}\n", marker, "new ".repeat(30));

    render_diff_content_block(
        &mut renderer,
        &diff,
        Some("apply_patch"),
        &git,
        &LsStyles::from_env(),
        MessageStyle::ToolDetail,
        ToolOutputMode::Full,
        100,
    )
    .expect("narrow diff should render");

    let collected = collect_inline_output(&mut receiver);
    let output = strip_ansi_codes(&collected);
    assert!(!output.contains("..."), "inline TUI diff rows must not ellipsis-truncate: {output:?}");
    assert!(
        output.contains(marker.trim_end()),
        "logical body must reach the transcript so reflow can wrap it: {output:?}"
    );
}
#[test]
fn wrap_for_reflow_keeps_long_diff_bodies_whole() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let long_text = format!("| {} | {} |", "What can go wrong", "How VT Code responds".repeat(4));
    let line = test_diff_line(DiffDisplayKind::Addition, None, Some(83), &long_text);
    let mut buffer = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax_to_width(
        &line,
        git.add,
        3,
        git.add_word.and_then(|style| style.get_bg_color()),
        &git,
        Some(28),
        true,
        None,
        true,
        &mut buffer,
    );
    let stripped = strip_ansi_codes(rendered);
    assert!(!stripped.contains("..."), "wrap mode must not ellipsize: {stripped:?}");
    assert!(stripped.contains("How VT Code responds"), "body must stay whole: {stripped:?}");
    assert!(
        vtcode_commons::preview::display_width(&stripped) > 28,
        "wrap mode logical row may exceed measured width: {stripped:?}"
    );
}
