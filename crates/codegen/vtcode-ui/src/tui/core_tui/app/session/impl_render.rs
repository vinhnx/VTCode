use super::layout::{BottomPanelKind, resolve_bottom_panel_spec, split_input_and_bottom_panel_area};
use super::task_panel;
use super::*;
use crate::tui::config::constants::ui;
use crate::tui::core_tui::app::session::transient::TransientSurface;
use crate::tui::core_tui::session::render as core_render;
use crate::tui::core_tui::session::{list_panel, message_renderer};
use ratatui::{buffer::Buffer, style::Modifier};

impl Session {
    #[cfg_attr(feature = "profiling", hotpath::measure)]
    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let Some(viewport) = self.core.begin_frame(frame) else {
            return;
        };
        let mut metrics = self.core.measure_frame(viewport);
        // The slash palette renders its own focused search field with a visible
        // cursor. Keeping the base input rendered alongside it produces a double
        // input + double cursor, so hand the full input region to the panel and
        // suppress the base input (and its status line) while it is open.
        let panel_captures_input = self.inline_lists_visible()
            && matches!(self.visible_bottom_docked_surface(), Some(TransientSurface::SlashPalette));
        let panel = resolve_bottom_panel_spec(
            self,
            viewport,
            metrics.header_height,
            if panel_captures_input {
                0
            } else {
                metrics.input_core_height
            },
        );
        if panel_captures_input {
            metrics.input_core_height = 0;
        }
        let layout = self.core.build_frame_layout(viewport, metrics, panel.height);
        self.core.set_modal_list_area(None);
        let modal_area = self
            .has_active_overlay()
            .then(|| core_render::floating_modal_area(layout.viewport));
        let transcript_area = modal_area
            .map_or(layout.main_area, |modal_area| core_render::clip_transcript_area(layout.main_area, modal_area));
        let (input_area, bottom_panel_area) = if matches!(panel.kind, BottomPanelKind::SlashPalette) {
            (
                Rect::new(layout.input_area.x, layout.input_area.y, layout.input_area.width, 0),
                Some(layout.input_area),
            )
        } else {
            split_input_and_bottom_panel_area(layout.input_area, panel.height)
        };
        self.core.set_bottom_panel_area(bottom_panel_area);
        self.core.render_base_frame(frame, &layout, transcript_area);
        {
            let buffer = &*frame.buffer_mut();
            let body = self.core.transcript_area().unwrap_or(transcript_area);
            self.rebuild_compact_activity_hit_regions(buffer, body);
        }
        self.core.render_input(frame, input_area);
        if let Some(panel_area) = bottom_panel_area {
            match panel.kind {
                BottomPanelKind::AgentPalette => {
                    render::render_agent_palette(self, frame, panel_area);
                }
                BottomPanelKind::FilePalette => {
                    render::render_file_palette(self, frame, panel_area);
                }
                BottomPanelKind::HistoryPicker => {
                    render::render_history_picker(self, frame, panel_area);
                }
                BottomPanelKind::SlashPalette => {
                    slash::render_slash_palette(self, frame, panel_area);
                }
                BottomPanelKind::TaskPanel => {
                    render_task_panel(self, frame, panel_area);
                }
                BottomPanelKind::LocalAgents => {
                    render::render_local_agents(self, frame, panel_area);
                }
                BottomPanelKind::None => {
                    frame.render_widget(Clear, panel_area);
                }
            }
        }

        if let Some(modal_area) = modal_area {
            core_render::render_modal(self, frame, modal_area);
        }

        if self.diff_preview_state().is_some() {
            diff_preview::render_diff_preview(self, frame, layout.viewport);
        }
        if let Some(mut state) = self.tool_output_viewer_state.take() {
            let width = tool_output_viewer::viewer_content_width(layout.viewport);
            let height = tool_output_viewer::viewer_content_height(self, &state, layout.viewport);
            state.refresh(self, width, height);
            tool_output_viewer::render_tool_output_viewer(self, frame, layout.viewport, &mut state);
            self.tool_output_viewer_state = Some(state);
        }
        if self.diff_preview_state().is_some() || self.tool_output_viewer_state().is_some() {
            self.core.clear_sticky_prompt_target();
            self.core.occlude_progress_feedback(layout.viewport);
        }
        self.core.finalize_mouse_selection(frame, layout.viewport);
        self.core.observe_progress_feedback();
    }

    #[expect(
        dead_code,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    fn render_message_spans(&self, index: usize) -> Vec<Span<'static>> {
        let Some(line) = self.core.lines.get(index) else {
            return vec![Span::raw(String::new())];
        };
        message_renderer::render_message_spans(
            line,
            &self.core.theme,
            &self.core.labels,
            |kind| self.core.prefix_text(kind),
            |line| self.core.prefix_style(line),
            |kind| self.core.text_fallback(kind),
        )
    }
}

impl Session {
    #[cfg_attr(feature = "profiling", hotpath::measure)]
    fn rebuild_compact_activity_hit_regions(&mut self, buffer: &Buffer, area: Rect) {
        self.compact_activity_hit_regions.clear();
        if tool_output_viewer::compact_activity_hint_text(self).is_none() {
            return;
        }
        if area.width == 0 || area.height == 0 || self.core.transcript_width == 0 {
            return;
        }

        let activity_ranges = self
            .compact_activity_entries
            .iter()
            .filter_map(|entry| entry.metadata.review_anchor.map(|anchor| (entry.line_index, anchor)))
            .collect::<Vec<_>>();
        let transcript_width = self.core.transcript_width;
        let view_top = self.core.transcript_view_top;

        for (line_index, review_anchor) in activity_ranges {
            let Some((start_row, end_row)) = self.core.transcript_message_row_range(transcript_width, line_index)
            else {
                continue;
            };
            for transcript_row in start_row..end_row {
                let Some(screen_row) = transcript_row
                    .checked_sub(view_top)
                    .and_then(|row| u16::try_from(row).ok())
                    .and_then(|row| area.y.checked_add(row))
                else {
                    continue;
                };
                if screen_row >= area.bottom() {
                    continue;
                }
                for hit_area in find_underlined_text_regions(buffer, area, screen_row) {
                    self.compact_activity_hit_regions
                        .push(CompactActivityHitRegion { area: hit_area, review_anchor });
                }
            }
        }

        // Exec-session expand notices carry their capture id on the notice row
        // (`click to expand` is underlined). Reuse the same hit-region list so
        // the existing compact-activity click path opens the viewer. Compact
        // activity rows already register their own hint regions — skip them.
        let compact_activity_lines = self
            .compact_activity_entries
            .iter()
            .map(|entry| entry.line_index)
            .collect::<std::collections::HashSet<_>>();
        let expand_targets = self
            .tool_output_blocks
            .iter()
            .filter_map(|block| {
                // Prefer a line that actually carries the expand action:
                // `anchor_line` can point at a live PTY header when one
                // matched the capture's first row.
                let line_index = [block.anchor_line, block.recorded_at_line]
                    .into_iter()
                    .flatten()
                    .find(|&index| {
                        !compact_activity_lines.contains(&index)
                            && self.core.lines.get(index).is_some_and(|line| {
                                line.segments.iter().any(|segment| segment.text.contains("click to expand"))
                            })
                    })?;
                Some((line_index, block.id))
            })
            .collect::<Vec<_>>();
        for (line_index, review_anchor) in expand_targets {
            let Some((start_row, end_row)) = self.core.transcript_message_row_range(transcript_width, line_index)
            else {
                continue;
            };
            for transcript_row in start_row..end_row {
                let Some(screen_row) = transcript_row
                    .checked_sub(view_top)
                    .and_then(|row| u16::try_from(row).ok())
                    .and_then(|row| area.y.checked_add(row))
                else {
                    continue;
                };
                if screen_row >= area.bottom() {
                    continue;
                }
                let mut hit_regions = find_underlined_text_regions(buffer, area, screen_row);
                if hit_regions.is_empty() {
                    // Underline can be lost to theme re-styling; fall back to
                    // the `click to expand` phrase's column span on this row.
                    hit_regions = find_expand_action_text_region(buffer, area, screen_row);
                }
                for hit_area in hit_regions {
                    self.compact_activity_hit_regions
                        .push(CompactActivityHitRegion { area: hit_area, review_anchor });
                }
            }
        }
    }
}

/// Column span of the `click to expand` phrase on a rendered screen row.
///
/// Walks buffer cells (not UTF-8 bytes): the notice embeds `…` and `·`, so a
/// byte offset into the concatenated symbols would misplace the hit target.
fn find_expand_action_text_region(buffer: &Buffer, area: Rect, row: u16) -> Vec<Rect> {
    if area.width == 0 || row < area.y || row >= area.bottom() {
        return Vec::new();
    }
    let phrase = "click to expand";
    let phrase_chars: Vec<char> = phrase.chars().collect();
    let phrase_width = phrase_chars.len() as u16;
    if area.right().saturating_sub(area.x) < phrase_width {
        return Vec::new();
    }
    for start_column in area.x..=area.right().saturating_sub(phrase_width) {
        let matched = phrase_chars.iter().enumerate().all(|(offset, expected)| {
            let column = start_column + offset as u16;
            if column >= area.right() {
                return false;
            }
            // Compare against a stack-encoded UTF-8 buffer — no per-cell String.
            let mut utf8 = [0u8; 4];
            let expected_str = expected.encode_utf8(&mut utf8);
            buffer[(column, row)].symbol() == expected_str
        });
        if matched {
            return vec![Rect::new(start_column, row, phrase_width, 1)];
        }
    }
    Vec::new()
}

fn find_underlined_text_regions(buffer: &Buffer, area: Rect, row: u16) -> Vec<Rect> {
    if area.width == 0 || area.height == 0 || row < area.y || row >= area.bottom() {
        return Vec::new();
    }

    let mut regions = Vec::new();
    let mut start = None;
    for column in area.x..area.right() {
        let underlined = buffer[(column, row)].style().add_modifier.contains(Modifier::UNDERLINED);
        match (start, underlined) {
            (None, true) => start = Some(column),
            (Some(start_column), false) => {
                regions.push(Rect::new(start_column, row, column.saturating_sub(start_column), 1));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(start_column) = start {
        regions.push(Rect::new(start_column, row, area.right().saturating_sub(start_column), 1));
    }
    regions
}

fn render_task_panel(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let (panel_lines, panel_statuses, panel_current) = task_panel::aligned_body(
        &session.task_panel_lines,
        &session.task_panel_statuses,
        session.task_panel_current,
        session.task_panel_metadata.as_ref(),
    );
    let base = session.core.header_secondary_style();
    let row_styles: Vec<Style> = panel_lines
        .iter()
        .enumerate()
        .map(|(index, _)| match panel_statuses.get(index).copied() {
            Some(status) => task_panel::row_style(status, Some(index) == panel_current, base),
            None => base,
        })
        .collect();
    let rows = task_panel::styled_rows(panel_lines, &row_styles, base, area.width);
    let item_count = panel_lines.len();
    let (title, progress) = task_panel::header(session.task_panel_metadata.as_ref(), item_count);
    let sections = list_panel::SharedListPanelSections {
        header: vec![Line::from(vec![Span::styled(
            title.to_string(),
            session.core.section_title_style(),
        )])],
        info: vec![Line::from(progress)],
        search: None,
    };
    let styles = list_panel::SharedListPanelStyles {
        base_style: session.core.styles.default_style(),
        selected_style: Some(session.core.styles.modal_list_highlight_style()),
        text_style: session.core.styles.default_style(),
        divider_style: None,
        input_styles: list_panel::input_styles_from_theme(&session.core.theme),
        show_divider: false,
    };
    let mut model = list_panel::StaticRowsListPanelModel {
        rows,
        selected: None,
        offset: 0,
        visible_rows: area.height as usize,
    };
    list_panel::render_shared_list_panel(frame, area, sections, styles, &mut model);
}

#[cfg(test)]
mod tests {
    use super::find_expand_action_text_region;
    use ratatui::{buffer::Buffer, layout::Rect};

    fn row_buffer(text: &str, width: u16) -> Buffer {
        let mut buffer = Buffer::empty(Rect::new(0, 0, width, 1));
        for (index, symbol) in text.chars().enumerate() {
            let x = index as u16;
            if x >= width {
                break;
            }
            buffer[(x, 0)].set_symbol(&symbol.to_string());
        }
        buffer
    }

    #[test]
    fn expand_action_region_skips_multibyte_prefix_columns() {
        // `…` and `·` are multi-byte in UTF-8: a byte-offset lookup would
        // place the hit target several columns to the right of the phrase.
        let buffer = row_buffer("… +2 lines · click to expand", 40);
        let area = Rect::new(0, 0, 40, 1);
        let regions = find_expand_action_text_region(&buffer, area, 0);
        assert_eq!(regions.len(), 1, "expected one hit region");
        let region = regions[0];
        assert_eq!(region.y, 0);
        assert_eq!(region.height, 1);
        assert_eq!(region.width, "click to expand".len() as u16);
        // The phrase starts at display column 13 (after "… +2 lines · ").
        assert_eq!(region.x, 13, "hit region must sit on the phrase: {region:?}");
        assert_eq!(buffer[(region.x, 0)].symbol(), "c", "region must start at 'click': {region:?}");
    }

    #[test]
    fn expand_action_region_absent_without_phrase() {
        let buffer = row_buffer("… +2 lines (/share html)", 40);
        let area = Rect::new(0, 0, 40, 1);
        assert!(find_expand_action_text_region(&buffer, area, 0).is_empty());
    }
}
