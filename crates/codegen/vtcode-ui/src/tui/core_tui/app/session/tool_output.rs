use super::*;

impl AppSession {
    pub(super) fn open_tool_output_viewer(&mut self, width: u16, height: u16, review_anchor: Option<ToolOutputId>) {
        self.tool_output_viewer_state = Some(ToolOutputViewerState::open_focused(self, width, height, review_anchor));
        self.show_transient_surface(TransientSurface::ToolOutputViewer);
        self.core.mark_dirty();
    }

    pub(crate) fn record_tool_output_block(&mut self, id: ToolOutputId, mut lines: Vec<String>) {
        if lines.is_empty() {
            return;
        }
        // Bound retained capture size so long-running floods cannot grow the
        // TUI heap without limit. Keep the tail — that is what review shows.
        let max_lines = ui::TUI_TOOL_OUTPUT_CAPTURE_MAX_LINES;
        if lines.len() > max_lines {
            let drop = lines.len() - max_lines;
            lines.drain(..drop);
        }

        // A non-PTY capture is recorded after the live PTY stream has rendered
        // its command header. Restrict the fallback search to the current
        // trailing PTY block so repeated commands cannot borrow an older
        // header from the conversation.
        let recorded_at_line = self.core.lines.len();
        let mut anchor_search_start = recorded_at_line;
        while anchor_search_start > 0
            && self
                .core
                .lines
                .get(anchor_search_start - 1)
                .is_some_and(|line| line.kind == InlineMessageKind::Pty)
        {
            anchor_search_start -= 1;
        }
        let anchor_line = self.find_live_pty_anchor(lines.first().map(String::as_str), anchor_search_start);

        self.tool_output_blocks.push(ToolOutputBlock {
            id,
            anchor_line,
            anchor_search_start,
            recorded_at_line: Some(recorded_at_line),
            lines,
        });
        self.trim_tool_output_blocks();
        self.tool_output_revision = self.tool_output_revision.wrapping_add(1);
        self.core.mark_dirty();
    }

    /// FIFO-bound capture blocks. Blocks shown in an open tool-output viewer
    /// are pinned so review does not lose content mid-read; the newest block
    /// is never dropped. Call again after the viewer closes to re-trim.
    fn trim_tool_output_blocks(&mut self) {
        let max_blocks = ui::TUI_TOOL_OUTPUT_BLOCKS_MAX;
        if self.tool_output_blocks.len() <= max_blocks {
            return;
        }
        let pinned = self
            .tool_output_viewer_state
            .as_ref()
            .map(|viewer| viewer.retained_tool_ids())
            .unwrap_or_default();
        while self.tool_output_blocks.len() > max_blocks {
            let newest = self.tool_output_blocks.len() - 1;
            let Some(pos) = self
                .tool_output_blocks
                .iter()
                .take(newest)
                .position(|block| !pinned.contains(&block.id))
            else {
                break;
            };
            self.tool_output_blocks.remove(pos);
        }
    }

    fn find_live_pty_anchor(&self, header: Option<&str>, search_start: usize) -> Option<usize> {
        let header = header.map(normalize_tool_output_header)?;
        let mut index = self.core.lines.len();
        while index > search_start {
            let line_index = index - 1;
            let line = self.core.lines.get(line_index)?;
            if line.kind != InlineMessageKind::Pty {
                break;
            }
            if folded_pty_header(&self.core, line_index).trim_end() == header.trim_end() {
                return Some(line_index);
            }
            index = line_index;
        }
        None
    }

    pub(super) fn handle_core_command(&mut self, command: crate::tui::core_tui::types::InlineCommand) {
        let evicted_before = self.core.evicted_message_count;
        self.core.handle_command(command);
        let evicted = self.core.evicted_message_count.saturating_sub(evicted_before);
        if evicted == 0 {
            return;
        }

        self.compact_activity_entries.retain_mut(|entry| {
            if entry.line_index < evicted {
                return false;
            }
            entry.line_index -= evicted;
            true
        });
        for block in &mut self.tool_output_blocks {
            block.anchor_line = block.anchor_line.filter(|&line| line >= evicted).map(|line| line - evicted);
            block.anchor_search_start = block.anchor_search_start.saturating_sub(evicted);
            block.recorded_at_line = block.recorded_at_line.map(|line| line.saturating_sub(evicted));
        }
        self.compact_activity_hit_regions.clear();
        self.tool_output_revision = self.tool_output_revision.wrapping_add(1);
    }

    pub(super) fn append_tool_output_line(
        &mut self,
        id: ToolOutputId,
        kind: InlineMessageKind,
        segments: Vec<InlineSegment>,
    ) {
        self.handle_core_command(crate::tui::core_tui::types::InlineCommand::AppendLine { kind, segments });
        let line_index = self.core.lines.len().saturating_sub(1);
        // An expand notice must win over an earlier live-PTY header anchor:
        // the notice is the row the user clicks to open this capture.
        let is_expand_notice = self
            .core
            .lines
            .get(line_index)
            .is_some_and(|line| line.segments.iter().any(|segment| segment.text.contains("click to expand")));
        if let Some(block) = self.tool_output_blocks.iter_mut().find(|block| block.id == id)
            && (block.anchor_line.is_none() || is_expand_notice)
        {
            block.anchor_line = Some(line_index);
            self.tool_output_revision = self.tool_output_revision.wrapping_add(1);
            self.core.mark_dirty();
        }
    }

    fn update_tool_output_anchors(&mut self, metadata: &CompactActivityMetadata, line_index: usize) {
        let mut ids = metadata.review_anchors.clone();
        if ids.is_empty()
            && let Some(review_anchor) = metadata.review_anchor
        {
            ids.push(review_anchor);
        }
        for id in ids {
            if let Some(block) = self.tool_output_blocks.iter_mut().find(|block| block.id == id) {
                block.anchor_line = Some(line_index);
            }
        }
    }

    pub(super) fn refresh_compact_activity_presentations(&mut self) {
        let entries = self.compact_activity_entries.clone();
        for entry in entries {
            let is_compact_line = self
                .core
                .lines
                .get(entry.line_index)
                .is_some_and(|line| line.kind == InlineMessageKind::Info);
            if !is_compact_line {
                continue;
            }

            let segments = tool_output_viewer::compact_activity_segments(self, &entry.metadata);
            let revision = self.core.next_revision();
            if let Some(line) = self.core.lines.get_mut(entry.line_index) {
                line.segments = segments;
                line.link_ranges.clear();
                line.revision = revision;
                self.core.mark_line_dirty(entry.line_index);
            }
        }
        self.core.invalidate_scroll_metrics();
    }

    /// Visual-only redraw (cursor, scroll, hover). Does not drop header/sidebar caches.
    pub(crate) fn mark_visual_dirty(&mut self) {
        self.core.mark_visual_dirty();
    }

    pub(super) fn append_compact_activity(&mut self, metadata: CompactActivityMetadata) {
        let segments = tool_output_viewer::compact_activity_segments(self, &metadata);
        self.handle_core_command(crate::tui::core_tui::types::InlineCommand::AppendLine {
            kind: InlineMessageKind::Info,
            segments,
        });
        let line_index = self.core.lines.len().saturating_sub(1);
        self.compact_activity_entries
            .push(CompactActivityEntry { line_index, metadata: metadata.clone() });
        let max_entries = ui::TUI_COMPACT_ACTIVITY_MAX_ENTRIES;
        if self.compact_activity_entries.len() > max_entries {
            let drop = self.compact_activity_entries.len() - max_entries;
            self.compact_activity_entries.drain(..drop);
        }
        self.update_tool_output_anchors(&metadata, line_index);
    }

    pub(super) fn replace_compact_activity(&mut self, metadata: CompactActivityMetadata) {
        let can_replace = self
            .compact_activity_entries
            .last()
            .is_some_and(|entry| entry.line_index + 1 == self.core.lines.len());
        if !can_replace {
            self.append_compact_activity(metadata);
            return;
        }

        let segments = tool_output_viewer::compact_activity_segments(self, &metadata);
        self.handle_core_command(crate::tui::core_tui::types::InlineCommand::ReplaceLast {
            count: 1,
            kind: InlineMessageKind::Info,
            lines: vec![segments],
            link_ranges: None,
        });
        if let Some(entry) = self.compact_activity_entries.last_mut() {
            entry.metadata = metadata.clone();
            entry.line_index = self.core.lines.len().saturating_sub(1);
        }
        let line_index = self.core.lines.len().saturating_sub(1);
        self.update_tool_output_anchors(&metadata, line_index);
    }

    pub(super) fn collapse_pty_block(&mut self, metadata: CompactActivityMetadata) {
        let pty_count = self
            .core
            .lines
            .iter()
            .rev()
            .take_while(|line| line.kind == InlineMessageKind::Pty)
            .count();
        if pty_count == 0 {
            self.replace_compact_activity(metadata);
            return;
        }

        let old_first_removed = self.core.lines.len().saturating_sub(pty_count);
        let preceding_activity = self.compact_activity_entries.last().is_some_and(|entry| {
            entry.line_index + 1 == old_first_removed && entry.metadata.group_id == metadata.group_id
        });
        let remove_count = pty_count + usize::from(preceding_activity);
        let first_removed = self.core.lines.len().saturating_sub(remove_count);
        let segments = tool_output_viewer::compact_activity_segments(self, &metadata);
        self.handle_core_command(crate::tui::core_tui::types::InlineCommand::ReplaceLast {
            count: remove_count,
            kind: InlineMessageKind::Info,
            lines: vec![segments],
            link_ranges: None,
        });
        self.compact_activity_entries.retain(|entry| entry.line_index < first_removed);
        let line_index = self.core.lines.len().saturating_sub(1);
        self.compact_activity_entries
            .push(CompactActivityEntry { line_index, metadata: metadata.clone() });
        self.update_tool_output_anchors(&metadata, line_index);
    }

    pub(crate) fn compact_activity_for_line(&self, line_index: usize) -> Option<&CompactActivityMetadata> {
        self.compact_activity_entries
            .iter()
            .find(|entry| entry.line_index == line_index)
            .map(|entry| &entry.metadata)
    }

    pub(crate) fn compact_activity_review_anchor_at(&self, column: u16, row: u16) -> Option<ToolOutputId> {
        self.compact_activity_hit_regions
            .iter()
            .find(|region| region.area.contains(Position { x: column, y: row }))
            .map(|region| region.review_anchor)
    }

    pub(super) fn close_tool_output_viewer(&mut self) {
        if self.tool_output_viewer_state.is_none() {
            return;
        }
        self.tool_output_viewer_state = None;
        self.close_transient_surface(TransientSurface::ToolOutputViewer);
        // Unpin and drop any over-cap blocks that were held for the viewer.
        self.trim_tool_output_blocks();
        self.core.mark_dirty();
    }
}
