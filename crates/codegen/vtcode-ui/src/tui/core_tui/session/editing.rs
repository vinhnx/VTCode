use super::{InlinePromptSuggestionSource, Session};
use crate::tui::config::constants::ui;
use crate::vim::{next_char_boundary, prev_char_boundary};
/// Text editing and cursor movement operations for Session
///
/// This module handles all text manipulation and cursor navigation including:
/// - Character insertion and deletion
/// - Word and sentence-level editing
/// - Cursor movement (character, word, line boundaries)
/// - Input history navigation
/// - Newline handling with capacity limits
use unicode_segmentation::UnicodeSegmentation;

const WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";

fn is_word_separator(ch: char) -> bool {
    WORD_SEPARATORS.contains(ch)
}

/// Strip carriage returns and DEL bytes from pasted text so terminal and
/// clipboard pastes land verbatim without control-code side effects.
fn sanitize_pasted_text(text: &str) -> String {
    text.chars().filter(|&ch| ch != '\r' && ch != '\u{7f}').collect()
}

fn is_separator_piece(piece: &str) -> bool {
    piece.chars().all(is_word_separator)
}

fn split_word_pieces(run: &str) -> Vec<(usize, &str)> {
    let mut pieces = Vec::new();
    for (segment_start, segment) in run.split_word_bound_indices() {
        let mut piece_start = 0;
        let mut chars = segment.char_indices();
        let Some((_, first_char)) = chars.next() else {
            continue;
        };
        let mut in_separator = is_word_separator(first_char);

        for (idx, ch) in chars {
            let is_separator = is_word_separator(ch);
            if is_separator == in_separator {
                continue;
            }

            pieces.push((segment_start + piece_start, &segment[piece_start..idx]));
            piece_start = idx;
            in_separator = is_separator;
        }

        pieces.push((segment_start + piece_start, &segment[piece_start..]));
    }

    pieces
}

fn previous_word_boundary(content: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }

    let prefix = &content[..cursor];
    let Some((first_non_ws_idx, ch)) = prefix.char_indices().rev().find(|&(_, ch)| !ch.is_whitespace()) else {
        return 0;
    };

    let run_start = prefix[..first_non_ws_idx]
        .char_indices()
        .rev()
        .find(|&(_, ch)| ch.is_whitespace())
        .map_or(0, |(idx, ch)| idx + ch.len_utf8());
    let run_end = first_non_ws_idx + ch.len_utf8();
    let pieces = split_word_pieces(&prefix[run_start..run_end]);
    let mut pieces = pieces.into_iter().rev().peekable();
    let Some((piece_start, piece)) = pieces.next() else {
        return run_start;
    };
    let mut start = run_start + piece_start;

    if is_separator_piece(piece) {
        while let Some((idx, piece)) = pieces.peek() {
            if !is_separator_piece(piece) {
                break;
            }
            start = run_start + *idx;
            pieces.next();
        }
    }

    start
}

fn next_word_boundary(content: &str, cursor: usize) -> usize {
    if cursor >= content.len() {
        return content.len();
    }

    let suffix = &content[cursor..];
    let Some(first_non_ws) = suffix.find(|ch: char| !ch.is_whitespace()) else {
        return content.len();
    };

    let run = &suffix[first_non_ws..];
    let run = &run[..run.find(char::is_whitespace).unwrap_or(run.len())];
    let mut pieces = split_word_pieces(run).into_iter().peekable();
    let Some((start, piece)) = pieces.next() else {
        return cursor + first_non_ws;
    };

    let word_start = cursor + first_non_ws + start;
    let mut end = word_start + piece.len();
    if is_separator_piece(piece) {
        while let Some((idx, piece)) = pieces.peek() {
            if !is_separator_piece(piece) {
                break;
            }
            end = cursor + first_non_ws + *idx + piece.len();
            pieces.next();
        }
    }

    end
}

impl Session {
    pub(crate) fn refresh_input_edit_state(&mut self) {
        self.clear_suggested_prompt_state();
        self.clear_inline_prompt_suggestion();
        self.input_compact_mode = self.input_compact_placeholder().is_some();
        self.invalidate_header_cache();
    }

    pub(crate) fn set_inline_prompt_suggestion(&mut self, suggestion: String, llm_generated: bool) {
        let trimmed = suggestion.trim();
        if trimmed.is_empty() {
            self.clear_inline_prompt_suggestion();
            return;
        }

        self.inline_prompt_suggestion.suggestion = Some(trimmed.to_string());
        self.inline_prompt_suggestion.source = Some(if llm_generated {
            InlinePromptSuggestionSource::Llm
        } else {
            InlinePromptSuggestionSource::Local
        });
        self.mark_dirty();
    }

    pub(crate) fn accept_inline_prompt_suggestion(&mut self) -> bool {
        let Some(suffix) = self.visible_inline_prompt_suggestion_suffix() else {
            return false;
        };

        self.input_manager.insert_text(&suffix);
        self.clear_inline_prompt_suggestion();
        self.mark_dirty();
        true
    }

    /// Insert a character at the current cursor position
    #[cfg_attr(feature = "profiling", hotpath::measure)]
    pub(crate) fn insert_char(&mut self, ch: char) {
        if ch == '\u{7f}' {
            return;
        }
        if ch == '\n' && !self.can_insert_newline() {
            return;
        }
        self.input_manager.insert_char(ch);
        self.refresh_input_edit_state();
    }

    /// Insert pasted text without enforcing the inline newline cap.
    ///
    /// This preserves the full block (including large multi-line pastes) so the
    /// agent receives the exact content instead of dropping line breaks after
    /// hitting the interactive input's visual limit. Large pastes — by lines,
    /// chars, images, or file tokens — are tracked as a collapsible block so
    /// the composer summarizes previous content instead of showing full text.
    /// Pasting while already collapsed expands to the full text instead, so
    /// paste-once summarizes and paste-twice reviews the complete content.
    pub(crate) fn insert_paste_text(&mut self, text: &str) {
        let sanitized = sanitize_pasted_text(text);

        if sanitized.is_empty() {
            return;
        }

        let was_collapsed = self.input_compact_mode;
        let paste_start = self
            .input_manager
            .selection_range()
            .map_or_else(|| self.input_manager.cursor(), |(start, _)| start);
        let paste_end = paste_start.saturating_add(sanitized.len());
        let should_collapse = super::input::should_track_compact_paste(&sanitized);
        self.input_manager.insert_text(&sanitized);
        if should_collapse {
            self.input_manager.set_compact_paste_range(paste_start..paste_end);
        }
        self.refresh_input_edit_state();
        if was_collapsed && self.input_compact_placeholder().is_some() {
            self.input_compact_mode = false;
        }
    }

    /// Insert pasted text verbatim, bypassing collapse tracking.
    ///
    /// Shift+Ctrl+V pastes clipboard text as raw full content: no compact
    /// block is tracked and the composer stays expanded so nothing is
    /// summarized away, no matter how large the pasted text is.
    pub(crate) fn insert_raw_paste_text(&mut self, text: &str) {
        let sanitized = sanitize_pasted_text(text);

        if sanitized.is_empty() {
            return;
        }

        self.input_manager.insert_text(&sanitized);
        self.refresh_input_edit_state();
        self.input_compact_mode = false;
    }

    pub(crate) fn apply_suggested_prompt(&mut self, text: String) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        let merged = if self.input_manager.content().trim().is_empty() {
            trimmed.to_string()
        } else {
            let trimmed_end = self.input_manager.content().trim_end();
            let cap = trimmed_end.len() + 2 + trimmed.len();
            let mut s = String::with_capacity(cap);
            s.push_str(trimmed_end);
            s.push_str("\n\n");
            s.push_str(trimmed);
            s
        };

        self.input_manager.set_content(merged);
        self.input_manager.set_cursor(self.input_manager.content().len());
        self.suggested_prompt_state.active = true;
        self.input_compact_mode = self.input_compact_placeholder().is_some();
        self.mark_dirty();
    }

    /// Calculate remaining newline capacity in the input field
    fn remaining_newline_capacity(&self) -> usize {
        let content = self.input_manager.content();
        let mut newline_count = content.matches('\n').count();
        if let Some(range) = self.input_manager.compact_paste_range()
            && range.end <= content.len()
            && content.is_char_boundary(range.start)
            && content.is_char_boundary(range.end)
        {
            newline_count = newline_count.saturating_sub(content[range].matches('\n').count());
        }

        ui::INLINE_INPUT_MAX_LINES.saturating_sub(1).saturating_sub(newline_count)
    }

    /// Check if a newline can be inserted
    fn can_insert_newline(&self) -> bool {
        self.remaining_newline_capacity() > 0
    }

    /// Delete the character before the cursor (backspace)
    pub(crate) fn delete_char(&mut self) {
        // One Backspace right after a collapsed multi-line paste removes the
        // whole inserted block instead of peeling single characters. This only
        // applies while collapsed: in the expanded full-text view Backspace
        // peels one character like normal multiline editing. When the
        // user has an active selection, the selection must win (it removes
        // exactly what the user highlighted).
        if self.input_compact_mode
            && let Some(range) = self.input_manager.compact_paste_range()
            && range.end > range.start
            && self.input_manager.selection_range().is_none()
            && self.input_manager.cursor() == range.end
        {
            let content = self.input_manager.content();
            if content.is_char_boundary(range.start) && content.is_char_boundary(range.end) {
                self.input_manager.replace_range(range.start, range.end, "");
                self.refresh_input_edit_state();
                return;
            }
        }
        self.input_manager.backspace();
        self.refresh_input_edit_state();
    }

    /// Delete the character at the cursor (forward delete)
    pub(crate) fn delete_char_forward(&mut self) {
        self.input_manager.delete();
        self.refresh_input_edit_state();
    }

    /// Delete the word before the cursor
    pub(crate) fn delete_word_backward(&mut self) {
        if self.input_manager.delete_selection() {
            self.refresh_input_edit_state();
            return;
        }
        let cursor = self.input_manager.cursor();
        if cursor == 0 {
            return;
        }

        let delete_start = previous_word_boundary(self.input_manager.content(), cursor);

        if delete_start < cursor {
            self.input_manager.replace_range(delete_start, cursor, "");
            self.input_manager.set_cursor(delete_start);
            self.refresh_input_edit_state();
        }
    }

    pub(crate) fn delete_word_forward(&mut self) {
        self.input_manager.delete_word_forward();
        self.refresh_input_edit_state();
    }

    /// Delete whitespace around the cursor (Readline Alt+\)
    pub(crate) fn delete_whitespace_around_cursor(&mut self) {
        self.input_manager.delete_whitespace_around_cursor();
        self.refresh_input_edit_state();
    }

    /// Transpose characters at cursor position (Readline Ctrl+T)
    pub(crate) fn transpose_chars(&mut self) {
        self.input_manager.transpose_chars();
        self.refresh_input_edit_state();
    }

    /// Transpose words at cursor position (Readline Alt+T)
    pub(crate) fn transpose_words(&mut self) {
        self.input_manager.transpose_words();
        self.refresh_input_edit_state();
    }

    /// Uppercase the current word (Readline Alt+U)
    pub(crate) fn uppercase_word(&mut self) {
        self.input_manager.uppercase_word();
        self.refresh_input_edit_state();
    }

    /// Lowercase the current word (Readline Alt+L)
    pub(crate) fn lowercase_word(&mut self) {
        self.input_manager.lowercase_word();
        self.refresh_input_edit_state();
    }

    /// Capitalize the current word (Readline Alt+C)
    pub(crate) fn capitalize_word(&mut self) {
        self.input_manager.capitalize_word();
        self.refresh_input_edit_state();
    }

    /// Delete from cursor to end of current line (Command+Delete on macOS)
    pub(crate) fn delete_to_end_of_line(&mut self) {
        if self.input_manager.delete_selection() {
            self.refresh_input_edit_state();
            return;
        }
        let content = self.input_manager.content();
        let cursor = self.input_manager.cursor();

        let rest = &content[cursor..];
        let delete_len = if let Some(newline_pos) = rest.find('\n') {
            newline_pos
        } else {
            rest.len()
        };

        if delete_len > 0 {
            self.input_manager.replace_range(cursor, cursor + delete_len, "");
            self.refresh_input_edit_state();
        }
    }

    /// Move cursor left by one character
    pub(crate) fn move_left(&mut self) {
        self.input_manager.move_cursor_left();
    }

    /// Move cursor right by one character
    pub(crate) fn move_right(&mut self) {
        self.input_manager.move_cursor_right();
    }

    pub(crate) fn select_left(&mut self) {
        let cursor = self.input_manager.cursor();
        let content = self.input_manager.content();
        let pos = prev_char_boundary(content, cursor);
        self.input_manager.set_cursor_with_selection(pos);
    }

    pub(crate) fn select_right(&mut self) {
        let cursor = self.input_manager.cursor();
        let content = self.input_manager.content();
        let pos = next_char_boundary(content, cursor);
        self.input_manager.set_cursor_with_selection(pos);
    }

    /// Move cursor left to the start of the previous word
    pub(crate) fn move_left_word(&mut self) {
        let cursor = previous_word_boundary(self.input_manager.content(), self.input_manager.cursor());
        self.input_manager.set_cursor(cursor);
    }
    /// Move cursor right to the start of the next word
    pub(crate) fn move_right_word(&mut self) {
        let cursor = next_word_boundary(self.input_manager.content(), self.input_manager.cursor());
        self.input_manager.set_cursor(cursor);
    }
    /// Move cursor to the start of the line
    pub(crate) fn move_to_start(&mut self) {
        self.input_manager.move_cursor_to_start();
    }

    /// Move cursor to the end of the line
    pub(crate) fn move_to_end(&mut self) {
        self.input_manager.move_cursor_to_end();
    }

    /// Move cursor to the beginning of the current logical line.
    ///
    /// Multi-line input moves within the cursor's line; single-line input
    /// degenerates to buffer start via [`InputManager::move_cursor_to_start_of_line`].
    pub(crate) fn move_to_start_of_line(&mut self) {
        self.input_manager.move_cursor_to_start_of_line();
    }

    /// Move cursor to the end of the current logical line.
    ///
    /// Multi-line input moves within the cursor's line; single-line input
    /// degenerates to buffer end via [`InputManager::move_cursor_to_end_of_line`].
    pub(crate) fn move_to_end_of_line(&mut self) {
        self.input_manager.move_cursor_to_end_of_line();
    }

    pub(crate) fn select_to_start(&mut self) {
        self.input_manager.set_cursor_with_selection(0);
    }

    pub(crate) fn select_to_end(&mut self) {
        self.input_manager.set_cursor_with_selection(self.input_manager.content().len());
    }

    /// Extend selection to the beginning of the current logical line.
    pub(crate) fn select_to_start_of_line(&mut self) {
        let (line_start, _) = self.input_manager.current_line_byte_range();
        self.input_manager.set_cursor_with_selection(line_start);
    }

    /// Extend selection to the end of the current logical line.
    pub(crate) fn select_to_end_of_line(&mut self) {
        let (_, line_end) = self.input_manager.current_line_byte_range();
        self.input_manager.set_cursor_with_selection(line_end);
    }

    /// Clear the current logical line, or the entire input when single-line.
    ///
    /// Dimension key: `line_start..line_end` are byte offsets of the cursor's
    /// logical line; `clear_start..clear_end` expands that range to cover an
    /// overlapping compact paste block (`[Pasted Content N chars]`) so a
    /// collapsed multi-line paste is removed atomically. The expansion only
    /// applies while collapsed; in the expanded full-text view line clears
    /// behave like normal multiline editing. Image placeholders
    /// (`[Image #N]`) live inside their line, so line deletion already covers
    /// them; single-line clears go through [`InputManager::clear`] which also
    /// drops attachments and compact state.
    pub(crate) fn clear_current_line_or_all(&mut self) {
        if self.input_manager.delete_selection() {
            self.refresh_input_edit_state();
            return;
        }
        let content_len = self.input_manager.content().len();
        if content_len == 0 {
            return;
        }
        if self.input_manager.is_single_line() {
            self.input_manager.clear();
            self.refresh_input_edit_state();
            return;
        }

        let cursor = self.input_manager.cursor().min(content_len);
        let (line_start, line_end) = self.input_manager.current_line_byte_range();
        let mut clear_start = line_start.min(content_len);
        let mut clear_end = line_end.min(content_len);

        if self.input_compact_mode
            && let Some(range) = self.input_manager.compact_paste_range()
            && range.start < range.end
        {
            // Clamp stale ranges to current content before expanding.
            let range_start = range.start.min(content_len);
            let range_end = range.end.min(content_len);
            if range_start < range_end {
                let overlaps_line = range_start < clear_end && clear_start < range_end;
                let cursor_inside = cursor >= range_start && cursor <= range_end;
                if overlaps_line || cursor_inside {
                    clear_start = clear_start.min(range_start);
                    clear_end = clear_end.max(range_end);
                }
            }
        }

        // Clamp to char boundaries so multi-byte content never panics.
        let content = self.input_manager.content();
        while clear_start > 0 && !content.is_char_boundary(clear_start) {
            clear_start -= 1;
        }
        while clear_end < content.len() && !content.is_char_boundary(clear_end) {
            clear_end += 1;
        }
        if clear_start >= clear_end {
            return;
        }
        self.input_manager.replace_range(clear_start, clear_end, "");
        self.input_manager.set_cursor(clear_start);
        self.refresh_input_edit_state();
    }

    /// Remember submitted input in history
    pub(crate) fn remember_submitted_input(&mut self, submitted: super::input_manager::InputHistoryEntry) {
        self.input_manager.add_to_history(submitted);
    }

    /// Navigate to previous history entry (disabled to prevent cursor flickering)
    pub(crate) fn navigate_history_previous(&mut self) -> bool {
        if let Some(previous) = self.input_manager.go_to_previous_history() {
            self.input_manager.apply_history_entry(previous);
            self.input_compact_mode = self.input_compact_placeholder().is_some();
            true
        } else {
            false
        }
    }

    /// Navigate to next history entry (disabled to prevent cursor flickering)
    pub(crate) fn navigate_history_next(&mut self) -> bool {
        if let Some(next) = self.input_manager.go_to_next_history() {
            self.input_manager.apply_history_entry(next);
            self.input_compact_mode = self.input_compact_placeholder().is_some();
            true
        } else {
            false
        }
    }

    /// Arrow-Up handling for single-row history gating.
    ///
    /// Returns `true` when the key was consumed by an intra-composer cursor
    /// move (caller should `mark_dirty()` and emit no history event).
    /// Movement is by logical lines; wrapped visual rows are handled by
    /// [`Session::move_up_within_composer`]. The events layer consumes the
    /// key for all multi-row input, so a `false` return only reaches history
    /// traversal for single-row input.
    pub(crate) fn move_cursor_up_for_history(&mut self) -> bool {
        if !self.input_enabled {
            return false;
        }
        self.clear_inline_prompt_suggestion();
        self.input_manager.move_cursor_up()
    }

    /// Arrow-Down handling for single-row history gating.
    ///
    /// Returns `true` when the key was consumed by an intra-composer cursor
    /// move (caller should `mark_dirty()` and emit no history event).
    /// Movement is by logical lines; wrapped visual rows are handled by
    /// [`Session::move_down_within_composer`]. The events layer consumes the
    /// key for all multi-row input, so a `false` return only reaches history
    /// traversal for single-row input.
    pub(crate) fn move_cursor_down_for_history(&mut self) -> bool {
        if !self.input_enabled {
            return false;
        }
        self.clear_inline_prompt_suggestion();
        self.input_manager.move_cursor_down()
    }

    /// Arrow-Up movement within a multi-row composer, honoring soft wraps.
    ///
    /// Uses visual-row movement when the input area is known, logical-line
    /// movement before the first render. Returns `true` when the cursor
    /// moved; the events layer consumes the key for multi-row input even
    /// when already at the edge (returns `false` there).
    pub(crate) fn move_up_within_composer(&mut self) -> bool {
        if !self.input_enabled {
            return false;
        }
        self.clear_inline_prompt_suggestion();
        if self.input_visual_geometry().is_some() {
            self.move_cursor_up_within_visual()
        } else {
            self.input_manager.move_cursor_up()
        }
    }

    /// Arrow-Down movement within a multi-row composer, honoring soft wraps.
    ///
    /// Uses visual-row movement when the input area is known, logical-line
    /// movement before the first render. Returns `true` when the cursor
    /// moved; the events layer consumes the key for multi-row input even
    /// when already at the edge (returns `false` there).
    pub(crate) fn move_down_within_composer(&mut self) -> bool {
        if !self.input_enabled {
            return false;
        }
        self.clear_inline_prompt_suggestion();
        if self.input_visual_geometry().is_some() {
            self.move_cursor_down_within_visual()
        } else {
            self.input_manager.move_cursor_down()
        }
    }

    /// Returns the current history position for status bar display
    /// Returns (current_index, total_entries) or None if not navigating history
    pub fn history_position(&self) -> Option<(usize, usize)> {
        self.input_manager.history_index().map(|idx| {
            let total = self.input_manager.history().len();
            (total - idx, total)
        })
    }
}
