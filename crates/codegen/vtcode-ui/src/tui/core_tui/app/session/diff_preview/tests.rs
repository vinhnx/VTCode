use super::{
    build_inline_diff_line, build_side_pane_line, control_lines, effective_diff_layout, header_action_label,
    pad_line_to_width, remaining_inline_rows_for_test, should_show_inline_gutter, side_by_side_display_lines,
    styled_content_spans,
};
use crate::tui::core_tui::app::types::{DiffPreviewMode, DiffPreviewState};
use crate::tui::ui::syntax_highlight::DiffScopeBackgroundRgbs;
use crate::tui::utils::diff_styles::{
    DiffColorLevel, DiffLineType, DiffTheme, diff_render_style_context_for, style_line_bg, style_word_bg,
};
use ratatui::text::{Line, Span};
use vtcode_commons::ui_protocol::DiffPreviewMode as DiffLayoutMode;
use vtcode_diff::{DiffDisplayKind, DiffDisplayLine, DiffLayout, DiffRowKind, LayoutOptions, layout_display_lines};

#[test]
fn conflict_controls_show_proceed_reload_abort_copy() {
    let preview = DiffPreviewState::new_with_mode(
        "src/main.rs".to_string(),
        "before".to_string(),
        "after".to_string(),
        Vec::new(),
        DiffPreviewMode::FileConflict,
    );

    let lines = control_lines(&preview);
    let first_line: String = lines[0].spans.iter().map(|span| span.content.clone().into_owned()).collect();

    assert!(first_line.contains("Proceed"));
    assert!(first_line.contains("Reload"));
    assert!(first_line.contains("Abort"));
}

#[test]
fn remaining_inline_rows_reports_hidden_wrapped_content() {
    let before = (0..40).map(|index| format!("old-{index}\n")).collect::<String>();
    let after = (0..40).map(|index| format!("new-{index}\n")).collect::<String>();
    let preview = DiffPreviewState::new_with_mode(
        "src/main.rs".to_string(),
        before,
        after,
        Vec::new(),
        DiffPreviewMode::ReadonlyReview,
    );
    let content = ratatui::layout::Rect::new(0, 0, 80, 8);

    assert!(remaining_inline_rows_for_test(&preview, content) > 0);
}

#[test]
fn readonly_review_controls_show_back_navigation() {
    let preview = DiffPreviewState::new_with_mode(
        "src/main.rs".to_string(),
        "before".to_string(),
        "after".to_string(),
        Vec::new(),
        DiffPreviewMode::ReadonlyReview,
    );

    let lines = control_lines(&preview);
    let first_line: String = lines[0].spans.iter().map(|span| span.content.clone().into_owned()).collect();

    assert!(first_line.contains("Back"));
    assert!(!first_line.contains("Proceed"));
    assert!(!first_line.contains("Reload"));
}

#[test]
fn conflict_header_uses_conflict_label() {
    assert_eq!(header_action_label(DiffPreviewMode::FileConflict), "← Conflict ");
    assert_eq!(header_action_label(DiffPreviewMode::EditApproval), "← Edit ");
    assert_eq!(header_action_label(DiffPreviewMode::ReadonlyReview), "← Review ");
}

#[test]
fn side_by_side_scroll_keeps_replacement_rows_paired() {
    let mut preview = DiffPreviewState::new_with_mode(
        "src/main.rs".to_string(),
        "old-1\nold-2\n".to_string(),
        "new-1\nnew-2\n".to_string(),
        Vec::new(),
        DiffPreviewMode::ReadonlyReview,
    );
    preview.scroll_by(3);

    let rows = layout_display_lines(
        side_by_side_display_lines(&preview),
        LayoutOptions {
            layout: DiffLayout::SideBySide,
            ..LayoutOptions::default()
        },
    );
    assert_eq!(rows[0].kind, DiffRowKind::Addition);
    assert_eq!(rows[0].left.as_ref().map(|cell| cell.marker), Some('-'));
    assert_eq!(rows[0].right.as_ref().map(|cell| cell.marker), Some('+'));
}

#[test]
fn syntax_segments_keep_foregrounds_and_add_intraline_emphasis() {
    let syntax = vec![
        (
            anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Blue))),
            "let ".to_owned(),
        ),
        (
            anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow))),
            "value".to_owned(),
        ),
    ];
    let fallback = ratatui::style::Style::default().fg(ratatui::style::Color::LightGreen);
    let row_bg = Some(ratatui::style::Color::Rgb(20, 58, 45));
    let word_bg = Some(ratatui::style::Color::Rgb(36, 100, 70));
    let spans = styled_content_spans("let value", &[(4, 9)], Some(&syntax), 0, fallback, row_bg, word_bg);

    assert_eq!(spans.iter().map(|span| span.content.as_ref()).collect::<String>(), "let value");
    assert_eq!(spans[0].style.fg, Some(ratatui::style::Color::Blue));
    assert_eq!(spans[1].style.fg, Some(ratatui::style::Color::Yellow));
    assert!(spans[1].style.add_modifier.contains(ratatui::style::Modifier::BOLD));
    assert!(spans[1].style.add_modifier.contains(ratatui::style::Modifier::UNDERLINED));
    assert_eq!(spans[0].style.bg, row_bg);
    assert_eq!(spans[1].style.bg, word_bg);
}

#[test]
fn diff_rows_tint_gutter_words_padding_and_wrapped_continuations() {
    let context =
        diff_render_style_context_for(DiffTheme::Dark, DiffColorLevel::TrueColor, DiffScopeBackgroundRgbs::default());
    let mut added = DiffDisplayLine::body(DiffDisplayKind::Addition, None, Some(7), "changed tail".to_owned());
    added.changed = vec![(0, 7)];

    let line = build_side_pane_line(&added, None, 0, context, 24);
    let row_bg = style_line_bg(DiffLineType::Insert, context).bg;
    let word_bg = style_word_bg(DiffLineType::Insert, context).bg;
    assert_eq!(line.spans.first().and_then(|span| span.style.bg), row_bg);
    assert!(line.spans.iter().any(|span| span.style.bg == word_bg));
    assert_eq!(line.spans.last().and_then(|span| span.style.bg), row_bg);

    let continuation = build_side_pane_line(&added, None, 7, context, 24);
    assert_eq!(continuation.spans.last().and_then(|span| span.style.bg), row_bg);

    let context_line = DiffDisplayLine::body(DiffDisplayKind::Context, Some(7), Some(7), "same".to_owned());
    let context_rendered = build_side_pane_line(&context_line, None, 0, context, 24);
    assert!(context_rendered.spans.iter().all(|span| span.style.bg.is_none()));
}

#[test]
fn padding_preserves_base_tint_and_empty_panes_are_unstyled() {
    let base = ratatui::style::Style::default().bg(ratatui::style::Color::Rgb(20, 58, 45));
    let word = ratatui::style::Style::default().bg(ratatui::style::Color::Rgb(36, 100, 70));
    let line = Line::from(vec![Span::styled("base", base), Span::styled("word", word)]);
    let padded = pad_line_to_width(line, 16, base);
    assert_eq!(padded.spans.last().and_then(|span| span.style.bg), base.bg);
    assert_eq!(padded.spans[1].style.bg, word.bg);
    assert!(Line::default().spans.is_empty(), "missing side-by-side cells must stay terminal-default");
}

#[test]
fn inline_diff_gutter_hides_only_when_source_room_is_tight() {
    let context =
        diff_render_style_context_for(DiffTheme::Dark, DiffColorLevel::TrueColor, DiffScopeBackgroundRgbs::default());
    let added = DiffDisplayLine::body(DiffDisplayKind::Addition, None, Some(7), "changed".to_owned());

    let compact = build_inline_diff_line(&added, None, 0, context, 28, 5, false);
    let compact_text: String = compact.spans.iter().map(|span| span.content.as_ref()).collect();
    assert!(!compact_text.contains('+'));
    assert!(!compact_text.contains('│'));
    assert!(!compact_text.contains('7'));
    assert!(compact.spans.iter().any(|span| span.style.fg.is_some()));
    assert_eq!(compact.spans.iter().map(Span::width).sum::<usize>(), 28);

    let full = build_inline_diff_line(&added, None, 0, context, 29, 5, true);
    let full_text: String = full.spans.iter().map(|span| span.content.as_ref()).collect();
    assert!(full_text.starts_with('+'));
    assert!(full_text.contains('│'));
    assert!(full_text.contains('7'));
}

#[test]
fn ansi16_inline_diff_keeps_the_gutter_when_source_room_is_tight() {
    let context =
        diff_render_style_context_for(DiffTheme::Dark, DiffColorLevel::Ansi16, DiffScopeBackgroundRgbs::default());
    assert!(should_show_inline_gutter(context, 28, 5));

    let added = DiffDisplayLine::body(DiffDisplayKind::Addition, None, Some(7), "changed".to_owned());
    let line = build_inline_diff_line(&added, None, 0, context, 28, 5, true);
    let text: String = line.spans.iter().map(|span| span.content.as_ref()).collect();
    assert!(text.starts_with('+'));
    assert!(text.contains('│'));
}

#[test]
fn configured_side_by_side_resolves_back_after_resize() {
    assert_eq!(effective_diff_layout(DiffLayoutMode::SideBySide, 59), DiffLayoutMode::Inline);
    assert_eq!(effective_diff_layout(DiffLayoutMode::SideBySide, 60), DiffLayoutMode::SideBySide);
    assert_eq!(effective_diff_layout(DiffLayoutMode::Inline, 120), DiffLayoutMode::Inline);
}

#[test]
fn preview_caches_syntax_segments_for_each_hunk_line() {
    let preview = DiffPreviewState::new_with_mode(
        "src/main.rs".to_owned(),
        "fn main() {\n    let old = 1;\n}\n".to_owned(),
        "fn main() {\n    let new = 2;\n}\n".to_owned(),
        Vec::new(),
        DiffPreviewMode::ReadonlyReview,
    );
    let body_line_count = preview.document.hunks.iter().map(|hunk| hunk.lines.len()).sum::<usize>();

    assert_eq!(preview.syntax_lines.len(), body_line_count);
    assert!(preview.document.hunks.iter().flat_map(|hunk| &hunk.lines).all(|line| {
        preview
            .syntax_segments(
                match line.kind {
                    vtcode_diff::DiffLineKind::Context => DiffDisplayKind::Context,
                    vtcode_diff::DiffLineKind::Addition => DiffDisplayKind::Addition,
                    vtcode_diff::DiffLineKind::Deletion => DiffDisplayKind::Deletion,
                },
                line.old_line,
                line.new_line,
            )
            .is_some()
    }));
}
