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
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use std::cmp::max;
use std::io;
use std::mem;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::time::{Interval, MissedTickBehavior, interval};
use unicode_width::UnicodeWidthStr;

const ESCAPE_DOUBLE_MS: u64 = 750;
const REDRAW_INTERVAL_MS: u64 = 33;
const BORDER_THICKNESS: u16 = 1;
const MESSAGE_HORIZONTAL_PADDING: u16 = 1;
const MESSAGE_VERTICAL_PADDING: u16 = 0;
const MESSAGE_SPACING: usize = 1;
const TITLE_AGENT: &str = "VT Code";
const TITLE_ERROR: &str = "Error";
const TITLE_INFO: &str = "Info";
const TITLE_POLICY: &str = "Policy";
const TITLE_PROMPT: &str = "Prompt";
const TITLE_PTY: &str = "PTY";
const TITLE_TOOL: &str = "Tool";
const TITLE_USER: &str = "User";

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
        self.cursor = (self.cursor + advance).min(self.value.len());
    }

    fn move_home(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.value.len();
    }

    fn value(&self) -> &str {
        &self.value
    }

    fn take(&mut self) -> String {
        let value = self.value.clone();
        self.clear();
        value
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

struct RenderEntry {
    height: usize,
    kind: EntryKind,
}

impl RenderEntry {
    fn message(
        kind: RatatuiMessageKind,
        lines: Vec<Line<'static>>,
        content_height: usize,
        chrome_height: usize,
    ) -> Self {
        Self {
            height: content_height + chrome_height,
            kind: EntryKind::Message {
                kind,
                lines,
                content_height,
            },
        }
    }

    fn prompt(lines: Vec<Line<'static>>, content_height: usize, chrome_height: usize) -> Self {
        Self {
            height: content_height + chrome_height,
            kind: EntryKind::Prompt {
                lines,
                content_height,
            },
        }
    }

    fn spacer(height: usize) -> Self {
        Self {
            height,
            kind: EntryKind::Spacer,
        }
    }
}

enum EntryKind {
    Message {
        kind: RatatuiMessageKind,
        lines: Vec<Line<'static>>,
        content_height: usize,
    },
    Prompt {
        lines: Vec<Line<'static>>,
        content_height: usize,
    },
    Spacer,
}

struct LayoutEntries {
    entries: Vec<RenderEntry>,
    prompt_inner_width: u16,
    total_height: usize,
}

struct CursorOffsets {
    row: usize,
    col: usize,
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

    fn horizontal_chrome(&self) -> u16 {
        (BORDER_THICKNESS + MESSAGE_HORIZONTAL_PADDING) * 2
    }

    fn vertical_chrome(&self) -> usize {
        usize::from((BORDER_THICKNESS + MESSAGE_VERTICAL_PADDING) * 2)
    }

    fn block_inner_width(&self, width: u16) -> u16 {
        width.saturating_sub(self.horizontal_chrome())
    }

    fn build_layout_entries(&self, viewport_width: u16) -> LayoutEntries {
        let chrome_height = self.vertical_chrome();
        let inner_width = self.block_inner_width(viewport_width);
        let mut entries = Vec::new();
        let mut total_height = 0usize;
        let mut active_attached = false;

        for (index, block) in self.messages.iter().enumerate() {
            if !entries.is_empty() {
                entries.push(RenderEntry::spacer(MESSAGE_SPACING));
                total_height = total_height.saturating_add(MESSAGE_SPACING);
            }

            let include_active = self.current_active
                && self.current_kind == Some(block.kind)
                && index + 1 == self.messages.len();
            if include_active {
                active_attached = true;
            }

            let mut refs: Vec<&StyledLine> = block.lines.iter().collect();
            if include_active {
                refs.push(&self.current_line);
            }

            let content_height = self.message_height_from_refs(&refs, inner_width);
            let lines = self.convert_message_lines(block, include_active);
            let entry = RenderEntry::message(block.kind, lines, content_height, chrome_height);
            total_height = total_height.saturating_add(entry.height);
            entries.push(entry);
        }

        if self.current_active && !active_attached {
            if !entries.is_empty() {
                entries.push(RenderEntry::spacer(MESSAGE_SPACING));
                total_height = total_height.saturating_add(MESSAGE_SPACING);
            }

            if let Some(kind) = self.current_kind {
                let content_height = self.visual_height(&self.current_line, inner_width).max(1);
                let lines = vec![self.convert_line(&self.current_line)];
                let entry = RenderEntry::message(kind, lines, content_height, chrome_height);
                total_height = total_height.saturating_add(entry.height);
                entries.push(entry);
            }
        }

        if !entries.is_empty() {
            entries.push(RenderEntry::spacer(MESSAGE_SPACING));
            total_height = total_height.saturating_add(MESSAGE_SPACING);
        }

        let prompt_lines = vec![Line::from(self.build_input_line())];
        let prompt_content_height = self.input_visual_rows(inner_width).max(1);
        let prompt_entry = RenderEntry::prompt(prompt_lines, prompt_content_height, chrome_height);
        total_height = total_height.saturating_add(prompt_entry.height);
        entries.push(prompt_entry);

        LayoutEntries {
            entries,
            prompt_inner_width: inner_width,
            total_height,
        }
    }

    fn message_height_from_refs(&self, lines: &[&StyledLine], inner_width: u16) -> usize {
        if lines.is_empty() {
            return 1;
        }

        if inner_width == 0 {
            return lines.len().max(1);
        }

        let mut total = 0usize;
        for line in lines {
            total = total.saturating_add(self.visual_height(line, inner_width));
        }
        total.max(1)
    }

    fn convert_message_lines(
        &self,
        block: &MessageBlock,
        include_active: bool,
    ) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for line in &block.lines {
            lines.push(self.convert_line(line));
        }
        if include_active {
            lines.push(self.convert_line(&self.current_line));
        }
        lines
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
                Ok(true)
            }
            KeyCode::Esc => {
                let now = Instant::now();
                if self
                    .last_escape
                    .and_then(|previous| now.checked_duration_since(previous))
                    .map(|elapsed| elapsed <= Duration::from_millis(ESCAPE_DOUBLE_MS))
                    .unwrap_or(false)
                {
                    let _ = events.send(RatatuiEvent::Exit);
                    self.should_exit = true;
                } else {
                    self.last_escape = Some(now);
                    let _ = events.send(RatatuiEvent::Cancel);
                }
                Ok(true)
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let _ = events.send(RatatuiEvent::Interrupt);
                self.should_exit = true;
                Ok(true)
            }
            KeyCode::Char('k')
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                let handled = self.scroll_line_up();
                let _ = events.send(RatatuiEvent::ScrollLineUp);
                Ok(handled)
            }
            KeyCode::Char('j')
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                let handled = self.scroll_line_down();
                let _ = events.send(RatatuiEvent::ScrollLineDown);
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
        self.needs_autoscroll = false;
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

        if let Some(position) = self.render_transcript(frame, area) {
            frame.set_cursor_position(position);
        }
    }

    fn foreground_style(&self) -> Style {
        self.theme
            .foreground
            .map(|fg| Style::default().fg(fg))
            .unwrap_or_default()
    }

    fn render_transcript(&mut self, frame: &mut Frame, area: Rect) -> Option<(u16, u16)> {
        if area.width == 0 || area.height == 0 {
            self.needs_autoscroll = false;
            return None;
        }

        let layout = self.build_layout_entries(area.width);
        let viewport_height = usize::from(area.height);
        self.scroll_state
            .update_bounds(layout.total_height, viewport_height);
        if self.needs_autoscroll {
            self.scroll_state.scroll_to_bottom();
        }

        let mut offset = self.scroll_state.offset();
        let mut remaining = usize::from(area.height);
        let mut cursor = None;
        let mut y = area.y;

        for entry in layout.entries.iter() {
            if remaining == 0 {
                break;
            }

            if offset >= entry.height {
                offset -= entry.height;
                continue;
            }

            let visible_height = (entry.height - offset).min(remaining);

            match &entry.kind {
                EntryKind::Spacer => {
                    y = y.saturating_add(visible_height as u16);
                    remaining -= visible_height;
                    offset = 0;
                }
                EntryKind::Message {
                    kind,
                    lines,
                    content_height,
                } => {
                    let title = self.block_title_text(*kind);
                    self.render_entry_block(
                        frame,
                        area.x,
                        y,
                        area.width,
                        *content_height,
                        lines,
                        *kind,
                        offset,
                        visible_height,
                        None,
                        title,
                    );
                    y = y.saturating_add(visible_height as u16);
                    remaining -= visible_height;
                    offset = 0;
                }
                EntryKind::Prompt {
                    lines,
                    content_height,
                } => {
                    let cursor_offsets = self
                        .cursor_visual_position(layout.prompt_inner_width)
                        .map(|(row, col)| CursorOffsets { row, col });
                    let candidate = self.render_entry_block(
                        frame,
                        area.x,
                        y,
                        area.width,
                        *content_height,
                        lines,
                        RatatuiMessageKind::Info,
                        offset,
                        visible_height,
                        cursor_offsets,
                        TITLE_PROMPT,
                    );
                    if candidate.is_some() {
                        cursor = candidate;
                    }
                    y = y.saturating_add(visible_height as u16);
                    remaining -= visible_height;
                    offset = 0;
                }
            }
        }

        self.needs_autoscroll = false;
        cursor
    }

    fn input_visual_rows(&self, inner_width: u16) -> usize {
        if inner_width == 0 {
            return 0;
        }

        let text_width = self.input_text_area_width(inner_width);
        if text_width == 0 {
            return 0;
        }

        let display_width = self.input_display_width();
        max((display_width + text_width - 1) / text_width, 1)
    }

    fn cursor_visual_position(&self, inner_width: u16) -> Option<(usize, usize)> {
        if inner_width == 0 {
            return None;
        }

        let text_width = self.input_text_area_width(inner_width);
        if text_width == 0 {
            return None;
        }

        let prompt_width = self.prompt_width();
        let input_width = self.input.width_before_cursor();
        let cursor_column = prompt_width.saturating_add(input_width);
        Some((cursor_column / text_width, cursor_column % text_width))
    }

    fn block_title_text(&self, kind: RatatuiMessageKind) -> &'static str {
        match kind {
            RatatuiMessageKind::Agent => TITLE_AGENT,
            RatatuiMessageKind::Error => TITLE_ERROR,
            RatatuiMessageKind::Info => TITLE_INFO,
            RatatuiMessageKind::Policy => TITLE_POLICY,
            RatatuiMessageKind::Pty => TITLE_PTY,
            RatatuiMessageKind::Tool => TITLE_TOOL,
            RatatuiMessageKind::User => TITLE_USER,
        }
    }

    fn border_style(&self, kind: RatatuiMessageKind) -> Style {
        let color = match kind {
            RatatuiMessageKind::Agent => self.theme.secondary.or(self.theme.primary),
            RatatuiMessageKind::Error => Some(Color::Red),
            RatatuiMessageKind::Info => self.theme.foreground,
            RatatuiMessageKind::Policy => self.theme.secondary.or(self.theme.foreground),
            RatatuiMessageKind::Pty => self.theme.foreground,
            RatatuiMessageKind::Tool => self.theme.primary.or(self.theme.foreground),
            RatatuiMessageKind::User => self.theme.primary.or(self.theme.secondary),
        }
        .unwrap_or_else(|| self.theme.foreground.unwrap_or(Color::White));

        Style::default().fg(color)
    }

    fn render_entry_block(
        &self,
        frame: &mut Frame,
        x: u16,
        y: u16,
        width: u16,
        content_height: usize,
        lines: &[Line<'static>],
        kind: RatatuiMessageKind,
        mut skip: usize,
        mut visible_height: usize,
        cursor_offsets: Option<CursorOffsets>,
        title: &str,
    ) -> Option<(u16, u16)> {
        if width == 0 || visible_height == 0 {
            return None;
        }

        let mut cursor_position = None;
        let mut current_y = y;
        let inner_width = self.block_inner_width(width);
        let top_border = usize::from(BORDER_THICKNESS);
        let bottom_border = usize::from(BORDER_THICKNESS);
        let padding_top = usize::from(MESSAGE_VERTICAL_PADDING);
        let padding_bottom = usize::from(MESSAGE_VERTICAL_PADDING);

        if skip >= top_border {
            skip -= top_border;
        } else {
            if visible_height > 0 {
                self.draw_horizontal_border(frame, x, current_y, width, kind, true, title);
                current_y = current_y.saturating_add(1);
                visible_height -= 1;
            }
            skip = 0;
        }

        if visible_height == 0 {
            return cursor_position;
        }

        if padding_top > 0 {
            if skip >= padding_top {
                skip -= padding_top;
            } else {
                let draw_rows = (padding_top - skip).min(visible_height);
                self.draw_vertical_borders(frame, x, current_y, width, draw_rows, kind);
                current_y = current_y.saturating_add(draw_rows as u16);
                visible_height -= draw_rows;
                skip = 0;
            }
        }

        if visible_height == 0 {
            return cursor_position;
        }

        let content_skip = skip.min(content_height);
        skip = skip.saturating_sub(content_skip);

        if content_height > content_skip && visible_height > 0 {
            let draw_rows = (content_height - content_skip).min(visible_height);
            if inner_width > 0 && draw_rows > 0 {
                let content_area = Rect::new(
                    x + BORDER_THICKNESS + MESSAGE_HORIZONTAL_PADDING,
                    current_y,
                    inner_width,
                    draw_rows as u16,
                );
                let paragraph = Paragraph::new(lines.to_vec())
                    .wrap(Wrap { trim: false })
                    .style(self.foreground_style())
                    .scroll((content_skip as u16, 0));
                frame.render_widget(paragraph, content_area);
                self.draw_vertical_borders(frame, x, current_y, width, draw_rows, kind);

                if let Some(offsets) = cursor_offsets {
                    if offsets.row >= content_skip && offsets.row < content_skip + draw_rows {
                        let row_offset = offsets.row - content_skip;
                        if inner_width > 0 {
                            let column = offsets.col.min(inner_width.saturating_sub(1) as usize);
                            cursor_position = Some((
                                content_area.x + column as u16,
                                content_area.y + row_offset as u16,
                            ));
                        }
                    }
                }
            } else {
                self.draw_vertical_borders(frame, x, current_y, width, draw_rows, kind);
            }

            current_y = current_y.saturating_add(draw_rows as u16);
            visible_height -= draw_rows;
        }

        if visible_height == 0 {
            return cursor_position;
        }

        if padding_bottom > 0 {
            if skip >= padding_bottom {
                skip -= padding_bottom;
            } else {
                let draw_rows = (padding_bottom - skip).min(visible_height);
                self.draw_vertical_borders(frame, x, current_y, width, draw_rows, kind);
                current_y = current_y.saturating_add(draw_rows as u16);
                visible_height -= draw_rows;
                skip = 0;
            }
        }

        if visible_height == 0 {
            return cursor_position;
        }

        if skip < bottom_border && visible_height > 0 {
            self.draw_horizontal_border(frame, x, current_y, width, kind, false, "");
        }

        cursor_position
    }

    fn draw_horizontal_border(
        &self,
        frame: &mut Frame,
        x: u16,
        y: u16,
        width: u16,
        kind: RatatuiMessageKind,
        is_top: bool,
        title: &str,
    ) {
        if width == 0 {
            return;
        }
        let style = self.border_style(kind);
        let buffer = frame.buffer_mut();
        let left = if is_top { "╭" } else { "╰" };
        let right = if is_top { "╮" } else { "╯" };
        buffer[(x, y)].set_symbol(left).set_style(style);
        if width == 1 {
            return;
        }
        buffer[(x + width - 1, y)]
            .set_symbol(right)
            .set_style(style);
        if width <= 2 {
            return;
        }

        let inner_width = width - 2;
        if is_top && !title.is_empty() {
            let mut label = format!(" {title} ");
            if label.chars().count() as u16 > inner_width {
                label = label.chars().take(inner_width as usize).collect::<String>();
            }
            let label_len = label.chars().count() as u16;
            let start = 1 + (inner_width.saturating_sub(label_len)) / 2;
            for col in 1..start {
                buffer[(x + col, y)].set_symbol("─").set_style(style);
            }
            buffer.set_stringn(x + start, y, label.as_str(), inner_width as usize, style);
            let mut col = start + label_len;
            while col < inner_width + 1 {
                buffer[(x + col, y)].set_symbol("─").set_style(style);
                col += 1;
            }
        } else {
            for col in 1..=inner_width {
                buffer[(x + col, y)].set_symbol("─").set_style(style);
            }
        }
    }

    fn draw_vertical_borders(
        &self,
        frame: &mut Frame,
        x: u16,
        y: u16,
        width: u16,
        height: usize,
        kind: RatatuiMessageKind,
    ) {
        if width <= 1 || height == 0 {
            return;
        }
        let style = self.border_style(kind);
        let buffer = frame.buffer_mut();
        for row in 0..height {
            let cy = y + row as u16;
            buffer[(x, cy)].set_symbol("│").set_style(style);
            buffer[(x + width - 1, cy)].set_symbol("│").set_style(style);
        }
    }

    fn visual_height(&self, line: &StyledLine, viewport_width: u16) -> usize {
        if viewport_width == 0 {
            return 0;
        }

        let width = viewport_width as usize;
        let mut line_width = 0usize;
        for segment in &line.segments {
            line_width = line_width.saturating_add(UnicodeWidthStr::width(segment.text.as_str()));
        }

        if line_width == 0 {
            return 1;
        }

        let rows = (line_width + width - 1) / width;
        max(rows, 1)
    }

    fn convert_line(&self, line: &StyledLine) -> Line<'static> {
        let mut spans = Vec::with_capacity(line.segments.len());
        for segment in &line.segments {
            let style = segment.style.to_style(self.theme.foreground);
            spans.push(Span::styled(segment.text.clone(), style));
        }
        Line::from(spans)
    }

    fn build_input_line(&self) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        if !self.prompt_prefix.is_empty() {
            let style = self
                .prompt_style
                .to_style(self.theme.secondary.or(self.theme.foreground));
            spans.push(Span::styled(self.prompt_prefix.clone(), style));
        }
        let input_value = self.input.value().to_string();
        spans.push(Span::styled(
            input_value.clone(),
            Style::default().fg(self.theme.foreground.unwrap_or(Color::White)),
        ));
        if self.show_placeholder {
            if let Some(hint) = &self.placeholder_hint {
                if input_value.is_empty() {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        hint.clone(),
                        Style::default()
                            .fg(self
                                .theme
                                .secondary
                                .or(self.theme.foreground)
                                .unwrap_or(Color::White))
                            .add_modifier(Modifier::ITALIC),
                    ));
                }
            }
        }
        spans
    }

    fn prompt_width(&self) -> usize {
        UnicodeWidthStr::width(self.prompt_prefix.as_str())
    }

    fn input_text_area_width(&self, layout_width: u16) -> usize {
        usize::from(layout_width)
    }

    fn input_display_width(&self) -> usize {
        let mut width = self.prompt_width();
        let value = self.input.value();
        width = width.saturating_add(UnicodeWidthStr::width(value));
        if self.show_placeholder && value.is_empty() {
            if let Some(hint) = &self.placeholder_hint {
                width = width.saturating_add(1);
                width = width.saturating_add(UnicodeWidthStr::width(hint.as_str()));
            }
        }
        width
    }
}

fn create_ticker() -> Interval {
    let mut ticker = interval(Duration::from_millis(REDRAW_INTERVAL_MS));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker
}

pub fn convert_style(style: anstyle::Style) -> RatatuiTextStyle {
    let color = style.get_fg_color().and_then(|color| convert_color(color));
    let effects = style.get_effects();
    let bold = effects.contains(anstyle::Effects::BOLD);
    let italic = effects.contains(anstyle::Effects::ITALIC);

    RatatuiTextStyle {
        color,
        bold,
        italic,
    }
}

pub fn convert_color(color: anstyle::Color) -> Option<Color> {
    match color {
        anstyle::Color::Ansi(ansi) => Some(match ansi {
            anstyle::AnsiColor::Black => Color::Black,
            anstyle::AnsiColor::Red => Color::Red,
            anstyle::AnsiColor::Green => Color::Green,
            anstyle::AnsiColor::Yellow => Color::Yellow,
            anstyle::AnsiColor::Blue => Color::Blue,
            anstyle::AnsiColor::Magenta => Color::Magenta,
            anstyle::AnsiColor::Cyan => Color::Cyan,
            anstyle::AnsiColor::White => Color::Gray,
            anstyle::AnsiColor::BrightBlack => Color::DarkGray,
            anstyle::AnsiColor::BrightRed => Color::LightRed,
            anstyle::AnsiColor::BrightGreen => Color::LightGreen,
            anstyle::AnsiColor::BrightYellow => Color::LightYellow,
            anstyle::AnsiColor::BrightBlue => Color::LightBlue,
            anstyle::AnsiColor::BrightMagenta => Color::LightMagenta,
            anstyle::AnsiColor::BrightCyan => Color::LightCyan,
            anstyle::AnsiColor::BrightWhite => Color::White,
        }),
        anstyle::Color::Ansi256(value) => Some(Color::Indexed(value.index())),
        anstyle::Color::Rgb(rgb) => Some(Color::Rgb(rgb.r(), rgb.g(), rgb.b())),
    }
}

pub fn theme_from_styles(styles: &crate::ui::theme::ThemeStyles) -> RatatuiTheme {
    RatatuiTheme {
        background: None,
        foreground: convert_style(styles.output).color,
        primary: convert_style(styles.primary).color,
        secondary: convert_style(styles.secondary).color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn segment_from(text: &str) -> RatatuiSegment {
        RatatuiSegment {
            text: text.to_string(),
            style: RatatuiTextStyle::default(),
        }
    }

    #[test]
    fn prompt_renders_on_bottom_row_for_small_viewports() {
        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = RatatuiLoop::new(RatatuiTheme::default(), None);
        app.handle_command(RatatuiCommand::AppendLine {
            kind: RatatuiMessageKind::Info,
            segments: vec![segment_from("hello world")],
        });
        for ch in "example user input".chars() {
            app.input.insert(ch);
        }

        terminal.draw(|frame| app.draw(frame)).expect("draw");

        let backend = terminal.backend();
        let size = backend.size();
        let bottom_row = size.height.saturating_sub(1) as usize;
        let prompt_row = bottom_row.saturating_sub(1);
        let prompt_cell = backend.buffer().get(2, prompt_row);
        assert_eq!(prompt_cell.symbol(), "❯");
        let bottom_left = backend.buffer().get(0, bottom_row);
        assert_eq!(bottom_left.symbol(), "╰");
        let top_left = backend.buffer().get(0, 0);
        assert_eq!(top_left.symbol(), "╭");
    }

    #[test]
    fn input_wraps_without_truncating_transcript_area() {
        let backend = TestBackend::new(20, 6);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = RatatuiLoop::new(RatatuiTheme::default(), None);
        app.handle_command(RatatuiCommand::AppendLine {
            kind: RatatuiMessageKind::Info,
            segments: vec![segment_from("line one"), segment_from("line two")],
        });
        let long_input = "abcdefghijklmnopqrstuvwxyz0123456789";
        for ch in long_input.chars() {
            app.input.insert(ch);
        }

        terminal.draw(|frame| app.draw(frame)).expect("draw");

        let backend = terminal.backend();
        let size = backend.size();
        let buffer = backend.buffer();
        let bottom_row = size.height.saturating_sub(1) as usize;
        let prompt_row = bottom_row.saturating_sub(1);
        let prompt_cell = buffer.get(2, prompt_row);
        assert_eq!(prompt_cell.symbol(), "❯");
        let wrapped_row = bottom_row.saturating_sub(2);
        let wrapped_cell = buffer.get(2, wrapped_row);
        assert_ne!(wrapped_cell.symbol(), " ");
        let spacer_row = wrapped_row.saturating_sub(1);
        assert_eq!(buffer.get(0, spacer_row).symbol(), " ");
        let top_left = buffer.get(0, 0);
        assert_eq!(top_left.symbol(), "╭");
    }
}
