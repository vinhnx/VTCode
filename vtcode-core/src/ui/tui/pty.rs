use anstyle::{Ansi256Color, AnsiColor, Color as AnsiColorEnum, RgbColor};
use anyhow::{Context, Result};
use ratatui::{
    Terminal,
    backend::TestBackend,
    layout::Position,
    style::{Color as RatColor, Modifier, Style as RatStyle},
};
use tui_term::vt100::{Parser, Screen as PtyScreen};
use tui_term::widget::{Cursor as PtyCursor, PseudoTerminal};

use super::types::{InlineSegment, InlineTextStyle};
use std::borrow::Cow;

const MAX_PTY_RENDER_ROWS: u16 = 200;
const MAX_PTY_RENDER_COLS: u16 = 500;
const DEFAULT_PTY_RENDER_COLS: u16 = 80;

fn normalized_dimensions(
    rows: u16,
    cols: u16,
    fallback_rows: usize,
    fallback_cols: usize,
) -> (u16, u16) {
    let height = if rows > 0 {
        rows.min(MAX_PTY_RENDER_ROWS)
    } else {
        fallback_rows.max(1).min(MAX_PTY_RENDER_ROWS as usize) as u16
    };

    let width = if cols > 0 {
        cols.min(MAX_PTY_RENDER_COLS)
    } else {
        let inferred = fallback_cols
            .max(DEFAULT_PTY_RENDER_COLS as usize)
            .min(MAX_PTY_RENDER_COLS as usize);
        inferred.max(1) as u16
    };

    (height, width)
}

/// Snapshot of a VT100 screen rendered into inline UI segments.
pub struct PtySnapshotRender {
    pub screen: PtyScreen,
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

    let stream = normalize_newlines(contents);
    let inferred_rows = stream.lines().count().max(1);
    let inferred_cols = infer_snapshot_width(stream.as_ref());
    let (height, width) = normalized_dimensions(rows, cols, inferred_rows, inferred_cols);

    let mut parser = Parser::new(height, width, 0);
    parser.process(stream.as_bytes());
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

fn infer_snapshot_width(contents: &str) -> usize {
    contents
        .split('\n')
        .map(visible_line_width)
        .max()
        .unwrap_or(0)
}

fn visible_line_width(line: &str) -> usize {
    #[derive(Copy, Clone)]
    enum EscapeState {
        None,
        Csi,
        Osc,
        StTerminated,
    }

    let mut state = EscapeState::None;
    let mut chars = line.chars().peekable();
    let mut width = 0usize;
    let mut max_width = 0usize;

    while let Some(ch) = chars.next() {
        match state {
            EscapeState::Csi => {
                if ('@'..='~').contains(&ch) {
                    state = EscapeState::None;
                }
                continue;
            }
            EscapeState::Osc => {
                if ch == '\u{7}' {
                    state = EscapeState::None;
                    continue;
                }
                if ch == '\x1b' {
                    if let Some('\\') = chars.peek() {
                        chars.next();
                        state = EscapeState::None;
                    }
                }
                continue;
            }
            EscapeState::StTerminated => {
                if ch == '\x1b' {
                    if let Some('\\') = chars.peek() {
                        chars.next();
                        state = EscapeState::None;
                    }
                }
                continue;
            }
            EscapeState::None => {}
        }

        match ch {
            '\x1b' => match chars.next() {
                Some('[') => state = EscapeState::Csi,
                Some(']') => state = EscapeState::Osc,
                Some('P' | 'X' | '^' | '_') => state = EscapeState::StTerminated,
                Some(_) => {}
                None => {}
            },
            '\r' => {
                width = 0;
            }
            _ if ch.is_control() => {}
            _ => {
                width += 1;
                if width > max_width {
                    max_width = width;
                }
            }
        }
    }

    max_width.max(width)
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

fn normalize_newlines(contents: &str) -> Cow<'_, str> {
    let mut prev_was_cr = false;
    let mut needs_normalization = false;
    for ch in contents.chars() {
        match ch {
            '\r' => prev_was_cr = true,
            '\n' => {
                if !prev_was_cr {
                    needs_normalization = true;
                    break;
                }
                prev_was_cr = false;
            }
            _ => prev_was_cr = false,
        }
    }

    if !needs_normalization {
        return Cow::Borrowed(contents);
    }

    let mut normalized = String::with_capacity(contents.len() + contents.matches('\n').count());
    let mut pending_cr = false;
    for ch in contents.chars() {
        match ch {
            '\r' => {
                normalized.push('\r');
                pending_cr = true;
            }
            '\n' => {
                if !pending_cr {
                    normalized.push('\r');
                }
                normalized.push('\n');
                pending_cr = false;
            }
            _ => {
                normalized.push(ch);
                pending_cr = false;
            }
        }
    }

    Cow::Owned(normalized)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_newlines_before_rendering() {
        let render = render_pty_snapshot("hello\nworld\n", 24, 80).expect("snapshot should render");
        assert_eq!(render.lines.len(), 2);
        let line0: String = render.lines[0]
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();
        let line1: String = render.lines[1]
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();
        assert_eq!(line0, "hello");
        assert_eq!(line1, "world");
    }

    #[test]
    fn leaves_crlf_streams_untouched() {
        let render = render_pty_snapshot("foo\r\nbar\r\n", 24, 80).expect("snapshot should render");
        assert_eq!(render.lines.len(), 2);
        let line0: String = render.lines[0]
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();
        let line1: String = render.lines[1]
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();
        assert_eq!(line0, "foo");
        assert_eq!(line1, "bar");
    }
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
