use anyhow::Result;
use crossterm::event::{
    Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;

use super::state::{
    ESCAPE_DOUBLE_MS, RatatuiEvent, RatatuiLoop, ScrollFocus, TranscriptScrollState,
};

impl RatatuiLoop {
    pub(crate) fn handle_event(
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
            CrosstermEvent::FocusGained | CrosstermEvent::FocusLost => {
                Ok(false)
            }
            CrosstermEvent::Paste(text) => {
                if !self.input_enabled {
                    return Ok(false);
                }
                // Insert pasted text at cursor position
                for ch in text.chars() {
                    self.input.insert(ch);
                }
                self.update_input_state();
                self.last_escape = None;
                self.transcript_autoscroll = true;
                Ok(true)
            }
        }
    }

    fn handle_key_event(
        &mut self,
        key: KeyEvent,
        events: &UnboundedSender<RatatuiEvent>,
    ) -> Result<bool> {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
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
            KeyCode::PageUp if key.modifiers.contains(KeyModifiers::ALT) => {
                if self.view_previous_conversation() {
                    return Ok(true);
                }
                Ok(false)
            }
            KeyCode::PageDown if key.modifiers.contains(KeyModifiers::ALT) => {
                if self.view_next_conversation() {
                    return Ok(true);
                }
                Ok(false)
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
}
