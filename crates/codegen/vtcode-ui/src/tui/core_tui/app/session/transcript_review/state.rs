//! Viewer state lifecycle: open, refresh, and revision tracking.

use super::*;

impl ToolOutputViewerState {
    pub(crate) fn open(session: &Session, width: u16, height: u16) -> Self {
        Self::open_focused(session, width, height, None)
    }
    pub(crate) fn open_focused(session: &Session, width: u16, height: u16, focus_target: Option<u64>) -> Self {
        let mode = if session.core.appearance.screen_reader_mode {
            TranscriptRenderMode::Raw
        } else {
            TranscriptRenderMode::Rich
        };
        let mut state = Self { focus_target, mode, ..Self::default() };
        state.refresh(session, width, height);
        if focus_target.is_none() {
            state.scroll_to_bottom(height);
        }
        state
    }

    /// Tool capture ids currently held in this open viewer. Used to pin those
    /// blocks against FIFO eviction so an open review does not lose content.
    ///
    /// Anchored captures key as `Tool(id)`; unanchored ones key as
    /// `OrphanTool(index)` but still carry `block.id` in `revision`.
    pub(crate) fn retained_tool_ids(&self) -> HashSet<u64> {
        self.messages
            .iter()
            .filter_map(|message| match message.key {
                ReviewBlockKey::Tool(_) | ReviewBlockKey::OrphanTool(_) => Some(message.revision),
                ReviewBlockKey::Core(_) => None,
            })
            .collect()
    }
    pub(crate) fn refresh(&mut self, session: &Session, width: u16, height: u16) {
        let width = width.max(1);
        let height = height.max(1);
        let revision = ReviewRevision {
            transcript: session.core.current_transcript_revision(),
            tool_output: session.tool_output_revision,
            presentation: session.core.transcript_presentation_revision,
        };
        if self.width == width && self.height == height && self.source_revision == revision {
            self.focus_pending_target(height);
            self.clamp_scroll(height);
            return;
        }

        let was_at_bottom = self.is_at_bottom(self.height);
        let width_changed = self.width != width;
        let presentation_changed = self.source_revision.presentation != revision.presentation;
        self.refresh_messages(session, width, width_changed || presentation_changed);
        self.width = width;
        self.height = height;
        self.source_revision = revision;
        self.recompute_matches_after_refresh();

        let focused = self.focus_pending_target(height);
        if focused {
            return;
        }
        if was_at_bottom {
            self.scroll_to_bottom(height);
        } else {
            self.clamp_scroll(height);
        }
    }
    pub(crate) fn focus_pending_target(&mut self, height: u16) -> bool {
        let Some(target) = self.focus_target else {
            return false;
        };
        let Some(message_index) = self
            .messages
            .iter()
            .position(|message| message.key == ReviewBlockKey::Tool(target))
        else {
            return false;
        };
        self.scroll_top = self.row_offsets.get(message_index).copied().unwrap_or_default();
        self.scroll_top = self.scroll_top.min(self.max_scroll(height));
        self.focus_target = None;
        true
    }
    pub(crate) fn refresh_messages(&mut self, session: &Session, width: u16, width_changed: bool) {
        let mut previous = HashMap::with_capacity(self.messages.len());
        for message in self.messages.drain(..) {
            previous.insert(message.key, message);
        }
        let sources = collect_review_sources(session);
        let mut messages = Vec::with_capacity(sources.len());

        for source in sources {
            let cached = previous
                .remove(&source.key)
                .filter(|message| !width_changed && message.revision == source.revision);
            messages.push(cached.unwrap_or_else(|| build_cached_block(session, source, width)));
        }

        self.messages = messages;
        self.cached_export_text = None;
        self.update_row_offsets();
    }
    pub(crate) fn update_row_offsets(&mut self) {
        self.row_offsets.clear();
        self.row_offsets.reserve(self.messages.len());

        let mut current_offset = 0;
        for message in &self.messages {
            self.row_offsets.push(current_offset);
            current_offset += self.message_line_count(message);
        }

        self.total_lines = current_offset;
    }
}
