use super::*;

#[test]
fn side_by_side_diff_falls_back_narrow_and_recovers_wide() {
    fn render(mode: vtcode_commons::ui_protocol::DiffPreviewMode, width: usize) -> String {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        renderer.set_diff_preview_mode(mode);
        renderer.set_table_max_width(Some(width));
        let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
        render_diff_content_block(
            &mut renderer,
            "--- a/file.rs\n+++ b/file.rs\n@@ -1 +1 @@\n-old value\n+new value\n",
            Some("apply_patch"),
            &git,
            &LsStyles::from_env(),
            MessageStyle::ToolDetail,
            ToolOutputMode::Full,
            100,
        )
        .expect("diff preview should render");
        strip_ansi_codes(&collect_inline_output(&mut receiver)).into_owned()
    }

    assert_eq!(
        render(vtcode_commons::ui_protocol::DiffPreviewMode::SideBySide, 50),
        render(vtcode_commons::ui_protocol::DiffPreviewMode::Inline, 50),
        "narrow side-by-side should use the inline renderer"
    );
    assert_ne!(
        render(vtcode_commons::ui_protocol::DiffPreviewMode::SideBySide, 80),
        render(vtcode_commons::ui_protocol::DiffPreviewMode::Inline, 80),
        "wide side-by-side should recover the configured layout"
    );
}
#[test]
fn wide_side_by_side_rows_are_not_clipped_to_inline_preview_limit() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_diff_preview_mode(vtcode_commons::ui_protocol::DiffPreviewMode::SideBySide);
    renderer.set_table_max_width(Some(200));
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let old = format!("-{}\n", "old ".repeat(40));
    let new = format!("+{}\n", "new ".repeat(40));
    let diff = format!("@@ -1 +1 @@\n{old}{new}");

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
    .expect("wide side-by-side diff should render");

    let collected = collect_inline_output(&mut receiver);
    let output = strip_ansi_codes(&collected);
    assert!(
        output
            .lines()
            .any(|line| vtcode_commons::preview::display_width(line) > MAX_LINE_LENGTH),
        "source panes should use the measured width instead of the inline cap: {output:?}"
    );
}
#[test]
fn side_by_side_metadata_emits_full_paths_for_reflow() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_diff_preview_mode(vtcode_commons::ui_protocol::DiffPreviewMode::SideBySide);
    renderer.set_table_max_width(Some(80));
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let path = "very-long-directory-name/another-long-directory-name/source-file.rs";
    let diff = format!("diff --git a/{path} b/{path}\n@@ -1 +1 @@\n-old\n+new\n");

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
    .expect("side-by-side metadata should render");

    let collected = collect_inline_output(&mut receiver);
    let output = strip_ansi_codes(&collected);
    assert!(output.contains(path), "inline TUI metadata must keep the full path for reflow wrap: {output:?}");
    assert!(
        !output.contains("..."),
        "inline TUI metadata must not ellipsis-truncate under the safety cap: {output:?}"
    );
}
#[test]
fn diff_section_headers_are_foreground_only_and_highlighted() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let mut buffer = String::new();

    let file_header = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::Metadata, None, None, "--- a/README.md"),
        git.file_old,
        3,
        None,
        &git,
        &mut buffer,
    )
    .to_owned();
    assert!(!file_header.contains("48;"), "file header must not have a background: {file_header:?}");
    assert!(file_header.contains("38;2;255;180;180"), "file header foreground missing: {file_header:?}");

    let hunk_header = format_diff_line_with_gutter_and_syntax(
        &test_diff_line(DiffDisplayKind::HunkHeader, None, None, "@@ -1 +1 @@"),
        git.hunk,
        3,
        None,
        &git,
        &mut buffer,
    )
    .to_owned();
    assert!(!hunk_header.contains("48;"), "hunk header must not have a background: {hunk_header:?}");
    assert!(hunk_header.contains("36m"), "hunk header foreground missing: {hunk_header:?}");
}
#[test]
fn side_by_side_diff_resets_empty_pane_and_keeps_tints_isolated() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
    let mut deleted = test_diff_line(DiffDisplayKind::Deletion, Some(3), None, "old value");
    deleted.changed = vec![(4, 9)];
    let mut added = test_diff_line(DiffDisplayKind::Addition, None, Some(3), "new value");
    added.changed = vec![(4, 9)];

    let mut buffer = String::new();
    let left_only = format_side_by_side_row_ansi(
        &SideBySideRow { left: Some(deleted), right: None },
        2,
        24,
        &git,
        None,
        &mut buffer,
    )
    .to_owned();
    assert!(left_only.contains("48;2;70;38;42"));
    assert!(left_only.contains("48;2;140;52;58"));
    assert!(!left_only.contains("48;2;20;58;45"));
    assert!(left_only.contains("\x1b[0m\x1b[49m"), "empty right pane must reset its background");

    let right_only =
        format_side_by_side_row_ansi(&SideBySideRow { left: None, right: Some(added) }, 2, 24, &git, None, &mut buffer)
            .to_owned();
    assert!(right_only.contains("48;2;20;58;45"));
    assert!(right_only.contains("48;2;36;100;70"));
    assert!(!right_only.contains("48;2;70;38;42"));
    assert!(right_only.starts_with("\x1b[0m\x1b[49m"), "empty left pane must reset its background");
}
#[test]
fn ansi16_side_by_side_diff_stays_foreground_only() {
    let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::Ansi16);
    let added = test_diff_line(DiffDisplayKind::Addition, None, Some(3), "new value");
    let mut buffer = String::new();
    let rendered =
        format_side_by_side_row_ansi(&SideBySideRow { left: None, right: Some(added) }, 2, 24, &git, None, &mut buffer);

    assert!(!rendered.contains("48;"), "ANSI16 must not paint a background: {rendered:?}");
    assert!(!rendered.contains("49m"), "ANSI16 must not emit background resets: {rendered:?}");
    assert!(rendered.contains("\x1b[92m"), "ANSI16 foreground should remain visible: {rendered:?}");
}
