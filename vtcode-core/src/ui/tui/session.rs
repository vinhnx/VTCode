use std::cell::RefCell;
use std::cmp::min;
use std::io::{self, IsTerminal};
use std::rc::Rc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};
use termion::terminal_size;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tuirealm::{
    Application, ApplicationError, Component, MockComponent, PollStrategy, State,
    command::{Cmd, CmdResult},
    event::{Event, Key, KeyEvent, KeyModifiers, NoUserEvent},
    listener::EventListenerCfg,
    props::{AttrValue, Attribute},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::config::types::UiSurfacePreference;

const INLINE_FALLBACK_ROWS: u16 = 24;
const DEFAULT_PROMPT_PREFIX: &str = "❯ ";
const DEFAULT_STATUS_LEFT: &str = "Esc cancel";
const DEFAULT_STATUS_RIGHT: &str = "Ctrl+C interrupt";
const STATUS_SEPARATOR: &str = "  ·  ";
const INDICATOR_USER_PREFIX: &str = "> ";
const INDICATOR_AGENT_PREFIX: &str = "● ";
const TUI_REALM_POLL_TIMEOUT_MS: u64 = 50;

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

    fn formatted(&self, max_width: u16) -> String {
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
        let joined = parts.join(STATUS_SEPARATOR);
        self.truncate_to_width(&joined, max_width)
    }

    fn truncate_to_width(&self, content: &str, max_width: u16) -> String {
        if max_width == 0 {
            return String::new();
        }
        let max_width = max_width as usize;
        if UnicodeWidthStr::width(content) <= max_width {
            return content.to_string();
        }
        let ellipsis_width = UnicodeWidthChar::width('…').unwrap_or(1);
        if max_width <= ellipsis_width {
            return if max_width == 0 {
                String::new()
            } else {
                "…".to_string()
            };
        }
        let mut result = String::new();
        let mut consumed_width = 0usize;
        let mut widths = Vec::new();
        for ch in content.chars() {
            let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if consumed_width + char_width > max_width {
                break;
            }
            result.push(ch);
            widths.push(char_width);
            consumed_width += char_width;
        }
        while consumed_width + ellipsis_width > max_width && !result.is_empty() {
            if let Some(last_width) = widths.pop() {
                consumed_width = consumed_width.saturating_sub(last_width);
            }
            result.pop();
        }
        result.push('…');
        result
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
        let start = self.value[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        self.value.drain(start..self.cursor);
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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ViewId {
    Transcript,
    Status,
    Prompt,
}

#[derive(PartialEq, Eq)]
enum RealmMsg {
    Cancel,
    Exit,
    Interrupt,
    Redraw,
    ScrollLineDown,
    ScrollLineUp,
    ScrollPageDown,
    ScrollPageUp,
    Submit(String),
}

struct TranscriptState {
    lines: Vec<MessageLine>,
    theme: RatatuiTheme,
    scroll_offset: usize,
    viewport_height: usize,
}

impl TranscriptState {
    fn new(theme: RatatuiTheme) -> Self {
        Self {
            lines: Vec::new(),
            theme,
            scroll_offset: 0,
            viewport_height: 1,
        }
    }

    fn set_theme(&mut self, theme: RatatuiTheme) {
        self.theme = theme;
    }

    fn set_labels(&mut self, _agent: Option<String>, _user: Option<String>) {}

    fn push_line(&mut self, kind: RatatuiMessageKind, segments: Vec<RatatuiSegment>) {
        if self.scroll_offset > 0 {
            self.scroll_offset = min(self.scroll_offset + 1, self.lines.len() + 1);
        }
        self.lines.push(MessageLine { kind, segments });
        self.trim_scroll_bounds();
    }

    fn append_inline(&mut self, kind: RatatuiMessageKind, segment: RatatuiSegment) {
        if let Some(last) = self.lines.last_mut() {
            if last.kind == kind {
                last.segments.push(segment);
                return;
            }
        }
        self.lines.push(MessageLine {
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
        let remove_count = min(count, self.lines.len());
        for _ in 0..remove_count {
            self.lines.pop();
        }
        for segments in lines {
            self.lines.push(MessageLine { kind, segments });
        }
        self.trim_scroll_bounds();
    }

    fn visible_lines(&self) -> Vec<Line<'static>> {
        if self.lines.is_empty() {
            return vec![Line::from("")];
        }
        let total = self.lines.len();
        let end = total.saturating_sub(self.scroll_offset);
        let visible_height = self.viewport_height.max(1);
        let start = end.saturating_sub(visible_height);
        self.lines[start..end]
            .iter()
            .map(|line| self.render_line(line))
            .collect()
    }

    fn render_line(&self, line: &MessageLine) -> Line<'static> {
        let mut spans: Vec<Span> = Vec::new();
        let indicator = self.indicator_text(line.kind);
        if !indicator.is_empty() {
            let indicator_style = self.indicator_style(line);
            spans.push(Span::styled(indicator.to_string(), indicator_style));
        }
        let fallback = self.fallback_color(line.kind);
        if line.segments.is_empty() {
            spans.push(Span::raw(""));
        } else {
            for segment in &line.segments {
                let style = segment.style.to_style(fallback.or(self.theme.foreground));
                spans.push(Span::styled(segment.text.clone(), style));
            }
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

    fn indicator_text(&self, kind: RatatuiMessageKind) -> &'static str {
        match kind {
            RatatuiMessageKind::User => INDICATOR_USER_PREFIX,
            _ => INDICATOR_AGENT_PREFIX,
        }
    }

    fn indicator_style(&self, line: &MessageLine) -> Style {
        let fallback = self
            .fallback_color(line.kind)
            .or(self.theme.foreground)
            .unwrap_or(Color::White);
        let color = line
            .segments
            .iter()
            .find_map(|segment| segment.style.color)
            .unwrap_or(fallback);
        Style::default().fg(color)
    }

    fn set_viewport_height(&mut self, height: usize) {
        self.viewport_height = height.max(1);
    }

    fn scroll_line_up(&mut self) {
        let max_offset = self.lines.len();
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
        let max_offset = self.lines.len();
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
        let max_offset = self.lines.len();
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
    }
}

struct PromptState {
    input: InputState,
    prompt_prefix: String,
    prompt_style: RatatuiTextStyle,
    placeholder_hint: Option<String>,
    placeholder_style: RatatuiTextStyle,
    theme: RatatuiTheme,
    cursor_visible: bool,
    input_enabled: bool,
}

impl PromptState {
    fn new(theme: RatatuiTheme, placeholder: Option<String>) -> Self {
        Self {
            input: InputState::default(),
            prompt_prefix: DEFAULT_PROMPT_PREFIX.to_string(),
            prompt_style: RatatuiTextStyle::default(),
            placeholder_hint: placeholder,
            placeholder_style: RatatuiTextStyle::default(),
            theme,
            cursor_visible: true,
            input_enabled: true,
        }
    }

    fn set_theme(&mut self, theme: RatatuiTheme) {
        self.theme = theme;
    }

    fn set_prompt(&mut self, prefix: String, style: RatatuiTextStyle) {
        self.prompt_prefix = prefix;
        self.prompt_style = style;
    }

    fn set_placeholder(&mut self, hint: Option<String>, style: Option<RatatuiTextStyle>) {
        self.placeholder_hint = hint;
        if let Some(style) = style {
            self.placeholder_style = style;
        }
    }

    fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor_visible = visible;
    }

    fn set_input_enabled(&mut self, enabled: bool) {
        self.input_enabled = enabled;
    }

    fn clear_input(&mut self) {
        self.input.clear();
    }

    fn cursor_offset(&self) -> usize {
        UnicodeWidthStr::width(self.input.current_prefix())
    }

    fn prefix_width(&self) -> usize {
        UnicodeWidthStr::width(self.prompt_prefix.as_str())
    }
}

struct StatusState {
    content: StatusBarContent,
    theme: RatatuiTheme,
}

impl StatusState {
    fn new(theme: RatatuiTheme) -> Self {
        Self {
            content: StatusBarContent::new(),
            theme,
        }
    }

    fn update(&mut self, left: Option<String>, center: Option<String>, right: Option<String>) {
        self.content.update(left, center, right);
    }

    fn set_theme(&mut self, theme: RatatuiTheme) {
        self.theme = theme;
    }
}

struct TranscriptComponent {
    state: Rc<RefCell<TranscriptState>>,
}

impl TranscriptComponent {
    fn new(state: Rc<RefCell<TranscriptState>>) -> Self {
        Self { state }
    }
}

impl MockComponent for TranscriptComponent {
    fn view(&mut self, frame: &mut Frame, area: tuirealm::ratatui::layout::Rect) {
        let mut state = self.state.borrow_mut();
        state.set_viewport_height(area.height as usize);
        let mut paragraph = Paragraph::new(state.visible_lines()).wrap(Wrap { trim: false });
        if let Some(bg) = state.theme.background {
            paragraph = paragraph.style(Style::default().bg(bg));
        }
        frame.render_widget(Clear, area);
        frame.render_widget(paragraph, area);
    }

    fn query(&self, _attr: Attribute) -> Option<AttrValue> {
        None
    }

    fn attr(&mut self, _attr: Attribute, _value: AttrValue) {}

    fn state(&self) -> State {
        State::None
    }

    fn perform(&mut self, _cmd: Cmd) -> CmdResult {
        CmdResult::None
    }
}

impl Component<RealmMsg, NoUserEvent> for TranscriptComponent {
    fn on(&mut self, _ev: Event<NoUserEvent>) -> Option<RealmMsg> {
        None
    }
}

struct StatusComponent {
    state: Rc<RefCell<StatusState>>,
}

impl StatusComponent {
    fn new(state: Rc<RefCell<StatusState>>) -> Self {
        Self { state }
    }
}

impl MockComponent for StatusComponent {
    fn view(&mut self, frame: &mut Frame, area: tuirealm::ratatui::layout::Rect) {
        let state = self.state.borrow();
        let content = state.content.formatted(area.width);
        let mut paragraph = Paragraph::new(Line::from(content));
        if let Some(bg) = state.theme.background {
            paragraph = paragraph.style(Style::default().bg(bg));
        }
        frame.render_widget(Clear, area);
        frame.render_widget(paragraph, area);
    }

    fn query(&self, _attr: Attribute) -> Option<AttrValue> {
        None
    }

    fn attr(&mut self, _attr: Attribute, _value: AttrValue) {}

    fn state(&self) -> State {
        State::None
    }

    fn perform(&mut self, _cmd: Cmd) -> CmdResult {
        CmdResult::None
    }
}

impl Component<RealmMsg, NoUserEvent> for StatusComponent {
    fn on(&mut self, _ev: Event<NoUserEvent>) -> Option<RealmMsg> {
        None
    }
}

struct PromptComponent {
    state: Rc<RefCell<PromptState>>,
}

impl PromptComponent {
    fn new(state: Rc<RefCell<PromptState>>) -> Self {
        Self { state }
    }

    fn handle_key_event(&self, key: KeyEvent) -> Option<RealmMsg> {
        let mut state = self.state.borrow_mut();
        if !state.input_enabled {
            return None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                Key::Char('c') | Key::Char('C') => return Some(RealmMsg::Interrupt),
                Key::Char('d') | Key::Char('D') => return Some(RealmMsg::Exit),
                Key::Char('u') | Key::Char('U') => {
                    state.clear_input();
                    return Some(RealmMsg::Redraw);
                }
                _ => return None,
            }
        }
        match key.code {
            Key::Enter => Some(RealmMsg::Submit(state.input.value.clone())),
            Key::Tab => {
                state.input.insert_char('\t');
                Some(RealmMsg::Redraw)
            }
            Key::Char(ch) => {
                state.input.insert_char(ch);
                Some(RealmMsg::Redraw)
            }
            Key::Backspace => {
                state.input.backspace();
                Some(RealmMsg::Redraw)
            }
            Key::Delete => {
                state.input.delete();
                Some(RealmMsg::Redraw)
            }
            Key::Left => {
                state.input.move_left();
                Some(RealmMsg::Redraw)
            }
            Key::Right => {
                state.input.move_right();
                Some(RealmMsg::Redraw)
            }
            Key::Home => {
                state.input.move_home();
                Some(RealmMsg::Redraw)
            }
            Key::End => {
                state.input.move_end();
                Some(RealmMsg::Redraw)
            }
            Key::Up => Some(RealmMsg::ScrollLineUp),
            Key::Down => Some(RealmMsg::ScrollLineDown),
            Key::PageUp => Some(RealmMsg::ScrollPageUp),
            Key::PageDown => Some(RealmMsg::ScrollPageDown),
            Key::Esc => Some(RealmMsg::Cancel),
            _ => None,
        }
    }
}

impl MockComponent for PromptComponent {
    fn view(&mut self, frame: &mut Frame, area: tuirealm::ratatui::layout::Rect) {
        let state = self.state.borrow();
        let mut spans = Vec::new();
        let prefix_style = state
            .prompt_style
            .clone()
            .merge_color(state.theme.primary.or(state.theme.foreground))
            .to_style(state.theme.foreground);
        spans.push(Span::styled(state.prompt_prefix.clone(), prefix_style));
        if !state.input.value.is_empty() {
            spans.push(Span::raw(state.input.value.clone()));
        } else if let Some(hint) = &state.placeholder_hint {
            let style = state
                .placeholder_style
                .clone()
                .merge_color(state.theme.foreground)
                .to_style(state.theme.foreground);
            spans.push(Span::styled(hint.clone(), style));
        }
        let mut prompt = Paragraph::new(Line::from(spans));
        if let Some(bg) = state.theme.background {
            prompt = prompt.style(Style::default().bg(bg));
        }
        frame.render_widget(Clear, area);
        frame.render_widget(prompt, area);
        if state.cursor_visible && state.input_enabled {
            let x = area.x + state.prefix_width() as u16 + state.cursor_offset() as u16;
            let y = area.y;
            frame.set_cursor_position((x, y));
        }
    }

    fn query(&self, _attr: Attribute) -> Option<AttrValue> {
        None
    }

    fn attr(&mut self, _attr: Attribute, _value: AttrValue) {}

    fn state(&self) -> State {
        State::None
    }

    fn perform(&mut self, _cmd: Cmd) -> CmdResult {
        CmdResult::None
    }
}

impl Component<RealmMsg, NoUserEvent> for PromptComponent {
    fn on(&mut self, ev: Event<NoUserEvent>) -> Option<RealmMsg> {
        match ev {
            Event::Keyboard(key) => self.handle_key_event(key),
            Event::Paste(text) => {
                let mut state = self.state.borrow_mut();
                if !state.input_enabled {
                    return None;
                }
                state.input.insert_str(&text);
                Some(RealmMsg::Redraw)
            }
            _ => None,
        }
    }
}

pub(crate) struct RatatuiLoop {
    app: Application<ViewId, RealmMsg, NoUserEvent>,
    transcript: Rc<RefCell<TranscriptState>>,
    prompt: Rc<RefCell<PromptState>>,
    status: Rc<RefCell<StatusState>>,
    needs_redraw: bool,
    should_exit: bool,
}

impl RatatuiLoop {
    pub(crate) fn new(theme: RatatuiTheme, placeholder: Option<String>) -> Result<Self> {
        let transcript_state = Rc::new(RefCell::new(TranscriptState::new(theme.clone())));
        let prompt_state = Rc::new(RefCell::new(PromptState::new(theme.clone(), placeholder)));
        let status_state = Rc::new(RefCell::new(StatusState::new(theme.clone())));
        let mut app = Application::init(
            EventListenerCfg::default()
                .termion_input_listener(Duration::from_millis(TUI_REALM_POLL_TIMEOUT_MS), 1),
        );
        app.mount(
            ViewId::Transcript,
            Box::new(TranscriptComponent::new(transcript_state.clone())),
            Vec::new(),
        )
        .map_err(Self::map_error)?;
        app.mount(
            ViewId::Prompt,
            Box::new(PromptComponent::new(prompt_state.clone())),
            Vec::new(),
        )
        .map_err(Self::map_error)?;
        app.mount(
            ViewId::Status,
            Box::new(StatusComponent::new(status_state.clone())),
            Vec::new(),
        )
        .map_err(Self::map_error)?;
        app.active(&ViewId::Prompt).map_err(Self::map_error)?;
        Ok(Self {
            app,
            transcript: transcript_state,
            prompt: prompt_state,
            status: status_state,
            needs_redraw: true,
            should_exit: false,
        })
    }

    fn map_error(error: ApplicationError) -> anyhow::Error {
        anyhow!(error.to_string())
    }

    pub(crate) fn should_exit(&self) -> bool {
        self.should_exit
    }

    pub(crate) fn set_should_exit(&mut self) {
        self.should_exit = true;
    }

    fn mark_redraw(&mut self) {
        self.needs_redraw = true;
    }

    pub(crate) fn take_redraw(&mut self) -> bool {
        if self.needs_redraw {
            self.needs_redraw = false;
            true
        } else {
            false
        }
    }

    pub(crate) fn handle_command(&mut self, command: RatatuiCommand) -> bool {
        match command {
            RatatuiCommand::AppendLine { kind, segments } => {
                self.transcript.borrow_mut().push_line(kind, segments);
                self.mark_redraw();
                true
            }
            RatatuiCommand::Inline { kind, segment } => {
                self.transcript.borrow_mut().append_inline(kind, segment);
                self.mark_redraw();
                true
            }
            RatatuiCommand::ReplaceLast { count, kind, lines } => {
                self.transcript
                    .borrow_mut()
                    .replace_last(count, kind, lines);
                self.mark_redraw();
                true
            }
            RatatuiCommand::SetPrompt { prefix, style } => {
                self.prompt.borrow_mut().set_prompt(prefix, style);
                self.mark_redraw();
                true
            }
            RatatuiCommand::SetPlaceholder { hint, style } => {
                self.prompt.borrow_mut().set_placeholder(hint, style);
                self.mark_redraw();
                true
            }
            RatatuiCommand::SetMessageLabels { agent, user } => {
                self.transcript.borrow_mut().set_labels(agent, user);
                self.mark_redraw();
                true
            }
            RatatuiCommand::SetTheme { theme } => {
                self.transcript.borrow_mut().set_theme(theme.clone());
                self.prompt.borrow_mut().set_theme(theme.clone());
                self.status.borrow_mut().set_theme(theme);
                self.mark_redraw();
                true
            }
            RatatuiCommand::UpdateStatusBar {
                left,
                center,
                right,
            } => {
                self.status.borrow_mut().update(left, center, right);
                self.mark_redraw();
                true
            }
            RatatuiCommand::SetCursorVisible(value) => {
                self.prompt.borrow_mut().set_cursor_visible(value);
                self.mark_redraw();
                true
            }
            RatatuiCommand::SetInputEnabled(value) => {
                self.prompt.borrow_mut().set_input_enabled(value);
                self.mark_redraw();
                true
            }
            RatatuiCommand::ClearInput => {
                self.prompt.borrow_mut().clear_input();
                self.mark_redraw();
                true
            }
            RatatuiCommand::ForceRedraw => {
                self.mark_redraw();
                true
            }
            RatatuiCommand::Shutdown => {
                self.set_should_exit();
                true
            }
        }
    }

    fn process_messages(
        &mut self,
        messages: Vec<RealmMsg>,
        events: &UnboundedSender<RatatuiEvent>,
    ) -> Result<bool> {
        let mut should_redraw = false;
        for message in messages {
            match message {
                RealmMsg::Submit(text) => {
                    self.prompt.borrow_mut().clear_input();
                    should_redraw = true;
                    let _ = events.send(RatatuiEvent::Submit(text));
                }
                RealmMsg::Cancel => {
                    let _ = events.send(RatatuiEvent::Cancel);
                }
                RealmMsg::Exit => {
                    let _ = events.send(RatatuiEvent::Exit);
                }
                RealmMsg::Interrupt => {
                    let _ = events.send(RatatuiEvent::Interrupt);
                }
                RealmMsg::ScrollLineUp => {
                    self.transcript.borrow_mut().scroll_line_up();
                    let _ = events.send(RatatuiEvent::ScrollLineUp);
                    should_redraw = true;
                }
                RealmMsg::ScrollLineDown => {
                    self.transcript.borrow_mut().scroll_line_down();
                    let _ = events.send(RatatuiEvent::ScrollLineDown);
                    should_redraw = true;
                }
                RealmMsg::ScrollPageUp => {
                    self.transcript.borrow_mut().scroll_page_up();
                    let _ = events.send(RatatuiEvent::ScrollPageUp);
                    should_redraw = true;
                }
                RealmMsg::ScrollPageDown => {
                    self.transcript.borrow_mut().scroll_page_down();
                    let _ = events.send(RatatuiEvent::ScrollPageDown);
                    should_redraw = true;
                }
                RealmMsg::Redraw => {
                    should_redraw = true;
                }
            }
        }
        if should_redraw {
            self.mark_redraw();
        }
        Ok(should_redraw)
    }

    pub(crate) fn poll(&mut self, events: &UnboundedSender<RatatuiEvent>) -> Result<bool> {
        let messages = self
            .app
            .tick(PollStrategy::TryFor(Duration::from_millis(
                TUI_REALM_POLL_TIMEOUT_MS,
            )))
            .map_err(Self::map_error)
            .context("failed to poll tui-realm events")?;
        self.process_messages(messages, events)
    }

    pub(crate) fn draw(&mut self, frame: &mut Frame<'_>) {
        let status_has_text = self.status.borrow().content.has_text();
        let mut constraints = Vec::new();
        constraints.push(Constraint::Min(1));
        if status_has_text {
            constraints.push(Constraint::Length(1));
        }
        constraints.push(Constraint::Length(1));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(frame.area());
        let (transcript_area, status_area, prompt_area) = if status_has_text {
            (chunks[0], Some(chunks[1]), chunks[2])
        } else {
            (chunks[0], None, chunks[1])
        };
        self.app.view(&ViewId::Transcript, frame, transcript_area);
        if let Some(area) = status_area {
            self.app.view(&ViewId::Status, frame, area);
        }
        self.app.view(&ViewId::Prompt, frame, prompt_area);
    }
}
