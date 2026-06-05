use ratatui::{
    prelude::*,
    widgets::{Block, Paragraph, Wrap},
};
use ratatui_cheese::input::{Input, InputState, InputStyles};
use ratatui_cheese::theme::Palette;

use super::inline_list::{InlineListRenderOptions, InlineListRow, render_inline_list_with_options};
use crate::core_tui::style::ratatui_color_from_ansi;
use crate::ui::tui::types::InlineTheme;

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
    fixed_section_rows_with_divider(header_rows, info_rows, has_search, false)
}

pub(crate) fn fixed_section_rows_with_divider(
    header_rows: usize,
    info_rows: usize,
    has_search: bool,
    has_divider: bool,
) -> u16 {
    rows_to_u16(header_rows)
        .saturating_add(rows_to_u16(info_rows))
        .saturating_add(if has_search { 2 } else { 0 })
        .saturating_add(if has_divider { 1 } else { 0 })
}

pub(crate) struct ListPanelLayout {
    fixed_rows: u16,
    desired_list_rows: u16,
}

impl ListPanelLayout {
    pub(crate) fn new(fixed_rows: u16, desired_list_rows: u16) -> Self {
        Self {
            fixed_rows,
            desired_list_rows,
        }
    }

    pub(crate) fn split(&self, area: Rect) -> (Rect, Option<Rect>) {
        split_bottom_list_panel(area, self.fixed_rows, self.desired_list_rows)
    }

    pub(crate) fn visible_list_rows(&self, panel_area: Rect) -> usize {
        panel_area.height.saturating_sub(self.fixed_rows).into()
    }

    pub(crate) fn row_index(&self, panel_area: Rect, column: u16, row: u16) -> Option<usize> {
        if row < panel_area.y
            || row >= panel_area.y.saturating_add(panel_area.height)
            || column < panel_area.x
            || column >= panel_area.x.saturating_add(panel_area.width)
        {
            return None;
        }

        let relative_row = row.saturating_sub(panel_area.y);
        if relative_row < self.fixed_rows {
            return None;
        }

        let list_row = usize::from(relative_row - self.fixed_rows);
        let visible_rows = self.visible_list_rows(panel_area);
        (list_row < visible_rows).then_some(list_row)
    }
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

#[derive(Clone)]
pub(crate) struct SharedListPanelStyles {
    pub base_style: Style,
    pub selected_style: Option<Style>,
    pub text_style: Style,
    pub divider_style: Option<Style>,
    pub input_styles: InputStyles,
}

/// Build `InputStyles` from the app's `InlineTheme`.
pub(crate) fn input_styles_from_theme(theme: &InlineTheme) -> InputStyles {
    let palette = Palette {
        foreground: theme
            .foreground
            .map(ratatui_color_from_ansi)
            .unwrap_or(Color::White),
        muted: theme
            .secondary
            .map(ratatui_color_from_ansi)
            .unwrap_or(Color::Gray),
        faint: Color::DarkGray,
        primary: theme
            .primary
            .map(ratatui_color_from_ansi)
            .unwrap_or(Color::Cyan),
        secondary: theme
            .secondary
            .map(ratatui_color_from_ansi)
            .unwrap_or(Color::Gray),
        surface: Color::Black,
        border: Color::DarkGray,
        highlight: theme
            .primary
            .map(ratatui_color_from_ansi)
            .unwrap_or(Color::Cyan),
        on_highlight: Color::Black,
        error: Color::Red,
        success: Color::Green,
    };
    InputStyles::from_palette(&palette)
}

pub(crate) fn render_shared_search_field(
    frame: &mut Frame<'_>,
    area: Rect,
    search: &SharedSearchField,
    input_styles: &InputStyles,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let input_widget = Input::new(search.label.as_str())
        .prompt(">")
        .placeholder(search.placeholder.as_deref().unwrap_or("Type to filter..."))
        .styles(input_styles.clone());

    let mut input_state = InputState::new();
    input_state.set_value(search.query.clone());
    input_state.set_focused(true);
    for _ in 0..search.query.chars().count() {
        input_state.move_right();
    }

    frame.render_stateful_widget(&input_widget, area, &mut input_state);
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
    frame.render_widget(Block::default().style(styles.base_style), inner);

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
        constraints.push(Constraint::Length(2));
    }
    let show_divider = styles.divider_style.is_some()
        && (header_rows > 0 || info_rows > 0 || sections.search.is_some());
    if show_divider {
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
        render_shared_search_field(frame, chunks[idx], search, &styles.input_styles);
        idx += 1;
    }

    if show_divider && idx < chunks.len() {
        let divider_style = styles.divider_style.expect("divider style");
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                crate::config::constants::ui::INLINE_BLOCK_HORIZONTAL
                    .repeat(chunks[idx].width as usize),
                divider_style,
            )))
            .wrap(Wrap { trim: false }),
            chunks[idx],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::ui;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn shared_list_panel_renders_divider_before_list_when_enabled() {
        let backend = TestBackend::new(40, 6);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut model = StaticRowsListPanelModel {
            rows: vec![(
                InlineListRow::single(
                    Line::from(Span::styled("Item A".to_string(), Style::default())),
                    Style::default(),
                ),
                1,
            )],
            selected: Some(0),
            offset: 0,
            visible_rows: 0,
        };

        terminal
            .draw(|frame| {
                render_shared_list_panel(
                    frame,
                    Rect::new(0, 0, 40, 6),
                    SharedListPanelSections {
                        header: vec![Line::from("Header")],
                        info: vec![Line::from("Info")],
                        search: Some(SharedSearchField {
                            label: "Search".to_string(),
                            placeholder: Some("query".to_string()),
                            query: String::new(),
                        }),
                    },
                    SharedListPanelStyles {
                        base_style: Style::default(),
                        selected_style: Some(Style::default()),
                        text_style: Style::default(),
                        divider_style: Some(Style::default()),
                        input_styles: InputStyles::default(),
                    },
                    &mut model,
                );
            })
            .expect("list panel render");

        let buffer = terminal.backend().buffer();
        let divider_row = (0..buffer.area.width)
            .filter_map(|x| buffer.cell((x, 4)).map(|cell| cell.symbol().to_string()))
            .collect::<String>()
            .trim_end()
            .to_string();

        assert_eq!(
            divider_row,
            ui::INLINE_BLOCK_HORIZONTAL.repeat(buffer.area.width as usize)
        );
    }
}
