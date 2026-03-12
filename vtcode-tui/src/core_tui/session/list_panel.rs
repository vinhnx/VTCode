use ratatui::{
    prelude::*,
    widgets::{Paragraph, Wrap},
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

    let max_panel_height = area.height.saturating_sub(1);
    if max_panel_height <= fixed_rows {
        return (area, None);
    }

    let desired_height = fixed_rows.saturating_add(desired_list_rows.max(1));
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SharedSearchField {
    pub label: String,
    pub placeholder: Option<String>,
    pub query: String,
}

#[derive(Default)]
pub(crate) struct SharedListPanelSections {
    pub header: Vec<Line<'static>>,
    pub info: Vec<Line<'static>>,
    pub search: Option<SharedSearchField>,
}

#[derive(Clone, Copy)]
pub(crate) struct SharedListPanelStyles {
    pub base_style: Style,
    pub selected_style: Option<Style>,
    pub text_style: Style,
}

pub(crate) fn shared_search_field_line(
    search: &SharedSearchField,
    label_style: Style,
    value_style: Style,
    hint_style: Style,
    cursor_style: Style,
) -> Line<'static> {
    let mut spans = vec![
        Span::styled(format!("{}: ", search.label), label_style),
        Span::styled("[".to_owned(), hint_style),
    ];

    if search.query.is_empty() {
        if let Some(placeholder) = &search.placeholder {
            spans.push(Span::styled(
                placeholder.clone(),
                hint_style.add_modifier(Modifier::ITALIC),
            ));
        }
    } else {
        spans.push(Span::styled(search.query.clone(), value_style));
    }

    spans.push(Span::styled("▌".to_owned(), cursor_style));
    spans.push(Span::styled("]".to_owned(), hint_style));

    if !search.query.is_empty() {
        spans.push(Span::styled(" • Esc clears".to_owned(), hint_style));
    }

    Line::from(spans)
}

pub(crate) fn render_shared_search_field(
    frame: &mut Frame<'_>,
    area: Rect,
    search: &SharedSearchField,
    label_style: Style,
    value_style: Style,
    hint_style: Style,
    cursor_style: Style,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(
        Paragraph::new(shared_search_field_line(
            search,
            label_style,
            value_style,
            hint_style,
            cursor_style,
        ))
        .wrap(Wrap { trim: false }),
        area,
    );
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
    let inner = area;

    let mut constraints = Vec::new();
    let header_rows = rows_to_u16(sections.header.len());
    if header_rows > 0 {
        constraints.push(Constraint::Length(header_rows));
    }

    let info_rows = rows_to_u16(sections.info.len());
    if info_rows > 0 {
        constraints.push(Constraint::Length(info_rows));
    }

    if sections.search.is_some() {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(1));

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

    if let Some(search) = sections.search.as_ref()
        && idx < chunks.len()
    {
        render_shared_search_field(
            frame,
            chunks[idx],
            search,
            styles.text_style,
            styles.base_style,
            styles.text_style.add_modifier(Modifier::DIM),
            styles.selected_style.unwrap_or(styles.base_style),
        );
        idx += 1;
    }

    if idx >= chunks.len() {
        return;
    }

    let list_area = chunks[idx];
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
}
