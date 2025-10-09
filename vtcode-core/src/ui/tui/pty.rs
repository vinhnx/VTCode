use anstyle::{Ansi256Color, AnsiColor, Color as AnsiColorEnum, RgbColor};
use anyhow::{Context, Result};
use ratatui::{
    Terminal,
    backend::TestBackend,
    layout::Position,
    style::{Color as RatColor, Modifier, Style as RatStyle},
};
use tui_term::vt100::{Parser, Screen};
use tui_term::widget::{Cursor as PtyCursor, PseudoTerminal};

use super::types::{InlineSegment, InlineTextStyle};

const MAX_PTY_RENDER_ROWS: u16 = 200;

fn normalized_dimensions(rows: u16, cols: u16, fallback_rows: usize) -> (u16, u16) {
    let height = rows.max(1).min(MAX_PTY_RENDER_ROWS);
    let width = cols.max(1);
    if fallback_rows == 0 {
        (height, width)
    } else {
        let fallback_height = fallback_rows
            .max(1)
            .min(MAX_PTY_RENDER_ROWS as usize) as u16;
        (height.max(fallback_height), width)
    }
}

/// Snapshot of a VT100 screen rendered into inline UI segments.
pub struct PtySnapshotRender {
    pub screen: Screen,
    pub lines: Vec<Vec<InlineSegment>>,
}

/// Render a VT100 screen snapshot into inline UI segments.
///
/// The `contents` parameter should contain the escape sequence stream produced by the
/// pseudoterminal. The dimensions are best-effort hints; when the snapshot was captured without
/// explicit sizing information we fall back to the number of newline separated rows present in the
/// content, clamped to a reasonable limit to avoid allocating excessively large buffers.
pub fn render_pty_snapshot(contents: &str, rows: u16, cols: u16) -> Result<PtySnapshotRender> {
    if contents.trim().is_empty() {
        return Ok(PtySnapshotRender {
            screen: Parser::new(1, 1, 0).screen().clone(),
            lines: Vec::new(),
        });
    }

    let inferred_rows = contents.lines().count().max(1);
    let (height, width) = normalized_dimensions(rows, cols, inferred_rows);

    let mut parser = Parser::new(height, width, 0);
    parser.process(contents.as_bytes());
    let screen = parser.screen().clone();

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).context("failed to create PTY snapshot terminal")?;
    terminal
        .draw(|frame| {
            let cursor = PtyCursor::default().visibility(false);
            let widget = PseudoTerminal::new(&screen).cursor(cursor);
            frame.render_widget(widget, frame.area());
        })
        .context("failed to render PTY snapshot")?;

    let buffer = terminal.backend().buffer();
    let mut lines = Vec::with_capacity(height as usize);

    for row in 0..height as usize {
        let mut segments: Vec<InlineSegment> = Vec::new();
        let mut current_style: Option<InlineTextStyle> = None;
        let mut current_text = String::new();

        for col in 0..width as usize {
            let Some(cell) = buffer.cell(Position {
                x: col as u16,
                y: row as u16,
            }) else {
                continue;
            };
            let symbol = cell.symbol();
            let style = inline_style_from_ratatui(cell.style());

            match &current_style {
                Some(existing) if existing == &style => current_text.push_str(symbol),
                Some(existing) => {
                    segments.push(InlineSegment {
                        text: std::mem::take(&mut current_text),
                        style: existing.clone(),
                    });
                    current_style = Some(style);
                    current_text.push_str(symbol);
                }
                None => {
                    current_style = Some(style);
                    current_text.push_str(symbol);
                }
            }
        }

        if let Some(style) = current_style {
            segments.push(InlineSegment {
                text: current_text,
                style,
            });
        }

        trim_trailing_whitespace(&mut segments);
        lines.push(segments);
    }

    while lines.last().map_or(false, |segments| {
        segments
            .iter()
            .all(|segment| segment.text.trim().is_empty())
    }) {
        lines.pop();
    }

    Ok(PtySnapshotRender { screen, lines })
}

fn trim_trailing_whitespace(segments: &mut Vec<InlineSegment>) {
    while let Some(last) = segments.last_mut() {
        if last.text.trim_end().is_empty() {
            segments.pop();
            continue;
        }

        let trimmed = last.text.trim_end_matches(' ');
        if trimmed.len() == last.text.len() {
            break;
        }

        if trimmed.is_empty() {
            segments.pop();
        } else {
            last.text.truncate(trimmed.len());
        }
        break;
    }
}

fn inline_style_from_ratatui(style: RatStyle) -> InlineTextStyle {
    let mut resolved = InlineTextStyle::default();
    if let Some(color) = style.fg.and_then(ansi_from_ratatui_color) {
        resolved.color = Some(color);
    }

    if style.add_modifier.contains(Modifier::BOLD) {
        resolved.bold = true;
    }
    if style.add_modifier.contains(Modifier::ITALIC) {
        resolved.italic = true;
    }
    if style.sub_modifier.contains(Modifier::BOLD) {
        resolved.bold = false;
    }
    if style.sub_modifier.contains(Modifier::ITALIC) {
        resolved.italic = false;
    }

    resolved
}

fn ansi_from_ratatui_color(color: RatColor) -> Option<AnsiColorEnum> {
    match color {
        RatColor::Reset => None,
        RatColor::Black => Some(AnsiColorEnum::Ansi(AnsiColor::Black)),
        RatColor::Red => Some(AnsiColorEnum::Ansi(AnsiColor::Red)),
        RatColor::Green => Some(AnsiColorEnum::Ansi(AnsiColor::Green)),
        RatColor::Yellow => Some(AnsiColorEnum::Ansi(AnsiColor::Yellow)),
        RatColor::Blue => Some(AnsiColorEnum::Ansi(AnsiColor::Blue)),
        RatColor::Magenta => Some(AnsiColorEnum::Ansi(AnsiColor::Magenta)),
        RatColor::Cyan => Some(AnsiColorEnum::Ansi(AnsiColor::Cyan)),
        RatColor::Gray => Some(AnsiColorEnum::Ansi(AnsiColor::White)),
        RatColor::DarkGray => Some(AnsiColorEnum::Ansi(AnsiColor::BrightBlack)),
        RatColor::LightRed => Some(AnsiColorEnum::Ansi(AnsiColor::BrightRed)),
        RatColor::LightGreen => Some(AnsiColorEnum::Ansi(AnsiColor::BrightGreen)),
        RatColor::LightYellow => Some(AnsiColorEnum::Ansi(AnsiColor::BrightYellow)),
        RatColor::LightBlue => Some(AnsiColorEnum::Ansi(AnsiColor::BrightBlue)),
        RatColor::LightMagenta => Some(AnsiColorEnum::Ansi(AnsiColor::BrightMagenta)),
        RatColor::LightCyan => Some(AnsiColorEnum::Ansi(AnsiColor::BrightCyan)),
        RatColor::White => Some(AnsiColorEnum::Ansi(AnsiColor::BrightWhite)),
        RatColor::Rgb(r, g, b) => Some(AnsiColorEnum::Rgb(RgbColor(r, g, b))),
        RatColor::Indexed(value) => Some(AnsiColorEnum::Ansi256(Ansi256Color(value))),
    }
}
