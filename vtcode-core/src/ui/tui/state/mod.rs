use crate::ui::slash::{SlashCommandInfo, suggestions_for};
use ansi_to_tui::IntoText;
use anyhow::{Context, Result};
use crossterm::{
    ExecutableCommand, cursor,
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::ListState,
};
use serde_json::Value;
use std::collections::VecDeque;
use std::env;
use std::io::{self, IsTerminal};
use std::mem;
use std::time::Instant;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use unicode_width::UnicodeWidthStr;

pub(crate) const ESCAPE_DOUBLE_MS: u64 = 750;
pub(crate) const REDRAW_INTERVAL_MS: u64 = 33;
pub(crate) const MESSAGE_INDENT: usize = 2;
pub(crate) const NAVIGATION_HINT_TEXT: &str = "↵ send · esc exit · alt+Pg↑/Pg↓ history";
pub(crate) const MAX_SLASH_SUGGESTIONS: usize = 6;
const SURFACE_ENV_KEY: &str = "VT_RATATUI_SURFACE";
const INLINE_FALLBACK_ROWS: u16 = 24;

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
pub(crate) struct StyledLine {
    pub(crate) segments: Vec<RatatuiSegment>,
}

impl StyledLine {
    pub(crate) fn push_segment(&mut self, segment: RatatuiSegment) {
        if segment.text.is_empty() {
            return;
        }
        self.segments.push(segment);
    }

    pub(crate) fn has_visible_content(&self) -> bool {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SurfacePreference {
    Auto,
    Alternate,
    Inline,
}

impl SurfacePreference {
    fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }
        let normalized = trimmed.to_ascii_lowercase();
        match normalized.as_str() {
            "auto" => Some(Self::Auto),
            "alt" | "alternate" => Some(Self::Alternate),
            "inline" => Some(Self::Inline),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalSurface {
    Alternate,
    Inline { rows: u16 },
}

impl TerminalSurface {
    pub(crate) fn detect() -> Result<Self> {
        let preference = env::var(SURFACE_ENV_KEY)
            .ok()
            .and_then(|value| SurfacePreference::parse(&value))
            .unwrap_or(SurfacePreference::Auto);
        let is_tty = io::stdout().is_terminal();
        match preference {
            SurfacePreference::Alternate => {
                if is_tty {
                    Ok(Self::Alternate)
                } else {
                    Ok(Self::Inline {
                        rows: Self::inline_rows(false)?,
                    })
                }
            }
            SurfacePreference::Inline => Ok(Self::Inline {
                rows: Self::inline_rows(is_tty)?,
            }),
            SurfacePreference::Auto => {
                if is_tty {
                    Ok(Self::Alternate)
                } else {
                    Ok(Self::Inline {
                        rows: Self::inline_rows(false)?,
                    })
                }
            }
        }
    }

    fn inline_rows(is_tty: bool) -> Result<u16> {
        if !is_tty {
            return Ok(INLINE_FALLBACK_ROWS);
        }
        match crossterm::terminal::size() {
            Ok((_, rows)) => Ok(rows),
            Err(err) => {
                tracing::debug!("failed to query terminal size: {err}");
                Ok(INLINE_FALLBACK_ROWS)
            }
        }
    }

    pub(crate) fn uses_alternate_screen(self) -> bool {
        matches!(self, Self::Alternate)
    }
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

pub(crate) struct TerminalGuard {
    cursor_hidden: bool,
    alternate_screen_active: bool,
    raw_mode_enabled: bool,
    mouse_capture_enabled: bool,
}

impl TerminalGuard {
    pub(crate) fn activate(surface: TerminalSurface) -> Result<Self> {
        if !surface.uses_alternate_screen() {
            return Ok(Self {
                cursor_hidden: false,
                alternate_screen_active: false,
                raw_mode_enabled: false,
                mouse_capture_enabled: false,
            });
        }

        enable_raw_mode().context("failed to enable raw mode")?;
        let mut stdout = io::stdout();
        let alternate_screen_active = match stdout.execute(EnterAlternateScreen) {
            Ok(_) => true,
            Err(err) => {
                let _ = disable_raw_mode();
                return Err(err).context("failed to enter alternate screen");
            }
        };
        let mouse_capture_enabled = match stdout.execute(EnableMouseCapture) {
            Ok(_) => true,
            Err(err) => {
                if alternate_screen_active {
                    let _ = stdout.execute(LeaveAlternateScreen);
                }
                let _ = disable_raw_mode();
                return Err(err).context("failed to enable mouse capture");
            }
        };
        if let Err(err) = stdout.execute(cursor::Hide) {
            if mouse_capture_enabled {
                let _ = stdout.execute(DisableMouseCapture);
            }
            if alternate_screen_active {
                let _ = stdout.execute(LeaveAlternateScreen);
            }
            let _ = disable_raw_mode();
            return Err(err).context("failed to hide cursor");
        }
        Ok(Self {
            cursor_hidden: true,
            alternate_screen_active,
            raw_mode_enabled: true,
            mouse_capture_enabled,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.raw_mode_enabled {
            let _ = disable_raw_mode();
        }
        let mut stdout = io::stdout();
        if self.cursor_hidden {
            let _ = stdout.execute(cursor::Show);
        }
        if self.mouse_capture_enabled {
            let _ = stdout.execute(DisableMouseCapture);
        }
        if self.alternate_screen_active {
            let _ = stdout.execute(LeaveAlternateScreen);
        } else {
            let _ = stdout.execute(Clear(ClearType::FromCursorDown));
        }
    }
}

#[derive(Default)]
pub(crate) struct InputState {
    value: String,
    cursor: usize,
}

impl InputState {
    pub(crate) fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }

    pub(crate) fn insert(&mut self, ch: char) {
        self.value.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub(crate) fn backspace(&mut self) {
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

    pub(crate) fn delete(&mut self) {
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

    pub(crate) fn move_left(&mut self) {
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

    pub(crate) fn move_right(&mut self) {
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

    pub(crate) fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub(crate) fn move_end(&mut self) {
        self.cursor = self.value.len();
    }

    pub(crate) fn take(&mut self) -> String {
        let mut result = String::new();
        mem::swap(&mut result, &mut self.value);
        self.cursor = 0;
        result
    }

    pub(crate) fn value(&self) -> &str {
        &self.value
    }

    pub(crate) fn width_before_cursor(&self) -> usize {
        UnicodeWidthStr::width(&self.value[..self.cursor])
    }
}

#[derive(Default)]
pub(crate) struct TranscriptScrollState {
    offset: usize,
    viewport_height: usize,
    content_height: usize,
}

impl TranscriptScrollState {
    pub(crate) fn offset(&self) -> usize {
        self.offset
    }

    pub(crate) fn update_bounds(&mut self, content_height: usize, viewport_height: usize) {
        self.content_height = content_height;
        self.viewport_height = viewport_height;
        let max_offset = self.max_offset();
        if self.offset > max_offset {
            self.offset = max_offset;
        }
    }

    pub(crate) fn scroll_to_bottom(&mut self) {
        self.offset = self.max_offset();
    }

    pub(crate) fn scroll_to_top(&mut self) {
        self.offset = 0;
    }

    pub(crate) fn scroll_up(&mut self) {
        if self.offset > 0 {
            self.offset -= 1;
        }
    }

    pub(crate) fn scroll_down(&mut self) {
        let max_offset = self.max_offset();
        if self.offset < max_offset {
            self.offset += 1;
        }
    }

    pub(crate) fn scroll_page_up(&mut self) {
        if self.offset == 0 {
            return;
        }
        let step = self.viewport_height.max(1);
        self.offset = self.offset.saturating_sub(step);
    }

    pub(crate) fn scroll_page_down(&mut self) {
        let max_offset = self.max_offset();
        if self.offset >= max_offset {
            return;
        }
        let step = self.viewport_height.max(1);
        self.offset = (self.offset + step).min(max_offset);
    }

    pub(crate) fn jump_to(&mut self, offset: usize) {
        let max_offset = self.max_offset();
        self.offset = offset.min(max_offset);
    }

    pub(crate) fn is_at_bottom(&self) -> bool {
        self.offset >= self.max_offset()
    }

    pub(crate) fn should_follow_new_content(&self) -> bool {
        self.viewport_height == 0 || self.is_at_bottom()
    }

    pub(crate) fn max_offset(&self) -> usize {
        if self.content_height <= self.viewport_height {
            0
        } else {
            self.content_height - self.viewport_height
        }
    }

    pub(crate) fn content_height(&self) -> usize {
        self.content_height
    }

    pub(crate) fn viewport_height(&self) -> usize {
        self.viewport_height
    }

    pub(crate) fn has_overflow(&self) -> bool {
        self.content_height > self.viewport_height
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollFocus {
    Transcript,
    Pty,
}

#[derive(Clone)]
pub(crate) struct MessageBlock {
    pub(crate) kind: RatatuiMessageKind,
    pub(crate) lines: Vec<StyledLine>,
}

#[derive(Clone, Default)]
pub(crate) struct StatusBarContent {
    pub(crate) left: String,
    pub(crate) center: String,
    pub(crate) right: String,
}

impl StatusBarContent {
    pub(crate) fn new() -> Self {
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
pub(crate) struct PtyPlacement {
    pub(crate) top: usize,
    pub(crate) height: usize,
    pub(crate) indent: usize,
}

#[derive(Default, Clone)]
pub(crate) struct SelectionState {
    start: Option<usize>,
    end: Option<usize>,
    dragging: bool,
}

impl SelectionState {
    pub(crate) fn clear(&mut self) {
        self.start = None;
        self.end = None;
        self.dragging = false;
    }

    pub(crate) fn begin(&mut self, line: usize) {
        self.start = Some(line);
        self.end = Some(line);
        self.dragging = true;
    }

    pub(crate) fn update(&mut self, line: usize) {
        if self.start.is_some() {
            self.end = Some(line);
        }
    }

    pub(crate) fn finish(&mut self) {
        self.dragging = false;
    }

    pub(crate) fn is_active(&self) -> bool {
        self.start.is_some()
    }

    pub(crate) fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub(crate) fn range(&self) -> Option<(usize, usize)> {
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
pub(crate) struct SlashSuggestionState {
    pub(crate) items: Vec<&'static SlashCommandInfo>,
    pub(crate) list_state: ListState,
}

impl SlashSuggestionState {
    pub(crate) fn clear(&mut self) {
        self.items.clear();
        self.list_state.select(None);
    }

    pub(crate) fn update(&mut self, query: &str) {
        self.items = suggestions_for(query);
        if self.items.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
    }

    pub(crate) fn is_visible(&self) -> bool {
        !self.items.is_empty()
    }

    pub(crate) fn visible_capacity(&self) -> usize {
        self.items.len().min(MAX_SLASH_SUGGESTIONS)
    }

    pub(crate) fn desired_height(&self) -> u16 {
        if !self.is_visible() {
            return 0;
        }
        self.visible_capacity() as u16 + 2
    }

    pub(crate) fn visible_height(&self, available: u16) -> u16 {
        if available < 3 || !self.is_visible() {
            return 0;
        }
        self.desired_height().min(available)
    }

    pub(crate) fn items(&self) -> &[&'static SlashCommandInfo] {
        &self.items
    }

    pub(crate) fn list_state(&mut self) -> &mut ListState {
        &mut self.list_state
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        self.list_state.selected()
    }

    pub(crate) fn select_previous(&mut self) -> bool {
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

    pub(crate) fn select_next(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        let len = self.items.len();
        let current = self.list_state.selected().unwrap_or(0);
        let next = if current + 1 >= len { 0 } else { current + 1 };
        self.list_state.select(Some(next));
        true
    }

    pub(crate) fn selected(&self) -> Option<&'static SlashCommandInfo> {
        let index = self.list_state.selected()?;
        self.items.get(index).copied()
    }
}

const PTY_MAX_LINES: usize = 200;
const PTY_PANEL_MAX_HEIGHT: usize = 10;
pub(crate) const PTY_CONTENT_VIEW_LINES: usize = PTY_PANEL_MAX_HEIGHT - 2;

pub(crate) struct PtyPanel {
    pub(crate) tool_name: Option<String>,
    pub(crate) command_display: Option<String>,
    pub(crate) lines: VecDeque<String>,
    pub(crate) trailing: String,
    pub(crate) cached: Text<'static>,
    pub(crate) dirty: bool,
    pub(crate) cached_height: usize,
}

impl PtyPanel {
    pub(crate) fn new() -> Self {
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

    pub(crate) fn reset_output(&mut self) {
        self.lines.clear();
        self.trailing.clear();
        self.cached = Text::default();
        self.dirty = true;
        self.cached_height = 0;
    }

    pub(crate) fn clear(&mut self) {
        self.tool_name = None;
        self.command_display = None;
        self.reset_output();
    }

    pub(crate) fn set_tool_call(&mut self, tool_name: String, command_display: Option<String>) {
        self.tool_name = Some(tool_name);
        self.command_display = command_display;
        self.reset_output();
    }

    pub(crate) fn push_line(&mut self, text: &str) {
        self.push_text(text, true);
    }

    pub(crate) fn push_inline(&mut self, text: &str) {
        self.push_text(text, false);
    }

    pub(crate) fn push_text(&mut self, text: &str, newline: bool) {
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

    pub(crate) fn commit_line(&mut self) {
        let line = mem::take(&mut self.trailing);
        self.lines.push_back(line);
        if self.lines.len() > PTY_MAX_LINES {
            self.lines.pop_front();
        }
    }

    pub(crate) fn has_content(&self) -> bool {
        self.tool_name.is_some()
            || self.command_display.is_some()
            || !self.lines.is_empty()
            || !self.trailing.is_empty()
    }

    pub(crate) fn command_summary(&self) -> Option<String> {
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

    pub(crate) fn block_title_text(&self) -> String {
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

    pub(crate) fn view_text(&mut self) -> Text<'static> {
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

#[derive(Debug, Clone)]
pub(crate) struct ToolCallSummary {
    name: String,
    fields: Vec<(String, String)>,
}

pub(crate) struct TranscriptDisplay {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) total_height: usize,
}

pub(crate) struct InputDisplay {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) cursor: Option<(u16, u16)>,
    pub(crate) height: u16,
}

pub(crate) struct InputLayout {
    pub(crate) block_area: Rect,
    pub(crate) suggestion_area: Option<Rect>,
    pub(crate) display: InputDisplay,
}

pub(crate) struct AppLayout {
    pub(crate) message: Rect,
    pub(crate) input: Option<InputLayout>,
    pub(crate) status: Option<Rect>,
}

pub(crate) struct RatatuiLoop {
    pub(crate) messages: Vec<MessageBlock>,
    pub(crate) conversation_offsets: Vec<usize>,
    pub(crate) active_conversation: usize,
    pub(crate) conversation_line_offsets: Vec<usize>,
    pub(crate) current_line: StyledLine,
    pub(crate) current_kind: Option<RatatuiMessageKind>,
    pub(crate) current_active: bool,
    pub(crate) prompt_prefix: String,
    pub(crate) prompt_style: RatatuiTextStyle,
    pub(crate) input: InputState,
    pub(crate) base_placeholder: Option<String>,
    pub(crate) placeholder_hint: Option<String>,
    pub(crate) show_placeholder: bool,
    pub(crate) base_placeholder_style: RatatuiTextStyle,
    pub(crate) placeholder_style: RatatuiTextStyle,
    pub(crate) should_exit: bool,
    pub(crate) theme: RatatuiTheme,
    pub(crate) last_escape: Option<Instant>,
    pub(crate) transcript_scroll: TranscriptScrollState,
    pub(crate) transcript_autoscroll: bool,
    pub(crate) pty_scroll: TranscriptScrollState,
    pub(crate) pty_autoscroll: bool,
    pub(crate) scroll_focus: ScrollFocus,
    pub(crate) transcript_area: Option<Rect>,
    pub(crate) pty_area: Option<Rect>,
    pub(crate) pty_block: Option<PtyPlacement>,
    pub(crate) slash_suggestions: SlashSuggestionState,
    pub(crate) pty_panel: Option<PtyPanel>,
    pub(crate) status_bar: StatusBarContent,
    pub(crate) cursor_visible: bool,
    pub(crate) input_enabled: bool,
    pub(crate) selection: SelectionState,
}

impl RatatuiLoop {
    pub(crate) fn default_placeholder_style(theme: &RatatuiTheme) -> RatatuiTextStyle {
        let mut style = RatatuiTextStyle::default();
        style.italic = true;
        style.color = theme
            .secondary
            .or(theme.foreground)
            .or(Some(Color::DarkGray));
        style
    }

    pub(crate) fn new(theme: RatatuiTheme, placeholder: Option<String>) -> Self {
        let sanitized_placeholder = placeholder
            .map(|hint| hint.trim().to_string())
            .filter(|hint| !hint.is_empty());
        let base_placeholder = sanitized_placeholder.clone();
        let show_placeholder = base_placeholder.is_some();
        let base_placeholder_style = Self::default_placeholder_style(&theme);
        Self {
            messages: Vec::new(),
            conversation_offsets: vec![0],
            active_conversation: 0,
            conversation_line_offsets: vec![0],
            current_line: StyledLine::default(),
            current_kind: None,
            current_active: false,
            prompt_prefix: "❯ ".to_string(),
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

    pub(crate) fn should_exit(&self) -> bool {
        self.should_exit
    }

    pub(crate) fn set_should_exit(&mut self) {
        self.should_exit = true;
    }

    pub(crate) fn needs_tick(&self) -> bool {
        false
    }

    pub(crate) fn handle_command(&mut self, command: RatatuiCommand) -> bool {
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

    pub(crate) fn collect_plain_text(segments: &[RatatuiSegment]) -> String {
        segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<String>()
    }

    pub(crate) fn append_inline_segment(
        &mut self,
        kind: RatatuiMessageKind,
        segment: RatatuiSegment,
    ) {
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

    pub(crate) fn flush_current_line(&mut self, force: bool) {
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

    pub(crate) fn update_input_state(&mut self) {
        self.show_placeholder = self.placeholder_hint.is_some() && self.input.value().is_empty();
        self.refresh_slash_suggestions();
    }

    pub(crate) fn refresh_slash_suggestions(&mut self) {
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

    pub(crate) fn set_input_text(&mut self, value: String) {
        if !self.input_enabled {
            return;
        }
        self.input.value = value;
        self.input.cursor = self.input.value.len();
        self.update_input_state();
        self.transcript_autoscroll = true;
    }

    pub(crate) fn apply_selected_suggestion(&mut self) -> bool {
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

    pub(crate) fn push_line(&mut self, kind: RatatuiMessageKind, line: StyledLine) {
        if kind == RatatuiMessageKind::Agent && !line.has_visible_content() {
            return;
        }
        if kind == RatatuiMessageKind::Tool {
            let plain = Self::collect_plain_text(&line.segments);
            if self.try_push_tool_summary(&plain) {
                return;
            }
        }
        if let Some(block) = self.messages.last_mut() {
            if block.kind == kind {
                block.lines.push(line);
                return;
            }
        }

        if kind == RatatuiMessageKind::User && !self.messages.is_empty() {
            self.begin_new_conversation();
        }

        self.messages.push(MessageBlock {
            kind,
            lines: vec![line],
        });
    }

    pub(crate) fn try_push_tool_summary(&mut self, plain: &str) -> bool {
        let Some(summary) = Self::parse_tool_call(plain) else {
            return false;
        };
        let lines = self.build_tool_summary_lines(&summary);
        if lines.is_empty() {
            return false;
        }
        if let Some(block) = self.messages.last_mut() {
            if block.kind == RatatuiMessageKind::Tool {
                block.lines = lines;
                return true;
            }
        }
        self.messages.push(MessageBlock {
            kind: RatatuiMessageKind::Tool,
            lines,
        });
        true
    }

    pub(crate) fn build_tool_summary_lines(&self, summary: &ToolCallSummary) -> Vec<StyledLine> {
        let mut entries: Vec<(String, String)> = Vec::new();
        entries.push(("Tool".to_string(), summary.name.clone()));
        if summary.fields.is_empty() {
            entries.push(("Arguments".to_string(), "—".to_string()));
        } else {
            for (key, value) in &summary.fields {
                let label = Self::format_tool_field_key(key);
                let cleaned = if value.trim().is_empty() {
                    "—".to_string()
                } else {
                    value.clone()
                };
                entries.push((label, cleaned));
            }
        }

        let max_width = entries
            .iter()
            .map(|(label, _)| UnicodeWidthStr::width(label.as_str()))
            .max()
            .unwrap_or(0);

        let label_style = self.tool_label_style();
        let value_style = self.tool_value_style();

        entries
            .into_iter()
            .map(|(mut label, value)| {
                let width = UnicodeWidthStr::width(label.as_str());
                if width < max_width {
                    label.push_str(&" ".repeat(max_width - width));
                }
                let mut line = StyledLine::default();
                line.push_segment(RatatuiSegment {
                    text: label,
                    style: label_style.clone(),
                });
                line.push_segment(RatatuiSegment {
                    text: ": ".to_string(),
                    style: label_style.clone(),
                });
                line.push_segment(RatatuiSegment {
                    text: value,
                    style: value_style.clone(),
                });
                line
            })
            .collect()
    }

    pub(crate) fn tool_label_style(&self) -> RatatuiTextStyle {
        let mut style = RatatuiTextStyle::default();
        style.bold = true;
        style.color = self
            .theme
            .secondary
            .or(self.theme.primary)
            .or(self.theme.foreground);
        style
    }

    pub(crate) fn tool_value_style(&self) -> RatatuiTextStyle {
        let mut style = RatatuiTextStyle::default();
        style.color = self.theme.foreground;
        style
    }

    pub(crate) fn begin_new_conversation(&mut self) {
        let next_offset = self.messages.len();
        if let Some(&last) = self.conversation_offsets.last() {
            if last == next_offset {
                self.active_conversation = self.conversation_offsets.len().saturating_sub(1);
                return;
            }
        }
        self.conversation_offsets.push(next_offset);
        self.active_conversation = self.conversation_offsets.len().saturating_sub(1);
        self.transcript_autoscroll = true;
    }

    pub(crate) fn conversation_header(&self) -> Option<Line<'static>> {
        if self.conversation_offsets.len() <= 1 {
            return None;
        }
        if self.active_conversation + 1 == self.conversation_offsets.len() {
            return None;
        }
        let total = self.conversation_offsets.len();
        let current = self.active_conversation + 1;
        let text = format!("Viewing conversation {} of {} (history)", current, total);
        let style = Style::default()
            .fg(self
                .theme
                .secondary
                .or(self.theme.foreground)
                .unwrap_or(Color::Gray))
            .add_modifier(Modifier::ITALIC);
        Some(Line::from(vec![Span::styled(text, style)]))
    }

    pub(crate) fn view_previous_conversation(&mut self) -> bool {
        if self.active_conversation == 0 {
            return false;
        }
        self.active_conversation -= 1;
        self.transcript_autoscroll = false;
        if let Some(&offset) = self.conversation_line_offsets.get(self.active_conversation) {
            self.transcript_scroll.jump_to(offset);
        } else {
            self.transcript_scroll.scroll_to_top();
        }
        self.scroll_focus = ScrollFocus::Transcript;
        true
    }

    pub(crate) fn view_next_conversation(&mut self) -> bool {
        if self.active_conversation + 1 >= self.conversation_offsets.len() {
            return false;
        }
        self.active_conversation += 1;
        if self.active_conversation + 1 == self.conversation_offsets.len() {
            self.transcript_autoscroll = true;
            self.transcript_scroll.scroll_to_bottom();
        } else {
            self.transcript_autoscroll = false;
            if let Some(&offset) = self.conversation_line_offsets.get(self.active_conversation) {
                self.transcript_scroll.jump_to(offset);
            } else {
                self.transcript_scroll.scroll_to_top();
            }
        }
        self.scroll_focus = ScrollFocus::Transcript;
        true
    }

    pub(crate) fn trim_empty_conversations(&mut self) {
        while self.conversation_offsets.len() > 1 {
            let last = *self.conversation_offsets.last().unwrap();
            if last >= self.messages.len() {
                self.conversation_offsets.pop();
            } else {
                break;
            }
        }
        if self.active_conversation >= self.conversation_offsets.len() {
            self.active_conversation = self.conversation_offsets.len().saturating_sub(1);
            self.transcript_autoscroll = true;
        }
    }

    pub(crate) fn parse_tool_call(plain: &str) -> Option<ToolCallSummary> {
        let trimmed = plain.trim();
        let rest = trimmed.strip_prefix("[TOOL]")?.trim_start();
        let mut parts = rest.splitn(2, ' ');
        let name = parts.next()?.trim();
        if name.is_empty() {
            return None;
        }
        let payload = parts.next().unwrap_or("").trim();
        let mut fields = Vec::new();
        if !payload.is_empty() {
            match serde_json::from_str::<Value>(payload) {
                Ok(Value::Object(map)) => {
                    for (key, value) in map.into_iter() {
                        fields.push((key, Self::stringify_tool_value(&value)));
                    }
                }
                Ok(value) => {
                    fields.push(("value".to_string(), Self::stringify_tool_value(&value)));
                }
                Err(_) => {
                    fields.push(("payload".to_string(), payload.to_string()));
                }
            }
        }
        Some(ToolCallSummary {
            name: name.to_string(),
            fields,
        })
    }

    pub(crate) fn format_tool_field_key(raw: &str) -> String {
        if raw.is_empty() {
            return "Value".to_string();
        }
        let mut formatted = String::new();
        let mut capitalize_next = true;
        for ch in raw.chars() {
            if ch == '_' || ch == '-' {
                formatted.push(' ');
                capitalize_next = true;
                continue;
            }
            if capitalize_next {
                formatted.push(ch.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                formatted.push(ch);
            }
        }
        formatted
    }

    pub(crate) fn stringify_tool_value(value: &Value) -> String {
        match value {
            Value::Null => "null".to_string(),
            Value::Bool(flag) => flag.to_string(),
            Value::Number(number) => number.to_string(),
            Value::String(text) => text.clone(),
            Value::Array(items) => {
                if items.is_empty() {
                    String::new()
                } else if items.iter().all(|item| item.is_string()) {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .map(|text| text.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    items
                        .iter()
                        .map(Self::stringify_tool_value)
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            }
            Value::Object(map) => {
                if map.is_empty() {
                    String::new()
                } else {
                    let mut pairs = Vec::new();
                    for (key, nested) in map.iter() {
                        let rendered = Self::stringify_tool_value(nested);
                        if rendered.is_empty() {
                            pairs.push(key.clone());
                        } else {
                            pairs.push(format!("{}={}", key, rendered));
                        }
                    }
                    pairs.join(", ")
                }
            }
        }
    }

    pub(crate) fn remove_last_lines(&mut self, mut count: usize) {
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

        while self
            .messages
            .last()
            .map(|block| block.lines.is_empty())
            .unwrap_or(false)
        {
            self.messages.pop();
        }

        self.trim_empty_conversations();
    }

    pub(crate) fn ensure_pty_panel(&mut self) -> &mut PtyPanel {
        if self.pty_panel.is_none() {
            self.pty_panel = Some(PtyPanel::new());
        }
        self.pty_panel.as_mut().expect("pty_panel must exist")
    }

    pub(crate) fn track_pty_metadata(&mut self, kind: RatatuiMessageKind, plain: &str) {
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

    pub(crate) fn parse_run_command(json_segment: &str) -> Option<String> {
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

    pub(crate) fn parse_bash_command(json_segment: &str) -> Option<String> {
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

    pub(crate) fn forward_pty_line(&mut self, kind: RatatuiMessageKind, text: &str) {
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

    pub(crate) fn forward_pty_inline(&mut self, kind: RatatuiMessageKind, text: &str) {
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

    pub(crate) fn drain_command_queue(
        &mut self,
        commands: &mut UnboundedReceiver<RatatuiCommand>,
    ) -> bool {
        let mut needs_redraw = false;
        loop {
            match commands.try_recv() {
                Ok(command) => {
                    if self.handle_command(command) {
                        needs_redraw = true;
                    }
                    if self.should_exit() {
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.should_exit = true;
                    break;
                }
            }
        }
        needs_redraw
    }
}
