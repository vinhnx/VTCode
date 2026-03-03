use crate::config::constants::ui;
use ratatui::{
    prelude::*,
    widgets::{Paragraph, Widget, Wrap},
};
use tui_widget_list::{ListBuilder, ListState as WidgetListState, ListView};

#[derive(Clone)]
pub(crate) struct InlineListRow {
    pub lines: Vec<Line<'static>>,
    pub style: Style,
}

impl InlineListRow {
    pub(crate) fn single(line: Line<'static>, style: Style) -> Self {
        Self {
            lines: vec![line],
            style,
        }
    }
}

impl Widget for InlineListRow {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.lines)
            .style(self.style)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

pub(crate) fn row_height(lines: &[Line<'_>]) -> u16 {
    lines.len().max(1).min(u16::MAX as usize) as u16
}

pub(crate) fn selection_padding_width() -> usize {
    ui::MODAL_LIST_HIGHLIGHT_SYMBOL.chars().count().max(1)
}

pub(crate) fn selection_padding() -> String {
    " ".repeat(selection_padding_width())
}

#[derive(Clone, Copy)]
pub(crate) struct InlineListRenderOptions {
    pub base_style: Style,
    pub selected_style: Option<Style>,
    pub scroll_padding: u16,
    pub infinite_scrolling: bool,
}

pub(crate) fn render_inline_list_with_options(
    frame: &mut Frame<'_>,
    area: Rect,
    rows: Vec<(InlineListRow, u16)>,
    selected: Option<usize>,
    options: InlineListRenderOptions,
) -> WidgetListState {
    let item_count = rows.len();
    let mut widget_state = WidgetListState::default();
    widget_state.select(selected.filter(|index| *index < item_count));

    let builder = ListBuilder::new(move |context| {
        let (base_row, height) = &rows[context.index];
        let mut row = base_row.clone();
        if context.is_selected
            && let Some(style) = options.selected_style
        {
            row.style = style;
        }
        (row, *height)
    });

    let widget = ListView::new(builder, item_count)
        .style(options.base_style)
        .scroll_padding(options.scroll_padding)
        .infinite_scrolling(options.infinite_scrolling);
    frame.render_stateful_widget(widget, area, &mut widget_state);
    widget_state
}
