use super::*;
use ratatui::widgets::{Clear, Paragraph, Wrap};
use crate::ui::tui::session::inline_list::{InlineListRow, selection_padding};
use crate::ui::tui::session::list_panel::{
    SharedListPanelSections, SharedListPanelStyles, SharedSearchField, StaticRowsListPanelModel,
    fixed_section_rows, render_shared_list_panel, rows_to_u16, split_bottom_list_panel,
};

#[derive(Clone)]
struct FilePaletteRenderRow {
    text: String,
    style: Style,
    selectable: bool,
    selected: bool,
}

pub fn split_inline_file_palette_area(session: &mut Session, area: Rect) -> (Rect, Option<Rect>) {
    if area.height == 0
        || area.width == 0
        || session.has_active_overlay()
        || !session.inline_lists_visible()
        || session.history_picker_state.active
        || !session.file_palette_active
    {
        return (area, None);
    }

    let Some(palette) = session.file_palette.as_ref() else {
        return (area, None);
    };

    let info_rows = if palette.has_files() {
        file_palette_instructions(session, palette).len()
    } else {
        1
    };
    let fixed_rows = fixed_section_rows(1, info_rows, palette.has_files());

    let list_rows = if palette.has_files() {
        let mut rows = palette.current_page_items().len().max(1);
        if palette.has_more_items() {
            rows += 1;
        }
        rows.min(ui::INLINE_LIST_MAX_ROWS)
    } else {
        1
    };

    split_bottom_list_panel(area, fixed_rows, rows_to_u16(list_rows))
}

pub fn render_file_palette(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    if !session.file_palette_active
        || !session.inline_lists_visible()
        || area.height == 0
        || area.width == 0
        || session.has_active_overlay()
    {
        return;
    }

    let Some(palette) = session.file_palette.as_ref() else {
        return;
    };

    frame.render_widget(Clear, area);

    if !palette.has_files() {
        let loading = Paragraph::new(Line::from(Span::styled(
            "Loading workspace files...".to_owned(),
            default_style(session).add_modifier(Modifier::DIM),
        )))
        .wrap(Wrap { trim: true });
        frame.render_widget(loading, area);
        return;
    }

    let instructions = file_palette_instructions(session, palette);
    let rows = build_file_palette_rows(session, palette);
    let item_count = rows.len();
    if item_count == 0 {
        return;
    }

    let default_style = default_style(session);
    let highlight_style = modal_list_highlight_style(session);
    let unselected_prefix = selection_padding();

    let selected = rows.iter().position(|row| row.selectable && row.selected);
    let rendered_rows = rows
        .into_iter()
        .map(|row| {
            (
                InlineListRow::single(
                    Line::from(vec![
                        Span::styled(unselected_prefix.clone(), default_style),
                        Span::styled(row.text, row.style),
                    ]),
                    if row.selectable {
                        default_style
                    } else {
                        default_style.add_modifier(Modifier::DIM)
                    },
                ),
                1_u16,
            )
        })
        .collect::<Vec<_>>();

    let filter = palette.filter_query();
    let sections = SharedListPanelSections {
        header: vec![Line::from(Span::styled("Files".to_owned(), default_style))],
        info: instructions,
        search: Some(SharedSearchField {
            label: "Search files".to_owned(),
            placeholder: Some("filename or path".to_owned()),
            query: filter.to_owned(),
        }),
    };
    let mut model = StaticRowsListPanelModel {
        rows: rendered_rows,
        selected,
        offset: 0,
        visible_rows: 0,
    };

    render_shared_list_panel(
        frame,
        area,
        sections,
        SharedListPanelStyles {
            base_style: default_style,
            selected_style: Some(highlight_style),
            text_style: default_style,
        },
        &mut model,
    );
}

fn build_file_palette_rows(session: &Session, palette: &FilePalette) -> Vec<FilePaletteRenderRow> {
    let mut rows = Vec::new();
    let default = default_style(session);

    for (_global_idx, entry, selected) in palette.current_page_items() {
        let mut style = default;
        let prefix = if entry.is_dir {
            style = style.add_modifier(Modifier::BOLD);
            "↳  "
        } else {
            "  · "
        };

        rows.push(FilePaletteRenderRow {
            text: format!("{}{}", prefix, entry.display_name),
            style,
            selectable: true,
            selected,
        });
    }

    if rows.is_empty() {
        rows.push(FilePaletteRenderRow {
            text: "No matching files".to_owned(),
            style: default.add_modifier(Modifier::DIM),
            selectable: false,
            selected: false,
        });
    }

    if palette.has_more_items() {
        let remaining = palette
            .total_items()
            .saturating_sub(palette.current_page_number() * 20);
        rows.push(FilePaletteRenderRow {
            text: format!("  ... ({} more items)", remaining),
            style: default.add_modifier(Modifier::DIM | Modifier::ITALIC),
            selectable: false,
            selected: false,
        });
    }

    rows
}

fn file_palette_instructions(session: &Session, palette: &FilePalette) -> Vec<Line<'static>> {
    let mut lines = vec![];

    if palette.is_empty() {
        lines.push(Line::from(Span::styled(
            "No files found matching filter".to_owned(),
            default_style(session).add_modifier(Modifier::DIM),
        )));
    } else {
        let total = palette.total_items();
        let count_text = if total == 1 {
            "1 file".to_owned()
        } else {
            format!("{} files", total)
        };

        let nav_text = "↑↓ Navigate · PgUp/PgDn Page · Tab/Enter Select";

        lines.push(Line::from(vec![Span::styled(
            format!("{} · Esc Close", nav_text),
            default_style(session),
        )]));

        lines.push(Line::from(vec![
            Span::styled(
                format!("Showing {}", count_text),
                default_style(session).add_modifier(Modifier::DIM),
            ),
            Span::styled(
                if !palette.filter_query().is_empty() {
                    format!(" matching '{}'", palette.filter_query())
                } else {
                    String::new()
                },
                accent_style(session),
            ),
        ]));
    }

    lines
}
