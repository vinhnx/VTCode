use crate::ui::slash::{SlashCommandInfo, suggestions_for};
use crate::ui::theme;
use ansi_to_tui::IntoText;
use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects, Style as AnsiStyle};
use anyhow::{Context, Result};
use crossterm::{
    ExecutableCommand, cursor,
    event::{
        DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent, EventStream, KeyCode,
        KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use ratatui::{
    Frame, Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear as ClearWidget, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use serde::de::value::{Error as DeValueError, StrDeserializer};
use serde_json::Value;
use std::cmp;
use std::collections::VecDeque;
use std::io;
use std::mem;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::time::{Interval, MissedTickBehavior, interval};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const ESCAPE_DOUBLE_MS: u64 = 750;
const REDRAW_INTERVAL_MS: u64 = 33;
const MESSAGE_INDENT: usize = 2;
const NAVIGATION_HINT_TEXT: &str = "↵ send · esc exit";
const MAX_SLASH_SUGGESTIONS: usize = 6;

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

    fn to_style(&self, fallback: Option<Color>) -> Style {
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
struct StyledLine {
    segments: Vec<RatatuiSegment>,
}

impl StyledLine {
    fn push_segment(&mut self, segment: RatatuiSegment) {
        if segment.text.is_empty() {
            return;
        }
        self.segments.push(segment);
    }

    fn has_visible_content(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| segment.text.chars().any(|ch| !ch.is_whitespace()))
    }
}

#[derive(Clone)]
pub struct RatatuiTheme {
    pub background: Option<Color>,
    pub foreground: Option<Color>,
    pub primary: Option<Color>,
    pub secondary: Option<Color>,
}

impl Default for RatatuiTheme {
    fn default() -> Self {
        Self {
            background: None,
            foreground: None,
            primary: None,
            secondary: None,
        }
    }
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
    sender: UnboundedSender<RatatuiCommand>,
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

    pub fn shutdown(&self) {
        let _ = self.sender.send(RatatuiCommand::Shutdown);
    }
}

pub struct RatatuiSession {
    pub handle: RatatuiHandle,
    pub events: UnboundedReceiver<RatatuiEvent>,
}

pub fn spawn_session(theme: RatatuiTheme, placeholder: Option<String>) -> Result<RatatuiSession> {
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        if let Err(err) = run_ratatui(command_rx, event_tx, theme, placeholder).await {
            tracing::error!(error = ?err, "ratatui session terminated unexpectedly");
        }
    });

    Ok(RatatuiSession {
        handle: RatatuiHandle { sender: command_tx },
        events: event_rx,
    })
}

async fn run_ratatui(
    commands: UnboundedReceiver<RatatuiCommand>,
    events: UnboundedSender<RatatuiEvent>,
    theme: RatatuiTheme,
    placeholder: Option<String>,
) -> Result<()> {
    let mut stdout = io::stdout();
    let backend = CrosstermBackend::new(&mut stdout);
    let (_, rows) = crossterm::terminal::size().context("failed to query terminal size")?;
    let options = TerminalOptions {
        viewport: Viewport::Inline(rows),
    };
    let mut terminal = Terminal::with_options(backend, options)
        .context("failed to initialize ratatui terminal")?;
    let _guard = TerminalGuard::new().context("failed to configure terminal for ratatui")?;
    terminal
        .clear()
        .context("failed to clear terminal for ratatui")?;

    let mut app = RatatuiLoop::new(theme, placeholder);
    let mut command_rx = commands;
    let mut event_stream = EventStream::new();
    let mut redraw = true;
    let mut ticker = create_ticker();

    loop {
        if redraw {
            terminal
                .draw(|frame| app.draw(frame))
                .context("failed to draw ratatui frame")?;
            redraw = false;
        }

        tokio::select! {
            Some(cmd) = command_rx.recv() => {
                if app.handle_command(cmd) {
                    redraw = true;
                }
                if app.should_exit() {
                    break;
                }
            }
            event = event_stream.next() => {
                match event {
                    Some(Ok(evt)) => {
                        if matches!(evt, CrosstermEvent::Resize(_, _)) {
                            terminal
                                .autoresize()
                                .context("failed to autoresize terminal viewport")?;
                        }
                        if app.handle_event(evt, &events)? {
                            redraw = true;
                        }
                        if app.should_exit() {
                            break;
                        }
                    }
                    Some(Err(_)) => {
                        redraw = true;
                    }
                    None => {}
                }
            }
            _ = ticker.tick() => {
                if app.needs_tick() {
                    redraw = true;
                }
            }
        }

        if app.should_exit() {
            break;
        }
    }

    terminal.show_cursor().ok();
    terminal
        .clear()
        .context("failed to clear terminal after ratatui session")?;

    Ok(())
}

struct TerminalGuard {
    mouse_capture_enabled: bool,
    cursor_hidden: bool,
}

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode().context("failed to enable raw mode")?;
        let mut stdout = io::stdout();
        stdout
            .execute(EnableMouseCapture)
            .context("failed to enable mouse capture")?;
        stdout
            .execute(cursor::Hide)
            .context("failed to hide cursor")?;
        Ok(Self {
            mouse_capture_enabled: true,
            cursor_hidden: true,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        if self.mouse_capture_enabled {
            let _ = stdout.execute(DisableMouseCapture);
        }
        if self.cursor_hidden {
            let _ = stdout.execute(cursor::Show);
        }
        let _ = stdout.execute(Clear(ClearType::FromCursorDown));
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

    fn insert(&mut self, ch: char) {
        self.value.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let new_cursor = self.value[..self.cursor]
            .chars()
            .next_back()
            .map(|ch| self.cursor - ch.len_utf8())
            .unwrap_or(0);
        self.value.replace_range(new_cursor..self.cursor, "");
        self.cursor = new_cursor;
    }

    fn delete(&mut self) {
        if self.cursor >= self.value.len() {
            return;
        }
        let len = self.value[self.cursor..]
            .chars()
            .next()
            .map(|ch| ch.len_utf8())
            .unwrap_or(0);
        let end = self.cursor + len;
        self.value.replace_range(self.cursor..end, "");
    }

    fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let new_cursor = self.value[..self.cursor]
            .chars()
            .next_back()
            .map(|ch| self.cursor - ch.len_utf8())
            .unwrap_or(0);
        self.cursor = new_cursor;
    }

    fn move_right(&mut self) {
        if self.cursor >= self.value.len() {
            return;
        }
        let advance = self.value[self.cursor..]
            .chars()
            .next()
            .map(|ch| ch.len_utf8())
            .unwrap_or(0);
        self.cursor += advance;
    }

    fn move_home(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.value.len();
    }

    fn take(&mut self) -> String {
        let mut result = String::new();
        mem::swap(&mut result, &mut self.value);
        self.cursor = 0;
        result
    }

    fn value(&self) -> &str {
        &self.value
    }

    fn width_before_cursor(&self) -> usize {
        UnicodeWidthStr::width(&self.value[..self.cursor])
    }
}

#[derive(Default)]
struct TranscriptScrollState {
    offset: usize,
    viewport_height: usize,
    content_height: usize,
}

impl TranscriptScrollState {
    fn offset(&self) -> usize {
        self.offset
    }

    fn update_bounds(&mut self, content_height: usize, viewport_height: usize) {
        self.content_height = content_height;
        self.viewport_height = viewport_height;
        let max_offset = self.max_offset();
        if self.offset > max_offset {
            self.offset = max_offset;
        }
    }

    fn scroll_to_bottom(&mut self) {
        self.offset = self.max_offset();
    }

    fn scroll_up(&mut self) {
        if self.offset > 0 {
            self.offset -= 1;
        }
    }

    fn scroll_down(&mut self) {
        let max_offset = self.max_offset();
        if self.offset < max_offset {
            self.offset += 1;
        }
    }

    fn scroll_page_up(&mut self) {
        if self.offset == 0 {
            return;
        }
        let step = self.viewport_height.max(1);
        self.offset = self.offset.saturating_sub(step);
    }

    fn scroll_page_down(&mut self) {
        let max_offset = self.max_offset();
        if self.offset >= max_offset {
            return;
        }
        let step = self.viewport_height.max(1);
        self.offset = (self.offset + step).min(max_offset);
    }

    fn is_at_bottom(&self) -> bool {
        self.offset >= self.max_offset()
    }

    fn should_follow_new_content(&self) -> bool {
        self.viewport_height == 0 || self.is_at_bottom()
    }

    fn max_offset(&self) -> usize {
        if self.content_height <= self.viewport_height {
            0
        } else {
            self.content_height - self.viewport_height
        }
    }

    fn content_height(&self) -> usize {
        self.content_height
    }

    fn viewport_height(&self) -> usize {
        self.viewport_height
    }

    fn has_overflow(&self) -> bool {
        self.content_height > self.viewport_height
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScrollFocus {
    Transcript,
    Pty,
}

#[derive(Clone)]
struct MessageBlock {
    kind: RatatuiMessageKind,
    lines: Vec<StyledLine>,
}

#[derive(Clone, Default)]
struct StatusBarContent {
    left: String,
    center: String,
    right: String,
}

impl StatusBarContent {
    fn new() -> Self {
        Self {
            left: "? help · / command".to_string(),
            center: String::new(),
            right: NAVIGATION_HINT_TEXT.to_string(),
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
}

#[derive(Clone, Copy)]
struct PtyPlacement {
    top: usize,
    height: usize,
    indent: usize,
}

#[derive(Default, Clone)]
struct SelectionState {
    start: Option<usize>,
    end: Option<usize>,
    dragging: bool,
}

impl SelectionState {
    fn clear(&mut self) {
        self.start = None;
        self.end = None;
        self.dragging = false;
    }

    fn begin(&mut self, line: usize) {
        self.start = Some(line);
        self.end = Some(line);
        self.dragging = true;
    }

    fn update(&mut self, line: usize) {
        if self.start.is_some() {
            self.end = Some(line);
        }
    }

    fn finish(&mut self) {
        self.dragging = false;
    }

    fn is_active(&self) -> bool {
        self.start.is_some()
    }

    fn is_dragging(&self) -> bool {
        self.dragging
    }

    fn range(&self) -> Option<(usize, usize)> {
        let start = self.start?;
        let end = self.end?;
        if start <= end {
            Some((start, end))
        } else {
            Some((end, start))
        }
    }
}

#[derive(Default)]
struct SlashSuggestionState {
    items: Vec<&'static SlashCommandInfo>,
    list_state: ListState,
}

impl SlashSuggestionState {
    fn clear(&mut self) {
        self.items.clear();
        self.list_state.select(None);
    }

    fn update(&mut self, query: &str) {
        self.items = suggestions_for(query);
        if self.items.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
    }

    fn is_visible(&self) -> bool {
        !self.items.is_empty()
    }

    fn visible_capacity(&self) -> usize {
        self.items.len().min(MAX_SLASH_SUGGESTIONS)
    }

    fn desired_height(&self) -> u16 {
        if !self.is_visible() {
            return 0;
        }
        self.visible_capacity() as u16 + 2
    }

    fn visible_height(&self, available: u16) -> u16 {
        if available < 3 || !self.is_visible() {
            return 0;
        }
        self.desired_height().min(available)
    }

    fn items(&self) -> &[&'static SlashCommandInfo] {
        &self.items
    }

    fn list_state(&mut self) -> &mut ListState {
        &mut self.list_state
    }

    fn selected_index(&self) -> Option<usize> {
        self.list_state.selected()
    }

    fn select_previous(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let len = self.items.len();
        let next = if current == 0 {
            len.saturating_sub(1)
        } else {
            current.saturating_sub(1)
        };
        if len == 0 {
            self.list_state.select(None);
            return false;
        }
        if current != next {
            self.list_state.select(Some(next));
        } else {
            self.list_state.select(Some(next));
        }
        true
    }

    fn select_next(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        let len = self.items.len();
        let current = self.list_state.selected().unwrap_or(0);
        let next = if current + 1 >= len { 0 } else { current + 1 };
        self.list_state.select(Some(next));
        true
    }

    fn selected(&self) -> Option<&'static SlashCommandInfo> {
        let index = self.list_state.selected()?;
        self.items.get(index).copied()
    }
}

const PTY_MAX_LINES: usize = 200;
const PTY_PANEL_MAX_HEIGHT: usize = 10;
const PTY_CONTENT_VIEW_LINES: usize = PTY_PANEL_MAX_HEIGHT - 2;

struct PtyPanel {
    tool_name: Option<String>,
    command_display: Option<String>,
    lines: VecDeque<String>,
    trailing: String,
    cached: Text<'static>,
    dirty: bool,
    cached_height: usize,
}

impl PtyPanel {
    fn new() -> Self {
        Self {
            tool_name: None,
            command_display: None,
            lines: VecDeque::with_capacity(PTY_MAX_LINES),
            trailing: String::new(),
            cached: Text::default(),
            dirty: true,
            cached_height: 0,
        }
    }

    fn reset_output(&mut self) {
        self.lines.clear();
        self.trailing.clear();
        self.cached = Text::default();
        self.dirty = true;
        self.cached_height = 0;
    }

    fn clear(&mut self) {
        self.tool_name = None;
        self.command_display = None;
        self.reset_output();
    }

    fn set_tool_call(&mut self, tool_name: String, command_display: Option<String>) {
        self.tool_name = Some(tool_name);
        self.command_display = command_display;
        self.reset_output();
    }

    fn push_line(&mut self, text: &str) {
        self.push_text(text, true);
    }

    fn push_inline(&mut self, text: &str) {
        self.push_text(text, false);
    }

    fn push_text(&mut self, text: &str, newline: bool) {
        if text.is_empty() {
            if newline {
                self.commit_line();
            }
            return;
        }

        let mut remaining = text;
        while let Some(index) = remaining.find('\n') {
            let (segment, rest) = remaining.split_at(index);
            self.trailing.push_str(segment);
            self.commit_line();
            remaining = rest.get(1..).unwrap_or("");
        }

        if !remaining.is_empty() {
            self.trailing.push_str(remaining);
        }

        if newline {
            if !self.trailing.is_empty() || text.is_empty() {
                self.commit_line();
            }
        }

        self.dirty = true;
    }

    fn commit_line(&mut self) {
        let line = mem::take(&mut self.trailing);
        self.lines.push_back(line);
        if self.lines.len() > PTY_MAX_LINES {
            self.lines.pop_front();
        }
    }

    fn has_content(&self) -> bool {
        self.tool_name.is_some()
            || self.command_display.is_some()
            || !self.lines.is_empty()
            || !self.trailing.is_empty()
    }

    fn command_summary(&self) -> Option<String> {
        let command = self.command_display.as_ref()?;
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return None;
        }
        const MAX_CHARS: usize = 48;
        let mut summary = String::new();
        for (index, ch) in trimmed.chars().enumerate() {
            if index >= MAX_CHARS - 1 {
                summary.push('…');
                break;
            }
            summary.push(ch);
        }
        if summary.is_empty() {
            Some(trimmed.to_string())
        } else if summary.ends_with('…') {
            Some(summary)
        } else if trimmed.chars().count() > summary.chars().count() {
            let mut truncated = summary;
            truncated.push('…');
            Some(truncated)
        } else {
            Some(summary)
        }
    }

    fn block_title_text(&self) -> String {
        let base = self.tool_name.as_deref().unwrap_or("terminal").to_string();
        if let Some(summary) = self.command_summary() {
            if summary.is_empty() {
                base
            } else {
                format!("{base} · {summary}")
            }
        } else {
            base
        }
    }

    fn view_text(&mut self) -> Text<'static> {
        if !self.dirty {
            return self.cached.clone();
        }

        let mut lines = Vec::new();
        if let Some(command) = self.command_display.as_ref() {
            if !command.is_empty() {
                lines.push(format!("$ {}", command));
            }
        }
        for entry in &self.lines {
            lines.push(entry.clone());
        }
        if !self.trailing.is_empty() {
            lines.push(self.trailing.clone());
        }

        let combined = if lines.is_empty() {
            String::new()
        } else {
            lines.join("\n")
        };

        let parsed = if combined.is_empty() {
            Text::default()
        } else {
            combined
                .clone()
                .into_text()
                .unwrap_or_else(|_| Text::from(combined.clone()))
        };

        self.cached = parsed.clone();
        self.dirty = false;
        self.cached_height = self.cached.height();
        parsed
    }
}

struct TranscriptDisplay {
    lines: Vec<Line<'static>>,
    total_height: usize,
}

struct InputDisplay {
    lines: Vec<Line<'static>>,
    cursor: Option<(u16, u16)>,
    height: u16,
}

struct InputLayout {
    block_area: Rect,
    suggestion_area: Option<Rect>,
    display: InputDisplay,
}

struct RatatuiLoop {
    messages: Vec<MessageBlock>,
    current_line: StyledLine,
    current_kind: Option<RatatuiMessageKind>,
    current_active: bool,
    prompt_prefix: String,
    prompt_style: RatatuiTextStyle,
    input: InputState,
    base_placeholder: Option<String>,
    placeholder_hint: Option<String>,
    show_placeholder: bool,
    base_placeholder_style: RatatuiTextStyle,
    placeholder_style: RatatuiTextStyle,
    should_exit: bool,
    theme: RatatuiTheme,
    last_escape: Option<Instant>,
    transcript_scroll: TranscriptScrollState,
    transcript_autoscroll: bool,
    pty_scroll: TranscriptScrollState,
    pty_autoscroll: bool,
    scroll_focus: ScrollFocus,
    transcript_area: Option<Rect>,
    pty_area: Option<Rect>,
    pty_block: Option<PtyPlacement>,
    slash_suggestions: SlashSuggestionState,
    pty_panel: Option<PtyPanel>,
    status_bar: StatusBarContent,
    cursor_visible: bool,
    input_enabled: bool,
    selection: SelectionState,
}

impl RatatuiLoop {
    fn default_placeholder_style(theme: &RatatuiTheme) -> RatatuiTextStyle {
        let mut style = RatatuiTextStyle::default();
        style.italic = true;
        style.color = theme
            .secondary
            .or(theme.foreground)
            .or(Some(Color::DarkGray));
        style
    }

    fn new(theme: RatatuiTheme, placeholder: Option<String>) -> Self {
        let sanitized_placeholder = placeholder
            .map(|hint| hint.trim().to_string())
            .filter(|hint| !hint.is_empty());
        let base_placeholder = sanitized_placeholder.clone();
        let show_placeholder = base_placeholder.is_some();
        let base_placeholder_style = Self::default_placeholder_style(&theme);
        Self {
            messages: Vec::new(),
            current_line: StyledLine::default(),
            current_kind: None,
            current_active: false,
            prompt_prefix: "> ".to_string(),
            prompt_style: RatatuiTextStyle::default(),
            input: InputState::default(),
            base_placeholder: base_placeholder.clone(),
            placeholder_hint: base_placeholder.clone(),
            show_placeholder,
            base_placeholder_style: base_placeholder_style.clone(),
            placeholder_style: base_placeholder_style,
            should_exit: false,
            theme,
            last_escape: None,
            transcript_scroll: TranscriptScrollState::default(),
            transcript_autoscroll: true,
            pty_scroll: TranscriptScrollState::default(),
            pty_autoscroll: true,
            scroll_focus: ScrollFocus::Transcript,
            transcript_area: None,
            pty_area: None,
            pty_block: None,
            slash_suggestions: SlashSuggestionState::default(),
            pty_panel: None,
            status_bar: StatusBarContent::new(),
            cursor_visible: true,
            input_enabled: true,
            selection: SelectionState::default(),
        }
    }

    fn should_exit(&self) -> bool {
        self.should_exit
    }

    fn needs_tick(&self) -> bool {
        false
    }

    fn handle_command(&mut self, command: RatatuiCommand) -> bool {
        match command {
            RatatuiCommand::AppendLine { kind, segments } => {
                let follow_output = self.transcript_scroll.should_follow_new_content();
                let plain = Self::collect_plain_text(&segments);
                self.track_pty_metadata(kind, &plain);
                let was_active = self.current_active;
                self.flush_current_line(was_active);
                self.push_line(kind, StyledLine { segments });
                self.forward_pty_line(kind, &plain);
                if follow_output {
                    self.transcript_autoscroll = true;
                }
                true
            }
            RatatuiCommand::Inline { kind, segment } => {
                let follow_output = self.transcript_scroll.should_follow_new_content();
                let plain = segment.text.clone();
                self.forward_pty_inline(kind, &plain);
                self.append_inline_segment(kind, segment);
                if follow_output {
                    self.transcript_autoscroll = true;
                }
                true
            }
            RatatuiCommand::ReplaceLast { count, kind, lines } => {
                let follow_output = self.transcript_scroll.should_follow_new_content();
                let follow_pty = self.pty_scroll.should_follow_new_content();
                let was_active = self.current_active;
                self.flush_current_line(was_active);
                if kind == RatatuiMessageKind::Pty {
                    if let Some(panel) = self.pty_panel.as_mut() {
                        panel.reset_output();
                        for segments in &lines {
                            let plain = Self::collect_plain_text(segments);
                            panel.push_line(&plain);
                        }
                    }
                    if follow_pty {
                        self.pty_autoscroll = true;
                    }
                } else if kind == RatatuiMessageKind::Tool {
                    if let Some(first_line) = lines.first() {
                        let plain = Self::collect_plain_text(first_line);
                        self.track_pty_metadata(kind, &plain);
                    }
                }
                self.remove_last_lines(count);
                for segments in lines {
                    self.push_line(kind, StyledLine { segments });
                }
                if follow_output {
                    self.transcript_autoscroll = true;
                }
                true
            }
            RatatuiCommand::SetPrompt { prefix, style } => {
                self.prompt_prefix = prefix;
                self.prompt_style = style;
                true
            }
            RatatuiCommand::SetPlaceholder { hint, style } => {
                let resolved = hint.or_else(|| self.base_placeholder.clone());
                self.placeholder_hint = resolved;
                if let Some(new_style) = style {
                    self.placeholder_style = new_style;
                } else {
                    self.placeholder_style = self.base_placeholder_style.clone();
                }
                self.update_input_state();
                true
            }
            RatatuiCommand::SetTheme { theme } => {
                let previous_base = self.base_placeholder_style.clone();
                self.theme = theme;
                let new_base = Self::default_placeholder_style(&self.theme);
                self.base_placeholder_style = new_base.clone();
                if self.placeholder_style == previous_base {
                    self.placeholder_style = new_base;
                }
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
            RatatuiCommand::SetCursorVisible(visible) => {
                self.cursor_visible = visible;
                true
            }
            RatatuiCommand::SetInputEnabled(enabled) => {
                self.input_enabled = enabled;
                if !enabled {
                    self.slash_suggestions.clear();
                } else {
                    self.update_input_state();
                }
                true
            }
            RatatuiCommand::Shutdown => {
                self.should_exit = true;
                true
            }
        }
    }

    fn collect_plain_text(segments: &[RatatuiSegment]) -> String {
        segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<String>()
    }

    fn append_inline_segment(&mut self, kind: RatatuiMessageKind, segment: RatatuiSegment) {
        let text = segment.text;
        let style = segment.style;
        if text.is_empty() {
            return;
        }

        if self.current_kind != Some(kind) {
            if self.current_active {
                self.flush_current_line(true);
            }
            self.current_kind = Some(kind);
        }

        let mut parts = text.split('\n').peekable();
        let ends_with_newline = text.ends_with('\n');

        while let Some(part) = parts.next() {
            if !part.is_empty() {
                if self.current_kind != Some(kind) {
                    self.current_kind = Some(kind);
                }
                self.current_line.push_segment(RatatuiSegment {
                    text: part.to_string(),
                    style: style.clone(),
                });
                self.current_active = true;
            }

            if parts.peek().is_some() {
                self.flush_current_line(true);
                self.current_kind = Some(kind);
            }
        }

        if ends_with_newline {
            self.flush_current_line(true);
            self.current_kind = Some(kind);
        }
    }

    fn flush_current_line(&mut self, force: bool) {
        if !force && !self.current_active {
            return;
        }

        if let Some(kind) = self.current_kind {
            if !self.current_line.segments.is_empty() || force {
                let line = mem::take(&mut self.current_line);
                self.push_line(kind, line);
            } else {
                self.current_line = StyledLine::default();
            }
        }

        self.current_line = StyledLine::default();
        self.current_active = false;
        self.current_kind = None;
    }

    fn update_input_state(&mut self) {
        self.show_placeholder = self.placeholder_hint.is_some() && self.input.value().is_empty();
        self.refresh_slash_suggestions();
    }

    fn refresh_slash_suggestions(&mut self) {
        if let Some(rest) = self.input.value().strip_prefix('/') {
            let trimmed = rest.trim_start();
            if trimmed.chars().any(char::is_whitespace) {
                self.slash_suggestions.clear();
                return;
            }
            let query = trimmed.trim_end();
            self.slash_suggestions.update(query);
        } else {
            self.slash_suggestions.clear();
        }
    }

    fn set_input_text(&mut self, value: String) {
        if !self.input_enabled {
            return;
        }
        self.input.value = value;
        self.input.cursor = self.input.value.len();
        self.update_input_state();
        self.transcript_autoscroll = true;
    }

    fn apply_selected_suggestion(&mut self) -> bool {
        if !self.input_enabled {
            return false;
        }
        let Some(selected) = self.slash_suggestions.selected() else {
            return false;
        };
        let raw = self.input.value().to_string();
        let remainder = raw
            .strip_prefix('/')
            .and_then(|rest| {
                rest.char_indices()
                    .find(|(_, ch)| ch.is_whitespace())
                    .map(|(idx, _)| rest[idx..].trim_start().to_string())
            })
            .unwrap_or_default();

        let mut new_value = format!("/{}", selected.name);
        if remainder.is_empty() {
            new_value.push(' ');
        } else {
            new_value.push(' ');
            new_value.push_str(&remainder);
        }
        self.set_input_text(new_value);
        true
    }

    fn push_line(&mut self, kind: RatatuiMessageKind, line: StyledLine) {
        if kind == RatatuiMessageKind::Agent && !line.has_visible_content() {
            return;
        }
        if let Some(block) = self.messages.last_mut() {
            if block.kind == kind {
                block.lines.push(line);
                return;
            }
        }

        self.messages.push(MessageBlock {
            kind,
            lines: vec![line],
        });
    }

    fn remove_last_lines(&mut self, mut count: usize) {
        while count > 0 {
            let Some(block) = self.messages.last_mut() else {
                break;
            };

            if block.lines.len() <= count {
                count -= block.lines.len();
                self.messages.pop();
            } else {
                let new_len = block.lines.len() - count;
                block.lines.truncate(new_len);
                count = 0;
            }
        }
    }

    fn ensure_pty_panel(&mut self) -> &mut PtyPanel {
        if self.pty_panel.is_none() {
            self.pty_panel = Some(PtyPanel::new());
        }
        self.pty_panel.as_mut().expect("pty_panel must exist")
    }

    fn track_pty_metadata(&mut self, kind: RatatuiMessageKind, plain: &str) {
        if kind != RatatuiMessageKind::Tool {
            return;
        }
        let trimmed = plain.trim();
        if let Some(rest) = trimmed.strip_prefix("[TOOL]") {
            let mut parts = rest.trim_start().splitn(2, ' ');
            let tool_name = parts.next().map(str::trim).unwrap_or("");
            let payload = parts.next().map(str::trim).unwrap_or("");
            match tool_name {
                "run_terminal_cmd" => {
                    let command = Self::parse_run_command(payload);
                    let panel = self.ensure_pty_panel();
                    panel.set_tool_call(tool_name.to_string(), command);
                    self.pty_autoscroll = true;
                }
                "bash_command" => {
                    let command = Self::parse_bash_command(payload);
                    let panel = self.ensure_pty_panel();
                    panel.set_tool_call(tool_name.to_string(), command);
                    self.pty_autoscroll = true;
                }
                _ => {
                    if let Some(panel) = self.pty_panel.as_mut() {
                        panel.clear();
                    }
                }
            }
        }
    }

    fn parse_run_command(json_segment: &str) -> Option<String> {
        let value: Value = serde_json::from_str(json_segment).ok()?;
        let array = value.get("command")?.as_array()?;
        let mut parts = Vec::with_capacity(array.len());
        for entry in array {
            if let Some(text) = entry.as_str() {
                parts.push(text.to_string());
            }
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    }

    fn parse_bash_command(json_segment: &str) -> Option<String> {
        let value: Value = serde_json::from_str(json_segment).ok()?;
        if let Some(command) = value.get("bash_command").and_then(|val| val.as_str()) {
            let trimmed = command.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        } else if let Some(array) = value.get("command").and_then(|val| val.as_array()) {
            let mut parts = Vec::with_capacity(array.len());
            for entry in array {
                if let Some(text) = entry.as_str() {
                    parts.push(text.to_string());
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        } else {
            None
        }
    }

    fn forward_pty_line(&mut self, kind: RatatuiMessageKind, text: &str) {
        if kind != RatatuiMessageKind::Pty {
            return;
        }
        if let Some(panel) = self.pty_panel.as_mut() {
            let follow = self.pty_scroll.should_follow_new_content();
            panel.push_line(text);
            if follow {
                self.pty_autoscroll = true;
            }
        }
    }

    fn forward_pty_inline(&mut self, kind: RatatuiMessageKind, text: &str) {
        if kind != RatatuiMessageKind::Pty {
            return;
        }
        if let Some(panel) = self.pty_panel.as_mut() {
            let follow = self.pty_scroll.should_follow_new_content();
            panel.push_inline(text);
            if follow {
                self.pty_autoscroll = true;
            }
        }
    }

    fn render_slash_suggestions(&mut self, frame: &mut Frame, area: Rect) {
        if !self.slash_suggestions.is_visible() {
            return;
        }
        if area.width <= 2 || area.height < 3 {
            return;
        }

        let capacity = cmp::min(
            MAX_SLASH_SUGGESTIONS,
            area.height.saturating_sub(2) as usize,
        );
        if capacity == 0 {
            return;
        }

        let items: Vec<&SlashCommandInfo> = self
            .slash_suggestions
            .items()
            .iter()
            .take(capacity)
            .copied()
            .collect();
        if items.is_empty() {
            return;
        }

        if let Some(selected) = self.slash_suggestions.selected_index() {
            if selected >= items.len() {
                let clamped = items.len().saturating_sub(1);
                self.slash_suggestions.list_state.select(Some(clamped));
            }
        }

        let max_name_len = items.iter().map(|info| info.name.len()).max().unwrap_or(0);
        let entries: Vec<String> = items
            .iter()
            .map(|info| {
                let mut line = format!("/{:<width$}", info.name, width = max_name_len);
                line.push(' ');
                line.push_str(info.description);
                line
            })
            .collect();

        let max_width = entries
            .iter()
            .map(|value| UnicodeWidthStr::width(value.as_str()))
            .max()
            .unwrap_or(0);
        let visible_height = entries.len().min(capacity) as u16 + 2;
        let height = visible_height.min(area.height);
        let required_width = cmp::max(4, cmp::min(area.width as usize, max_width + 4)) as u16;
        let suggestion_area = Rect::new(area.x, area.y, required_width, height);
        frame.render_widget(ClearWidget, suggestion_area);

        let list_items: Vec<ListItem> = entries.into_iter().map(ListItem::new).collect();
        let border_style = Style::default().fg(self.theme.primary.unwrap_or(Color::LightBlue));
        let list = List::new(list_items)
            .block(
                Block::default()
                    .title(Line::from("? help · / commands"))
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(
                Style::default()
                    .fg(self.theme.primary.unwrap_or(Color::LightBlue))
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_stateful_widget(list, suggestion_area, self.slash_suggestions.list_state());
    }

    fn handle_event(
        &mut self,
        event: CrosstermEvent,
        events: &UnboundedSender<RatatuiEvent>,
    ) -> Result<bool> {
        match event {
            CrosstermEvent::Key(key) => self.handle_key_event(key, events),
            CrosstermEvent::Resize(_, _) => {
                self.transcript_autoscroll = true;
                self.pty_autoscroll = true;
                Ok(true)
            }
            CrosstermEvent::Mouse(mouse) => self.handle_mouse_event(mouse, events),
            CrosstermEvent::FocusGained | CrosstermEvent::FocusLost | CrosstermEvent::Paste(_) => {
                Ok(false)
            }
        }
    }

    fn handle_key_event(
        &mut self,
        key: KeyEvent,
        events: &UnboundedSender<RatatuiEvent>,
    ) -> Result<bool> {
        if key.kind == KeyEventKind::Release {
            return Ok(false);
        }

        let suggestions_active = self.slash_suggestions.is_visible();
        if suggestions_active {
            match key.code {
                KeyCode::Up => {
                    if self.slash_suggestions.select_previous() {
                        return Ok(true);
                    }
                }
                KeyCode::Down => {
                    if self.slash_suggestions.select_next() {
                        return Ok(true);
                    }
                }
                KeyCode::Char('k') if key.modifiers.is_empty() => {
                    self.slash_suggestions.select_previous();
                    return Ok(true);
                }
                KeyCode::Char('j') if key.modifiers.is_empty() => {
                    self.slash_suggestions.select_next();
                    return Ok(true);
                }
                KeyCode::Enter | KeyCode::Tab => {
                    if self.apply_selected_suggestion() {
                        return Ok(true);
                    }
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Enter => {
                if !self.input_enabled {
                    return Ok(true);
                }
                let text = self.input.take();
                self.update_input_state();
                self.last_escape = None;
                let _ = events.send(RatatuiEvent::Submit(text));
                self.transcript_autoscroll = true;
                Ok(true)
            }
            KeyCode::Esc => {
                if self.input.value().is_empty() {
                    let now = Instant::now();
                    let double_escape = self
                        .last_escape
                        .map(|last| {
                            now.duration_since(last).as_millis() <= u128::from(ESCAPE_DOUBLE_MS)
                        })
                        .unwrap_or(false);
                    self.last_escape = Some(now);
                    if double_escape {
                        let _ = events.send(RatatuiEvent::Exit);
                        self.should_exit = true;
                    } else {
                        let _ = events.send(RatatuiEvent::Cancel);
                    }
                } else {
                    if self.input_enabled {
                        self.input.clear();
                        self.update_input_state();
                    }
                }
                Ok(true)
            }
            KeyCode::Char('c') | KeyCode::Char('d') | KeyCode::Char('z')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if self.input_enabled {
                    self.input.clear();
                    self.update_input_state();
                }
                match key.code {
                    KeyCode::Char('c') => {
                        let _ = events.send(RatatuiEvent::Interrupt);
                    }
                    KeyCode::Char('d') => {
                        let _ = events.send(RatatuiEvent::Exit);
                        self.should_exit = true;
                    }
                    KeyCode::Char('z') => {
                        let _ = events.send(RatatuiEvent::Cancel);
                    }
                    _ => {}
                }
                Ok(true)
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.transcript_scroll.scroll_to_bottom();
                self.transcript_autoscroll = true;
                self.scroll_focus = ScrollFocus::Transcript;
                Ok(true)
            }
            KeyCode::Char('?') if key.modifiers.is_empty() => {
                if self.input_enabled {
                    self.set_input_text("/help".to_string());
                }
                Ok(true)
            }
            KeyCode::PageUp => {
                let focus = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    ScrollFocus::Pty
                } else {
                    self.scroll_focus
                };
                let handled = self.scroll_page_up_with_focus(focus);
                self.scroll_focus = focus;
                let _ = events.send(RatatuiEvent::ScrollPageUp);
                Ok(handled)
            }
            KeyCode::PageDown => {
                let focus = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    ScrollFocus::Pty
                } else {
                    self.scroll_focus
                };
                let handled = self.scroll_page_down_with_focus(focus);
                self.scroll_focus = focus;
                let _ = events.send(RatatuiEvent::ScrollPageDown);
                Ok(handled)
            }
            KeyCode::Up => {
                let focus = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    ScrollFocus::Pty
                } else {
                    self.scroll_focus
                };
                let handled = self.scroll_line_up_with_focus(focus);
                self.scroll_focus = focus;
                let _ = events.send(RatatuiEvent::ScrollLineUp);
                Ok(handled)
            }
            KeyCode::Down => {
                let focus = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    ScrollFocus::Pty
                } else {
                    self.scroll_focus
                };
                let handled = self.scroll_line_down_with_focus(focus);
                self.scroll_focus = focus;
                let _ = events.send(RatatuiEvent::ScrollLineDown);
                Ok(handled)
            }
            KeyCode::Backspace => {
                if !self.input_enabled {
                    return Ok(true);
                }
                self.input.backspace();
                self.update_input_state();
                self.transcript_autoscroll = true;
                Ok(true)
            }
            KeyCode::Delete => {
                if !self.input_enabled {
                    return Ok(true);
                }
                self.input.delete();
                self.update_input_state();
                self.transcript_autoscroll = true;
                Ok(true)
            }
            KeyCode::Left => {
                if !self.input_enabled {
                    return Ok(true);
                }
                self.input.move_left();
                Ok(true)
            }
            KeyCode::Right => {
                if !self.input_enabled {
                    return Ok(true);
                }
                self.input.move_right();
                Ok(true)
            }
            KeyCode::Home => {
                if !self.input_enabled {
                    return Ok(true);
                }
                self.input.move_home();
                Ok(true)
            }
            KeyCode::End => {
                if !self.input_enabled {
                    return Ok(true);
                }
                self.input.move_end();
                Ok(true)
            }
            KeyCode::Char(ch) => {
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    return Ok(false);
                }
                if !self.input_enabled {
                    return Ok(true);
                }
                self.input.insert(ch);
                self.update_input_state();
                self.last_escape = None;
                self.transcript_autoscroll = true;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn scroll_state_mut(&mut self, focus: ScrollFocus) -> &mut TranscriptScrollState {
        match focus {
            ScrollFocus::Transcript => &mut self.transcript_scroll,
            ScrollFocus::Pty => &mut self.pty_scroll,
        }
    }

    fn scroll_with<F>(&mut self, focus: ScrollFocus, mut apply: F) -> bool
    where
        F: FnMut(&mut TranscriptScrollState),
    {
        let state = self.scroll_state_mut(focus);
        let before = state.offset();
        apply(state);
        let changed = state.offset() != before;
        if changed {
            match focus {
                ScrollFocus::Transcript => self.transcript_autoscroll = false,
                ScrollFocus::Pty => self.pty_autoscroll = false,
            }
            self.scroll_focus = focus;
        }
        changed
    }

    fn alternate_focus(focus: ScrollFocus) -> ScrollFocus {
        match focus {
            ScrollFocus::Transcript => ScrollFocus::Pty,
            ScrollFocus::Pty => ScrollFocus::Transcript,
        }
    }

    fn scroll_line_up_with_focus(&mut self, focus: ScrollFocus) -> bool {
        if self.scroll_with(focus, |state| state.scroll_up()) {
            return true;
        }
        let alternate = Self::alternate_focus(focus);
        self.scroll_with(alternate, |state| state.scroll_up())
    }

    fn scroll_line_down_with_focus(&mut self, focus: ScrollFocus) -> bool {
        if self.scroll_with(focus, |state| state.scroll_down()) {
            return true;
        }
        let alternate = Self::alternate_focus(focus);
        self.scroll_with(alternate, |state| state.scroll_down())
    }

    fn scroll_page_up_with_focus(&mut self, focus: ScrollFocus) -> bool {
        if self.scroll_with(focus, |state| state.scroll_page_up()) {
            return true;
        }
        let alternate = Self::alternate_focus(focus);
        self.scroll_with(alternate, |state| state.scroll_page_up())
    }

    fn scroll_page_down_with_focus(&mut self, focus: ScrollFocus) -> bool {
        if self.scroll_with(focus, |state| state.scroll_page_down()) {
            return true;
        }
        let alternate = Self::alternate_focus(focus);
        self.scroll_with(alternate, |state| state.scroll_page_down())
    }

    fn highlight_transcript(
        &self,
        lines: Vec<Line<'static>>,
        _offset: usize,
    ) -> Vec<Line<'static>> {
        let Some((start, end)) = self.selection.range() else {
            return lines;
        };
        let highlight_color = self
            .theme
            .secondary
            .or(self.theme.primary)
            .unwrap_or(Color::DarkGray);
        let highlight_style = Style::default().bg(highlight_color);

        lines
            .into_iter()
            .enumerate()
            .map(|(index, mut line)| {
                if index >= start && index <= end {
                    if line.spans.is_empty() {
                        line.spans
                            .push(Span::styled(" ".to_string(), highlight_style));
                    } else {
                        line.spans = line
                            .spans
                            .into_iter()
                            .map(|mut span| {
                                span.style = span.style.patch(highlight_style);
                                span
                            })
                            .collect();
                    }
                }
                line
            })
            .collect()
    }

    fn is_in_transcript_area(&self, column: u16, row: u16) -> bool {
        self.transcript_area
            .map(|area| {
                let within_x = column >= area.x && column < area.x.saturating_add(area.width);
                let within_y = row >= area.y && row < area.y.saturating_add(area.height);
                within_x && within_y
            })
            .unwrap_or(false)
    }

    fn transcript_line_index_at(&self, column: u16, row: u16) -> Option<usize> {
        let area = self.transcript_area?;
        if column < area.x || column >= area.x.saturating_add(area.width) {
            return None;
        }
        if row < area.y || row >= area.y.saturating_add(area.height) {
            return None;
        }
        let relative = usize::from(row.saturating_sub(area.y));
        let index = self.transcript_scroll.offset().saturating_add(relative);
        let content = self.transcript_scroll.content_height();
        if content == 0 {
            Some(0)
        } else if index >= content {
            Some(content.saturating_sub(1))
        } else {
            Some(index)
        }
    }

    fn update_pty_area(&mut self, text_area: Rect) {
        let Some(placement) = self.pty_block else {
            self.pty_area = None;
            return;
        };
        if placement.height == 0 || text_area.height == 0 {
            self.pty_area = None;
            return;
        }

        let offset = self.transcript_scroll.offset();
        let viewport = self.transcript_scroll.viewport_height();
        let start = placement.top;
        let end = placement.top + placement.height;
        let view_start = offset;
        let view_end = offset + viewport;
        if end <= view_start || start >= view_end {
            self.pty_area = None;
            return;
        }

        let visible_start = start.max(view_start) - view_start;
        let visible_end = end.min(view_end) - view_start;
        if visible_end <= visible_start {
            self.pty_area = None;
            return;
        }

        let indent = placement.indent.min(text_area.width as usize) as u16;
        let x = text_area.x.saturating_add(indent);
        let width = text_area.width.saturating_sub(indent);
        let y = text_area.y + visible_start as u16;
        let height = (visible_end - visible_start) as u16;
        if width == 0 || height == 0 {
            self.pty_area = None;
            return;
        }

        self.pty_area = Some(Rect::new(x, y, width, height));
    }

    fn is_in_pty_area(&self, column: u16, row: u16) -> bool {
        self.pty_area
            .map(|area| {
                let within_x = column >= area.x && column < area.x.saturating_add(area.width);
                let within_y = row >= area.y && row < area.y.saturating_add(area.height);
                within_x && within_y
            })
            .unwrap_or(false)
    }

    fn handle_mouse_event(
        &mut self,
        mouse: MouseEvent,
        events: &UnboundedSender<RatatuiEvent>,
    ) -> Result<bool> {
        let in_transcript = self.is_in_transcript_area(mouse.column, mouse.row);
        let in_pty = self.is_in_pty_area(mouse.column, mouse.row);
        let focus = if in_pty {
            Some(ScrollFocus::Pty)
        } else if in_transcript {
            Some(ScrollFocus::Transcript)
        } else {
            None
        };

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(line) = self.transcript_line_index_at(mouse.column, mouse.row) {
                    self.selection.begin(line);
                    self.transcript_autoscroll = false;
                    if focus == Some(ScrollFocus::Pty) {
                        self.pty_autoscroll = false;
                    }
                    if let Some(target) = focus {
                        self.scroll_focus = target;
                    }
                    return Ok(true);
                } else {
                    self.selection.clear();
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.selection.is_active() {
                    if let Some(line) = self.transcript_line_index_at(mouse.column, mouse.row) {
                        self.selection.update(line);
                        return Ok(true);
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.selection.is_dragging() {
                    self.selection.finish();
                    return Ok(true);
                }
            }
            _ => {}
        }

        let Some(target) = focus else {
            return Ok(false);
        };

        self.scroll_focus = target;

        let handled = match mouse.kind {
            MouseEventKind::ScrollUp => {
                let scrolled = self.scroll_line_up_with_focus(target);
                if scrolled {
                    let _ = events.send(RatatuiEvent::ScrollLineUp);
                }
                scrolled
            }
            MouseEventKind::ScrollDown => {
                let scrolled = self.scroll_line_down_with_focus(target);
                if scrolled {
                    let _ = events.send(RatatuiEvent::ScrollLineDown);
                }
                scrolled
            }
            _ => false,
        };

        Ok(handled)
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        if area.width == 0 || area.height == 0 {
            return;
        }

        let (body_area, status_area) = if area.height > 1 {
            let segments = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(area);
            (segments[0], Some(segments[1]))
        } else {
            (area, None)
        };

        let content_area = body_area;

        let (message_area, input_layout) = if content_area.height == 0 {
            (
                Rect::new(content_area.x, content_area.y, content_area.width, 0),
                None,
            )
        } else {
            let inner_width = content_area.width.saturating_sub(2);
            let display = self.build_input_display(inner_width);
            let mut block_height = display.height.saturating_add(2);
            if block_height < 3 {
                block_height = 3;
            }
            let available_for_suggestions = content_area.height.saturating_sub(block_height);
            let suggestion_height = self
                .slash_suggestions
                .visible_height(available_for_suggestions);
            let input_total_height = block_height
                .saturating_add(suggestion_height)
                .min(content_area.height);
            let message_height = content_area.height.saturating_sub(input_total_height);
            let message_area = Rect::new(
                content_area.x,
                content_area.y,
                content_area.width,
                message_height,
            );
            let input_y = content_area.y.saturating_add(message_height);
            let input_container = Rect::new(
                content_area.x,
                input_y,
                content_area.width,
                input_total_height,
            );
            let block_area_height = block_height.min(input_container.height);
            let block_area = Rect::new(
                input_container.x,
                input_container.y,
                input_container.width,
                block_area_height,
            );
            let suggestion_area =
                if suggestion_height > 0 && input_container.height > block_area_height {
                    Some(Rect::new(
                        input_container.x,
                        input_container.y + block_area_height,
                        input_container.width,
                        input_container.height.saturating_sub(block_area_height),
                    ))
                } else {
                    None
                };
            (
                message_area,
                Some(InputLayout {
                    block_area,
                    suggestion_area,
                    display,
                }),
            )
        };

        let foreground_style = self
            .theme
            .foreground
            .map(|fg| Style::default().fg(fg))
            .unwrap_or_default();

        let mut scrollbar_area = None;

        if message_area.width > 0 && message_area.height > 0 {
            let viewport_height = usize::from(message_area.height);
            let mut display = self.build_display(message_area.width);
            self.transcript_scroll
                .update_bounds(display.total_height, viewport_height);
            if !self.transcript_scroll.is_at_bottom() {
                self.transcript_autoscroll = false;
            }
            if self.transcript_autoscroll {
                self.transcript_scroll.scroll_to_bottom();
                self.transcript_autoscroll = false;
            }

            let mut needs_scrollbar =
                self.transcript_scroll.has_overflow() && message_area.width > 1;
            let text_area = if needs_scrollbar {
                let adjusted_width = message_area.width.saturating_sub(1);
                display = self.build_display(adjusted_width);
                self.transcript_scroll
                    .update_bounds(display.total_height, viewport_height);
                if !self.transcript_scroll.is_at_bottom() {
                    self.transcript_autoscroll = false;
                }
                if self.transcript_autoscroll {
                    self.transcript_scroll.scroll_to_bottom();
                    self.transcript_autoscroll = false;
                }
                needs_scrollbar = self.transcript_scroll.has_overflow();
                if needs_scrollbar {
                    let segments = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Length(adjusted_width), Constraint::Length(1)])
                        .split(message_area);
                    scrollbar_area = Some(segments[1]);
                    segments[0]
                } else {
                    message_area
                }
            } else {
                message_area
            };

            self.transcript_area = Some(text_area);

            let offset = self.transcript_scroll.offset();
            let highlighted = self.highlight_transcript(display.lines.clone(), offset);
            let mut paragraph = Paragraph::new(highlighted).alignment(Alignment::Left);
            if offset > 0 {
                paragraph = paragraph.scroll((offset as u16, 0));
            }
            paragraph = paragraph.style(foreground_style);
            frame.render_widget(paragraph, text_area);
            self.update_pty_area(text_area);

            if let Some(scroll_area) = scrollbar_area {
                if self.transcript_scroll.has_overflow() && scroll_area.width > 0 {
                    let mut scrollbar_state =
                        ScrollbarState::new(self.transcript_scroll.content_height())
                            .viewport_content_length(self.transcript_scroll.viewport_height())
                            .position(self.transcript_scroll.offset());
                    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
                    frame.render_stateful_widget(scrollbar, scroll_area, &mut scrollbar_state);
                }
            }
        } else {
            self.transcript_scroll.update_bounds(0, 0);
            self.transcript_area = Some(message_area);
        }

        if let Some(layout) = input_layout {
            let InputLayout {
                block_area,
                suggestion_area,
                display,
            } = layout;
            if block_area.width > 2 && block_area.height >= 3 {
                let accent = self
                    .theme
                    .secondary
                    .or(self.theme.foreground)
                    .unwrap_or(Color::DarkGray);
                let line_style = Style::default().fg(accent).add_modifier(Modifier::DIM);
                let horizontal = "─".repeat(block_area.width as usize);

                let top_area = Rect::new(block_area.x, block_area.y, block_area.width, 1);
                let bottom_y = block_area.y + block_area.height.saturating_sub(1);
                let bottom_area = Rect::new(block_area.x, bottom_y, block_area.width, 1);
                let top_line = Paragraph::new(Line::from(vec![Span::styled(
                    horizontal.clone(),
                    line_style,
                )]));
                frame.render_widget(top_line, top_area);
                let bottom_line =
                    Paragraph::new(Line::from(vec![Span::styled(horizontal, line_style)]));
                frame.render_widget(bottom_line, bottom_area);

                let input_height = block_area.height.saturating_sub(2);
                if input_height > 0 {
                    let input_area = Rect::new(
                        block_area.x,
                        block_area.y + 1,
                        block_area.width,
                        input_height,
                    );
                    let paragraph = Paragraph::new(display.lines.clone())
                        .wrap(Wrap { trim: false })
                        .style(foreground_style);
                    frame.render_widget(paragraph, input_area);

                    if let Some(area) = suggestion_area {
                        if area.width > 0 && area.height > 0 {
                            self.render_slash_suggestions(frame, area);
                        }
                    } else if input_area.width > 0 && input_area.height > 0 {
                        self.render_slash_suggestions(frame, input_area);
                    }

                    if self.cursor_visible {
                        if let Some((row, col)) = display.cursor {
                            if row < input_area.height && col < input_area.width {
                                let cursor_x = input_area.x + col;
                                let cursor_y = input_area.y + row;
                                frame.set_cursor_position((cursor_x, cursor_y));
                            }
                        }
                    }
                }
            }
        }

        if let Some(status_area) = status_area {
            if status_area.width > 0 {
                let left_text = self.status_bar.left.clone();
                let center_text = self.status_bar.center.clone();
                let right_text = self.status_bar.right.clone();

                let mut left_len = UnicodeWidthStr::width(left_text.as_str()) as u16;
                let mut right_len = UnicodeWidthStr::width(right_text.as_str()) as u16;
                if left_len > status_area.width {
                    left_len = status_area.width;
                }
                if right_len > status_area.width.saturating_sub(left_len) {
                    right_len = status_area.width.saturating_sub(left_len);
                }
                let center_len = status_area.width.saturating_sub(left_len + right_len);
                let sections = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(left_len),
                        Constraint::Length(center_len),
                        Constraint::Length(right_len),
                    ])
                    .split(status_area);

                let mut status_style = Style::default()
                    .fg(self.theme.foreground.unwrap_or(Color::Gray))
                    .add_modifier(Modifier::DIM);
                if let Some(background) = self.theme.background {
                    status_style = status_style.bg(background);
                }

                if let Some(area) = sections.get(0) {
                    if area.width > 0 {
                        let left = Paragraph::new(Line::from(left_text.clone()))
                            .alignment(Alignment::Left)
                            .style(status_style);
                        frame.render_widget(left, *area);
                    }
                }
                if let Some(area) = sections.get(1) {
                    if area.width > 0 {
                        let center = Paragraph::new(Line::from(center_text.clone()))
                            .alignment(Alignment::Center)
                            .style(status_style);
                        frame.render_widget(center, *area);
                    }
                }
                if let Some(area) = sections.get(2) {
                    if area.width > 0 {
                        let right = Paragraph::new(Line::from(right_text.clone()))
                            .alignment(Alignment::Right)
                            .style(status_style);
                        frame.render_widget(right, *area);
                    }
                }
            }
        }

        if self.pty_block.is_none() {
            self.pty_area = None;
            self.pty_scroll.update_bounds(0, 0);
        }
    }

    fn build_display(&mut self, width: u16) -> TranscriptDisplay {
        if width == 0 {
            return TranscriptDisplay {
                lines: Vec::new(),
                total_height: 0,
            };
        }

        self.pty_block = None;
        let mut lines = Vec::new();
        let mut total_height = 0usize;
        let width_usize = width as usize;
        let indent_width = MESSAGE_INDENT.min(width_usize);
        let mut first_rendered = true;

        self.pty_block = None;

        for index in 0..self.messages.len() {
            let kind = self.messages[index].kind;
            if !self.block_has_visible_content(&self.messages[index]) {
                continue;
            }

            let mut placement = None;
            let mut block_lines = if kind == RatatuiMessageKind::Pty {
                if let Some(lines) = self.build_pty_panel_lines(width_usize, indent_width) {
                    placement = Some(PtyPlacement {
                        top: 0,
                        height: lines.len(),
                        indent: indent_width,
                    });
                    lines
                } else {
                    Vec::new()
                }
            } else {
                let block = &self.messages[index];
                match kind {
                    RatatuiMessageKind::User => self.build_user_block(block, width_usize),
                    RatatuiMessageKind::Info => {
                        self.build_panel_block(block, width_usize, self.kind_color(kind))
                    }
                    RatatuiMessageKind::Policy => {
                        self.build_panel_block(block, width_usize, self.kind_color(kind))
                    }
                    _ => self.build_response_block(block, width_usize, kind),
                }
            };

            if block_lines.is_empty() {
                continue;
            }

            if !first_rendered {
                lines.push(Line::default());
                total_height += 1;
            }

            let block_top = total_height;
            total_height += block_lines.len();
            lines.append(&mut block_lines);

            if let Some(mut placement) = placement {
                placement.top = block_top;
                placement.height = total_height.saturating_sub(block_top);
                self.pty_block = Some(placement);
            }

            first_rendered = false;
        }

        if !lines.is_empty() {
            lines.push(Line::default());
            total_height += 1;
        }

        TranscriptDisplay {
            lines,
            total_height,
        }
    }

    fn build_input_display(&self, width: u16) -> InputDisplay {
        if width == 0 {
            return InputDisplay {
                lines: vec![Line::default()],
                cursor: None,
                height: 1,
            };
        }

        let width_usize = width as usize;
        let mut lines = self.wrap_segments(
            &self.prompt_segments(),
            width_usize,
            0,
            self.theme.foreground,
        );
        if lines.is_empty() {
            lines.push(Line::default());
        }

        let prefix_width = UnicodeWidthStr::width(self.prompt_prefix.as_str());
        let input_width = if self.show_placeholder {
            0
        } else {
            self.input.width_before_cursor()
        };
        let placeholder_width = if self.show_placeholder {
            self.placeholder_hint
                .as_deref()
                .map(UnicodeWidthStr::width)
                .unwrap_or(0)
        } else {
            0
        };
        let cursor_width = prefix_width + input_width + placeholder_width;
        let line_width = width_usize.max(1);
        let cursor_row = (cursor_width / line_width) as u16;
        let cursor_col = (cursor_width % line_width) as u16;
        let height = lines.len().max(1) as u16;

        InputDisplay {
            lines,
            cursor: Some((cursor_row, cursor_col)),
            height,
        }
    }

    fn block_has_visible_content(&self, block: &MessageBlock) -> bool {
        match block.kind {
            RatatuiMessageKind::Pty | RatatuiMessageKind::Tool | RatatuiMessageKind::Agent => {
                block.lines.iter().any(StyledLine::has_visible_content)
            }
            _ => true,
        }
    }

    fn build_user_block(&self, block: &MessageBlock, width: usize) -> Vec<Line<'static>> {
        let mut prefix_style = RatatuiTextStyle::default();
        prefix_style.color = Some(self.kind_color(RatatuiMessageKind::User));
        prefix_style.bold = true;
        self.build_prefixed_block(block, width, "> ", prefix_style, self.theme.foreground)
    }

    fn build_response_block(
        &self,
        block: &MessageBlock,
        width: usize,
        kind: RatatuiMessageKind,
    ) -> Vec<Line<'static>> {
        let marker = match kind {
            RatatuiMessageKind::Agent | RatatuiMessageKind::Tool => "✦",
            RatatuiMessageKind::Error => "!",
            RatatuiMessageKind::Policy => "ⓘ",
            RatatuiMessageKind::User => "❯",
            _ => "✻",
        };
        let prefix = format!("{}{} ", " ".repeat(MESSAGE_INDENT), marker);
        let mut style = RatatuiTextStyle::default();
        style.color = Some(self.kind_color(kind));
        if matches!(kind, RatatuiMessageKind::Agent | RatatuiMessageKind::Error) {
            style.bold = true;
        }
        self.build_prefixed_block(block, width, &prefix, style, self.theme.foreground)
    }

    fn build_panel_block(
        &self,
        block: &MessageBlock,
        width: usize,
        accent: Color,
    ) -> Vec<Line<'static>> {
        if width < 4 {
            let mut fallback = Vec::new();
            for line in &block.lines {
                let wrapped = self.wrap_segments(&line.segments, width, 0, self.theme.foreground);
                fallback.extend(wrapped);
            }
            return fallback;
        }

        let border_style = Style::default().fg(accent);
        let horizontal = "─".repeat(width.saturating_sub(2));
        let mut rendered = Vec::new();
        rendered.push(Line::from(vec![Span::styled(
            format!("╭{}╮", horizontal),
            border_style,
        )]));

        let content_width = width.saturating_sub(4);
        let mut emitted = false;
        for line in &block.lines {
            let wrapped =
                self.wrap_segments(&line.segments, content_width, 0, self.theme.foreground);
            if wrapped.is_empty() {
                let mut spans = Vec::new();
                spans.push(Span::styled("│ ", border_style));
                spans.push(Span::raw(" ".repeat(content_width)));
                spans.push(Span::styled(" │", border_style));
                rendered.push(Line::from(spans));
                continue;
            }

            for wrapped_line in wrapped {
                emitted = true;
                let mut spans = Vec::new();
                spans.push(Span::styled("│ ", border_style));
                let mut content_spans = wrapped_line.spans.clone();
                let mut occupied = 0usize;
                for span in &content_spans {
                    occupied += UnicodeWidthStr::width(span.content.as_ref());
                }
                if occupied < content_width {
                    content_spans.push(Span::raw(" ".repeat(content_width - occupied)));
                }
                spans.extend(content_spans);
                spans.push(Span::styled(" │", border_style));
                rendered.push(Line::from(spans));
            }
        }

        if !emitted {
            let mut spans = Vec::new();
            spans.push(Span::styled("│ ", border_style));
            spans.push(Span::raw(" ".repeat(content_width)));
            spans.push(Span::styled(" │", border_style));
            rendered.push(Line::from(spans));
        }

        rendered.push(Line::from(vec![Span::styled(
            format!("╰{}╯", horizontal),
            border_style,
        )]));
        rendered
    }

    fn build_prefixed_block(
        &self,
        block: &MessageBlock,
        width: usize,
        prefix: &str,
        prefix_style: RatatuiTextStyle,
        fallback: Option<Color>,
    ) -> Vec<Line<'static>> {
        if width == 0 {
            return Vec::new();
        }
        let prefix_width = UnicodeWidthStr::width(prefix);
        if prefix_width >= width {
            let mut lines = Vec::new();
            for line in &block.lines {
                let mut spans = Vec::new();
                spans.push(Span::styled(
                    prefix.to_string(),
                    prefix_style.to_style(fallback),
                ));
                lines.push(Line::from(spans));
                let wrapped = self.wrap_segments(&line.segments, width, 0, fallback);
                lines.extend(wrapped);
            }
            if lines.is_empty() {
                lines.push(Line::from(vec![Span::styled(
                    prefix.to_string(),
                    prefix_style.to_style(fallback),
                )]));
            }
            return lines;
        }

        let content_width = width - prefix_width;
        let continuation = " ".repeat(prefix_width);
        let mut rendered = Vec::new();
        let mut first = true;
        for line in &block.lines {
            let wrapped = self.wrap_segments(&line.segments, content_width, 0, fallback);
            if wrapped.is_empty() {
                let prefix_text = if first { prefix } else { continuation.as_str() };
                rendered.push(Line::from(vec![Span::styled(
                    prefix_text.to_string(),
                    prefix_style.to_style(fallback),
                )]));
                first = false;
                continue;
            }

            for (index, wrapped_line) in wrapped.into_iter().enumerate() {
                let prefix_text = if first && index == 0 {
                    prefix
                } else {
                    continuation.as_str()
                };
                let mut spans = Vec::new();
                spans.push(Span::styled(
                    prefix_text.to_string(),
                    prefix_style.to_style(fallback),
                ));
                spans.extend(wrapped_line.spans);
                rendered.push(Line::from(spans));
            }
            first = false;
        }

        if rendered.is_empty() {
            rendered.push(Line::from(vec![Span::styled(
                prefix.to_string(),
                prefix_style.to_style(fallback),
            )]));
        }

        rendered
    }

    fn prompt_segments(&self) -> Vec<RatatuiSegment> {
        let mut segments = Vec::new();
        segments.push(RatatuiSegment {
            text: self.prompt_prefix.clone(),
            style: self.prompt_style.clone(),
        });

        if self.show_placeholder {
            if let Some(hint) = &self.placeholder_hint {
                segments.push(RatatuiSegment {
                    text: hint.clone(),
                    style: self.placeholder_style.clone(),
                });
            }
        } else {
            segments.push(RatatuiSegment {
                text: self.input.value().to_string(),
                style: RatatuiTextStyle::default(),
            });
        }

        segments
    }

    fn wrap_segments(
        &self,
        segments: &[RatatuiSegment],
        width: usize,
        indent: usize,
        fallback: Option<Color>,
    ) -> Vec<Line<'static>> {
        if width == 0 {
            return vec![Line::default()];
        }

        let mut lines = Vec::new();
        let indent_width = indent.min(width);
        let indent_text = " ".repeat(indent_width);
        let mut current = Vec::new();
        let mut current_width = indent_width;

        if indent_width > 0 {
            current.push(Span::raw(indent_text.clone()));
        }

        for segment in segments {
            let style = segment.style.to_style(fallback);
            let mut buffer = String::new();
            let mut buffer_width = 0usize;

            for ch in segment.text.chars() {
                if ch == '\n' {
                    if !buffer.is_empty() {
                        current.push(Span::styled(buffer.clone(), style));
                        buffer.clear();
                        buffer_width = 0;
                    }
                    lines.push(Line::from(current));
                    current = Vec::new();
                    if indent_width > 0 {
                        current.push(Span::raw(indent_text.clone()));
                    }
                    current_width = indent_width;
                    continue;
                }

                let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
                if ch_width == 0 {
                    buffer.push(ch);
                    continue;
                }

                if current_width + buffer_width + ch_width > width
                    && current_width + buffer_width > indent_width
                {
                    if !buffer.is_empty() {
                        current.push(Span::styled(buffer.clone(), style));
                        buffer.clear();
                        buffer_width = 0;
                    }
                    lines.push(Line::from(current));
                    current = Vec::new();
                    if indent_width > 0 {
                        current.push(Span::raw(indent_text.clone()));
                    }
                    current_width = indent_width;
                }

                buffer.push(ch);
                buffer_width += ch_width;
            }

            if !buffer.is_empty() {
                current.push(Span::styled(buffer.clone(), style));
                current_width += buffer_width;
                buffer.clear();
            }
        }

        if current.is_empty() {
            if indent_width > 0 {
                current.push(Span::raw(indent_text));
            }
        }

        lines.push(Line::from(current));
        lines
    }

    fn build_pty_panel_lines(&mut self, width: usize, indent: usize) -> Option<Vec<Line<'static>>> {
        let Some(panel) = self.pty_panel.as_mut() else {
            self.pty_scroll.update_bounds(0, 0);
            return None;
        };
        if !panel.has_content() {
            self.pty_scroll.update_bounds(0, 0);
            return None;
        }
        if width <= indent + 2 {
            self.pty_scroll.update_bounds(0, 0);
            return None;
        }
        let available = width - indent;
        if available < 3 {
            self.pty_scroll.update_bounds(0, 0);
            return None;
        }
        let inner_width = available.saturating_sub(2);
        if inner_width == 0 {
            self.pty_scroll.update_bounds(0, 0);
            return None;
        }

        let title = panel.block_title_text();
        let text = panel.view_text();
        let mut wrapped = self.wrap_pty_text(&text, inner_width);
        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }

        let total_content = wrapped.len().max(1);
        let viewport = cmp::min(total_content, cmp::max(PTY_CONTENT_VIEW_LINES, 1));
        self.pty_scroll.update_bounds(total_content, viewport);
        if self.pty_autoscroll {
            self.pty_scroll.scroll_to_bottom();
            self.pty_autoscroll = false;
        }

        let offset = self.pty_scroll.offset();
        let mut visible: Vec<Line<'static>> =
            wrapped.into_iter().skip(offset).take(viewport).collect();
        while visible.len() < viewport {
            visible.push(Line::default());
        }

        let indent_text = " ".repeat(indent);
        let border_color = self
            .theme
            .secondary
            .or(self.theme.primary)
            .unwrap_or(Color::LightCyan);
        let border_style = Style::default().fg(border_color);
        let content_style = Style::default().fg(self.theme.foreground.unwrap_or(Color::Gray));
        let mut block_lines = Vec::new();
        block_lines.push(self.build_pty_top_line(&indent_text, inner_width, &title, border_style));

        for mut line in visible {
            let mut spans = Vec::new();
            spans.push(Span::raw(indent_text.clone()));
            spans.push(Span::styled("│".to_string(), border_style));
            let width_used = Self::line_display_width(&line);
            spans.append(&mut line.spans);
            if width_used < inner_width {
                spans.push(Span::styled(
                    " ".repeat(inner_width - width_used),
                    content_style,
                ));
            }
            spans.push(Span::styled("│".to_string(), border_style));
            block_lines.push(Line::from(spans));
        }

        block_lines.push(self.build_pty_bottom_line(&indent_text, inner_width, border_style));
        Some(block_lines)
    }

    fn wrap_pty_text(&self, text: &Text<'static>, inner_width: usize) -> Vec<Line<'static>> {
        if inner_width == 0 {
            return vec![Line::default()];
        }
        if text.lines.is_empty() {
            return vec![Line::default()];
        }

        let mut wrapped = Vec::new();
        for raw in &text.lines {
            let segments: Vec<RatatuiSegment> = raw
                .spans
                .iter()
                .map(|span| RatatuiSegment {
                    text: span.content.to_string(),
                    style: Self::style_to_text_style(span.style),
                })
                .collect();
            let mut lines = self.wrap_segments(&segments, inner_width, 0, self.theme.foreground);
            if lines.is_empty() {
                lines.push(Line::default());
            }
            wrapped.append(&mut lines);
        }

        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }
        wrapped
    }

    fn build_pty_top_line(
        &self,
        indent_text: &str,
        inner_width: usize,
        title: &str,
        style: Style,
    ) -> Line<'static> {
        let mut segments = Vec::new();
        segments.push(Span::raw(indent_text.to_string()));
        segments.push(Span::styled("╭".to_string(), style));
        segments.push(Span::styled(
            self.compose_pty_title_bar(inner_width, title),
            style,
        ));
        segments.push(Span::styled("╮".to_string(), style));
        Line::from(segments)
    }

    fn build_pty_bottom_line(
        &self,
        indent_text: &str,
        inner_width: usize,
        style: Style,
    ) -> Line<'static> {
        Line::from(vec![
            Span::raw(indent_text.to_string()),
            Span::styled("╰".to_string(), style),
            Span::styled("─".repeat(inner_width), style),
            Span::styled("╯".to_string(), style),
        ])
    }

    fn compose_pty_title_bar(&self, inner_width: usize, title: &str) -> String {
        if inner_width == 0 {
            return String::new();
        }
        let trimmed = title.trim();
        if trimmed.is_empty() || inner_width < 2 {
            return "─".repeat(inner_width);
        }
        let available = inner_width.saturating_sub(2);
        if available == 0 {
            return "─".repeat(inner_width);
        }
        let truncated = Self::truncate_to_width(trimmed, available);
        let decorated = format!(" {} ", truncated);
        let decorated_width = UnicodeWidthStr::width(decorated.as_str()).min(inner_width);
        let remaining = inner_width.saturating_sub(decorated_width);
        let left = remaining / 2;
        let right = remaining - left;
        format!("{}{}{}", "─".repeat(left), decorated, "─".repeat(right),)
    }

    fn truncate_to_width(text: &str, max_width: usize) -> String {
        if max_width == 0 {
            return String::new();
        }
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        if UnicodeWidthStr::width(trimmed) <= max_width {
            return trimmed.to_string();
        }
        let mut result = String::new();
        let mut width_used = 0usize;
        let limit = max_width.saturating_sub(1);
        for ch in trimmed.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if ch_width == 0 {
                continue;
            }
            if width_used + ch_width > limit {
                break;
            }
            result.push(ch);
            width_used += ch_width;
        }
        if result.is_empty() {
            "…".to_string()
        } else {
            result.push('…');
            result
        }
    }

    fn line_display_width(line: &Line<'_>) -> usize {
        line.spans
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum()
    }

    fn style_to_text_style(style: Style) -> RatatuiTextStyle {
        let mut text_style = RatatuiTextStyle::default();
        text_style.color = style.fg;
        if style.add_modifier.contains(Modifier::BOLD) {
            text_style.bold = true;
        }
        if style.add_modifier.contains(Modifier::ITALIC) {
            text_style.italic = true;
        }
        text_style
    }

    fn kind_color(&self, kind: RatatuiMessageKind) -> Color {
        match kind {
            RatatuiMessageKind::Agent => self.theme.primary.unwrap_or(Color::LightCyan),
            RatatuiMessageKind::User => self.theme.secondary.unwrap_or(Color::LightGreen),
            RatatuiMessageKind::Tool => self.theme.foreground.unwrap_or(Color::LightMagenta),
            RatatuiMessageKind::Pty => self.theme.primary.unwrap_or(Color::LightBlue),
            RatatuiMessageKind::Info => self.theme.foreground.unwrap_or(Color::Yellow),
            RatatuiMessageKind::Policy => self.theme.secondary.unwrap_or(Color::LightYellow),
            RatatuiMessageKind::Error => Color::LightRed,
        }
    }
}

fn convert_ansi_color(color: AnsiColorEnum) -> Option<Color> {
    match color {
        AnsiColorEnum::Ansi(ansi) => Some(match ansi {
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
        }),
        AnsiColorEnum::Ansi256(value) => Some(Color::Indexed(value.0)),
        AnsiColorEnum::Rgb(rgb) => Some(Color::Rgb(rgb.0, rgb.1, rgb.2)),
    }
}

fn convert_style_color(style: &AnsiStyle) -> Option<Color> {
    style.get_fg_color().and_then(convert_ansi_color)
}

pub fn convert_style(style: AnsiStyle) -> RatatuiTextStyle {
    let mut converted = RatatuiTextStyle::default();
    converted.color = convert_style_color(&style);
    let effects = style.get_effects();
    converted.bold = effects.contains(Effects::BOLD);
    converted.italic = effects.contains(Effects::ITALIC);
    converted
}

pub fn parse_tui_color(input: &str) -> Option<Color> {
    let deserializer = StrDeserializer::<DeValueError>::new(input);
    color_to_tui::deserialize(deserializer).ok()
}

pub fn theme_from_styles(styles: &theme::ThemeStyles) -> RatatuiTheme {
    RatatuiTheme {
        background: convert_ansi_color(styles.background),
        foreground: convert_ansi_color(styles.foreground),
        primary: convert_style_color(&styles.primary),
        secondary: convert_style_color(&styles.secondary),
    }
}

fn create_ticker() -> Interval {
    let mut ticker = interval(Duration::from_millis(REDRAW_INTERVAL_MS));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    ticker
}
