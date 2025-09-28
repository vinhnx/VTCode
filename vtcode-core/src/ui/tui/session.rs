use std::cmp::min;
use std::io::{self, Write};

use anstyle::Color as AnsiColorEnum;
use termion::clear;
use termion::cursor;
use termion::event::{Event as TermionEvent, Key};
use tokio::sync::mpsc::UnboundedSender;
use unicode_width::UnicodeWidthStr;

use super::types::{
    InlineCommand, InlineEvent, InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme,
};

const USER_PREFIX: &str = "❯ ";

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
}

impl Session {
    pub fn new(theme: InlineTheme, placeholder: Option<String>, view_rows: u16) -> Self {
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
            view_rows: view_rows.max(2),
            scroll_offset: 0,
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

    pub fn render(&self, stdout: &mut impl Write) -> io::Result<()> {
        let total_rows = self.view_rows.max(2);
        let transcript_rows = total_rows.saturating_sub(1) as usize;
        let lines = self.visible_lines(transcript_rows);

        write!(stdout, "{}{}", cursor::Goto(1, 1), clear::All)?;

        let mut row: u16 = 1;
        for line in lines {
            write!(stdout, "{}{}", cursor::Goto(1, row), clear::CurrentLine)?;
            write!(stdout, "{}", line)?;
            row += 1;
        }

        while row <= total_rows.saturating_sub(1) {
            write!(stdout, "{}{}", cursor::Goto(1, row), clear::CurrentLine)?;
            row += 1;
        }

        self.render_prompt(stdout, total_rows)?;
        stdout.flush()
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

    fn render_prompt(&self, stdout: &mut impl Write, row: u16) -> io::Result<()> {
        write!(stdout, "{}{}", cursor::Goto(1, row), clear::CurrentLine)?;
        let prompt_style = self.prompt_style.to_ansi_style(self.theme.foreground);
        write!(stdout, "{}", prompt_style.render())?;
        write!(stdout, "{}", self.prompt_prefix)?;
        write!(stdout, "{}", prompt_style.render_reset())?;

        if self.input.is_empty() {
            if let Some(placeholder) = &self.placeholder {
                let style = self
                    .placeholder_style
                    .clone()
                    .unwrap_or_default()
                    .to_ansi_style(self.theme.secondary.or(self.theme.foreground));
                write!(stdout, "{}", style.render())?;
                write!(stdout, "{}", placeholder)?;
                write!(stdout, "{}", style.render_reset())?;
            }
        } else {
            write!(stdout, "{}", &self.input)?;
        }

        let prompt_width = UnicodeWidthStr::width(self.prompt_prefix.as_str()) as u16;
        let before_cursor = &self.input[..self.cursor];
        let cursor_width = UnicodeWidthStr::width(before_cursor) as u16;
        let cursor_col = prompt_width + cursor_width + 1;

        if self.cursor_visible && self.input_enabled {
            write!(stdout, "{}{}", cursor::Goto(cursor_col, row), cursor::Show)?;
        } else {
            write!(stdout, "{}{}", cursor::Goto(cursor_col, row), cursor::Hide)?;
        }

        Ok(())
    }

    fn visible_lines(&self, capacity: usize) -> Vec<String> {
        if self.lines.is_empty() {
            return vec![String::new()];
        }

        let total = self.lines.len();
        let end = total.saturating_sub(self.scroll_offset);
        let window = capacity.max(1);
        let start = end.saturating_sub(window);

        self.lines[start..end]
            .iter()
            .map(|line| self.render_line(line))
            .collect()
    }

    fn render_line(&self, line: &MessageLine) -> String {
        let mut rendered = String::new();
        if let Some(prefix) = self.prefix_text(line.kind) {
            let style = self.prefix_style(line);
            rendered.push_str(&self.render_segment(&style, self.theme.foreground, &prefix));
        }

        if line.segments.is_empty() {
            return rendered;
        }

        let fallback = self.text_fallback(line.kind);
        for segment in &line.segments {
            rendered.push_str(&self.render_segment(&segment.style, fallback, &segment.text));
        }
        rendered
    }

    fn render_segment(
        &self,
        style: &InlineTextStyle,
        fallback: Option<AnsiColorEnum>,
        text: &str,
    ) -> String {
        let resolved = fallback.or(self.theme.foreground);
        let style = style.to_ansi_style(resolved);
        format!("{}{}{}", style.render(), text, style.render_reset())
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
        self.enforce_scroll_bounds();
    }

    fn append_text(&mut self, kind: InlineMessageKind, text: &str, style: &InlineTextStyle) {
        if text.is_empty() {
            return;
        }

        if let Some(line) = self.lines.last_mut() {
            if line.kind == kind {
                if let Some(last) = line.segments.last_mut() {
                    if last.style == *style {
                        last.text.push_str(text);
                        return;
                    }
                }
                line.segments.push(InlineSegment {
                    text: text.to_string(),
                    style: style.clone(),
                });
                return;
            }
        }

        self.lines.push(MessageLine {
            kind,
            segments: vec![InlineSegment {
                text: text.to_string(),
                style: style.clone(),
            }],
        });
    }

    fn start_line(&mut self, kind: InlineMessageKind) {
        self.lines.push(MessageLine {
            kind,
            segments: Vec::new(),
        });
    }

    fn reset_line(&mut self, kind: InlineMessageKind) {
        if let Some(line) = self.lines.last_mut() {
            if line.kind == kind {
                line.segments.clear();
                return;
            }
        }
        self.start_line(kind);
    }

    fn scroll_line_up(&mut self) {
        if self.scroll_offset < self.lines.len() {
            self.scroll_offset += 1;
        }
    }

    fn scroll_line_down(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    fn scroll_page_up(&mut self) {
        let page = self.viewport_height();
        self.scroll_offset = min(self.scroll_offset + page, self.lines.len());
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
        self.view_rows.saturating_sub(1) as usize
    }

    fn enforce_scroll_bounds(&mut self) {
        if self.scroll_offset > self.lines.len() {
            self.scroll_offset = self.lines.len();
        }
    }
}
