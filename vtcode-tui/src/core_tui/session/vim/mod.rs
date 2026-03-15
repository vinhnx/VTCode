use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::Session;

mod text;
mod types;

use self::text::{
    next_char_boundary, vim_current_line_bounds, vim_current_line_full_range, vim_end_word,
    vim_find_char, vim_is_linewise_range, vim_line_end, vim_line_first_non_ws, vim_line_start,
    vim_motion_range, vim_next_word_start, vim_prev_word_start, vim_text_object_range,
};
pub(crate) use self::types::VimMode;
pub(crate) use self::types::VimState;
use self::types::{
    ChangeTarget, ClipboardKind, FindState, InsertCapture, InsertKind, InsertRepeat, Motion,
    Operator, PendingState, RepeatableCommand, TextObjectSpec,
};

const INDENT: &str = "    ";

impl Session {
    pub(super) fn handle_vim_key(&mut self, key: &KeyEvent) -> bool {
        if !self.vim_state.enabled() || !self.input_enabled || self.file_palette_active {
            return false;
        }

        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return false;
        }

        match self.vim_state.mode() {
            VimMode::Insert => self.handle_vim_insert_key(key),
            VimMode::Normal => self.handle_vim_normal_key(key),
        }
    }

    fn handle_vim_insert_key(&mut self, key: &KeyEvent) -> bool {
        if matches!(key.code, KeyCode::Esc) {
            self.finish_vim_insert_capture();
            self.vim_state.set_mode(VimMode::Normal);
            self.input_manager.clear_selection();
            self.vim_state.pending = None;
            self.mark_dirty();
            return true;
        }
        false
    }

    fn handle_vim_normal_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.vim_state.pending = None;
                self.input_manager.clear_selection();
                self.mark_dirty();
                true
            }
            KeyCode::Char(ch) => {
                let handled = if let Some(pending) = self.vim_state.pending.take() {
                    self.handle_vim_pending(pending, ch)
                } else {
                    self.handle_vim_normal_char(ch)
                };
                if handled {
                    self.check_file_reference_trigger();
                    self.mark_dirty();
                }
                handled
            }
            KeyCode::Left => {
                self.move_left();
                self.vim_state.preferred_column = None;
                self.mark_dirty();
                true
            }
            KeyCode::Right => {
                self.move_right();
                self.vim_state.preferred_column = None;
                self.mark_dirty();
                true
            }
            KeyCode::Up => {
                self.vim_move_vertical(false);
                self.mark_dirty();
                true
            }
            KeyCode::Down => {
                self.vim_move_vertical(true);
                self.mark_dirty();
                true
            }
            _ => false,
        }
    }

    fn handle_vim_pending(&mut self, pending: PendingState, ch: char) -> bool {
        match pending {
            PendingState::Operator(operator) => self.handle_vim_operator(operator, ch),
            PendingState::TextObject(operator, around) => {
                self.handle_vim_text_object(operator, around, ch)
            }
            PendingState::Find { till, forward } => self.handle_vim_find(forward, till, ch),
            PendingState::GoToLine => {
                if ch == 'g' {
                    self.vim_move_to_line(true);
                    true
                } else {
                    false
                }
            }
        }
    }

    fn handle_vim_normal_char(&mut self, ch: char) -> bool {
        match ch {
            'h' => {
                self.move_left();
                self.vim_state.preferred_column = None;
                true
            }
            'l' => {
                self.move_right();
                self.vim_state.preferred_column = None;
                true
            }
            'j' => {
                self.vim_move_vertical(true);
                true
            }
            'k' => {
                self.vim_move_vertical(false);
                true
            }
            'w' => {
                self.vim_move_motion(Motion::WordForward);
                true
            }
            'e' => {
                self.vim_move_motion(Motion::EndWord);
                true
            }
            'b' => {
                self.vim_move_motion(Motion::WordBackward);
                true
            }
            '0' => {
                self.set_cursor(vim_line_start(self.input_manager.content(), self.cursor()));
                self.vim_state.preferred_column = None;
                true
            }
            '^' => {
                self.set_cursor(vim_line_first_non_ws(
                    self.input_manager.content(),
                    self.cursor(),
                ));
                self.vim_state.preferred_column = None;
                true
            }
            '$' => {
                self.set_cursor(vim_line_end(self.input_manager.content(), self.cursor()));
                self.vim_state.preferred_column = None;
                true
            }
            'g' => {
                self.vim_state.pending = Some(PendingState::GoToLine);
                true
            }
            'G' => {
                self.vim_move_to_line(false);
                true
            }
            'f' => {
                self.vim_state.pending = Some(PendingState::Find {
                    till: false,
                    forward: true,
                });
                true
            }
            'F' => {
                self.vim_state.pending = Some(PendingState::Find {
                    till: false,
                    forward: false,
                });
                true
            }
            't' => {
                self.vim_state.pending = Some(PendingState::Find {
                    till: true,
                    forward: true,
                });
                true
            }
            'T' => {
                self.vim_state.pending = Some(PendingState::Find {
                    till: true,
                    forward: false,
                });
                true
            }
            ';' => self.repeat_vim_find(false),
            ',' => self.repeat_vim_find(true),
            'x' => {
                if self.cursor() < self.input_manager.content().len() {
                    self.delete_char_forward();
                    self.vim_state.last_change = Some(RepeatableCommand::DeleteChar);
                }
                true
            }
            'd' => {
                self.vim_state.pending = Some(PendingState::Operator(Operator::Delete));
                true
            }
            'c' => {
                self.vim_state.pending = Some(PendingState::Operator(Operator::Change));
                true
            }
            'y' => {
                self.vim_state.pending = Some(PendingState::Operator(Operator::Yank));
                true
            }
            '>' => {
                self.vim_state.pending = Some(PendingState::Operator(Operator::Indent));
                true
            }
            '<' => {
                self.vim_state.pending = Some(PendingState::Operator(Operator::Outdent));
                true
            }
            'D' => {
                self.vim_delete_to_line_end();
                self.vim_state.last_change = Some(RepeatableCommand::DeleteToLineEnd);
                true
            }
            'C' => {
                self.vim_change_to_line_end();
                true
            }
            'Y' => {
                self.vim_yank_current_line();
                true
            }
            'p' => {
                self.vim_paste(true);
                self.vim_state.last_change = Some(RepeatableCommand::PasteAfter);
                true
            }
            'P' => {
                self.vim_paste(false);
                self.vim_state.last_change = Some(RepeatableCommand::PasteBefore);
                true
            }
            'J' => {
                self.vim_join_lines();
                self.vim_state.last_change = Some(RepeatableCommand::JoinLines);
                true
            }
            '.' => {
                self.vim_repeat_last_change();
                true
            }
            'i' => {
                self.start_vim_insert(InsertKind::Insert);
                true
            }
            'I' => {
                let start = vim_line_first_non_ws(self.input_manager.content(), self.cursor());
                self.set_cursor(start);
                self.start_vim_insert(InsertKind::InsertStart);
                true
            }
            'a' => {
                if self.cursor() < self.input_manager.content().len() {
                    self.move_right();
                }
                self.start_vim_insert(InsertKind::Append);
                true
            }
            'A' => {
                let end = vim_line_end(self.input_manager.content(), self.cursor());
                self.set_cursor(end);
                self.start_vim_insert(InsertKind::AppendEnd);
                true
            }
            'o' => {
                self.vim_open_line(true);
                self.start_vim_insert(InsertKind::OpenBelow);
                true
            }
            'O' => {
                self.vim_open_line(false);
                self.start_vim_insert(InsertKind::OpenAbove);
                true
            }
            _ => false,
        }
    }

    fn handle_vim_operator(&mut self, operator: Operator, ch: char) -> bool {
        match (operator, ch) {
            (Operator::Delete, 'd') | (Operator::Change, 'c') | (Operator::Yank, 'y') => {
                self.vim_apply_line_operator(operator);
                true
            }
            (Operator::Indent, '>') | (Operator::Outdent, '<') => {
                self.vim_apply_line_operator(operator);
                true
            }
            (_, 'w') => self.vim_apply_motion_operator(operator, Motion::WordForward),
            (_, 'e') => self.vim_apply_motion_operator(operator, Motion::EndWord),
            (_, 'b') => self.vim_apply_motion_operator(operator, Motion::WordBackward),
            (_, 'i') => {
                self.vim_state.pending = Some(PendingState::TextObject(operator, false));
                true
            }
            (_, 'a') => {
                self.vim_state.pending = Some(PendingState::TextObject(operator, true));
                true
            }
            _ => false,
        }
    }

    fn handle_vim_text_object(&mut self, operator: Operator, around: bool, ch: char) -> bool {
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

        let handled = self.vim_apply_text_object_operator(operator, object);
        if handled {
            self.vim_state.last_change = match operator {
                Operator::Delete | Operator::Indent | Operator::Outdent => {
                    Some(RepeatableCommand::OperateTextObject { operator, object })
                }
                Operator::Yank => None,
                Operator::Change => None,
            };
        }
        handled
    }

    fn handle_vim_find(&mut self, forward: bool, till: bool, ch: char) -> bool {
        if let Some(pos) = vim_find_char(
            self.input_manager.content(),
            self.cursor(),
            ch,
            forward,
            till,
        ) {
            self.set_cursor(pos);
            self.vim_state.last_find = Some(FindState { ch, till, forward });
            self.vim_state.preferred_column = None;
        }
        true
    }

    fn repeat_vim_find(&mut self, reverse: bool) -> bool {
        let Some(find) = self.vim_state.last_find else {
            return true;
        };
        let forward = if reverse { !find.forward } else { find.forward };
        self.handle_vim_find(forward, find.till, find.ch)
    }

    fn start_vim_insert(&mut self, kind: InsertKind) {
        self.vim_state.set_mode(VimMode::Insert);
        self.vim_state.pending = None;
        self.vim_state.insert_capture = Some(InsertCapture {
            repeat: InsertRepeat::Insert(kind),
            start: self.cursor(),
        });
    }

    fn finish_vim_insert_capture(&mut self) {
        let Some(capture) = self.vim_state.insert_capture.take() else {
            return;
        };
        let cursor = self.cursor();
        if cursor >= capture.start {
            let inserted = self.input_manager.content()[capture.start..cursor].to_string();
            self.vim_state.last_change = match capture.repeat {
                InsertRepeat::Insert(kind) if inserted.is_empty() => None,
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

    fn vim_begin_change(&mut self, start: usize, end: usize, target: ChangeTarget) {
        self.clipboard = self.input_manager.content()[start..end].to_string();
        self.vim_state.clipboard_kind =
            if vim_is_linewise_range(self.input_manager.content(), start, end) {
                ClipboardKind::LineWise
            } else {
                ClipboardKind::CharWise
            };
        self.vim_replace_range(start, end, "");
        self.vim_state.set_mode(VimMode::Insert);
        self.vim_state.insert_capture = Some(InsertCapture {
            repeat: InsertRepeat::Change(target),
            start,
        });
    }

    fn vim_start_change(&mut self, target: ChangeTarget) -> bool {
        match target {
            ChangeTarget::Motion(motion) => {
                let Some((start, end)) =
                    vim_motion_range(self.input_manager.content(), self.cursor(), motion)
                else {
                    return true;
                };
                self.vim_begin_change(start, end, target);
                true
            }
            ChangeTarget::TextObject(object) => {
                let Some((start, end)) =
                    vim_text_object_range(self.input_manager.content(), self.cursor(), object)
                else {
                    return true;
                };
                self.vim_begin_change(start, end, target);
                true
            }
            ChangeTarget::Line => {
                let (start, end) =
                    vim_current_line_bounds(self.input_manager.content(), self.cursor());
                self.vim_begin_change(start, end, target);
                true
            }
            ChangeTarget::LineEnd => {
                let start = self.cursor();
                let end = vim_line_end(self.input_manager.content(), self.cursor());
                self.vim_begin_change(start, end, target);
                true
            }
        }
    }

    fn vim_move_motion(&mut self, motion: Motion) {
        let next = match motion {
            Motion::WordForward => vim_next_word_start(self.input_manager.content(), self.cursor()),
            Motion::EndWord => vim_end_word(self.input_manager.content(), self.cursor()),
            Motion::WordBackward => {
                vim_prev_word_start(self.input_manager.content(), self.cursor())
            }
        };
        self.set_cursor(next);
        self.vim_state.preferred_column = None;
    }

    fn vim_move_vertical(&mut self, down: bool) {
        let content = self.input_manager.content();
        let (line_start, line_end) = vim_current_line_bounds(content, self.cursor());
        let column = self
            .vim_state
            .preferred_column
            .unwrap_or_else(|| self.cursor().saturating_sub(line_start));
        let target = if down {
            if line_end >= content.len() {
                self.cursor()
            } else {
                let next_start = line_end + 1;
                let next_end = content[next_start..]
                    .find('\n')
                    .map(|idx| next_start + idx)
                    .unwrap_or(content.len());
                (next_start + column).min(next_end)
            }
        } else if line_start == 0 {
            self.cursor()
        } else {
            let prev_end = line_start - 1;
            let prev_start = content[..prev_end]
                .rfind('\n')
                .map(|idx| idx + 1)
                .unwrap_or(0);
            (prev_start + column).min(prev_end)
        };
        self.set_cursor(target);
        self.vim_state.preferred_column = Some(column);
    }

    fn vim_move_to_line(&mut self, first: bool) {
        let content = self.input_manager.content();
        let (current_start, _) = vim_current_line_bounds(content, self.cursor());
        let column = self
            .vim_state
            .preferred_column
            .unwrap_or_else(|| self.cursor().saturating_sub(current_start));
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
        self.set_cursor(target);
        self.vim_state.preferred_column = Some(column);
    }

    fn vim_apply_motion_operator(&mut self, operator: Operator, motion: Motion) -> bool {
        if operator == Operator::Change {
            return self.vim_start_change(ChangeTarget::Motion(motion));
        }

        let Some((start, end)) =
            vim_motion_range(self.input_manager.content(), self.cursor(), motion)
        else {
            return true;
        };
        self.vim_apply_range_operator(operator, start, end);
        if operator != Operator::Yank {
            self.vim_state.last_change =
                Some(RepeatableCommand::OperateMotion { operator, motion });
        }
        true
    }

    fn vim_apply_text_object_operator(
        &mut self,
        operator: Operator,
        object: TextObjectSpec,
    ) -> bool {
        if operator == Operator::Change {
            return self.vim_start_change(ChangeTarget::TextObject(object));
        }

        let Some((start, end)) =
            vim_text_object_range(self.input_manager.content(), self.cursor(), object)
        else {
            return true;
        };
        self.vim_apply_range_operator(operator, start, end);
        true
    }

    fn vim_apply_line_operator(&mut self, operator: Operator) {
        if operator == Operator::Change {
            let _ = self.vim_start_change(ChangeTarget::Line);
            return;
        }

        let (start, end) = vim_current_line_full_range(self.input_manager.content(), self.cursor());
        self.vim_apply_range_operator(operator, start, end);
        if operator != Operator::Yank {
            self.vim_state.last_change = Some(RepeatableCommand::OperateLine { operator });
        }
    }

    fn vim_apply_range_operator(&mut self, operator: Operator, start: usize, end: usize) {
        if start > end || end > self.input_manager.content().len() {
            return;
        }

        match operator {
            Operator::Yank => {
                self.clipboard = self.input_manager.content()[start..end].to_string();
                self.vim_state.clipboard_kind =
                    if vim_is_linewise_range(self.input_manager.content(), start, end) {
                        ClipboardKind::LineWise
                    } else {
                        ClipboardKind::CharWise
                    };
            }
            Operator::Delete => {
                self.clipboard = self.input_manager.content()[start..end].to_string();
                self.vim_state.clipboard_kind =
                    if vim_is_linewise_range(self.input_manager.content(), start, end) {
                        ClipboardKind::LineWise
                    } else {
                        ClipboardKind::CharWise
                    };
                self.vim_replace_range(start, end, "");
            }
            Operator::Indent => self.vim_indent_range(start, end, true),
            Operator::Outdent => self.vim_indent_range(start, end, false),
            Operator::Change => {}
        }
    }

    fn vim_indent_range(&mut self, start: usize, end: usize, indent: bool) {
        let content = self.input_manager.content().to_string();
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
        self.input_manager.set_content(out);
        self.input_manager
            .set_cursor(start.min(self.input_manager.content().len()));
        self.refresh_input_edit_state();
    }

    fn vim_replace_range(&mut self, start: usize, end: usize, replacement: &str) {
        let mut content = self.input_manager.content().to_string();
        content.replace_range(start..end, replacement);
        self.input_manager.set_content(content);
        self.input_manager.set_cursor(start + replacement.len());
        self.refresh_input_edit_state();
    }

    fn vim_delete_to_line_end(&mut self) {
        let end = vim_line_end(self.input_manager.content(), self.cursor());
        self.clipboard = self.input_manager.content()[self.cursor()..end].to_string();
        self.vim_state.clipboard_kind = ClipboardKind::CharWise;
        self.vim_replace_range(self.cursor(), end, "");
    }

    fn vim_change_to_line_end(&mut self) {
        let _ = self.vim_start_change(ChangeTarget::LineEnd);
    }

    fn vim_yank_current_line(&mut self) {
        let (start, end) = vim_current_line_full_range(self.input_manager.content(), self.cursor());
        self.clipboard = self.input_manager.content()[start..end].to_string();
        self.vim_state.clipboard_kind = ClipboardKind::LineWise;
    }

    fn vim_open_line(&mut self, below: bool) {
        let insert_at = if below {
            let end = vim_line_end(self.input_manager.content(), self.cursor());
            if end < self.input_manager.content().len() {
                end + 1
            } else {
                end
            }
        } else {
            vim_line_start(self.input_manager.content(), self.cursor())
        };
        let mut content = self.input_manager.content().to_string();
        content.insert(insert_at, '\n');
        self.input_manager.set_content(content);
        self.input_manager.set_cursor(insert_at + 1);
        self.refresh_input_edit_state();
    }

    fn vim_paste(&mut self, after: bool) {
        if self.clipboard.is_empty() {
            return;
        }
        match self.vim_state.clipboard_kind {
            ClipboardKind::CharWise => {
                let mut content = self.input_manager.content().to_string();
                let insert_at = if after && self.cursor() < content.len() {
                    next_char_boundary(&content, self.cursor())
                } else {
                    self.cursor()
                };
                content.insert_str(insert_at, &self.clipboard);
                self.input_manager.set_content(content);
                self.input_manager
                    .set_cursor(insert_at + self.clipboard.len());
            }
            ClipboardKind::LineWise => {
                let mut content = self.input_manager.content().to_string();
                let (line_start, line_end) = vim_current_line_bounds(&content, self.cursor());
                let insert_at = if after {
                    if line_end < content.len() {
                        line_end + 1
                    } else {
                        line_end
                    }
                } else {
                    line_start
                };
                content.insert_str(insert_at, &self.clipboard);
                self.input_manager.set_content(content);
                self.input_manager.set_cursor(
                    (insert_at + self.clipboard.len()).min(self.input_manager.content().len()),
                );
            }
        }
        self.refresh_input_edit_state();
    }

    fn vim_join_lines(&mut self) {
        let content = self.input_manager.content().to_string();
        let (_, line_end) = vim_current_line_bounds(&content, self.cursor());
        if line_end >= content.len() {
            return;
        }
        let next_start = line_end + 1;
        let next_non_ws = content[next_start..]
            .char_indices()
            .find_map(|(idx, ch)| (!ch.is_whitespace()).then_some(next_start + idx))
            .unwrap_or(next_start);
        let mut joined = content.clone();
        joined.replace_range(line_end..next_non_ws, " ");
        self.input_manager.set_content(joined);
        self.input_manager.set_cursor(line_end + 1);
        self.refresh_input_edit_state();
    }

    fn vim_repeat_last_change(&mut self) {
        let Some(command) = self.vim_state.last_change.clone() else {
            return;
        };
        match command {
            RepeatableCommand::DeleteChar => {
                if self.cursor() < self.input_manager.content().len() {
                    self.delete_char_forward();
                }
            }
            RepeatableCommand::PasteAfter => self.vim_paste(true),
            RepeatableCommand::PasteBefore => self.vim_paste(false),
            RepeatableCommand::JoinLines => self.vim_join_lines(),
            RepeatableCommand::InsertText { kind, text } => self.vim_repeat_insert(kind, &text),
            RepeatableCommand::OperateMotion { operator, motion } => {
                let _ = self.vim_apply_motion_operator(operator, motion);
            }
            RepeatableCommand::OperateTextObject { operator, object } => {
                let _ = self.vim_apply_text_object_operator(operator, object);
            }
            RepeatableCommand::OperateLine { operator } => self.vim_apply_line_operator(operator),
            RepeatableCommand::DeleteToLineEnd => self.vim_delete_to_line_end(),
            RepeatableCommand::Change { target, text } => self.vim_repeat_change(target, &text),
        }
    }

    fn vim_repeat_insert(&mut self, kind: InsertKind, text: &str) {
        match kind {
            InsertKind::Insert => {}
            InsertKind::InsertStart => {
                let start = vim_line_first_non_ws(self.input_manager.content(), self.cursor());
                self.set_cursor(start);
            }
            InsertKind::Append => {
                if self.cursor() < self.input_manager.content().len() {
                    self.move_right();
                }
            }
            InsertKind::AppendEnd => {
                let end = vim_line_end(self.input_manager.content(), self.cursor());
                self.set_cursor(end);
            }
            InsertKind::OpenBelow => self.vim_open_line(true),
            InsertKind::OpenAbove => self.vim_open_line(false),
        }
        self.input_manager.insert_text(text);
        self.refresh_input_edit_state();
        self.vim_state.set_mode(VimMode::Normal);
    }

    fn vim_repeat_change(&mut self, target: ChangeTarget, text: &str) {
        if !self.vim_start_change(target) {
            return;
        }
        self.input_manager.insert_text(text);
        self.refresh_input_edit_state();
        self.finish_vim_insert_capture();
        self.vim_state.set_mode(VimMode::Normal);
    }
}
