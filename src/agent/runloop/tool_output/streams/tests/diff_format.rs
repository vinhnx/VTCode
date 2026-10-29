use super::*;

#[test]
fn render_preview_line_truncates_and_prefixes() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let line = "x".repeat(MAX_LINE_LENGTH + 10);

    render_preview_line(&mut renderer, &line, None, Some("  "), true, MessageStyle::ToolOutput, None)
        .expect("preview line should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.starts_with("  "));
    assert!(inline_output.ends_with("..."));
}
#[test]
fn diff_stream_preview_keeps_head_tail_and_omission_marker() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let mut diff = String::from("@@ -1,600 +1,600 @@\n");
    for index in 0..600 {
        diff.push_str(&format!("-old-{index}\n+new-{index}\n"));
    }

    render_diff_content_block(
        &mut renderer,
        &diff,
        Some("apply_patch"),
        &GitStyles::new(),
        &LsStyles::from_env(),
        MessageStyle::ToolDetail,
        ToolOutputMode::Compact,
        5,
    )
    .expect("bounded diff should render");

    let collected = collect_inline_output(&mut receiver);
    let output = strip_ansi_codes(&collected);
    assert!(output.contains("old-0"), "head should be retained: {output:?}");
    assert!(output.contains("new-599"), "tail should be retained: {output:?}");
    assert!(output.contains("review full diff"), "vertical omission must advertise expand: {output:?}");
    assert!(
        output.contains("lines") && (output.contains("omitted") || output.contains('+')),
        "omission count should remain discoverable: {output:?}"
    );
}
#[tokio::test]
async fn safety_capped_diff_body_advertises_expand_and_records_anchor() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    // One logical addition row far above DIFF_WRAP_SOURCE_MAX_WIDTH (2000).
    let long_row = format!("+{}", "x".repeat(2_050));
    let diff = format!("@@ -1,1 +1,1 @@\n{long_row}\n");

    render_diff_content_block(
        &mut renderer,
        &diff,
        Some("apply_patch"),
        &GitStyles::new(),
        &LsStyles::from_env(),
        MessageStyle::ToolDetail,
        ToolOutputMode::Full,
        50,
    )
    .expect("safety-capped diff should render");

    let collected = collect_inline_output(&mut receiver);
    let output = strip_ansi_codes(&collected);
    assert!(output.contains("review full diff"), "safety-cap truncation must advertise expand: {output:?}");
    assert!(
        output.contains("RecordDiffReview") || output.contains("diff truncated"),
        "safety-cap path must record an expand payload: {output:?}"
    );
}
#[test]
fn format_diff_line_styles_gutter_for_additions() {
    let style = anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green)));
    let mut buf = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::Addition, None, Some(1377), "let x = 1;"),
        Some(style),
        5,
        None,
        &GitStyles::new(),
        &mut buf,
    );
    assert!(rendered.contains("\u{1b}["));
    let stripped = strip_ansi_codes(rendered);
    // Blank old column (5) + ` │ ` + ` 1377` + ` │ ` + `+ content`.
    assert!(stripped.contains("+ 1377 │ let x = 1;"), "got: {stripped:?}");
}
#[test]
fn format_diff_line_preserves_code_indentation() {
    let mut buf = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::Addition, None, Some(1384), "    line,"),
        None,
        5,
        None,
        &GitStyles::new(),
        &mut buf,
    );
    let stripped = strip_ansi_codes(rendered);
    assert!(stripped.contains("+ 1384 │     line,"));
}
#[test]
fn format_diff_line_keeps_blank_line_spacing() {
    let mut buf = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::Addition, None, Some(42), ""),
        None,
        5,
        None,
        &GitStyles::new(),
        &mut buf,
    );
    let stripped = strip_ansi_codes(rendered);
    assert!(stripped.contains("+   42 │ "), "got: {stripped:?}");
}
#[test]
fn format_diff_line_clears_reused_buffer_for_metadata() {
    let mut buf = String::new();
    let _ = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::Addition, None, Some(1), "let x = 1;"),
        None,
        5,
        None,
        &GitStyles::new(),
        &mut buf,
    );
    let rendered = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::Metadata, None, None, "diff --git a/src/lib.rs b/src/lib.rs"),
        None,
        5,
        None,
        &GitStyles::new(),
        &mut buf,
    );

    assert_eq!(strip_ansi_codes(rendered), "diff --git a/src/lib.rs b/src/lib.rs");
}
#[test]
fn format_diff_line_keeps_markdown_bullet_distinct_from_marker() {
    let mut buf = String::new();
    let rendered = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::Addition, None, Some(53), "- **Agent-first by design*: prose"),
        None,
        5,
        None,
        &GitStyles::new(),
        &mut buf,
    );
    let stripped = strip_ansi_codes(rendered);
    assert!(stripped.contains("+   53 │ - **Agent-first"), "got: {stripped:?}");
}
#[test]
fn format_diff_line_truncates_long_addition() {
    let mut buf = String::new();
    let long_text = "y".repeat(MAX_LINE_LENGTH * 2);
    // Single gutter: sign(1) + number(5) + " │ "(3) = 9.
    let gutter_width = 9;
    let mut line = test_diff_line(DiffDisplayKind::Addition, None, Some(9), &long_text);
    // Simulate a chip that would be invalid after truncation.
    line.changed = vec![(0, long_text.len())];
    let word_bg = Some(anstyle::Color::Rgb(anstyle::RgbColor(36, 100, 70)));
    let rendered = format_diff_line_with_gutter_and_syntax(&line, None, 5, word_bg, &GitStyles::new(), &mut buf);
    let stripped = strip_ansi_codes(rendered);
    assert!(
        vtcode_commons::preview::display_width(&stripped) <= MAX_LINE_LENGTH + gutter_width,
        "rendered diff line must not exceed MAX_LINE_LENGTH + gutter width"
    );
    assert!(stripped.contains("..."));
    // Truncated rows must not paint word chips with stale offsets.
    assert!(!rendered.contains("48;2;36;100;70"), "no chip on truncated line");
}
#[test]
fn diff_bodies_stay_solid_no_syntax_brightness() {
    let bg = Some(anstyle::Color::Rgb(anstyle::RgbColor(20, 58, 45)));
    let rendered = highlight_diff_content("- **bold** and `code`", bg, &[], None).expect("solid body");
    // Solid tint only: no bright syntax SGR, no bold, no extra fg.
    assert!(rendered.contains("48;2;20;58;45"));
    assert!(!rendered.contains("38;2;"));
    assert!(!rendered.contains("\u{1b}[1m"));
    assert!(!rendered.contains("\u{1b}[91m"));
    assert!(!rendered.contains("\u{1b}[92m"));
    assert!(highlight_diff_content("plain", None, &[], None).is_none());
}
#[test]
fn diff_language_hint_infers_extension_from_headers() {
    let diff = "diff --git a/src/main.rs b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n";
    assert_eq!(diff_language_hint_from_content(diff).as_deref(), Some("rs"));
    let markers = "--- a/app.ts\n+++ b/app.ts\n@@ -1 +1 @@\n-old\n+new\n";
    assert_eq!(diff_language_hint_from_content(markers).as_deref(), Some("ts"));
    assert_eq!(diff_language_hint_from_content("@@ -1 +1 @@\n-old\n+new\n"), None);
    let patch = "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n+new\n";
    assert_eq!(diff_language_hint_from_content(patch).as_deref(), Some("rs"));
    let quoted = "diff --git \"a/my file.rs\" \"b/my file.rs\"\n@@ -1 +1 @@\n-old\n+new\n";
    assert_eq!(diff_language_hint_from_content(quoted).as_deref(), Some("rs"));
}
