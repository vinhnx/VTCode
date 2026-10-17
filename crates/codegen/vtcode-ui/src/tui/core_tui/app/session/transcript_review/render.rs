//! Viewer rendering, hit-testing, and export.

use super::*;

impl ToolOutputViewerState {
    pub(crate) fn evidence_at(&self, column: u16, row: u16) -> Option<&str> {
        if !self.body_contains(column, row) || self.content_area.height == 0 {
            return None;
        }
        let logical_row = self
            .scroll_top
            .saturating_add(usize::from(row.saturating_sub(self.content_area.y)));
        let (message, local_row) = self.block_at(logical_row)?;
        message.evidence_links.get(local_row)?.as_deref()
    }
    pub(crate) fn toggle_render_mode(&mut self) {
        self.set_render_mode(self.mode.toggle());
    }
    pub(crate) fn set_render_mode(&mut self, mode: TranscriptRenderMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        for message in &mut self.messages {
            // Search rows are derived from the active render mode. Invalidate
            // the lowercase cache when rich wrapping and raw lines switch.
            message.lowered_lines = None;
        }
        self.update_row_offsets();
        self.recompute_matches();
        self.clamp_scroll(self.height);
        self.search.restore_scroll_top = self.search.restore_scroll_top.min(self.max_scroll(self.height));
        self.search.restore_match = self.search.current_match;
    }
    pub(crate) fn set_viewer_area(&mut self, area: Rect) {
        self.viewer_area = area;
    }
    pub(crate) fn viewer_contains(&self, column: u16, row: u16) -> bool {
        rect_contains_or_empty(self.viewer_area, column, row)
    }
    pub(crate) fn body_contains(&self, column: u16, row: u16) -> bool {
        rect_contains_or_empty(self.content_area, column, row)
    }
    pub(crate) fn content_height_or(&self, fallback: u16) -> u16 {
        if self.content_area.height == 0 {
            fallback.max(1)
        } else {
            self.content_area.height
        }
    }
    pub(crate) fn mode_control_contains(&self, column: u16, row: u16) -> bool {
        opt_rect_contains(self.title_mode_hit_region, column, row)
    }
    pub(crate) fn close_control_contains(&self, column: u16, row: u16) -> bool {
        opt_rect_contains(self.title_close_hit_region, column, row)
    }
    pub(crate) fn update_mode_hover(&mut self, column: u16, row: u16) -> bool {
        let hovered = self.mode_control_contains(column, row);
        set_hover(&mut self.hovered_mode_control, hovered)
    }
    pub(crate) fn update_close_hover(&mut self, column: u16, row: u16) -> bool {
        let hovered = self.close_control_contains(column, row);
        set_hover(&mut self.hovered_close_control, hovered)
    }

    #[cfg(test)]
    pub(crate) fn render_mode(&self) -> TranscriptRenderMode {
        self.mode
    }
    pub(crate) fn export_text(&mut self) -> String {
        if let Some(text) = &self.cached_export_text {
            return text.clone();
        }

        let mut export = String::new();
        let mut wrote_line = false;
        for message in &self.messages {
            for line in &message.lines {
                if wrote_line {
                    export.push('\n');
                }
                export.push_str(line);
                wrote_line = true;
            }
        }

        self.cached_export_text = Some(export.clone());
        export
    }
    pub(crate) fn visible_lines(&self, height: usize) -> Vec<Line<'static>> {
        let height = height.max(1);
        let end = self.scroll_top.saturating_add(height).min(self.total_lines);
        let current_match_line = self.current_match_line();
        let mut visible = Vec::with_capacity(height);

        for row in self.scroll_top..end {
            let mut line = self.line_for_mode_at(row).unwrap_or_default();
            if current_match_line == Some(row) {
                let style = line.style.add_modifier(Modifier::REVERSED);
                line = line.style(style);
            }
            visible.push(line);
        }

        while visible.len() < height {
            visible.push(Line::default());
        }

        visible
    }
    pub(crate) fn message_line_count(&self, message: &CachedToolOutputBlock) -> usize {
        let count = match self.mode {
            TranscriptRenderMode::Rich => message.rich_lines.len(),
            TranscriptRenderMode::Raw => message.lines.len(),
        };
        count.max(1)
    }
    pub(crate) fn block_at(&self, row: usize) -> Option<(&CachedToolOutputBlock, usize)> {
        if row >= self.total_lines || self.row_offsets.is_empty() {
            return None;
        }

        let message_index = self.row_offsets.partition_point(|offset| *offset <= row).saturating_sub(1);
        let local_index = row.saturating_sub(self.row_offsets[message_index]);
        self.messages.get(message_index).map(|message| (message, local_index))
    }
    pub(crate) fn line_for_mode_at(&self, row: usize) -> Option<Line<'static>> {
        let (message, local_index) = self.block_at(row)?;
        match self.mode {
            TranscriptRenderMode::Rich => message.rich_lines.get(local_index).cloned(),
            TranscriptRenderMode::Raw => message.lines.get(local_index).map(|line| Line::raw(line.clone())),
        }
    }
}
