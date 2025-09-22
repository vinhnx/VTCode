use crate::ui::slash::{SlashCommandInfo, suggestions_for};
use crate::ui::theme;
use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects, Style as AnsiStyle};
use anyhow::{Context, Result};
use crossterm::{
    ExecutableCommand, cursor,
    event::{
        DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent, EventStream, KeyCode,
        KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
    },
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use ratatui::{
    Frame, Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear as ClearWidget, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState,
    },
};
use serde_json::Value;
use std::cmp;
use std::io;
use std::mem;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::time::{Interval, MissedTickBehavior, interval};
use tui_term::vt100::Parser as VtParser;
use tui_term::widget::{Cursor as TermCursor, PseudoTerminal};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const ESCAPE_DOUBLE_MS: u64 = 750;
const REDRAW_INTERVAL_MS: u64 = 33;
const MESSAGE_INDENT: usize = 2;
const KEY_HINT_TEXT: &str = "Scroll ↑/↓ PgUp/PgDn · Submit Enter · Cancel Esc";

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
        stdout
            .execute(EnableMouseCapture)
            .context("failed to enable mouse capture")?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = stdout.execute(DisableMouseCapture);
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

#[derive(Clone)]
struct MessageBlock {
    kind: RatatuiMessageKind,
    lines: Vec<StyledLine>,
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

    fn items(&self) -> &[&'static SlashCommandInfo] {
        &self.items
    }

    fn list_state(&mut self) -> &mut ListState {
        &mut self.list_state
    }
}

struct PtyPanel {
    parser: VtParser,
    command: Option<Vec<String>>,
    dirty: bool,
}

impl PtyPanel {
    fn new() -> Self {
        Self {
            parser: VtParser::new(24, 80, 200),
            command: None,
            dirty: false,
        }
    }

    fn reset_output(&mut self) {
        self.parser = VtParser::new(24, 80, 200);
        self.dirty = false;
    }

    fn clear(&mut self) {
        self.reset_output();
        self.command = None;
    }

    fn set_command(&mut self, command: Vec<String>) {
        self.reset_output();
        self.command = Some(command);
    }

    fn push_line(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let mut bytes = text.as_bytes().to_vec();
        if !text.ends_with('\n') {
            bytes.push(b'\n');
        }
        self.parser.process(&bytes);
        self.dirty = true;
    }

    fn push_inline(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.parser.process(text.as_bytes());
        self.dirty = true;
    }

    fn has_content(&self) -> bool {
        self.command.is_some() || self.dirty
    }

    fn ensure_size(&mut self, rows: u16, cols: u16) {
        if rows == 0 || cols == 0 {
            return;
        }
        if self.parser.screen().size() != (rows, cols) {
            self.parser.set_size(rows, cols);
        }
    }

    fn info_height(&self) -> u16 {
        self.command
            .as_ref()
            .map(|cmd| if cmd.len() > 1 { 2 } else { 1 })
            .unwrap_or(0)
    }

    fn desired_height(&self, total_height: u16) -> u16 {
        if total_height <= 3 || !self.has_content() {
            return 0;
        }
        let info = self.info_height();
        let (rows, cols) = self.parser.screen().size();
        let used = self
            .parser
            .screen()
            .rows(0, cols)
            .take(rows as usize)
            .filter(|row| !row.trim().is_empty())
            .count() as u16;
        let base_rows = used.max(3).min(rows);
        let mut desired = base_rows.saturating_add(info).saturating_add(2);
        let max_allowed = total_height.saturating_sub(1);
        if desired > max_allowed {
            desired = cmp::max(info + 3, max_allowed);
        }
        desired
    }

    fn command_lines(&self) -> Option<(String, Option<String>)> {
        let command = self.command.as_ref()?;
        if command.is_empty() {
            return None;
        }
        let program = command[0].clone();
        let args = if command.len() > 1 {
            Some(command[1..].join(" "))
        } else {
            None
        };
        Some((program, args))
    }
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
    transcript_area: Option<Rect>,
    slash_suggestions: SlashSuggestionState,
    pty_panel: Option<PtyPanel>,
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
            transcript_area: None,
            slash_suggestions: SlashSuggestionState::default(),
            pty_panel: None,
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
                let plain = Self::collect_plain_text(&segments);
                self.track_pty_metadata(kind, &plain);
                let was_active = self.current_active;
                self.flush_current_line(was_active);
                self.push_line(kind, StyledLine { segments });
                self.forward_pty_line(kind, &plain);
                self.needs_autoscroll = true;
                true
            }
            RatatuiCommand::Inline { kind, segment } => {
                let plain = segment.text.clone();
                self.forward_pty_inline(kind, &plain);
                self.append_inline_segment(kind, segment);
                self.needs_autoscroll = true;
                true
            }
            RatatuiCommand::ReplaceLast { count, kind, lines } => {
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
                self.update_input_state();
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
            let query = rest.trim();
            self.slash_suggestions.update(query);
        } else {
            self.slash_suggestions.clear();
        }
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
            let command_text = rest.trim_start();
            if let Some(json_part) = command_text.strip_prefix("run_terminal_cmd") {
                if let Some(command) = Self::parse_run_command(json_part.trim()) {
                    let panel = self.ensure_pty_panel();
                    panel.set_command(command);
                }
            } else if let Some(panel) = self.pty_panel.as_mut() {
                if !command_text.starts_with('[') {
                    panel.clear();
                }
            }
        }
    }

    fn parse_run_command(json_segment: &str) -> Option<Vec<String>> {
        let value: Value = serde_json::from_str(json_segment).ok()?;
        let array = value.get("command")?.as_array()?;
        let mut command = Vec::with_capacity(array.len());
        for entry in array {
            if let Some(text) = entry.as_str() {
                command.push(text.to_string());
            }
        }
        if command.is_empty() {
            None
        } else {
            Some(command)
        }
    }

    fn forward_pty_line(&mut self, kind: RatatuiMessageKind, text: &str) {
        if kind != RatatuiMessageKind::Pty {
            return;
        }
        if let Some(panel) = self.pty_panel.as_mut() {
            panel.push_line(text);
        }
    }

    fn forward_pty_inline(&mut self, kind: RatatuiMessageKind, text: &str) {
        if kind != RatatuiMessageKind::Pty {
            return;
        }
        if let Some(panel) = self.pty_panel.as_mut() {
            panel.push_inline(text);
        }
    }

    fn render_slash_suggestions(&mut self, frame: &mut Frame, area: Rect) {
        if !self.slash_suggestions.is_visible() {
            return;
        }
        if area.width <= 4 || area.height == 0 {
            return;
        }

        const MAX_VISIBLE: usize = 6;
        let items: Vec<&SlashCommandInfo> = self
            .slash_suggestions
            .items()
            .iter()
            .take(MAX_VISIBLE)
            .copied()
            .collect();
        if items.is_empty() {
            return;
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
        let required_width = cmp::max(4, (max_width + 4).min(area.width as usize)) as u16;
        let visible_height = entries.len().min(MAX_VISIBLE) as u16 + 2;
        if visible_height > area.height {
            return;
        }

        let suggestion_area = Rect::new(
            area.x,
            area.y + area.height.saturating_sub(visible_height),
            required_width,
            visible_height,
        );
        frame.render_widget(ClearWidget, suggestion_area);

        let list_items: Vec<ListItem> = entries.into_iter().map(ListItem::new).collect();
        let border_style = Style::default().fg(self.theme.primary.unwrap_or(Color::LightBlue));
        let list = List::new(list_items)
            .block(
                Block::default()
                    .title(Line::from("/ commands"))
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

    fn render_pty_panel(&mut self, frame: &mut Frame, area: Rect) {
        let Some(panel) = self.pty_panel.as_mut() else {
            return;
        };
        if !panel.has_content() {
            return;
        }
        if area.height <= 2 || area.width <= 3 {
            return;
        }

        frame.render_widget(ClearWidget, area);
        let info_height = panel.info_height();
        let (info_area, term_area) = if info_height > 0 && area.height > info_height {
            let splits = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(info_height), Constraint::Min(2)])
                .split(area);
            (Some(splits[0]), splits[1])
        } else {
            (None, area)
        };

        if let Some(info_area) = info_area {
            if let Some((program, args)) = panel.command_lines() {
                let mut lines = vec![Line::from(format!("Command: {program}"))];
                if let Some(args) = args {
                    lines.push(Line::from(format!("Args: {args}")));
                }
                let info_style =
                    Style::default().fg(self.theme.secondary.unwrap_or(Color::LightCyan));
                let info = Paragraph::new(lines).style(info_style);
                frame.render_widget(info, info_area);
            }
        }

        if term_area.height <= 2 || term_area.width <= 3 {
            return;
        }
        let inner_height = term_area.height.saturating_sub(2).max(1);
        let inner_width = term_area.width.saturating_sub(2).max(1);
        panel.ensure_size(inner_height, inner_width);

        let border_style = Style::default().fg(self.theme.secondary.unwrap_or(Color::LightCyan));
        let cursor = TermCursor::default().visibility(false);
        let terminal_style = Style::default().fg(self.theme.foreground.unwrap_or(Color::Gray));
        let widget = PseudoTerminal::new(panel.parser.screen())
            .block(
                Block::default()
                    .title("PTY")
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .cursor(cursor)
            .style(terminal_style);
        frame.render_widget(widget, term_area);
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

        match key.code {
            KeyCode::Enter => {
                let text = self.input.take();
                self.update_input_state();
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
                    self.update_input_state();
                }
                Ok(true)
            }
            KeyCode::Char('c') | KeyCode::Char('d') | KeyCode::Char('z')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.input.clear();
                self.update_input_state();
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
                self.update_input_state();
                self.needs_autoscroll = true;
                Ok(true)
            }
            KeyCode::Delete => {
                self.input.delete();
                self.update_input_state();
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
                self.update_input_state();
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

    fn is_in_transcript_area(&self, column: u16, row: u16) -> bool {
        self.transcript_area
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
        if !self.is_in_transcript_area(mouse.column, mouse.row) {
            return Ok(false);
        }

        let handled = match mouse.kind {
            MouseEventKind::ScrollUp => {
                let scrolled = self.scroll_line_up();
                if scrolled {
                    let _ = events.send(RatatuiEvent::ScrollLineUp);
                }
                scrolled
            }
            MouseEventKind::ScrollDown => {
                let scrolled = self.scroll_line_down();
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

        let (transcript_area, pty_area) = if let Some(panel) = self.pty_panel.as_ref() {
            if panel.has_content() {
                let desired = panel.desired_height(body_area.height);
                if desired > 0 && body_area.height > desired {
                    let segments = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(1), Constraint::Length(desired)])
                        .split(body_area);
                    (segments[0], Some(segments[1]))
                } else {
                    (body_area, None)
                }
            } else {
                (body_area, None)
            }
        } else {
            (body_area, None)
        };

        self.transcript_area = Some(transcript_area);

        let reserve_scrollbar = transcript_area.width > 1;
        let text_width = if reserve_scrollbar {
            transcript_area.width.saturating_sub(1)
        } else {
            transcript_area.width
        };

        let display = self.build_display(text_width);
        let viewport_height = usize::from(transcript_area.height);
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

        let (text_area, scrollbar_area) = if reserve_scrollbar {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(text_width), Constraint::Length(1)])
                .split(transcript_area);
            (chunks[0], Some(chunks[1]))
        } else {
            (transcript_area, None)
        };

        frame.render_widget(paragraph, text_area);
        self.render_slash_suggestions(frame, text_area);

        if let Some(scroll_area) = scrollbar_area {
            if self.scroll_state.has_overflow() && scroll_area.width > 0 {
                let mut scrollbar_state = ScrollbarState::new(self.scroll_state.content_height())
                    .viewport_content_length(self.scroll_state.viewport_height())
                    .position(self.scroll_state.offset());
                let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
                frame.render_stateful_widget(scrollbar, scroll_area, &mut scrollbar_state);
            }
        }

        if let Some((row, col)) = display.cursor {
            if row >= offset {
                let visible_row = row - offset;
                if visible_row < viewport_height {
                    let cursor_x = text_area.x + col as u16;
                    let cursor_y = text_area.y + visible_row as u16;
                    frame.set_cursor_position((cursor_x, cursor_y));
                }
            }
        }

        if let Some(status_area) = status_area {
            if status_area.width > 0 {
                let status = Paragraph::new(Line::from(KEY_HINT_TEXT))
                    .alignment(Alignment::Center)
                    .style(style);
                frame.render_widget(status, status_area);
            }
        }

        if let Some(pty_area) = pty_area {
            self.render_pty_panel(frame, pty_area);
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
        let mut first_rendered = true;

        for block in &self.messages {
            if !self.block_has_visible_content(block) {
                continue;
            }

            if !first_rendered {
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

            first_rendered = false;
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

    fn block_has_visible_content(&self, block: &MessageBlock) -> bool {
        if !matches!(
            block.kind,
            RatatuiMessageKind::Pty | RatatuiMessageKind::Tool
        ) {
            return true;
        }

        block.lines.iter().any(StyledLine::has_visible_content)
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
