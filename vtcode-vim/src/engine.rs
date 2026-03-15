use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::text::{
    next_char_boundary, vim_current_line_bounds, vim_current_line_full_range, vim_end_word,
    vim_find_char, vim_is_linewise_range, vim_line_end, vim_line_first_non_ws, vim_line_start,
    vim_motion_range, vim_next_word_start, vim_prev_word_start, vim_text_object_range,
};
use crate::types::{
    ChangeTarget, ClipboardKind, FindState, InsertCapture, InsertKind, InsertRepeat, Motion,
    Operator, PendingState, RepeatableCommand, TextObjectSpec, VimMode, VimState,
};

const INDENT: &str = "    ";

/// Minimal text-editor surface required by the Vim engine.
pub trait Editor {
    fn content(&self) -> &str;
    fn cursor(&self) -> usize;
    fn set_cursor(&mut self, pos: usize);
    fn move_left(&mut self);
    fn move_right(&mut self);
    fn delete_char_forward(&mut self);
    fn insert_text(&mut self, text: &str);
    fn replace(&mut self, content: String, cursor: usize);
}

/// Result of routing a single key through the Vim engine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HandleKeyOutcome {
    pub handled: bool,
    pub clear_selection: bool,
}

/// Apply a single key event to the Vim state and editor surface.
#[must_use]
pub fn handle_key<E: Editor>(
    state: &mut VimState,
    editor: &mut E,
    clipboard: &mut String,
    key: &KeyEvent,
) -> HandleKeyOutcome {
    if !state.enabled() {
        return HandleKeyOutcome::default();
    }

    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return HandleKeyOutcome::default();
    }

    let mut ctx = VimContext {
        state,
        editor,
        clipboard,
    };
    ctx.handle_key(key)
}

struct VimContext<'a, E> {
    state: &'a mut VimState,
    editor: &'a mut E,
    clipboard: &'a mut String,
}

impl<E: Editor> VimContext<'_, E> {
    fn handle_key(&mut self, key: &KeyEvent) -> HandleKeyOutcome {
        match self.state.mode() {
            VimMode::Insert => self.handle_insert_key(key),
            VimMode::Normal => self.handle_normal_key(key),
        }
    }

    fn handle_insert_key(&mut self, key: &KeyEvent) -> HandleKeyOutcome {
        if matches!(key.code, KeyCode::Esc) {
            self.finish_insert_capture();
            self.state.set_mode(VimMode::Normal);
            self.state.pending = None;
            return HandleKeyOutcome {
                handled: true,
                clear_selection: true,
            };
        }
        HandleKeyOutcome::default()
    }

    fn handle_normal_key(&mut self, key: &KeyEvent) -> HandleKeyOutcome {
        let handled = match key.code {
            KeyCode::Esc => {
                self.state.pending = None;
                return HandleKeyOutcome {
                    handled: true,
                    clear_selection: true,
                };
            }
            KeyCode::Char(ch) => {
                if let Some(pending) = self.state.pending.take() {
                    self.handle_pending(pending, ch)
                } else {
                    self.handle_normal_char(ch)
                }
            }
            KeyCode::Left => {
                self.editor.move_left();
                self.state.preferred_column = None;
                true
            }
            KeyCode::Right => {
                self.editor.move_right();
                self.state.preferred_column = None;
                true
            }
            KeyCode::Up => {
                self.move_vertical(false);
                true
            }
            KeyCode::Down => {
                self.move_vertical(true);
                true
            }
            _ => false,
        };

        HandleKeyOutcome {
            handled,
            clear_selection: false,
        }
    }

    fn handle_pending(&mut self, pending: PendingState, ch: char) -> bool {
        match pending {
            PendingState::Operator(operator) => self.handle_operator(operator, ch),
            PendingState::TextObject(operator, around) => {
                self.handle_text_object(operator, around, ch)
            }
            PendingState::Find { till, forward } => self.handle_find(forward, till, ch),
            PendingState::GoToLine => {
                if ch == 'g' {
                    self.move_to_line(true);
                    true
                } else {
                    false
                }
            }
        }
    }

    fn handle_normal_char(&mut self, ch: char) -> bool {
        match ch {
            'h' => {
                self.editor.move_left();
                self.state.preferred_column = None;
                true
            }
            'l' => {
                self.editor.move_right();
                self.state.preferred_column = None;
                true
            }
            'j' => {
                self.move_vertical(true);
                true
            }
            'k' => {
                self.move_vertical(false);
                true
            }
            'w' => {
                self.move_motion(Motion::WordForward);
                true
            }
            'e' => {
                self.move_motion(Motion::EndWord);
                true
            }
            'b' => {
                self.move_motion(Motion::WordBackward);
                true
            }
            '0' => {
                self.editor
                    .set_cursor(vim_line_start(self.editor.content(), self.editor.cursor()));
                self.state.preferred_column = None;
                true
            }
            '^' => {
                self.editor.set_cursor(vim_line_first_non_ws(
                    self.editor.content(),
                    self.editor.cursor(),
                ));
                self.state.preferred_column = None;
                true
            }
            '$' => {
                self.editor
                    .set_cursor(vim_line_end(self.editor.content(), self.editor.cursor()));
                self.state.preferred_column = None;
                true
            }
            'g' => {
                self.state.pending = Some(PendingState::GoToLine);
                true
            }
            'G' => {
                self.move_to_line(false);
                true
            }
            'f' => {
                self.state.pending = Some(PendingState::Find {
                    till: false,
                    forward: true,
                });
                true
            }
            'F' => {
                self.state.pending = Some(PendingState::Find {
                    till: false,
                    forward: false,
                });
                true
            }
            't' => {
                self.state.pending = Some(PendingState::Find {
                    till: true,
                    forward: true,
                });
                true
            }
            'T' => {
                self.state.pending = Some(PendingState::Find {
                    till: true,
                    forward: false,
                });
                true
            }
            ';' => self.repeat_find(false),
            ',' => self.repeat_find(true),
            'x' => {
                if self.editor.cursor() < self.editor.content().len() {
                    self.editor.delete_char_forward();
                    self.state.last_change = Some(RepeatableCommand::DeleteChar);
                }
                true
            }
            'd' => {
                self.state.pending = Some(PendingState::Operator(Operator::Delete));
                true
            }
            'c' => {
                self.state.pending = Some(PendingState::Operator(Operator::Change));
                true
            }
            'y' => {
                self.state.pending = Some(PendingState::Operator(Operator::Yank));
                true
            }
            '>' => {
                self.state.pending = Some(PendingState::Operator(Operator::Indent));
                true
            }
            '<' => {
                self.state.pending = Some(PendingState::Operator(Operator::Outdent));
                true
            }
            'D' => {
                self.delete_to_line_end();
                self.state.last_change = Some(RepeatableCommand::DeleteToLineEnd);
                true
            }
            'C' => {
                self.change_to_line_end();
                true
            }
            'Y' => {
                self.yank_current_line();
                true
            }
            'p' => {
                self.paste(true);
                self.state.last_change = Some(RepeatableCommand::PasteAfter);
                true
            }
            'P' => {
                self.paste(false);
                self.state.last_change = Some(RepeatableCommand::PasteBefore);
                true
            }
            'J' => {
                self.join_lines();
                self.state.last_change = Some(RepeatableCommand::JoinLines);
                true
            }
            '.' => {
                self.repeat_last_change();
                true
            }
            'i' => {
                self.start_insert(InsertKind::Insert);
                true
            }
            'I' => {
                let start = vim_line_first_non_ws(self.editor.content(), self.editor.cursor());
                self.editor.set_cursor(start);
                self.start_insert(InsertKind::InsertStart);
                true
            }
            'a' => {
                if self.editor.cursor() < self.editor.content().len() {
                    self.editor.move_right();
                }
                self.start_insert(InsertKind::Append);
                true
            }
            'A' => {
                let end = vim_line_end(self.editor.content(), self.editor.cursor());
                self.editor.set_cursor(end);
                self.start_insert(InsertKind::AppendEnd);
                true
            }
            'o' => {
                self.open_line(true);
                self.start_insert(InsertKind::OpenBelow);
                true
            }
            'O' => {
                self.open_line(false);
                self.start_insert(InsertKind::OpenAbove);
                true
            }
            _ => false,
        }
    }

    fn handle_operator(&mut self, operator: Operator, ch: char) -> bool {
        match (operator, ch) {
            (Operator::Delete, 'd') | (Operator::Change, 'c') | (Operator::Yank, 'y') => {
                self.apply_line_operator(operator);
                true
            }
            (Operator::Indent, '>') | (Operator::Outdent, '<') => {
                self.apply_line_operator(operator);
                true
            }
            (_, 'w') => self.apply_motion_operator(operator, Motion::WordForward),
            (_, 'e') => self.apply_motion_operator(operator, Motion::EndWord),
            (_, 'b') => self.apply_motion_operator(operator, Motion::WordBackward),
            (_, 'i') => {
                self.state.pending = Some(PendingState::TextObject(operator, false));
                true
            }
            (_, 'a') => {
                self.state.pending = Some(PendingState::TextObject(operator, true));
                true
            }
            _ => false,
        }
    }

    fn handle_text_object(&mut self, operator: Operator, around: bool, ch: char) -> bool {
        let object = match ch {
            'w' => TextObjectSpec::Word { around, big: false },
            'W' => TextObjectSpec::Word { around, big: true },
            '"' => TextObjectSpec::Delimited {
                around,
                open: '"',
                close: '"',
            },
            '\'' => TextObjectSpec::Delimited {
                around,
                open: '\'',
                close: '\'',
            },
            '(' => TextObjectSpec::Delimited {
                around,
                open: '(',
                close: ')',
            },
            '[' => TextObjectSpec::Delimited {
                around,
                open: '[',
                close: ']',
            },
            '{' => TextObjectSpec::Delimited {
                around,
                open: '{',
                close: '}',
            },
            _ => return false,
        };

        let handled = self.apply_text_object_operator(operator, object);
        if handled {
            self.state.last_change = match operator {
                Operator::Delete | Operator::Indent | Operator::Outdent => {
                    Some(RepeatableCommand::OperateTextObject { operator, object })
                }
                Operator::Change | Operator::Yank => None,
            };
        }
        handled
    }

    fn handle_find(&mut self, forward: bool, till: bool, ch: char) -> bool {
        if let Some(pos) = vim_find_char(
            self.editor.content(),
            self.editor.cursor(),
            ch,
            forward,
            till,
        ) {
            self.editor.set_cursor(pos);
            self.state.last_find = Some(FindState { ch, till, forward });
            self.state.preferred_column = None;
        }
        true
    }

    fn repeat_find(&mut self, reverse: bool) -> bool {
        let Some(find) = self.state.last_find else {
            return true;
        };
        let forward = if reverse { !find.forward } else { find.forward };
        self.handle_find(forward, find.till, find.ch)
    }

    fn start_insert(&mut self, kind: InsertKind) {
        self.state.set_mode(VimMode::Insert);
        self.state.pending = None;
        self.state.insert_capture = Some(InsertCapture {
            repeat: InsertRepeat::Insert(kind),
            start: self.editor.cursor(),
        });
    }

    fn finish_insert_capture(&mut self) {
        let Some(capture) = self.state.insert_capture.take() else {
            return;
        };
        let cursor = self.editor.cursor();
        if cursor >= capture.start {
            let inserted = self.editor.content()[capture.start..cursor].to_string();
            self.state.last_change = match capture.repeat {
                InsertRepeat::Insert(_) if inserted.is_empty() => None,
                InsertRepeat::Insert(kind) => Some(RepeatableCommand::InsertText {
                    kind,
                    text: inserted,
                }),
                InsertRepeat::Change(target) => Some(RepeatableCommand::Change {
                    target,
                    text: inserted,
                }),
            };
        }
    }

    fn begin_change(&mut self, start: usize, end: usize, target: ChangeTarget) {
        self.capture_range(start, end);
        self.replace_range(start, end, "");
        self.state.set_mode(VimMode::Insert);
        self.state.insert_capture = Some(InsertCapture {
            repeat: InsertRepeat::Change(target),
            start,
        });
    }

    fn start_change(&mut self, target: ChangeTarget) -> bool {
        match target {
            ChangeTarget::Motion(motion) => {
                let Some((start, end)) =
                    vim_motion_range(self.editor.content(), self.editor.cursor(), motion)
                else {
                    return true;
                };
                self.begin_change(start, end, target);
                true
            }
            ChangeTarget::TextObject(object) => {
                let Some((start, end)) =
                    vim_text_object_range(self.editor.content(), self.editor.cursor(), object)
                else {
                    return true;
                };
                self.begin_change(start, end, target);
                true
            }
            ChangeTarget::Line => {
                let (start, end) =
                    vim_current_line_bounds(self.editor.content(), self.editor.cursor());
                self.begin_change(start, end, target);
                true
            }
            ChangeTarget::LineEnd => {
                let start = self.editor.cursor();
                let end = vim_line_end(self.editor.content(), self.editor.cursor());
                self.begin_change(start, end, target);
                true
            }
        }
    }

    fn move_motion(&mut self, motion: Motion) {
        let next = match motion {
            Motion::WordForward => vim_next_word_start(self.editor.content(), self.editor.cursor()),
            Motion::EndWord => vim_end_word(self.editor.content(), self.editor.cursor()),
            Motion::WordBackward => {
                vim_prev_word_start(self.editor.content(), self.editor.cursor())
            }
        };
        self.editor.set_cursor(next);
        self.state.preferred_column = None;
    }

    fn move_vertical(&mut self, down: bool) {
        let content = self.editor.content();
        let (line_start, line_end) = vim_current_line_bounds(content, self.editor.cursor());
        let column = self
            .state
            .preferred_column
            .unwrap_or_else(|| self.editor.cursor().saturating_sub(line_start));
        let target = if down {
            if line_end >= content.len() {
                self.editor.cursor()
            } else {
                let next_start = line_end + 1;
                let next_end = content[next_start..]
                    .find('\n')
                    .map(|idx| next_start + idx)
                    .unwrap_or(content.len());
                (next_start + column).min(next_end)
            }
        } else if line_start == 0 {
            self.editor.cursor()
        } else {
            let prev_end = line_start - 1;
            let prev_start = content[..prev_end]
                .rfind('\n')
                .map(|idx| idx + 1)
                .unwrap_or(0);
            (prev_start + column).min(prev_end)
        };
        self.editor.set_cursor(target);
        self.state.preferred_column = Some(column);
    }

    fn move_to_line(&mut self, first: bool) {
        let content = self.editor.content();
        let (current_start, _) = vim_current_line_bounds(content, self.editor.cursor());
        let column = self
            .state
            .preferred_column
            .unwrap_or_else(|| self.editor.cursor().saturating_sub(current_start));
        let target = if first {
            column.min(content.find('\n').unwrap_or(content.len()))
        } else {
            let last_start = content.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
            let last_end = content[last_start..]
                .find('\n')
                .map(|idx| last_start + idx)
                .unwrap_or(content.len());
            (last_start + column).min(last_end)
        };
        self.editor.set_cursor(target);
        self.state.preferred_column = Some(column);
    }

    fn apply_motion_operator(&mut self, operator: Operator, motion: Motion) -> bool {
        if operator == Operator::Change {
            return self.start_change(ChangeTarget::Motion(motion));
        }

        let Some((start, end)) =
            vim_motion_range(self.editor.content(), self.editor.cursor(), motion)
        else {
            return true;
        };
        self.apply_range_operator(operator, start, end);
        if operator != Operator::Yank {
            self.state.last_change = Some(RepeatableCommand::OperateMotion { operator, motion });
        }
        true
    }

    fn apply_text_object_operator(&mut self, operator: Operator, object: TextObjectSpec) -> bool {
        if operator == Operator::Change {
            return self.start_change(ChangeTarget::TextObject(object));
        }

        let Some((start, end)) =
            vim_text_object_range(self.editor.content(), self.editor.cursor(), object)
        else {
            return true;
        };
        self.apply_range_operator(operator, start, end);
        true
    }

    fn apply_line_operator(&mut self, operator: Operator) {
        if operator == Operator::Change {
            let _ = self.start_change(ChangeTarget::Line);
            return;
        }

        let (start, end) = vim_current_line_full_range(self.editor.content(), self.editor.cursor());
        self.apply_range_operator(operator, start, end);
        if operator != Operator::Yank {
            self.state.last_change = Some(RepeatableCommand::OperateLine { operator });
        }
    }

    fn apply_range_operator(&mut self, operator: Operator, start: usize, end: usize) {
        if start > end || end > self.editor.content().len() {
            return;
        }

        match operator {
            Operator::Yank => self.capture_range(start, end),
            Operator::Delete => {
                self.capture_range(start, end);
                self.replace_range(start, end, "");
            }
            Operator::Indent => self.indent_range(start, end, true),
            Operator::Outdent => self.indent_range(start, end, false),
            Operator::Change => {}
        }
    }

    fn indent_range(&mut self, start: usize, end: usize, indent: bool) {
        let content = self.editor.content().to_string();
        let mut out = String::with_capacity(content.len() + INDENT.len());
        let mut line_start = 0;
        for segment in content.split_inclusive('\n') {
            let line_end = line_start + segment.len();
            if line_end > start && line_start <= end {
                if indent {
                    out.push_str(INDENT);
                    out.push_str(segment);
                } else if let Some(stripped) = segment.strip_prefix(INDENT) {
                    out.push_str(stripped);
                } else {
                    out.push_str(segment.trim_start_matches(' '));
                }
            } else {
                out.push_str(segment);
            }
            line_start = line_end;
        }
        if !content.ends_with('\n') && line_start < content.len() {
            let tail = &content[line_start..];
            if line_start <= end && content.len() > start {
                if indent {
                    out.push_str(INDENT);
                    out.push_str(tail);
                } else if let Some(stripped) = tail.strip_prefix(INDENT) {
                    out.push_str(stripped);
                } else {
                    out.push_str(tail.trim_start_matches(' '));
                }
            }
        }
        self.editor.replace(out, start.min(content.len()));
    }

    fn replace_range(&mut self, start: usize, end: usize, replacement: &str) {
        let mut content = self.editor.content().to_string();
        content.replace_range(start..end, replacement);
        self.editor.replace(content, start + replacement.len());
    }

    fn capture_range(&mut self, start: usize, end: usize) {
        *self.clipboard = self.editor.content()[start..end].to_string();
        self.state.clipboard_kind = if vim_is_linewise_range(self.editor.content(), start, end) {
            ClipboardKind::LineWise
        } else {
            ClipboardKind::CharWise
        };
    }

    fn delete_to_line_end(&mut self) {
        let end = vim_line_end(self.editor.content(), self.editor.cursor());
        self.capture_range(self.editor.cursor(), end);
        self.replace_range(self.editor.cursor(), end, "");
    }

    fn change_to_line_end(&mut self) {
        let _ = self.start_change(ChangeTarget::LineEnd);
    }

    fn yank_current_line(&mut self) {
        let (start, end) = vim_current_line_full_range(self.editor.content(), self.editor.cursor());
        self.capture_range(start, end);
    }

    fn open_line(&mut self, below: bool) {
        let insert_at = if below {
            let end = vim_line_end(self.editor.content(), self.editor.cursor());
            if end < self.editor.content().len() {
                end + 1
            } else {
                end
            }
        } else {
            vim_line_start(self.editor.content(), self.editor.cursor())
        };
        let mut content = self.editor.content().to_string();
        content.insert(insert_at, '\n');
        self.editor.replace(content, insert_at + 1);
    }

    fn paste(&mut self, after: bool) {
        if self.clipboard.is_empty() {
            return;
        }
        match self.state.clipboard_kind {
            ClipboardKind::CharWise => {
                let mut content = self.editor.content().to_string();
                let insert_at = if after && self.editor.cursor() < content.len() {
                    next_char_boundary(&content, self.editor.cursor())
                } else {
                    self.editor.cursor()
                };
                content.insert_str(insert_at, self.clipboard);
                self.editor
                    .replace(content, insert_at + self.clipboard.len());
            }
            ClipboardKind::LineWise => {
                let mut content = self.editor.content().to_string();
                let (line_start, line_end) =
                    vim_current_line_bounds(&content, self.editor.cursor());
                let insert_at = if after {
                    if line_end < content.len() {
                        line_end + 1
                    } else {
                        line_end
                    }
                } else {
                    line_start
                };
                content.insert_str(insert_at, self.clipboard);
                let cursor = (insert_at + self.clipboard.len()).min(content.len());
                self.editor.replace(content, cursor);
            }
        }
    }

    fn join_lines(&mut self) {
        let content = self.editor.content().to_string();
        let (_, line_end) = vim_current_line_bounds(&content, self.editor.cursor());
        if line_end >= content.len() {
            return;
        }
        let next_start = line_end + 1;
        let next_non_ws = content[next_start..]
            .char_indices()
            .find_map(|(idx, ch)| (!ch.is_whitespace()).then_some(next_start + idx))
            .unwrap_or(next_start);
        let mut joined = content;
        joined.replace_range(line_end..next_non_ws, " ");
        self.editor.replace(joined, line_end + 1);
    }

    fn repeat_last_change(&mut self) {
        let Some(command) = self.state.last_change.clone() else {
            return;
        };
        match command {
            RepeatableCommand::DeleteChar => {
                if self.editor.cursor() < self.editor.content().len() {
                    self.editor.delete_char_forward();
                }
            }
            RepeatableCommand::PasteAfter => self.paste(true),
            RepeatableCommand::PasteBefore => self.paste(false),
            RepeatableCommand::JoinLines => self.join_lines(),
            RepeatableCommand::InsertText { kind, text } => self.repeat_insert(kind, &text),
            RepeatableCommand::OperateMotion { operator, motion } => {
                let _ = self.apply_motion_operator(operator, motion);
            }
            RepeatableCommand::OperateTextObject { operator, object } => {
                let _ = self.apply_text_object_operator(operator, object);
            }
            RepeatableCommand::OperateLine { operator } => self.apply_line_operator(operator),
            RepeatableCommand::DeleteToLineEnd => self.delete_to_line_end(),
            RepeatableCommand::Change { target, text } => self.repeat_change(target, &text),
        }
    }

    fn repeat_insert(&mut self, kind: InsertKind, text: &str) {
        match kind {
            InsertKind::Insert => {}
            InsertKind::InsertStart => {
                let start = vim_line_first_non_ws(self.editor.content(), self.editor.cursor());
                self.editor.set_cursor(start);
            }
            InsertKind::Append => {
                if self.editor.cursor() < self.editor.content().len() {
                    self.editor.move_right();
                }
            }
            InsertKind::AppendEnd => {
                let end = vim_line_end(self.editor.content(), self.editor.cursor());
                self.editor.set_cursor(end);
            }
            InsertKind::OpenBelow => self.open_line(true),
            InsertKind::OpenAbove => self.open_line(false),
        }
        self.editor.insert_text(text);
        self.state.set_mode(VimMode::Normal);
    }

    fn repeat_change(&mut self, target: ChangeTarget, text: &str) {
        if !self.start_change(target) {
            return;
        }
        self.editor.insert_text(text);
        self.finish_insert_capture();
        self.state.set_mode(VimMode::Normal);
    }
}
