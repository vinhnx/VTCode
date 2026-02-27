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

use super::super::style::{ratatui_color_from_ansi, ratatui_style_from_ansi};
use crate::config::constants::ui;
use crate::ui::markdown::highlight_line_for_diff;
use crate::ui::tui::session::Session;
use crate::ui::tui::types::{DiffPreviewState, TrustMode};
use crate::utils::diff::{DiffBundle, DiffLineKind, DiffOptions, compute_diff_with_theme};
use crate::utils::diff_styles::DiffColorPalette;

pub fn render_diff_preview(session: &Session, frame: &mut Frame<'_>, area: Rect) {
    let Some(preview) = session.diff_preview.as_ref() else {
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
    let (additions, deletions) = count_diff_changes(&diff_bundle);

    let chunks = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(5),
        Constraint::Length(4),
    ])
    .split(area);

    render_file_header(frame, chunks[0], preview, &palette, additions, deletions);
    render_diff_content(frame, chunks[1], preview, &palette, &diff_bundle);
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
        Span::styled("← Edit ", header_style),
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

fn detect_language(file_path: &str) -> Option<&'static str> {
    let ext = file_path.rsplit('.').next()?;
    match ext.to_lowercase().as_str() {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "js" => Some("javascript"),
        "ts" | "tsx" => Some("typescript"),
        "go" => Some("go"),
        "java" => Some("java"),
        "sh" | "bash" => Some("bash"),
        "swift" => Some("swift"),
        "c" | "h" => Some("c"),
        "cpp" | "cc" | "cxx" | "hpp" => Some("cpp"),
        "json" => Some("json"),
        "yaml" | "yml" => Some("yaml"),
        "toml" => Some("toml"),
        "md" => Some("markdown"),
        "html" | "htm" => Some("html"),
        "css" | "scss" => Some("css"),
        _ => None,
    }
}

fn highlight_line_with_bg(
    line: &str,
    language: Option<&str>,
    bg: Option<Color>,
) -> Vec<Span<'static>> {
    let text = line.trim_end_matches('\n');
    if let Some(segments) = highlight_line_for_diff(text, language) {
        segments
            .into_iter()
            .map(|(anstyle, t)| {
                let mut style = ratatui_style_from_ansi(anstyle);
                if let Some(bg_color) = bg {
                    style = style.bg(bg_color);
                }
                Span::styled(t, style)
            })
            .collect()
    } else {
        let mut style = Style::default();
        if let Some(bg_color) = bg {
            style = style.bg(bg_color);
        }
        vec![Span::styled(text.to_string(), style)]
    }
}

fn count_diff_changes(diff_bundle: &DiffBundle) -> (usize, usize) {
    let mut additions = 0usize;
    let mut deletions = 0usize;

    for hunk in &diff_bundle.hunks {
        for line in &hunk.lines {
            match line.kind {
                DiffLineKind::Addition => additions += 1,
                DiffLineKind::Deletion => deletions += 1,
                DiffLineKind::Context => {}
            }
        }
    }

    (additions, deletions)
}

fn render_diff_content(
    frame: &mut Frame<'_>,
    area: Rect,
    preview: &DiffPreviewState,
    palette: &DiffColorPalette,
    diff_bundle: &DiffBundle,
) {
    let language = detect_language(&preview.file_path);

    let mut lines: Vec<Line> = Vec::new();
    let max_display = area.height.saturating_sub(1) as usize;

    for hunk in &diff_bundle.hunks {
        if lines.len() >= max_display {
            break;
        }

        lines.push(Line::from(Span::styled(
            format!("@@ -{} +{} @@", hunk.old_start, hunk.new_start),
            Style::default().fg(Color::Cyan),
        )));

        for diff_line in &hunk.lines {
            if lines.len() >= max_display {
                break;
            }

            let line_num = match diff_line.kind {
                DiffLineKind::Context => diff_line.new_line.unwrap_or(0),
                DiffLineKind::Addition => diff_line.new_line.unwrap_or(0),
                DiffLineKind::Deletion => diff_line.old_line.unwrap_or(0),
            };
            let line_num_str = format!("{:>4} ", line_num);
            let text = diff_line.text.trim_end_matches('\n');

            let fg_line_num = Color::Rgb(
                ui::DIFF_LINE_NUMBER_R,
                ui::DIFF_LINE_NUMBER_G,
                ui::DIFF_LINE_NUMBER_B,
            );
            let (prefix, fg, is_change) = match diff_line.kind {
                DiffLineKind::Context => (" ", fg_line_num, false),
                DiffLineKind::Addition => ("+", ratatui_color_from_ansi(palette.added_fg), true),
                DiffLineKind::Deletion => ("-", ratatui_color_from_ansi(palette.removed_fg), true),
            };

            let mut spans = vec![
                Span::styled(
                    prefix,
                    Style::default().fg(fg).add_modifier(if is_change {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ),
                Span::styled(line_num_str, Style::default().fg(fg)),
            ];
            spans.extend(highlight_line_with_bg(text, language, None));
            lines.push(Line::from(spans));
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

fn render_controls(frame: &mut Frame<'_>, area: Rect, preview: &DiffPreviewState) {
    let trust = match preview.trust_mode {
        TrustMode::Once => "Once",
        TrustMode::Session => "Session",
        TrustMode::Always => "Always",
        TrustMode::AutoTrust => "Auto",
    };

    let lines = vec![
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
    ];

    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        area,
    );
}
