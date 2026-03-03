use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::inline_list::{InlineListRenderOptions, InlineListRow, render_inline_list_with_options};

pub(crate) trait SharedListWidgetModel {
    fn rows(&self, width: u16) -> Vec<(InlineListRow, u16)>;
    fn selected(&self) -> Option<usize>;
    fn set_selected(&mut self, selected: Option<usize>);
    fn set_scroll_offset(&mut self, offset: usize);
    fn set_viewport_rows(&mut self, _rows: u16) {}
}

pub(crate) struct StaticRowsListPanelModel {
    pub rows: Vec<(InlineListRow, u16)>,
    pub selected: Option<usize>,
    pub offset: usize,
    pub visible_rows: usize,
}

impl SharedListWidgetModel for StaticRowsListPanelModel {
    fn rows(&self, _width: u16) -> Vec<(InlineListRow, u16)> {
        self.rows.clone()
    }

    fn selected(&self) -> Option<usize> {
        self.selected
    }

    fn set_selected(&mut self, selected: Option<usize>) {
        self.selected = selected;
    }

    fn set_scroll_offset(&mut self, offset: usize) {
        self.offset = offset;
    }

    fn set_viewport_rows(&mut self, rows: u16) {
        self.visible_rows = rows as usize;
    }
}

pub(crate) fn split_bottom_list_panel(
    area: Rect,
    fixed_rows: u16,
    desired_list_rows: u16,
) -> (Rect, Option<Rect>) {
    if area.width == 0 || area.height == 0 {
        return (area, None);
    }

    let border_rows = 2_u16;
    let max_panel_height = area.height.saturating_sub(1);
    if max_panel_height <= fixed_rows.saturating_add(border_rows) {
        return (area, None);
    }

    let desired_height = fixed_rows
        .saturating_add(desired_list_rows.max(1))
        .saturating_add(border_rows);
    let panel_height = desired_height.min(max_panel_height);
    let chunks =
        Layout::vertical([Constraint::Min(1), Constraint::Length(panel_height)]).split(area);
    (chunks[0], Some(chunks[1]))
}

pub(crate) fn rows_to_u16(rows: usize) -> u16 {
    rows.min(u16::MAX as usize) as u16
}

pub(crate) fn fixed_section_rows(header_rows: usize, info_rows: usize, has_search: bool) -> u16 {
    rows_to_u16(header_rows)
        .saturating_add(rows_to_u16(info_rows))
        .saturating_add(if has_search { 1 } else { 0 })
}

#[derive(Default)]
pub(crate) struct SharedListPanelSections {
    pub header: Vec<Line<'static>>,
    pub info: Vec<Line<'static>>,
    pub search: Option<Line<'static>>,
}

#[derive(Clone, Copy)]
pub(crate) struct SharedListPanelStyles {
    pub base_style: Style,
    pub selected_style: Option<Style>,
    pub text_style: Style,
}

pub(crate) fn render_shared_list_panel<M: SharedListWidgetModel>(
    frame: &mut Frame<'_>,
    area: Rect,
    sections: SharedListPanelSections,
    styles: SharedListPanelStyles,
    model: &mut M,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(styles.text_style.add_modifier(Modifier::DIM));
    frame.render_widget(block.clone(), area);
    let inner = block.inner(area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut constraints = Vec::new();
    let header_rows = rows_to_u16(sections.header.len());
    if header_rows > 0 {
        constraints.push(Constraint::Length(header_rows));
    }

    let info_rows = rows_to_u16(sections.info.len());
    if info_rows > 0 {
        constraints.push(Constraint::Length(info_rows));
    }

    constraints.push(Constraint::Min(1));
    if sections.search.is_some() {
        constraints.push(Constraint::Length(1));
    }

    let chunks = Layout::vertical(constraints).split(inner);
    if chunks.is_empty() {
        return;
    }
    let section_text_style = styles.text_style.add_modifier(Modifier::DIM);

    let mut idx = 0usize;
    if header_rows > 0 {
        frame.render_widget(
            Paragraph::new(sections.header).style(section_text_style),
            chunks[idx],
        );
        idx += 1;
    }

    if info_rows > 0 {
        frame.render_widget(
            Paragraph::new(sections.info)
                .style(section_text_style)
                .wrap(Wrap { trim: true }),
            chunks[idx],
        );
        idx += 1;
    }

    if idx >= chunks.len() {
        return;
    }

    let list_area = chunks[idx];
    idx += 1;
    model.set_viewport_rows(list_area.height);

    let rows = model.rows(list_area.width);
    if rows.is_empty() {
        model.set_selected(None);
        model.set_scroll_offset(0);
    } else {
        let selected = model.selected().filter(|index| *index < rows.len());
        let widget_state = render_inline_list_with_options(
            frame,
            list_area,
            rows,
            selected,
            InlineListRenderOptions {
                base_style: styles.base_style,
                selected_style: styles.selected_style,
                scroll_padding: crate::config::constants::ui::INLINE_LIST_SCROLL_PADDING,
                infinite_scrolling: false,
            },
        );
        model.set_selected(widget_state.selected);
        model.set_scroll_offset(widget_state.scroll_offset_index());
    }

    if let Some(search_line) = sections.search
        && idx < chunks.len()
    {
        frame.render_widget(
            Paragraph::new(search_line)
                .style(section_text_style)
                .wrap(Wrap { trim: true }),
            chunks[idx],
        );
    }
}
