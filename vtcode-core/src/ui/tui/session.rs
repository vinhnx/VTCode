use std::cmp::min;

use anstyle::{AnsiColor, Color as AnsiColorEnum, RgbColor};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};
use termion::event::{Event as TermionEvent, Key};
use tokio::sync::mpsc::UnboundedSender;
use unicode_width::UnicodeWidthStr;

use super::types::{
    InlineCommand, InlineEvent, InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme,
};

const USER_PREFIX: &str = "❯ ";
const PLACEHOLDER_COLOR: RgbColor = RgbColor(0xD3, 0xD3, 0xD3);

#[derive(Clone)]
struct MessageLine {
    kind: InlineMessageKind,
    segments: Vec<InlineSegment>,
}

#[derive(Clone, Default)]
struct MessageLabels {
    agent: Option<String>,
    user: Option<String>,
}

fn ratatui_color_from_ansi(color: AnsiColorEnum) -> Color {
    match color {
        AnsiColorEnum::Ansi(base) => match base {
            AnsiColor::Black => Color::Black,
            AnsiColor::Red => Color::Red,
            AnsiColor::Green => Color::Green,
            AnsiColor::Yellow => Color::Yellow,
            AnsiColor::Blue => Color::Blue,
            AnsiColor::Magenta => Color::Magenta,
            AnsiColor::Cyan => Color::Cyan,
            AnsiColor::White => Color::White,
            AnsiColor::BrightBlack => Color::DarkGray,
            AnsiColor::BrightRed => Color::LightRed,
            AnsiColor::BrightGreen => Color::LightGreen,
            AnsiColor::BrightYellow => Color::LightYellow,
            AnsiColor::BrightBlue => Color::LightBlue,
            AnsiColor::BrightMagenta => Color::LightMagenta,
            AnsiColor::BrightCyan => Color::LightCyan,
            AnsiColor::BrightWhite => Color::Gray,
        },
        AnsiColorEnum::Ansi256(value) => Color::Indexed(value.index()),
        AnsiColorEnum::Rgb(RgbColor(red, green, blue)) => Color::Rgb(red, green, blue),
    }
}

fn ratatui_style_from_inline(style: &InlineTextStyle, fallback: Option<AnsiColorEnum>) -> Style {
    let mut resolved = Style::default();
    if let Some(color) = style.color.or(fallback) {
        resolved = resolved.fg(ratatui_color_from_ansi(color));
    }
    if style.bold {
        resolved = resolved.add_modifier(Modifier::BOLD);
    }
    if style.italic {
        resolved = resolved.add_modifier(Modifier::ITALIC);
    }
    resolved
}

pub struct Session {
    lines: Vec<MessageLine>,
    theme: InlineTheme,
    labels: MessageLabels,
    prompt_prefix: String,
    prompt_style: InlineTextStyle,
    placeholder: Option<String>,
    placeholder_style: Option<InlineTextStyle>,
    input: String,
    cursor: usize,
    input_enabled: bool,
    cursor_visible: bool,
    needs_redraw: bool,
    should_exit: bool,
    view_rows: u16,
    scroll_offset: usize,
    transcript_rows: u16,
    transcript_width: u16,
    cached_max_scroll_offset: usize,
    scroll_metrics_dirty: bool,
}

impl Session {
    pub fn new(theme: InlineTheme, placeholder: Option<String>, view_rows: u16) -> Self {
        let resolved_rows = view_rows.max(2);
        Self {
            lines: Vec::new(),
            theme,
            labels: MessageLabels::default(),
            prompt_prefix: USER_PREFIX.to_string(),
            prompt_style: InlineTextStyle::default(),
            placeholder,
            placeholder_style: None,
            input: String::new(),
            cursor: 0,
            input_enabled: true,
            cursor_visible: true,
            needs_redraw: true,
            should_exit: false,
            view_rows: resolved_rows,
            scroll_offset: 0,
            transcript_rows: resolved_rows.saturating_sub(1).max(1),
            transcript_width: 0,
            cached_max_scroll_offset: 0,
            scroll_metrics_dirty: true,
        }
    }

    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    pub fn request_exit(&mut self) {
        self.should_exit = true;
    }

    pub fn take_redraw(&mut self) -> bool {
        if self.needs_redraw {
            self.needs_redraw = false;
            true
        } else {
            false
        }
    }

    pub fn handle_command(&mut self, command: InlineCommand) {
        match command {
            InlineCommand::AppendLine { kind, segments } => {
                self.push_line(kind, segments);
            }
            InlineCommand::Inline { kind, segment } => {
                self.append_inline(kind, segment);
            }
            InlineCommand::ReplaceLast { count, kind, lines } => {
                self.replace_last(count, kind, lines);
            }
            InlineCommand::SetPrompt { prefix, style } => {
                self.prompt_prefix = prefix;
                self.prompt_style = style;
            }
            InlineCommand::SetPlaceholder { hint, style } => {
                self.placeholder = hint;
                self.placeholder_style = style;
            }
            InlineCommand::SetMessageLabels { agent, user } => {
                self.labels.agent = agent.filter(|label| !label.is_empty());
                self.labels.user = user.filter(|label| !label.is_empty());
            }
            InlineCommand::SetTheme { theme } => {
                self.theme = theme;
            }
            InlineCommand::SetCursorVisible(value) => {
                self.cursor_visible = value;
            }
            InlineCommand::SetInputEnabled(value) => {
                self.input_enabled = value;
            }
            InlineCommand::ClearInput => {
                self.clear_input();
            }
            InlineCommand::ForceRedraw => {
                self.mark_dirty();
            }
            InlineCommand::Shutdown => {
                self.request_exit();
            }
        }
        self.mark_dirty();
    }

    pub fn handle_event(&mut self, event: TermionEvent, events: &UnboundedSender<InlineEvent>) {
        if let TermionEvent::Key(key) = event {
            if let Some(outbound) = self.process_key(key) {
                let _ = events.send(outbound);
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();
        if area.height == 0 {
            return;
        }

        self.apply_view_rows(area.height);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        let transcript_area = chunks[0];
        let input_area = chunks[1];

        self.render_transcript(frame, transcript_area);
        self.render_input(frame, input_area);
    }

    fn apply_view_rows(&mut self, rows: u16) {
        let resolved = rows.max(2);
        if self.view_rows != resolved {
            self.view_rows = resolved;
            self.transcript_rows = resolved.saturating_sub(1).max(1);
            self.invalidate_scroll_metrics();
            self.enforce_scroll_bounds();
        }
    }

    #[cfg(test)]
    fn force_view_rows(&mut self, rows: u16) {
        self.apply_view_rows(rows);
    }

    fn apply_transcript_rows(&mut self, rows: u16) {
        let resolved = rows.max(1);
        if self.transcript_rows != resolved {
            self.transcript_rows = resolved;
            self.invalidate_scroll_metrics();
        }
    }

    fn apply_transcript_width(&mut self, width: u16) {
        if self.transcript_width != width {
            self.transcript_width = width;
            self.invalidate_scroll_metrics();
        }
    }

    fn render_transcript(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        if area.height == 0 || area.width == 0 {
            return;
        }

        self.apply_transcript_rows(area.height);
        self.apply_transcript_width(area.width);

        let lines = self.transcript_lines();
        let mut paragraph = Paragraph::new(lines)
            .style(self.default_style())
            .wrap(Wrap { trim: false });

        let total_rows = paragraph.line_count(area.width);
        let viewport_rows = area.height as usize;
        let max_offset = total_rows.saturating_sub(viewport_rows);
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
        self.cached_max_scroll_offset = max_offset;
        self.scroll_metrics_dirty = false;

        let top_offset = max_offset.saturating_sub(self.scroll_offset);
        let vertical_offset = top_offset.min(u16::MAX as usize) as u16;
        paragraph = paragraph.scroll((vertical_offset, 0));

        frame.render_widget(paragraph, area);
    }

    fn render_input(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        if area.height == 0 {
            return;
        }

        let paragraph = Paragraph::new(self.render_input_line())
            .style(self.default_style())
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);

        if self.cursor_should_be_visible() {
            let (x, y) = self.cursor_position(area);
            frame.set_cursor_position((x, y));
        }
    }

    fn transcript_lines(&self) -> Vec<Line<'static>> {
        if self.lines.is_empty() {
            return vec![Line::default()];
        }

        self.lines
            .iter()
            .map(|line| Line::from(self.render_message_spans(line)))
            .collect()
    }

    fn render_input_line(&self) -> Line<'static> {
        let mut spans = Vec::new();
        let prompt_style = ratatui_style_from_inline(&self.prompt_style, self.theme.foreground);
        spans.push(Span::styled(self.prompt_prefix.clone(), prompt_style));

        if self.input.is_empty() {
            if let Some(placeholder) = &self.placeholder {
                let placeholder_style =
                    self.placeholder_style
                        .clone()
                        .unwrap_or_else(|| InlineTextStyle {
                            color: Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                            ..InlineTextStyle::default()
                        });
                let style = ratatui_style_from_inline(
                    &placeholder_style,
                    Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                );
                spans.push(Span::styled(placeholder.clone(), style));
            }
        } else {
            let style =
                ratatui_style_from_inline(&InlineTextStyle::default(), self.theme.foreground);
            spans.push(Span::styled(self.input.clone(), style));
        }

        Line::from(spans)
    }

    fn render_message_spans(&self, line: &MessageLine) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        if let Some(prefix) = self.prefix_text(line.kind) {
            let style = self.prefix_style(line);
            spans.push(Span::styled(
                prefix,
                ratatui_style_from_inline(&style, self.theme.foreground),
            ));
        }

        if line.segments.is_empty() {
            if spans.is_empty() {
                spans.push(Span::raw(String::new()));
            }
            return spans;
        }

        let fallback = self.text_fallback(line.kind).or(self.theme.foreground);
        for segment in &line.segments {
            let style = ratatui_style_from_inline(&segment.style, fallback);
            spans.push(Span::styled(segment.text.clone(), style));
        }

        if spans.is_empty() {
            spans.push(Span::raw(String::new()));
        }

        spans
    }

    fn default_style(&self) -> Style {
        let mut style = Style::default();
        if let Some(background) = self.theme.background.map(ratatui_color_from_ansi) {
            style = style.bg(background);
        }
        if let Some(foreground) = self.theme.foreground.map(ratatui_color_from_ansi) {
            style = style.fg(foreground);
        }
        style
    }

    fn cursor_position(&self, area: Rect) -> (u16, u16) {
        let prompt_width = UnicodeWidthStr::width(self.prompt_prefix.as_str()) as u16;
        let before_cursor = &self.input[..self.cursor];
        let cursor_width = UnicodeWidthStr::width(before_cursor) as u16;
        (area.x + prompt_width + cursor_width, area.y)
    }

    fn cursor_should_be_visible(&self) -> bool {
        self.cursor_visible && self.input_enabled
    }

    pub fn mark_dirty(&mut self) {
        self.needs_redraw = true;
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.mark_dirty();
    }

    fn process_key(&mut self, key: Key) -> Option<InlineEvent> {
        match key {
            Key::Ctrl('c') => {
                self.mark_dirty();
                Some(InlineEvent::Interrupt)
            }
            Key::Ctrl('d') => {
                self.mark_dirty();
                Some(InlineEvent::Exit)
            }
            Key::Esc => {
                self.mark_dirty();
                Some(InlineEvent::Cancel)
            }
            Key::PageUp => {
                self.scroll_page_up();
                self.mark_dirty();
                Some(InlineEvent::ScrollPageUp)
            }
            Key::PageDown => {
                self.scroll_page_down();
                self.mark_dirty();
                Some(InlineEvent::ScrollPageDown)
            }
            Key::Up => {
                self.scroll_line_up();
                self.mark_dirty();
                Some(InlineEvent::ScrollLineUp)
            }
            Key::Down => {
                self.scroll_line_down();
                self.mark_dirty();
                Some(InlineEvent::ScrollLineDown)
            }
            Key::Char('\n') | Key::Char('\r') => {
                if self.input_enabled {
                    let submitted = std::mem::take(&mut self.input);
                    self.cursor = 0;
                    self.mark_dirty();
                    Some(InlineEvent::Submit(submitted))
                } else {
                    None
                }
            }
            Key::Char(ch) => {
                if self.input_enabled {
                    self.insert_char(ch);
                    self.mark_dirty();
                }
                None
            }
            Key::Backspace => {
                if self.input_enabled {
                    self.delete_char();
                    self.mark_dirty();
                }
                None
            }
            Key::Left => {
                if self.input_enabled {
                    self.move_left();
                    self.mark_dirty();
                }
                None
            }
            Key::Right => {
                if self.input_enabled {
                    self.move_right();
                    self.mark_dirty();
                }
                None
            }
            Key::Home => {
                if self.input_enabled {
                    self.cursor = 0;
                    self.mark_dirty();
                }
                None
            }
            Key::End => {
                if self.input_enabled {
                    self.cursor = self.input.len();
                    self.mark_dirty();
                }
                None
            }
            _ => None,
        }
    }

    fn insert_char(&mut self, ch: char) {
        if ch == '\u{7f}' {
            return;
        }
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    fn delete_char(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some((index, _)) = self
            .input
            .char_indices()
            .take_while(|(idx, _)| *idx < self.cursor)
            .last()
        {
            self.input.drain(index..self.cursor);
            self.cursor = index;
        }
    }

    fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some((index, _)) = self
            .input
            .char_indices()
            .take_while(|(idx, _)| *idx < self.cursor)
            .last()
        {
            self.cursor = index;
        }
    }

    fn move_right(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        let slice = &self.input[self.cursor..];
        if let Some((_, ch)) = slice.char_indices().next() {
            self.cursor += ch.len_utf8();
        } else {
            self.cursor = self.input.len();
        }
    }

    fn prefix_text(&self, kind: InlineMessageKind) -> Option<String> {
        match kind {
            InlineMessageKind::User => Some(
                self.labels
                    .user
                    .clone()
                    .unwrap_or_else(|| USER_PREFIX.to_string()),
            ),
            InlineMessageKind::Agent | InlineMessageKind::Policy => self.labels.agent.clone(),
            InlineMessageKind::Tool | InlineMessageKind::Pty | InlineMessageKind::Error => None,
            InlineMessageKind::Info => None,
        }
    }

    fn prefix_style(&self, line: &MessageLine) -> InlineTextStyle {
        let fallback = self.text_fallback(line.kind).or(self.theme.foreground);

        let color = line
            .segments
            .iter()
            .find_map(|segment| segment.style.color)
            .or(fallback);

        InlineTextStyle {
            color,
            ..InlineTextStyle::default()
        }
    }

    fn text_fallback(&self, kind: InlineMessageKind) -> Option<AnsiColorEnum> {
        match kind {
            InlineMessageKind::Agent | InlineMessageKind::Policy => {
                self.theme.primary.or(self.theme.foreground)
            }
            InlineMessageKind::User => self.theme.secondary.or(self.theme.foreground),
            InlineMessageKind::Tool | InlineMessageKind::Pty | InlineMessageKind::Error => {
                self.theme.primary.or(self.theme.foreground)
            }
            InlineMessageKind::Info => self.theme.foreground,
        }
    }

    fn push_line(&mut self, kind: InlineMessageKind, segments: Vec<InlineSegment>) {
        if self.scroll_offset > 0 {
            self.scroll_offset = min(self.scroll_offset + 1, self.lines.len() + 1);
        }
        self.lines.push(MessageLine { kind, segments });
        self.invalidate_scroll_metrics();
        self.enforce_scroll_bounds();
    }

    fn append_inline(&mut self, kind: InlineMessageKind, segment: InlineSegment) {
        let mut remaining = segment.text.as_str();
        let style = segment.style.clone();

        while !remaining.is_empty() {
            if let Some((index, control)) = remaining
                .char_indices()
                .find(|(_, ch)| matches!(ch, '\n' | '\r'))
            {
                let (text, _) = remaining.split_at(index);
                if !text.is_empty() {
                    self.append_text(kind, text, &style);
                }

                let control_char = control;
                let next_index = index + control_char.len_utf8();
                remaining = &remaining[next_index..];

                match control_char {
                    '\n' => self.start_line(kind),
                    '\r' => {
                        if remaining.starts_with('\n') {
                            remaining = &remaining[1..];
                            self.start_line(kind);
                        } else {
                            self.reset_line(kind);
                        }
                    }
                    _ => {}
                }
            } else {
                if !remaining.is_empty() {
                    self.append_text(kind, remaining, &style);
                }
                break;
            }
        }

        self.invalidate_scroll_metrics();
        self.enforce_scroll_bounds();
    }

    fn replace_last(
        &mut self,
        count: usize,
        kind: InlineMessageKind,
        lines: Vec<Vec<InlineSegment>>,
    ) {
        let remove_count = min(count, self.lines.len());
        for _ in 0..remove_count {
            self.lines.pop();
        }
        for segments in lines {
            self.lines.push(MessageLine { kind, segments });
        }
        self.invalidate_scroll_metrics();
        self.enforce_scroll_bounds();
    }

    fn append_text(&mut self, kind: InlineMessageKind, text: &str, style: &InlineTextStyle) {
        if text.is_empty() {
            return;
        }

        let mut appended = false;

        if let Some(line) = self.lines.last_mut() {
            if line.kind == kind {
                if let Some(last) = line.segments.last_mut() {
                    if last.style == *style {
                        last.text.push_str(text);
                        appended = true;
                    }
                }
                if !appended {
                    line.segments.push(InlineSegment {
                        text: text.to_string(),
                        style: style.clone(),
                    });
                    appended = true;
                }
            }
        }

        if !appended {
            self.lines.push(MessageLine {
                kind,
                segments: vec![InlineSegment {
                    text: text.to_string(),
                    style: style.clone(),
                }],
            });
        }

        self.invalidate_scroll_metrics();
    }

    fn start_line(&mut self, kind: InlineMessageKind) {
        self.push_line(kind, Vec::new());
    }

    fn reset_line(&mut self, kind: InlineMessageKind) {
        if let Some(line) = self.lines.last_mut() {
            if line.kind == kind {
                line.segments.clear();
                self.invalidate_scroll_metrics();
                return;
            }
        }
        self.start_line(kind);
    }

    fn scroll_line_up(&mut self) {
        let max_offset = self.max_scroll_offset();
        if max_offset == 0 {
            self.scroll_offset = 0;
            return;
        }

        self.scroll_offset = min(self.scroll_offset + 1, max_offset);
    }

    fn scroll_line_down(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    fn scroll_page_up(&mut self) {
        let max_offset = self.max_scroll_offset();
        if max_offset == 0 {
            self.scroll_offset = 0;
            return;
        }

        let page = self.viewport_height().max(1);
        self.scroll_offset = min(self.scroll_offset + page, max_offset);
    }

    fn scroll_page_down(&mut self) {
        let page = self.viewport_height();
        if self.scroll_offset > page {
            self.scroll_offset -= page;
        } else {
            self.scroll_offset = 0;
        }
    }

    fn viewport_height(&self) -> usize {
        self.transcript_rows.max(1) as usize
    }

    fn max_scroll_offset(&self) -> usize {
        if self.scroll_metrics_dirty {
            let window = self.viewport_height().max(1);
            return self.lines.len().saturating_sub(window);
        }
        self.cached_max_scroll_offset
    }

    fn enforce_scroll_bounds(&mut self) {
        let max_offset = self.max_scroll_offset();
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
    }

    fn invalidate_scroll_metrics(&mut self) {
        self.scroll_metrics_dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    const VIEW_ROWS: u16 = 6;
    const VIEW_WIDTH: u16 = 40;
    const LINE_COUNT: usize = 10;
    const LABEL_PREFIX: &str = "line";
    const EXTRA_SEGMENT: &str = "\nextra-line";

    fn make_segment(text: &str) -> InlineSegment {
        InlineSegment {
            text: text.to_string(),
            style: InlineTextStyle::default(),
        }
    }

    fn visible_transcript(session: &mut Session) -> Vec<String> {
        let backend = TestBackend::new(VIEW_WIDTH, VIEW_ROWS);
        let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
        terminal
            .draw(|frame| session.render(frame))
            .expect("failed to render test session");

        let buffer = terminal.backend().buffer();
        let transcript_rows = VIEW_ROWS.saturating_sub(1);

        (0..transcript_rows)
            .map(|row| {
                let mut line = String::new();
                for col in 0..VIEW_WIDTH {
                    line.push_str(buffer[(col, row)].symbol());
                }
                line.trim_end().to_string()
            })
            .collect()
    }

    #[test]
    fn streaming_new_lines_preserves_scrolled_view() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

        for index in 1..=LINE_COUNT {
            let label = format!("{LABEL_PREFIX}-{index}");
            session.push_line(InlineMessageKind::Agent, vec![make_segment(label.as_str())]);
        }

        session.scroll_page_up();
        let before = visible_transcript(&mut session);

        session.append_inline(InlineMessageKind::Agent, make_segment(EXTRA_SEGMENT));

        let after = visible_transcript(&mut session);
        assert_eq!(before, after);
    }

    #[test]
    fn page_up_reveals_prior_lines_until_buffer_start() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

        for index in 1..=LINE_COUNT {
            let label = format!("{LABEL_PREFIX}-{index}");
            session.push_line(InlineMessageKind::Agent, vec![make_segment(label.as_str())]);
        }

        let mut transcripts = Vec::new();
        loop {
            transcripts.push(visible_transcript(&mut session));
            let previous_offset = session.scroll_offset;
            session.scroll_page_up();
            if session.scroll_offset == previous_offset {
                break;
            }
        }

        assert!(transcripts.len() > 1);

        for window in transcripts.windows(2) {
            assert_ne!(window[0], window[1]);
        }

        let top_view = transcripts
            .last()
            .expect("a top-of-buffer page should exist after scrolling");
        let first_label = format!("{LABEL_PREFIX}-1");
        let last_label = format!("{LABEL_PREFIX}-{LINE_COUNT}");

        assert!(top_view.iter().any(|line| line.contains(&first_label)));
        assert!(top_view.iter().all(|line| !line.contains(&last_label)));
        assert_eq!(session.scroll_offset, session.max_scroll_offset());
    }

    #[test]
    fn resizing_viewport_clamps_scroll_offset() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

        for index in 1..=LINE_COUNT {
            let label = format!("{LABEL_PREFIX}-{index}");
            session.push_line(InlineMessageKind::Agent, vec![make_segment(label.as_str())]);
        }

        session.scroll_page_up();
        assert!(session.scroll_offset > 0);

        session.force_view_rows((LINE_COUNT as u16) + 2);

        assert_eq!(session.scroll_offset, 0);
        assert_eq!(session.max_scroll_offset(), 0);
    }
}
