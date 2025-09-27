use std::cmp::min;
use std::io::{self, IsTerminal};

use anyhow::Result;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use termion::{
    event::{Event as TermionEvent, Key},
    input::TermRead,
    terminal_size,
};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use unicode_width::UnicodeWidthStr;

use crate::config::types::UiSurfacePreference;

const INLINE_FALLBACK_ROWS: u16 = 24;
const DEFAULT_PROMPT_PREFIX: &str = "❯ ";
const DEFAULT_AGENT_LABEL: &str = "VT Code";
const DEFAULT_USER_LABEL: &str = "You";
const DEFAULT_STATUS_LEFT: &str = "Esc cancel";
const DEFAULT_STATUS_RIGHT: &str = "Ctrl+C interrupt";
const STATUS_SEPARATOR: &str = "  ·  ";

#[derive(Clone, Default, PartialEq)]
pub struct RatatuiTextStyle {
    pub color: Option<Color>,
    pub bold: bool,
    pub italic: bool,
}

impl RatatuiTextStyle {
    pub fn merge_color(mut self, fallback: Option<Color>) -> Self {
        if self.color.is_none() {
            self.color = fallback;
        }
        self
    }

    pub(crate) fn to_style(&self, fallback: Option<Color>) -> Style {
        let mut style = Style::default();
        if let Some(color) = self.color.or(fallback) {
            style = style.fg(color);
        }
        if self.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        style
    }
}

#[derive(Clone, Default)]
pub struct RatatuiSegment {
    pub text: String,
    pub style: RatatuiTextStyle,
}

#[derive(Clone, Default)]
pub struct RatatuiTheme {
    pub background: Option<Color>,
    pub foreground: Option<Color>,
    pub primary: Option<Color>,
    pub secondary: Option<Color>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RatatuiMessageKind {
    Agent,
    Error,
    Info,
    Policy,
    Pty,
    Tool,
    User,
}

pub enum RatatuiCommand {
    AppendLine {
        kind: RatatuiMessageKind,
        segments: Vec<RatatuiSegment>,
    },
    Inline {
        kind: RatatuiMessageKind,
        segment: RatatuiSegment,
    },
    ReplaceLast {
        count: usize,
        kind: RatatuiMessageKind,
        lines: Vec<Vec<RatatuiSegment>>,
    },
    SetPrompt {
        prefix: String,
        style: RatatuiTextStyle,
    },
    SetPlaceholder {
        hint: Option<String>,
        style: Option<RatatuiTextStyle>,
    },
    SetMessageLabels {
        agent: Option<String>,
        user: Option<String>,
    },
    SetTheme {
        theme: RatatuiTheme,
    },
    UpdateStatusBar {
        left: Option<String>,
        center: Option<String>,
        right: Option<String>,
    },
    SetCursorVisible(bool),
    SetInputEnabled(bool),
    ClearInput,
    ForceRedraw,
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum RatatuiEvent {
    Submit(String),
    Cancel,
    Exit,
    Interrupt,
    ScrollLineUp,
    ScrollLineDown,
    ScrollPageUp,
    ScrollPageDown,
}

#[derive(Clone)]
pub struct RatatuiHandle {
    pub(crate) sender: UnboundedSender<RatatuiCommand>,
}

impl RatatuiHandle {
    pub fn append_line(&self, kind: RatatuiMessageKind, segments: Vec<RatatuiSegment>) {
        if segments.is_empty() {
            let _ = self.sender.send(RatatuiCommand::AppendLine {
                kind,
                segments: vec![RatatuiSegment::default()],
            });
        } else {
            let _ = self
                .sender
                .send(RatatuiCommand::AppendLine { kind, segments });
        }
    }

    pub fn inline(&self, kind: RatatuiMessageKind, segment: RatatuiSegment) {
        let _ = self.sender.send(RatatuiCommand::Inline { kind, segment });
    }

    pub fn replace_last(
        &self,
        count: usize,
        kind: RatatuiMessageKind,
        lines: Vec<Vec<RatatuiSegment>>,
    ) {
        let _ = self
            .sender
            .send(RatatuiCommand::ReplaceLast { count, kind, lines });
    }

    pub fn set_prompt(&self, prefix: String, style: RatatuiTextStyle) {
        let _ = self
            .sender
            .send(RatatuiCommand::SetPrompt { prefix, style });
    }

    pub fn set_placeholder(&self, hint: Option<String>) {
        self.set_placeholder_with_style(hint, None);
    }

    pub fn set_placeholder_with_style(
        &self,
        hint: Option<String>,
        style: Option<RatatuiTextStyle>,
    ) {
        let _ = self
            .sender
            .send(RatatuiCommand::SetPlaceholder { hint, style });
    }

    pub fn set_message_labels(&self, agent: Option<String>, user: Option<String>) {
        let _ = self
            .sender
            .send(RatatuiCommand::SetMessageLabels { agent, user });
    }

    pub fn set_theme(&self, theme: RatatuiTheme) {
        let _ = self.sender.send(RatatuiCommand::SetTheme { theme });
    }

    pub fn update_status_bar(
        &self,
        left: Option<String>,
        center: Option<String>,
        right: Option<String>,
    ) {
        let _ = self.sender.send(RatatuiCommand::UpdateStatusBar {
            left,
            center,
            right,
        });
    }

    pub fn set_cursor_visible(&self, visible: bool) {
        let _ = self.sender.send(RatatuiCommand::SetCursorVisible(visible));
    }

    pub fn set_input_enabled(&self, enabled: bool) {
        let _ = self.sender.send(RatatuiCommand::SetInputEnabled(enabled));
    }

    pub fn clear_input(&self) {
        let _ = self.sender.send(RatatuiCommand::ClearInput);
    }

    pub fn force_redraw(&self) {
        let _ = self.sender.send(RatatuiCommand::ForceRedraw);
    }

    pub fn shutdown(&self) {
        let _ = self.sender.send(RatatuiCommand::Shutdown);
    }
}

pub struct RatatuiSession {
    pub handle: RatatuiHandle,
    pub events: UnboundedReceiver<RatatuiEvent>,
}

pub(crate) type InputEventResult = std::result::Result<TermionEvent, io::Error>;

pub(crate) fn spawn_termion_event_reader() -> UnboundedReceiver<InputEventResult> {
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::task::spawn_blocking(move || {
        let stdin = io::stdin();
        for event in stdin.lock().events() {
            match event {
                Ok(evt) => {
                    if sender.send(Ok(evt)).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    if sender.send(Err(err)).is_err() {
                        break;
                    }
                }
            }
        }
    });
    receiver
}

pub(crate) enum TerminalSurface {
    Inline { rows: u16 },
}

impl TerminalSurface {
    pub(crate) fn detect(_preference: UiSurfacePreference) -> Result<Self> {
        if io::stdout().is_terminal() {
            match terminal_size() {
                Ok((_, rows)) => Ok(Self::Inline { rows }),
                Err(err) => {
                    tracing::debug!(error = %err, "failed to determine terminal size");
                    Ok(Self::Inline {
                        rows: INLINE_FALLBACK_ROWS,
                    })
                }
            }
        } else {
            Ok(Self::Inline {
                rows: INLINE_FALLBACK_ROWS,
            })
        }
    }

    pub(crate) fn rows(&self) -> u16 {
        match self {
            Self::Inline { rows } => *rows,
        }
    }
}

#[derive(Clone)]
struct MessageLine {
    kind: RatatuiMessageKind,
    segments: Vec<RatatuiSegment>,
}

#[derive(Default, Clone)]
struct StatusBarContent {
    left: String,
    center: String,
    right: String,
}

impl StatusBarContent {
    fn new() -> Self {
        Self {
            left: DEFAULT_STATUS_LEFT.to_string(),
            center: String::new(),
            right: DEFAULT_STATUS_RIGHT.to_string(),
        }
    }

    fn update(&mut self, left: Option<String>, center: Option<String>, right: Option<String>) {
        if let Some(value) = left {
            self.left = value;
        }
        if let Some(value) = center {
            self.center = value;
        }
        if let Some(value) = right {
            self.right = value;
        }
    }

    fn has_text(&self) -> bool {
        !(self.left.is_empty() && self.center.is_empty() && self.right.is_empty())
    }

    fn formatted(&self) -> String {
        let mut parts = Vec::new();
        if !self.left.is_empty() {
            parts.push(self.left.clone());
        }
        if !self.center.is_empty() {
            parts.push(self.center.clone());
        }
        if !self.right.is_empty() {
            parts.push(self.right.clone());
        }
        parts.join(STATUS_SEPARATOR)
    }
}

#[derive(Default)]
struct InputState {
    value: String,
    cursor: usize,
}

impl InputState {
    fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }

    fn insert_char(&mut self, ch: char) {
        let mut buf = [0u8; 4];
        let slice = ch.encode_utf8(&mut buf);
        self.value.insert_str(self.cursor, slice);
        self.cursor += slice.len();
    }

    fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.value.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev_index = self.value[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(idx, ch)| idx + ch.len_utf8())
            .unwrap_or(0);
        let start = self.value[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        self.value.drain(start..prev_index);
        self.cursor = start;
    }

    fn delete(&mut self) {
        if self.cursor >= self.value.len() {
            return;
        }
        let end = self.value[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(idx, _)| self.cursor + idx)
            .unwrap_or_else(|| self.value.len());
        self.value.drain(self.cursor..end);
    }

    fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let new_cursor = self.value[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        self.cursor = new_cursor;
    }

    fn move_right(&mut self) {
        if self.cursor >= self.value.len() {
            return;
        }
        let new_cursor = self.value[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(idx, _)| self.cursor + idx)
            .unwrap_or_else(|| self.value.len());
        self.cursor = new_cursor;
    }

    fn move_home(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.value.len();
    }

    fn current_prefix(&self) -> &str {
        &self.value[..self.cursor]
    }
}

pub(crate) struct RatatuiLoop {
    theme: RatatuiTheme,
    transcript: Vec<MessageLine>,
    prompt_prefix: String,
    prompt_style: RatatuiTextStyle,
    placeholder_hint: Option<String>,
    placeholder_style: RatatuiTextStyle,
    input: InputState,
    input_enabled: bool,
    cursor_visible: bool,
    should_exit: bool,
    status_bar: StatusBarContent,
    agent_label: String,
    user_label: String,
    scroll_offset: usize,
    viewport_height: usize,
}

impl RatatuiLoop {
    pub(crate) fn new(theme: RatatuiTheme, placeholder: Option<String>) -> Self {
        let placeholder_style = RatatuiTextStyle {
            italic: true,
            color: theme.foreground,
            ..Default::default()
        };
        Self {
            theme,
            transcript: Vec::new(),
            prompt_prefix: DEFAULT_PROMPT_PREFIX.to_string(),
            prompt_style: RatatuiTextStyle {
                bold: true,
                color: None,
                italic: false,
            },
            placeholder_hint: placeholder,
            placeholder_style,
            input: InputState::default(),
            input_enabled: true,
            cursor_visible: true,
            should_exit: false,
            status_bar: StatusBarContent::new(),
            agent_label: DEFAULT_AGENT_LABEL.to_string(),
            user_label: DEFAULT_USER_LABEL.to_string(),
            scroll_offset: 0,
            viewport_height: 1,
        }
    }

    pub(crate) fn should_exit(&self) -> bool {
        self.should_exit
    }

    pub(crate) fn set_should_exit(&mut self) {
        self.should_exit = true;
    }

    pub(crate) fn handle_command(&mut self, command: RatatuiCommand) -> bool {
        match command {
            RatatuiCommand::AppendLine { kind, segments } => {
                self.push_line(kind, segments);
                true
            }
            RatatuiCommand::Inline { kind, segment } => {
                self.append_inline(kind, segment);
                true
            }
            RatatuiCommand::ReplaceLast { count, kind, lines } => {
                self.replace_last(count, kind, lines);
                true
            }
            RatatuiCommand::SetPrompt { prefix, style } => {
                self.prompt_prefix = prefix;
                self.prompt_style = style;
                true
            }
            RatatuiCommand::SetPlaceholder { hint, style } => {
                self.placeholder_hint = hint;
                if let Some(new_style) = style {
                    self.placeholder_style = new_style;
                }
                true
            }
            RatatuiCommand::SetMessageLabels { agent, user } => {
                if let Some(agent_label) = agent {
                    self.agent_label = agent_label;
                }
                if let Some(user_label) = user {
                    self.user_label = user_label;
                }
                true
            }
            RatatuiCommand::SetTheme { theme } => {
                self.theme = theme;
                true
            }
            RatatuiCommand::UpdateStatusBar {
                left,
                center,
                right,
            } => {
                self.status_bar.update(left, center, right);
                true
            }
            RatatuiCommand::SetCursorVisible(value) => {
                self.cursor_visible = value;
                true
            }
            RatatuiCommand::SetInputEnabled(value) => {
                self.input_enabled = value;
                true
            }
            RatatuiCommand::ClearInput => {
                self.input.clear();
                true
            }
            RatatuiCommand::ForceRedraw => true,
            RatatuiCommand::Shutdown => {
                self.should_exit = true;
                true
            }
        }
    }

    pub(crate) fn handle_event(
        &mut self,
        event: TermionEvent,
        events: &UnboundedSender<RatatuiEvent>,
    ) -> Result<bool> {
        match event {
            TermionEvent::Key(key) => self.handle_key_event(key, events),
            TermionEvent::Mouse(_) => Ok(false),
            TermionEvent::Unsupported(bytes) => {
                if self.input_enabled {
                    if let Ok(text) = String::from_utf8(bytes) {
                        if !text.is_empty() {
                            self.input.insert_str(&text);
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            }
        }
    }

    pub(crate) fn draw(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let mut constraints = Vec::new();
        constraints.push(Constraint::Min(1));
        let status_active = self.status_bar.has_text();
        if status_active {
            constraints.push(Constraint::Length(1));
        }
        constraints.push(Constraint::Length(1));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        let (transcript_area, status_area, prompt_area) = if status_active {
            (chunks[0], Some(chunks[1]), chunks[2])
        } else {
            (chunks[0], None, chunks[1])
        };

        self.viewport_height = transcript_area.height.max(1) as usize;

        let transcript_lines = self.visible_transcript();
        let mut transcript_paragraph = Paragraph::new(transcript_lines).wrap(Wrap { trim: false });
        if let Some(bg) = self.theme.background {
            transcript_paragraph = transcript_paragraph.style(Style::default().bg(bg));
        }
        frame.render_widget(transcript_paragraph, transcript_area);

        if let Some(status_area) = status_area {
            let status_text = self.status_bar.formatted();
            let status_paragraph =
                Paragraph::new(Line::from(status_text)).wrap(Wrap { trim: false });
            frame.render_widget(status_paragraph, status_area);
        }

        let prompt_line = self.prompt_line();
        let mut prompt_block = Paragraph::new(prompt_line).wrap(Wrap { trim: false });
        if let Some(bg) = self.theme.background {
            prompt_block = prompt_block.style(Style::default().bg(bg));
        }
        frame.render_widget(
            prompt_block.block(Block::default().borders(Borders::NONE)),
            prompt_area,
        );

        if self.cursor_visible && self.input_enabled {
            let prefix_width = UnicodeWidthStr::width(self.prompt_prefix.as_str());
            let cursor_str = self.input.current_prefix();
            let cursor_width = UnicodeWidthStr::width(cursor_str);
            let x = prompt_area.x + prefix_width as u16 + cursor_width as u16;
            let y = prompt_area.y;
            frame.set_cursor_position((x, y));
        }
    }

    fn handle_key_event(
        &mut self,
        key: Key,
        events: &UnboundedSender<RatatuiEvent>,
    ) -> Result<bool> {
        match key {
            Key::Ctrl(ch) => {
                match ch.to_ascii_lowercase() {
                    'c' => {
                        let _ = events.send(RatatuiEvent::Interrupt);
                    }
                    'd' => {
                        let _ = events.send(RatatuiEvent::Exit);
                    }
                    'u' => {
                        if self.input_enabled {
                            self.input.clear();
                            return Ok(true);
                        }
                    }
                    _ => {}
                }
                Ok(false)
            }
            Key::Char('\n') | Key::Char('\r') => {
                if self.input_enabled {
                    let text = self.input.value.clone();
                    let _ = events.send(RatatuiEvent::Submit(text));
                }
                Ok(false)
            }
            Key::Char('\t') => {
                if self.input_enabled {
                    self.input.insert_char('\t');
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Key::Char(ch) => {
                if self.input_enabled {
                    self.input.insert_char(ch);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Key::Backspace => {
                if self.input_enabled {
                    self.input.backspace();
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Key::Delete => {
                if self.input_enabled {
                    self.input.delete();
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Key::Left => {
                if self.input_enabled {
                    self.input.move_left();
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Key::Right => {
                if self.input_enabled {
                    self.input.move_right();
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Key::Home => {
                if self.input_enabled {
                    self.input.move_home();
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Key::End => {
                if self.input_enabled {
                    self.input.move_end();
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Key::Up => {
                self.scroll_line_up();
                let _ = events.send(RatatuiEvent::ScrollLineUp);
                Ok(true)
            }
            Key::Down => {
                self.scroll_line_down();
                let _ = events.send(RatatuiEvent::ScrollLineDown);
                Ok(true)
            }
            Key::PageUp => {
                self.scroll_page_up();
                let _ = events.send(RatatuiEvent::ScrollPageUp);
                Ok(true)
            }
            Key::PageDown => {
                self.scroll_page_down();
                let _ = events.send(RatatuiEvent::ScrollPageDown);
                Ok(true)
            }
            Key::Esc => {
                let _ = events.send(RatatuiEvent::Cancel);
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn push_line(&mut self, kind: RatatuiMessageKind, segments: Vec<RatatuiSegment>) {
        if self.scroll_offset > 0 {
            self.scroll_offset = min(self.scroll_offset + 1, self.transcript.len() + 1);
        }
        self.transcript.push(MessageLine { kind, segments });
        self.trim_scroll_bounds();
    }

    fn append_inline(&mut self, kind: RatatuiMessageKind, segment: RatatuiSegment) {
        if let Some(last) = self.transcript.last_mut() {
            if last.kind == kind {
                last.segments.push(segment);
                return;
            }
        }
        self.transcript.push(MessageLine {
            kind,
            segments: vec![segment],
        });
        self.trim_scroll_bounds();
    }

    fn replace_last(
        &mut self,
        count: usize,
        kind: RatatuiMessageKind,
        lines: Vec<Vec<RatatuiSegment>>,
    ) {
        let remove_count = min(count, self.transcript.len());
        for _ in 0..remove_count {
            self.transcript.pop();
        }
        for segments in lines {
            self.transcript.push(MessageLine { kind, segments });
        }
        self.trim_scroll_bounds();
    }

    fn visible_transcript(&self) -> Vec<Line<'static>> {
        if self.transcript.is_empty() {
            return vec![Line::from("")];
        }
        let total = self.transcript.len();
        let end = total.saturating_sub(self.scroll_offset);
        let visible_height = self.viewport_height.max(1);
        let start = end.saturating_sub(visible_height);
        self.transcript[start..end]
            .iter()
            .map(|line| self.render_line(line))
            .collect()
    }

    fn render_line(&self, line: &MessageLine) -> Line<'static> {
        let fallback = self.fallback_color(line.kind);
        let mut spans: Vec<Span> = Vec::new();
        let label = self.line_label(line.kind);
        if let Some(label) = label {
            if !label.is_empty() {
                let style = RatatuiTextStyle {
                    bold: true,
                    color: fallback,
                    italic: false,
                }
                .to_style(self.theme.foreground);
                spans.push(Span::styled(format!("{label}: "), style));
            }
        }
        for segment in &line.segments {
            let style = segment.style.to_style(fallback.or(self.theme.foreground));
            spans.push(Span::styled(segment.text.clone(), style));
        }
        Line::from(spans)
    }

    fn fallback_color(&self, kind: RatatuiMessageKind) -> Option<Color> {
        match kind {
            RatatuiMessageKind::Agent | RatatuiMessageKind::Policy => {
                self.theme.primary.or(self.theme.foreground)
            }
            RatatuiMessageKind::User => self.theme.secondary.or(self.theme.foreground),
            _ => self.theme.foreground,
        }
    }

    fn line_label(&self, kind: RatatuiMessageKind) -> Option<String> {
        match kind {
            RatatuiMessageKind::Agent => Some(self.agent_label.clone()),
            RatatuiMessageKind::User => Some(self.user_label.clone()),
            RatatuiMessageKind::Tool => Some("Tool".to_string()),
            RatatuiMessageKind::Info => Some("Info".to_string()),
            RatatuiMessageKind::Error => Some("Error".to_string()),
            RatatuiMessageKind::Policy => Some("Policy".to_string()),
            RatatuiMessageKind::Pty => Some("Shell".to_string()),
        }
    }

    fn prompt_line(&self) -> Line<'static> {
        let mut spans = Vec::new();
        let prefix_style = self
            .prompt_style
            .clone()
            .merge_color(self.theme.primary.or(self.theme.foreground))
            .to_style(self.theme.foreground);
        spans.push(Span::styled(self.prompt_prefix.clone(), prefix_style));
        if !self.input.value.is_empty() {
            spans.push(Span::raw(self.input.value.clone()));
        } else if let Some(hint) = &self.placeholder_hint {
            let style = self
                .placeholder_style
                .clone()
                .merge_color(self.theme.foreground)
                .to_style(self.theme.foreground);
            spans.push(Span::styled(hint.clone(), style));
        }
        Line::from(spans)
    }

    fn scroll_line_up(&mut self) {
        let max_offset = self.transcript.len();
        if self.scroll_offset < max_offset {
            self.scroll_offset += 1;
        }
    }

    fn scroll_line_down(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    fn scroll_page_up(&mut self) {
        let page = self.viewport_height.max(1);
        let max_offset = self.transcript.len();
        self.scroll_offset = min(self.scroll_offset + page, max_offset);
    }

    fn scroll_page_down(&mut self) {
        let page = self.viewport_height.max(1);
        if self.scroll_offset > page {
            self.scroll_offset -= page;
        } else {
            self.scroll_offset = 0;
        }
    }

    fn trim_scroll_bounds(&mut self) {
        let max_offset = self.transcript.len();
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
    }
}
