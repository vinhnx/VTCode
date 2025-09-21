use crate::ui::theme;
use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects, Style as AnsiStyle};
use anyhow::{Context, Result};
use crossterm::{
    ExecutableCommand, cursor,
    event::{Event as CrosstermEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use ratatui::{
    Frame, Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::io;
use std::mem;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::time::{Interval, MissedTickBehavior, interval};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const ESCAPE_DOUBLE_MS: u64 = 750;
const REDRAW_INTERVAL_MS: u64 = 33;
const MESSAGE_INDENT: usize = 2;

#[derive(Clone, Default)]
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
    },
    SetTheme {
        theme: RatatuiTheme,
    },
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
        let _ = self.sender.send(RatatuiCommand::SetPlaceholder { hint });
    }

    pub fn set_theme(&self, theme: RatatuiTheme) {
        let _ = self.sender.send(RatatuiCommand::SetTheme { theme });
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

struct TerminalGuard;

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode().context("failed to enable raw mode")?;
        let mut stdout = io::stdout();
        stdout
            .execute(cursor::Hide)
            .context("failed to hide cursor")?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = stdout.execute(cursor::Show);
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

    fn max_offset(&self) -> usize {
        if self.content_height <= self.viewport_height {
            0
        } else {
            self.content_height - self.viewport_height
        }
    }
}

#[derive(Clone)]
struct MessageBlock {
    kind: RatatuiMessageKind,
    lines: Vec<StyledLine>,
}

struct TranscriptDisplay {
    lines: Vec<Line<'static>>,
    total_height: usize,
    cursor: Option<(usize, usize)>,
}

struct RatatuiLoop {
    messages: Vec<MessageBlock>,
    current_line: StyledLine,
    current_kind: Option<RatatuiMessageKind>,
    current_active: bool,
    prompt_prefix: String,
    prompt_style: RatatuiTextStyle,
    input: InputState,
    placeholder_hint: Option<String>,
    show_placeholder: bool,
    should_exit: bool,
    theme: RatatuiTheme,
    last_escape: Option<Instant>,
    scroll_state: TranscriptScrollState,
    needs_autoscroll: bool,
}

impl RatatuiLoop {
    fn new(theme: RatatuiTheme, placeholder: Option<String>) -> Self {
        Self {
            messages: Vec::new(),
            current_line: StyledLine::default(),
            current_kind: None,
            current_active: false,
            prompt_prefix: "❯ ".to_string(),
            prompt_style: RatatuiTextStyle::default(),
            input: InputState::default(),
            placeholder_hint: placeholder.clone(),
            show_placeholder: placeholder.is_some(),
            should_exit: false,
            theme,
            last_escape: None,
            scroll_state: TranscriptScrollState::default(),
            needs_autoscroll: true,
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
                let was_active = self.current_active;
                self.flush_current_line(was_active);
                self.push_line(kind, StyledLine { segments });
                self.needs_autoscroll = true;
                true
            }
            RatatuiCommand::Inline { kind, segment } => {
                self.append_inline_segment(kind, segment);
                self.needs_autoscroll = true;
                true
            }
            RatatuiCommand::ReplaceLast { count, kind, lines } => {
                let was_active = self.current_active;
                self.flush_current_line(was_active);
                self.remove_last_lines(count);
                for segments in lines {
                    self.push_line(kind, StyledLine { segments });
                }
                self.needs_autoscroll = true;
                true
            }
            RatatuiCommand::SetPrompt { prefix, style } => {
                self.prompt_prefix = prefix;
                self.prompt_style = style;
                true
            }
            RatatuiCommand::SetPlaceholder { hint } => {
                self.placeholder_hint = hint.clone();
                self.show_placeholder = hint.is_some() && self.input.value().is_empty();
                true
            }
            RatatuiCommand::SetTheme { theme } => {
                self.theme = theme;
                true
            }
            RatatuiCommand::Shutdown => {
                self.should_exit = true;
                true
            }
        }
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

    fn push_line(&mut self, kind: RatatuiMessageKind, line: StyledLine) {
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

    fn handle_event(
        &mut self,
        event: CrosstermEvent,
        events: &UnboundedSender<RatatuiEvent>,
    ) -> Result<bool> {
        match event {
            CrosstermEvent::Key(key) => self.handle_key_event(key, events),
            CrosstermEvent::Resize(_, _) => {
                self.needs_autoscroll = true;
                Ok(true)
            }
            CrosstermEvent::FocusGained
            | CrosstermEvent::FocusLost
            | CrosstermEvent::Mouse(_)
            | CrosstermEvent::Paste(_) => Ok(false),
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

        match key.code {
            KeyCode::Enter => {
                let text = self.input.take();
                self.show_placeholder = self.placeholder_hint.is_some() && text.is_empty();
                self.last_escape = None;
                let _ = events.send(RatatuiEvent::Submit(text));
                self.needs_autoscroll = true;
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
                    self.input.clear();
                    self.show_placeholder = self.placeholder_hint.is_some();
                }
                Ok(true)
            }
            KeyCode::Char('c') | KeyCode::Char('d') | KeyCode::Char('z')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.input.clear();
                self.show_placeholder = self.placeholder_hint.is_some();
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
                self.scroll_state.scroll_to_bottom();
                self.needs_autoscroll = true;
                Ok(true)
            }
            KeyCode::PageUp => {
                let handled = self.scroll_page_up();
                let _ = events.send(RatatuiEvent::ScrollPageUp);
                Ok(handled)
            }
            KeyCode::PageDown => {
                let handled = self.scroll_page_down();
                let _ = events.send(RatatuiEvent::ScrollPageDown);
                Ok(handled)
            }
            KeyCode::Up => {
                let handled = self.scroll_line_up();
                let _ = events.send(RatatuiEvent::ScrollLineUp);
                Ok(handled)
            }
            KeyCode::Down => {
                let handled = self.scroll_line_down();
                let _ = events.send(RatatuiEvent::ScrollLineDown);
                Ok(handled)
            }
            KeyCode::Backspace => {
                self.input.backspace();
                self.show_placeholder =
                    self.placeholder_hint.is_some() && self.input.value().is_empty();
                self.needs_autoscroll = true;
                Ok(true)
            }
            KeyCode::Delete => {
                self.input.delete();
                self.show_placeholder =
                    self.placeholder_hint.is_some() && self.input.value().is_empty();
                self.needs_autoscroll = true;
                Ok(true)
            }
            KeyCode::Left => {
                self.input.move_left();
                Ok(true)
            }
            KeyCode::Right => {
                self.input.move_right();
                Ok(true)
            }
            KeyCode::Home => {
                self.input.move_home();
                Ok(true)
            }
            KeyCode::End => {
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
                self.input.insert(ch);
                self.show_placeholder = false;
                self.last_escape = None;
                self.needs_autoscroll = true;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn scroll_with<F>(&mut self, mut apply: F) -> bool
    where
        F: FnMut(&mut TranscriptScrollState),
    {
        let before = self.scroll_state.offset();
        apply(&mut self.scroll_state);
        let changed = self.scroll_state.offset() != before;
        if changed {
            self.needs_autoscroll = false;
        }
        changed
    }

    fn scroll_line_up(&mut self) -> bool {
        self.scroll_with(|state| state.scroll_up())
    }

    fn scroll_line_down(&mut self) -> bool {
        self.scroll_with(|state| state.scroll_down())
    }

    fn scroll_page_up(&mut self) -> bool {
        self.scroll_with(|state| state.scroll_page_up())
    }

    fn scroll_page_down(&mut self) -> bool {
        self.scroll_with(|state| state.scroll_page_down())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        if area.width == 0 || area.height == 0 {
            return;
        }

        let display = self.build_display(area.width);
        let viewport_height = usize::from(area.height);
        self.scroll_state
            .update_bounds(display.total_height, viewport_height);
        if self.needs_autoscroll {
            self.scroll_state.scroll_to_bottom();
        }

        let offset = self.scroll_state.offset();
        let mut paragraph = Paragraph::new(display.lines.clone()).alignment(Alignment::Left);
        if offset > 0 {
            paragraph = paragraph.scroll((offset as u16, 0));
        }

        let style = self
            .theme
            .foreground
            .map(|fg| Style::default().fg(fg))
            .unwrap_or_default();
        paragraph = paragraph.style(style);

        frame.render_widget(paragraph, area);

        if let Some((row, col)) = display.cursor {
            if row >= offset {
                let visible_row = row - offset;
                if visible_row < viewport_height {
                    let cursor_x = area.x + col as u16;
                    let cursor_y = area.y + visible_row as u16;
                    frame.set_cursor_position((cursor_x, cursor_y));
                }
            }
        }

        self.needs_autoscroll = false;
    }

    fn build_display(&self, width: u16) -> TranscriptDisplay {
        if width == 0 {
            return TranscriptDisplay {
                lines: Vec::new(),
                total_height: 0,
                cursor: None,
            };
        }

        let mut lines = Vec::new();
        let mut total_height = 0usize;
        let width_usize = width as usize;
        let indent_width = MESSAGE_INDENT.min(width_usize);
        let mut cursor = None;

        for (index, block) in self.messages.iter().enumerate() {
            if index > 0 {
                lines.push(Line::default());
                total_height += 1;
            }

            lines.push(self.message_header_line(block.kind));
            total_height += 1;

            for line in &block.lines {
                let wrapped = self.wrap_segments(
                    &line.segments,
                    width_usize,
                    indent_width,
                    Some(self.kind_color(block.kind)),
                );
                total_height += wrapped.len();
                lines.extend(wrapped);
            }
        }

        if !lines.is_empty() {
            lines.push(Line::default());
            total_height += 1;
        }

        let prompt_segments = self.prompt_segments();
        let prompt_lines = self.wrap_segments(
            &prompt_segments,
            width_usize,
            indent_width,
            self.theme.foreground,
        );
        let prompt_start = total_height;
        total_height += prompt_lines.len();
        lines.extend(prompt_lines.clone());

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
        let cursor_width = indent_width + prefix_width + input_width + placeholder_width;
        if width_usize > 0 {
            let line_width = width_usize.max(1);
            let cursor_row = prompt_start + cursor_width / line_width;
            let cursor_col = cursor_width % line_width;
            cursor = Some((cursor_row, cursor_col));
        }

        TranscriptDisplay {
            lines,
            total_height,
            cursor,
        }
    }

    fn prompt_segments(&self) -> Vec<RatatuiSegment> {
        let mut segments = Vec::new();
        segments.push(RatatuiSegment {
            text: self.prompt_prefix.clone(),
            style: self.prompt_style.clone(),
        });

        if self.show_placeholder {
            if let Some(hint) = &self.placeholder_hint {
                let mut style = RatatuiTextStyle::default();
                style.italic = true;
                segments.push(RatatuiSegment {
                    text: hint.clone(),
                    style,
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

    fn message_header_line(&self, kind: RatatuiMessageKind) -> Line<'static> {
        let title = match kind {
            RatatuiMessageKind::Agent => "Agent",
            RatatuiMessageKind::Error => "Error",
            RatatuiMessageKind::Info => "Info",
            RatatuiMessageKind::Policy => "Policy",
            RatatuiMessageKind::Pty => "PTY",
            RatatuiMessageKind::Tool => "Tool",
            RatatuiMessageKind::User => "User",
        };
        let style = Style::default()
            .fg(self.kind_color(kind))
            .add_modifier(Modifier::BOLD);
        Line::from(vec![Span::styled(title.to_string(), style)])
    }

    fn kind_color(&self, kind: RatatuiMessageKind) -> Color {
        match kind {
            RatatuiMessageKind::Agent => self.theme.primary.unwrap_or(Color::LightBlue),
            RatatuiMessageKind::User => self.theme.secondary.unwrap_or(Color::LightGreen),
            RatatuiMessageKind::Tool => Color::LightMagenta,
            RatatuiMessageKind::Pty => Color::LightCyan,
            RatatuiMessageKind::Info => Color::Yellow,
            RatatuiMessageKind::Policy => Color::LightYellow,
            RatatuiMessageKind::Error => Color::Red,
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
