//! Diff preview rendering for file edit approval
//!
//! Renders a syntax-highlighted diff preview with permission controls.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use vtcode_commons::diff_paths::language_hint_from_path;
use vtcode_commons::diff_preview::{
    DiffDisplayKind, count_diff_changes, display_lines_from_hunks,
};

use super::Session;
use crate::core_tui::app::types::{DiffPreviewMode, DiffPreviewState, TrustMode};
use crate::core_tui::style::{ratatui_color_from_ansi, ratatui_style_from_ansi};
use crate::ui::markdown::render_diff_content_segments;
use crate::utils::diff::{DiffBundle, DiffOptions, compute_diff_with_theme};
use crate::utils::diff_styles::{
    DiffColorPalette, DiffLineType, current_diff_render_style_context, style_content, style_gutter,
    style_line_bg, style_sign,
};

pub fn render_diff_preview(session: &Session, frame: &mut Frame<'_>, area: Rect) {
    let Some(preview) = session.diff_preview_state() else {
        return;
    };

    let palette = DiffColorPalette::default();
    let diff_bundle = compute_diff_with_theme(
        &preview.before,
        &preview.after,
        DiffOptions {
            context_lines: 3,
            old_label: None,
            new_label: None,
            missing_newline_hint: false,
        },
    );
    let counts = count_diff_changes(&diff_bundle.hunks);

    let chunks = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(5),
        Constraint::Length(4),
    ])
    .split(area);

    render_file_header(
        frame,
        chunks[0],
        preview,
        &palette,
        counts.additions,
        counts.deletions,
    );
    render_diff_content(frame, chunks[1], preview, &diff_bundle);
    render_controls(frame, chunks[2], preview);
}

fn render_file_header(
    frame: &mut Frame<'_>,
    area: Rect,
    preview: &DiffPreviewState,
    palette: &DiffColorPalette,
    additions: usize,
    deletions: usize,
) {
    let header_style = Style::default().fg(ratatui_color_from_ansi(palette.header_fg));
    let header = Line::from(vec![
        Span::styled(header_action_label(preview.mode), header_style),
        Span::styled(&preview.file_path, header_style),
        Span::styled(" (", header_style),
        Span::styled(
            format!("+{}", additions),
            Style::default().fg(ratatui_color_from_ansi(palette.added_fg)),
        ),
        Span::styled(" ", header_style),
        Span::styled(
            format!("-{}", deletions),
            Style::default().fg(ratatui_color_from_ansi(palette.removed_fg)),
        ),
        Span::styled(")", header_style),
    ]);
    frame.render_widget(Paragraph::new(header), area);
}

fn render_diff_content(
    frame: &mut Frame<'_>,
    area: Rect,
    preview: &DiffPreviewState,
    diff_bundle: &DiffBundle,
) {
    let language = language_hint_from_path(&preview.file_path);
    let style_context = current_diff_render_style_context();

    let mut lines: Vec<Line> = Vec::new();
    let max_display = area.height.saturating_sub(1) as usize;
    let display_lines = display_lines_from_hunks(&diff_bundle.hunks);

    for display_line in display_lines {
        if lines.len() >= max_display {
            break;
        }

        match display_line.kind {
            DiffDisplayKind::HunkHeader => {
                lines.push(Line::from(Span::styled(
                    display_line.text,
                    Style::default().fg(Color::Cyan),
                )));
            }
            DiffDisplayKind::Metadata => {
                lines.push(Line::from(Span::styled(
                    display_line.text,
                    Style::default().fg(Color::DarkGray),
                )));
            }
            DiffDisplayKind::Context | DiffDisplayKind::Addition | DiffDisplayKind::Deletion => {
                let line_num_str = format!("{:>4} ", display_line.line_number.unwrap_or(0));
                let line_type = match display_line.kind {
                    DiffDisplayKind::Context => DiffLineType::Context,
                    DiffDisplayKind::Addition => DiffLineType::Insert,
                    DiffDisplayKind::Deletion => DiffLineType::Delete,
                    DiffDisplayKind::Metadata | DiffDisplayKind::HunkHeader => unreachable!(),
                };

                let gutter_style = style_gutter(line_type);
                let sign_style = style_sign(line_type);
                let line_bg = style_line_bg(line_type, style_context);
                let content_style = style_content(line_type, style_context);

                let prefix = match line_type {
                    DiffLineType::Insert => "+",
                    DiffLineType::Delete => "-",
                    DiffLineType::Context => " ",
                };

                let mut spans = vec![
                    Span::styled(prefix.to_string(), sign_style),
                    Span::styled(line_num_str, gutter_style),
                ];

                for segment in render_diff_content_segments(
                    &display_line.text,
                    language.as_deref(),
                    anstyle::Style::new(),
                ) {
                    let style = content_style.patch(ratatui_style_from_ansi(segment.style));
                    spans.push(Span::styled(segment.text, style));
                }

                lines.push(Line::from(spans).style(line_bg));
            }
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no changes)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::NONE)),
        area,
    );
}

fn header_action_label(mode: DiffPreviewMode) -> &'static str {
    match mode {
        DiffPreviewMode::EditApproval => "← Edit ",
        DiffPreviewMode::FileConflict => "← Conflict ",
        DiffPreviewMode::ReadonlyReview => "← Review ",
    }
}

fn render_controls(frame: &mut Frame<'_>, area: Rect, preview: &DiffPreviewState) {
    let lines = control_lines(preview);

    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        area,
    );
}

fn control_lines(preview: &DiffPreviewState) -> Vec<Line<'static>> {
    match preview.mode {
        DiffPreviewMode::EditApproval => {
            let trust = match preview.trust_mode {
                TrustMode::Once => "Once",
                TrustMode::Session => "Session",
                TrustMode::Always => "Always",
                TrustMode::AutoTrust => "Auto",
            };

            vec![
                Line::from(vec![
                    Span::styled(
                        "Enter",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" Apply  "),
                    Span::styled(
                        "Esc",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" Reject  "),
                    Span::styled("Tab", Style::default().fg(Color::Yellow)),
                    Span::raw("/"),
                    Span::styled("S-Tab", Style::default().fg(Color::Yellow)),
                    Span::raw(" Nav"),
                ]),
                Line::from(vec![
                    Span::styled("1", Style::default().fg(Color::Cyan)),
                    Span::raw("-Once "),
                    Span::styled("2", Style::default().fg(Color::Cyan)),
                    Span::raw("-Sess "),
                    Span::styled("3", Style::default().fg(Color::Cyan)),
                    Span::raw("-Always "),
                    Span::styled("4", Style::default().fg(Color::Cyan)),
                    Span::raw("-Auto "),
                    Span::styled(
                        format!("[{}]", trust),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
            ]
        }
        DiffPreviewMode::FileConflict => vec![
            Line::from(vec![
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Proceed  "),
                Span::styled("r", Style::default().fg(Color::Cyan)),
                Span::raw(" Reload  "),
                Span::styled(
                    "Esc",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Abort"),
            ]),
            Line::from(vec![
                Span::styled("Tab", Style::default().fg(Color::Yellow)),
                Span::raw("/"),
                Span::styled("S-Tab", Style::default().fg(Color::Yellow)),
                Span::raw(" Nav"),
            ]),
        ],
        DiffPreviewMode::ReadonlyReview => vec![
            Line::from(vec![
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Back  "),
                Span::styled(
                    "Esc",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Back"),
            ]),
            Line::from(vec![
                Span::styled("Tab", Style::default().fg(Color::Yellow)),
                Span::raw("/"),
                Span::styled("S-Tab", Style::default().fg(Color::Yellow)),
                Span::raw(" Nav"),
            ]),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::{control_lines, header_action_label};
    use crate::core_tui::app::types::{DiffPreviewMode, DiffPreviewState};

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
        let first_line: String = lines[0]
            .spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect();

        assert!(first_line.contains("Proceed"));
        assert!(first_line.contains("Reload"));
        assert!(first_line.contains("Abort"));
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
        let first_line: String = lines[0]
            .spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect();

        assert!(first_line.contains("Back"));
        assert!(!first_line.contains("Proceed"));
        assert!(!first_line.contains("Reload"));
    }

    #[test]
    fn conflict_header_uses_conflict_label() {
        assert_eq!(
            header_action_label(DiffPreviewMode::FileConflict),
            "← Conflict "
        );
        assert_eq!(
            header_action_label(DiffPreviewMode::EditApproval),
            "← Edit "
        );
        assert_eq!(
            header_action_label(DiffPreviewMode::ReadonlyReview),
            "← Review "
        );
    }
}
