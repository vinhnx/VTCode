//! Viewer in-place search and match navigation.

use super::*;

impl ToolOutputViewerState {
    pub(crate) fn start_search(&mut self) {
        if self.search.active {
            return;
        }
        self.search.active = true;
        self.search.pending_query = self.search.query.clone();
        self.search.restore_scroll_top = self.scroll_top;
        self.search.restore_query = self.search.query.clone();
        self.search.restore_match = self.search.current_match;
    }
    pub(crate) fn search_active(&self) -> bool {
        self.search.active
    }
    pub(crate) fn search_query(&self) -> &str {
        if self.search.active {
            &self.search.pending_query
        } else {
            &self.search.query
        }
    }
    pub(crate) fn insert_search_text(&mut self, text: &str) {
        self.search.pending_query.push_str(text);
    }
    pub(crate) fn backspace_search(&mut self) {
        self.search.pending_query.pop();
    }
    pub(crate) fn cancel_search(&mut self) {
        self.search.active = false;
        self.scroll_top = self.search.restore_scroll_top;
        self.search.query = self.search.restore_query.clone();
        self.search.current_match = self.search.restore_match;
        self.search.pending_query.clear();
        // No rescan: typing only touched `pending_query`, so the committed
        // query is unchanged and `matches` is already current (streaming
        // refreshes kept it updated incrementally). The next `refresh`
        // reconciles any transcript change that landed after the last frame.
    }
    pub(crate) fn commit_search(&mut self, height: u16) {
        self.search.active = false;
        self.search.query = std::mem::take(&mut self.search.pending_query);
        self.recompute_matches();
        if !self.search.matches.is_empty() {
            self.search.current_match = Some(0);
            self.jump_to_current_match(height);
        } else {
            self.search.current_match = None;
        }
    }
    pub(crate) fn jump_next_match(&mut self, height: u16) {
        if let Some(next) = next_match_index(self.search.current_match, self.search.matches.len(), true) {
            self.search.current_match = Some(next);
            self.jump_to_current_match(height);
        }
    }
    pub(crate) fn jump_previous_match(&mut self, height: u16) {
        if let Some(next) = next_match_index(self.search.current_match, self.search.matches.len(), false) {
            self.search.current_match = Some(next);
            self.jump_to_current_match(height);
        }
    }
    pub(crate) fn status_label(&self) -> String {
        let total = self.line_count();
        let line = (self.scroll_top + 1).min(total);
        let match_status = if self.search.query.is_empty() {
            "search off".to_string()
        } else if self.search.matches.is_empty() {
            format!("search '{}' (0 matches)", self.search.query)
        } else {
            let current = self.search.current_match.unwrap_or(0) + 1;
            format!("search '{}' ({}/{})", self.search.query, current, self.search.matches.len())
        };
        format!("line {line}/{total} • {} • {match_status}", self.mode.label())
    }
    pub(crate) fn current_match_line(&self) -> Option<usize> {
        self.search
            .current_match
            .and_then(|index| self.search.matches.get(index).copied())
    }
    pub(crate) fn jump_to_current_match(&mut self, height: u16) {
        let Some(line) = self.current_match_line() else {
            return;
        };
        self.scroll_top = line.min(self.max_scroll(height));
    }
    pub(crate) fn recompute_matches(&mut self) {
        self.search.matches.clear();
        if self.search.query.is_empty() {
            self.search.current_match = None;
            return;
        }

        let needle = self.search.query.to_ascii_lowercase();
        let mode = self.mode;
        let mut row_index = 0usize;
        for message in &mut self.messages {
            let lowered_lines = message.lowered_lines.get_or_insert_with(|| match mode {
                TranscriptRenderMode::Rich => message
                    .rich_lines
                    .iter()
                    .map(line_text)
                    .map(|line| line.to_ascii_lowercase())
                    .collect(),
                TranscriptRenderMode::Raw => message.lines.iter().map(|line| line.to_ascii_lowercase()).collect(),
            });
            for line in lowered_lines {
                if line.contains(&needle) {
                    self.search.matches.push(row_index);
                }
                row_index += 1;
            }
        }

        if let Some(current) = self.search.current_match
            && current < self.search.matches.len()
        {
            return;
        }

        self.search.current_match = (!self.search.matches.is_empty()).then_some(0);
    }

    /// Incremental rescan after a transcript refresh. The committed query is
    /// unchanged on this path (only `commit_search` changes it, via a full
    /// rescan), so prefix matches stay valid: blocks reused from the previous
    /// frame keep their cached lowercase lines, and only the suffix from the
    /// first rebuilt block onward needs re-scanning. This keeps streaming
    /// appends O(tail) instead of O(transcript) while search is active.
    pub(crate) fn recompute_matches_after_refresh(&mut self) {
        if self.search.query.is_empty() {
            self.search.matches.clear();
            self.search.current_match = None;
            return;
        }

        let first_changed = self.messages.iter().position(|message| message.lowered_lines.is_none());
        let Some(first_changed) = first_changed else {
            // Nothing rebuilt (e.g. revision bump with identical sources).
            return;
        };

        let base_row = self.row_offsets.get(first_changed).copied().unwrap_or(0);
        // `matches` is built in row order, so a sorted truncate keeps the prefix.
        let keep = self.search.matches.partition_point(|&row| row < base_row);
        self.search.matches.truncate(keep);

        let needle = self.search.query.to_ascii_lowercase();
        let mode = self.mode;
        let mut row_index = base_row;
        for message in self.messages.iter_mut().skip(first_changed) {
            let lowered_lines = message.lowered_lines.get_or_insert_with(|| match mode {
                TranscriptRenderMode::Rich => message
                    .rich_lines
                    .iter()
                    .map(line_text)
                    .map(|line| line.to_ascii_lowercase())
                    .collect(),
                TranscriptRenderMode::Raw => message.lines.iter().map(|line| line.to_ascii_lowercase()).collect(),
            });
            for line in lowered_lines {
                if line.contains(&needle) {
                    self.search.matches.push(row_index);
                }
                row_index += 1;
            }
        }

        if let Some(current) = self.search.current_match
            && current < self.search.matches.len()
        {
            return;
        }

        self.search.current_match = (!self.search.matches.is_empty()).then_some(0);
    }
}

pub(crate) fn next_match_index(current: Option<usize>, len: usize, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if forward {
        Some(match current {
            Some(index) => (index + 1) % len,
            None => 0,
        })
    } else {
        Some(match current {
            Some(0) | None => len.saturating_sub(1),
            Some(index) => index.saturating_sub(1),
        })
    }
}
