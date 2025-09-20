use anyhow::{Context, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use std::cmp::min;
use std::convert::TryFrom;
use std::io::{Write, stdout};
use std::time::{Duration, Instant};

const ESCAPE_DOUBLE_MS: u64 = 750;

#[derive(Clone, Copy)]
pub(crate) enum ScrollAction {
    LineUp,
    LineDown,
    PageUp,
    PageDown,
}

pub(crate) enum InputOutcome {
    Submitted(String),
    CancelRun,
    ExitSession,
    Interrupted,
}

#[derive(Default)]
pub(crate) struct ChatInput {
    buffer: String,
    cursor: usize,
    last_escape: Option<Instant>,
}

impl ChatInput {
    pub(crate) fn read_line<F>(&mut self, mut on_scroll: F) -> Result<InputOutcome>
    where
        F: FnMut(ScrollAction) -> Result<(u16, u16)>,
    {
        enable_raw_mode().context("failed to enable raw mode for chat input")?;
        let mut guard = RawModeGuard(true);
        let mut stdout = stdout();
        stdout.flush().ok();
        let (mut start_col, mut start_row) =
            cursor::position().context("failed to read cursor position")?;
        self.buffer.clear();
        self.cursor = 0;

        loop {
            stdout.flush().ok();
            match event::read().context("failed to read terminal event")? {
                Event::Key(key) => {
                    if let Some(outcome) = self.handle_key(
                        key,
                        &mut start_col,
                        &mut start_row,
                        &mut stdout,
                        &mut on_scroll,
                    )? {
                        guard.0 = false;
                        disable_raw_mode().ok();
                        stdout.write_all(b"\r\n").ok();
                        stdout.flush().ok();
                        return Ok(outcome);
                    }
                }
                Event::Resize(_, _) => {
                    self.refresh(start_col, start_row, &mut stdout)?;
                }
                _ => {}
            }
        }
    }

    fn handle_key<F>(
        &mut self,
        key: KeyEvent,
        start_col: &mut u16,
        start_row: &mut u16,
        stdout: &mut std::io::Stdout,
        on_scroll: &mut F,
    ) -> Result<Option<InputOutcome>>
    where
        F: FnMut(ScrollAction) -> Result<(u16, u16)>,
    {
        if !matches!(key.code, KeyCode::Esc) {
            self.last_escape = None;
        }
        match key.code {
            KeyCode::Enter => {
                let submitted = std::mem::take(&mut self.buffer);
                self.cursor = 0;
                return Ok(Some(InputOutcome::Submitted(submitted)));
            }
            KeyCode::Left => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.move_word_left();
                } else if let Some(prev) = self.prev_char_boundary(self.cursor) {
                    self.cursor = prev;
                }
                self.refresh(*start_col, *start_row, stdout)?;
            }
            KeyCode::Right => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.move_word_right();
                } else if let Some(next) = self.next_char_boundary(self.cursor) {
                    self.cursor = next;
                }
                self.refresh(*start_col, *start_row, stdout)?;
            }
            KeyCode::Home => {
                self.cursor = 0;
                self.refresh(*start_col, *start_row, stdout)?;
            }
            KeyCode::End => {
                self.cursor = self.buffer.len();
                self.refresh(*start_col, *start_row, stdout)?;
            }
            KeyCode::Backspace => {
                if let Some(prev) = self.prev_char_boundary(self.cursor) {
                    self.buffer.drain(prev..self.cursor);
                    self.cursor = prev;
                    self.refresh(*start_col, *start_row, stdout)?;
                }
            }
            KeyCode::Delete => {
                if let Some(next) = self.next_char_boundary(self.cursor) {
                    self.buffer.drain(self.cursor..next);
                    self.refresh(*start_col, *start_row, stdout)?;
                }
            }
            KeyCode::Esc => {
                let now = Instant::now();
                let outcome = if self
                    .last_escape
                    .map(|prev| now.duration_since(prev) <= Duration::from_millis(ESCAPE_DOUBLE_MS))
                    .unwrap_or(false)
                {
                    InputOutcome::ExitSession
                } else {
                    self.last_escape = Some(now);
                    InputOutcome::CancelRun
                };
                return Ok(Some(outcome));
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(Some(InputOutcome::Interrupted));
            }
            KeyCode::Char('k')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT) =>
            {
                let (col, row) = on_scroll(ScrollAction::LineUp)?;
                *start_col = col;
                *start_row = row;
                self.refresh(*start_col, *start_row, stdout)?;
            }
            KeyCode::Char('j')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT) =>
            {
                let (col, row) = on_scroll(ScrollAction::LineDown)?;
                *start_col = col;
                *start_row = row;
                self.refresh(*start_col, *start_row, stdout)?;
            }
            KeyCode::Up => {
                let (col, row) = on_scroll(ScrollAction::LineUp)?;
                *start_col = col;
                *start_row = row;
                self.refresh(*start_col, *start_row, stdout)?;
            }
            KeyCode::Down => {
                let (col, row) = on_scroll(ScrollAction::LineDown)?;
                *start_col = col;
                *start_row = row;
                self.refresh(*start_col, *start_row, stdout)?;
            }
            KeyCode::PageUp => {
                let (col, row) = on_scroll(ScrollAction::PageUp)?;
                *start_col = col;
                *start_row = row;
                self.refresh(*start_col, *start_row, stdout)?;
            }
            KeyCode::PageDown => {
                let (col, row) = on_scroll(ScrollAction::PageDown)?;
                *start_col = col;
                *start_row = row;
                self.refresh(*start_col, *start_row, stdout)?;
            }
            KeyCode::Char(ch) => {
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    return Ok(None);
                }
                self.buffer.insert(self.cursor, ch);
                self.cursor += ch.len_utf8();
                self.refresh(*start_col, *start_row, stdout)?;
            }
            _ => {}
        }

        Ok(None)
    }

    fn move_word_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut idx = self.cursor;
        while let Some(prev) = self.prev_char_boundary(idx) {
            let ch = self.char_at(prev);
            if ch.is_whitespace() {
                idx = prev;
            } else {
                break;
            }
        }
        while let Some(prev) = self.prev_char_boundary(idx) {
            let ch = self.char_at(prev);
            if !ch.is_whitespace() {
                idx = prev;
            } else {
                break;
            }
        }
        self.cursor = idx;
    }

    fn move_word_right(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let mut idx = self.cursor;
        let mut iter = self.buffer[self.cursor..].char_indices().peekable();
        while let Some(&(offset, ch)) = iter.peek() {
            if ch.is_whitespace() {
                break;
            }
            idx = self.cursor + offset + ch.len_utf8();
            iter.next();
        }
        while let Some(&(offset, ch)) = iter.peek() {
            if !ch.is_whitespace() {
                break;
            }
            idx = self.cursor + offset + ch.len_utf8();
            iter.next();
        }
        self.cursor = min(idx, self.buffer.len());
    }

    fn refresh(&self, start_col: u16, start_row: u16, stdout: &mut std::io::Stdout) -> Result<()> {
        let cursor_col =
            u16::try_from(self.buffer[..self.cursor].chars().count()).unwrap_or(u16::MAX);
        crossterm::queue!(stdout, cursor::MoveTo(start_col, start_row))?;
        crossterm::queue!(stdout, Clear(ClearType::UntilNewLine))?;
        stdout.write_all(self.buffer.as_bytes()).ok();
        let final_col = start_col.saturating_add(cursor_col);
        crossterm::queue!(stdout, cursor::MoveTo(final_col, start_row))?;
        stdout.flush().ok();
        Ok(())
    }

    fn prev_char_boundary(&self, from: usize) -> Option<usize> {
        if from == 0 || from > self.buffer.len() {
            return None;
        }
        self.buffer[..from]
            .char_indices()
            .next_back()
            .map(|(idx, _)| idx)
    }

    fn next_char_boundary(&self, from: usize) -> Option<usize> {
        if from >= self.buffer.len() {
            return None;
        }
        self.buffer[from..]
            .char_indices()
            .next()
            .map(|(offset, ch)| from + offset + ch.len_utf8())
    }

    fn char_at(&self, start: usize) -> char {
        self.buffer[start..]
            .chars()
            .next()
            .expect("start should be a valid char boundary")
    }
}

struct RawModeGuard(bool);

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if self.0 {
            disable_raw_mode().ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result as AnyResult;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn noop_scroll() -> impl FnMut(ScrollAction) -> AnyResult<(u16, u16)> {
        move |_| Ok((0, 0))
    }

    fn build_key_event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn first_escape_cancels_run() {
        let mut input = ChatInput::default();
        let mut stdout = stdout();
        let mut col = 0;
        let mut row = 0;
        let outcome = input
            .handle_key(
                build_key_event(KeyCode::Esc, KeyModifiers::empty()),
                &mut col,
                &mut row,
                &mut stdout,
                &mut noop_scroll(),
            )
            .unwrap();
        assert!(matches!(outcome, Some(InputOutcome::CancelRun)));
        assert!(input.last_escape.is_some());
    }

    #[test]
    fn double_escape_exits_session() {
        let mut input = ChatInput::default();
        input.last_escape = Some(Instant::now());
        let mut stdout = stdout();
        let mut col = 0;
        let mut row = 0;
        let outcome = input
            .handle_key(
                build_key_event(KeyCode::Esc, KeyModifiers::empty()),
                &mut col,
                &mut row,
                &mut stdout,
                &mut noop_scroll(),
            )
            .unwrap();
        assert!(matches!(outcome, Some(InputOutcome::ExitSession)));
    }

    #[test]
    fn ctrl_c_interrupts() {
        let mut input = ChatInput::default();
        let mut stdout = stdout();
        let mut col = 0;
        let mut row = 0;
        let outcome = input
            .handle_key(
                build_key_event(KeyCode::Char('c'), KeyModifiers::CONTROL),
                &mut col,
                &mut row,
                &mut stdout,
                &mut noop_scroll(),
            )
            .unwrap();
        assert!(matches!(outcome, Some(InputOutcome::Interrupted)));
    }

    #[test]
    fn word_navigation_skips_whitespace_boundaries() {
        let mut input = ChatInput {
            buffer: "hello   world".to_string(),
            cursor: 13,
            last_escape: None,
        };
        input.move_word_left();
        assert_eq!(input.cursor, 8);
        input.move_word_left();
        assert_eq!(input.cursor, 0);
        input.cursor = 0;
        input.move_word_right();
        assert_eq!(input.cursor, 8);
    }

    #[test]
    fn editing_multibyte_characters_does_not_panic() {
        let mut input = ChatInput::default();
        let mut stdout = stdout();
        let mut col = 0;
        let mut row = 0;

        input
            .handle_key(
                build_key_event(KeyCode::Char('é'), KeyModifiers::empty()),
                &mut col,
                &mut row,
                &mut stdout,
                &mut noop_scroll(),
            )
            .unwrap();
        assert_eq!(input.buffer, "é");
        assert_eq!(input.cursor, "é".len());

        input
            .handle_key(
                build_key_event(KeyCode::Left, KeyModifiers::empty()),
                &mut col,
                &mut row,
                &mut stdout,
                &mut noop_scroll(),
            )
            .unwrap();
        assert_eq!(input.cursor, 0);

        input
            .handle_key(
                build_key_event(KeyCode::Right, KeyModifiers::empty()),
                &mut col,
                &mut row,
                &mut stdout,
                &mut noop_scroll(),
            )
            .unwrap();
        assert_eq!(input.cursor, "é".len());

        input
            .handle_key(
                build_key_event(KeyCode::Backspace, KeyModifiers::empty()),
                &mut col,
                &mut row,
                &mut stdout,
                &mut noop_scroll(),
            )
            .unwrap();
        assert!(input.buffer.is_empty());
        assert_eq!(input.cursor, 0);
    }
}
