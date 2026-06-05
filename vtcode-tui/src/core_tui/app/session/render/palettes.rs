use super::*;
use crate::config::constants::ui;
use crate::core_tui::session::inline_list::{InlineListRow, selection_padding};
use crate::core_tui::session::list_panel::{
    ListPanelLayout, SharedListPanelSections, SharedListPanelStyles, SharedSearchField,
    StaticRowsListPanelModel, fixed_section_rows, fixed_section_rows_with_divider,
    input_styles_from_theme, render_shared_list_panel, render_shared_search_field, rows_to_u16,
};
use ratatui::widgets::{Clear, Paragraph, StatefulWidget, Wrap};
use ratatui_cheese::tree::{Mode, Tree, TreeStyles};

#[derive(Clone)]
struct AgentPaletteRenderRow {
    text: String,
    subtitle: Option<String>,
    style: Style,
    selectable: bool,
    selected: bool,
}

pub(crate) fn agent_palette_panel_layout(session: &Session) -> Option<ListPanelLayout> {
    if !session.agent_palette_visible() || !session.inline_lists_visible() {
        return None;
    }

    let palette = session.agent_palette.as_ref()?;
    let info_rows = if palette.has_agents() {
        agent_palette_instructions(session, palette).len()
    } else {
        1
    };
    let fixed_rows = fixed_section_rows(1, info_rows, palette.has_agents());
    let list_rows = if palette.has_agents() {
        let mut rows = palette.current_page_items().len().max(1);
        if palette.has_more_items() {
            rows += 1;
        }
        rows.min(ui::INLINE_LIST_MAX_ROWS)
    } else {
        1
    };

    Some(ListPanelLayout::new(fixed_rows, rows_to_u16(list_rows)))
}

pub fn split_inline_agent_palette_area(session: &mut Session, area: Rect) -> (Rect, Option<Rect>) {
    if area.height == 0 || area.width == 0 {
        return (area, None);
    }

    let Some(layout) = agent_palette_panel_layout(session) else {
        return (area, None);
    };

    layout.split(area)
}

pub fn render_agent_palette(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    if !session.inline_lists_visible()
        || area.height == 0
        || area.width == 0
        || !session.agent_palette_visible()
    {
        return;
    }

    let Some(palette) = session.agent_palette.as_ref() else {
        return;
    };

    frame.render_widget(Clear, area);

    if !palette.has_agents() {
        let loading = Paragraph::new(Line::from(Span::styled(
            "Loading subagents...".to_owned(),
            default_style(session).add_modifier(Modifier::DIM),
        )))
        .wrap(Wrap { trim: true });
        frame.render_widget(loading, area);
        return;
    }

    let instructions = agent_palette_instructions(session, palette);
    let rows = build_agent_palette_rows(session, palette);
    if rows.is_empty() {
        return;
    }

    let default_style = default_style(session);
    let dim_style = default_style.add_modifier(Modifier::DIM);
    let highlight_style = modal_list_highlight_style(session);
    let blank_gutter = selection_padding();

    let selected = rows.iter().position(|row| row.selectable && row.selected);
    let rendered_rows = rows
        .into_iter()
        .enumerate()
        .map(|(idx, row)| {
            let is_selected = selected == Some(idx);
            let cursor = if is_selected {
                format!("{} ", ui::MODAL_LIST_HIGHLIGHT_SYMBOL)
            } else {
                blank_gutter.clone()
            };
            let cursor_style = if is_selected {
                highlight_style
            } else {
                dim_style
            };
            let name_style = if is_selected {
                highlight_style
            } else {
                row.style.add_modifier(Modifier::DIM)
            };
            let mut spans = vec![
                Span::styled(cursor, cursor_style),
                Span::styled(row.text, name_style),
            ];
            if let Some(subtitle) = row.subtitle {
                let sub_style = if is_selected {
                    highlight_style
                } else {
                    dim_style
                };
                spans.push(Span::styled(format!("  {}", subtitle), sub_style));
            }

            (
                InlineListRow::single(
                    Line::from(spans),
                    if row.selectable {
                        dim_style
                    } else {
                        dim_style.add_modifier(Modifier::DIM)
                    },
                ),
                1_u16,
            )
        })
        .collect::<Vec<_>>();

    let sections = SharedListPanelSections {
        header: vec![Line::from(Span::styled(
            "Agents".to_owned(),
            highlight_style,
        ))],
        info: instructions,
        search: Some(SharedSearchField {
            label: "Search agents".to_owned(),
            placeholder: Some("name or description".to_owned()),
            query: palette.filter_query().to_owned(),
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
            base_style: dim_style,
            selected_style: Some(highlight_style),
            text_style: dim_style,
            divider_style: None,
            input_styles: input_styles_from_theme(&session.core.theme),
        },
        &mut model,
    );
}

pub(crate) fn file_palette_panel_layout(session: &Session) -> Option<ListPanelLayout> {
    if !session.file_palette_visible() || !session.inline_lists_visible() {
        return None;
    }

    let palette = session.file_palette.as_ref()?;
    let info_rows = if palette.has_files() {
        file_palette_instructions(session, palette).len()
    } else {
        1
    };
    let has_files = palette.has_files();
    let fixed_rows = fixed_section_rows_with_divider(1, info_rows, has_files, true);
    let tree_rows: u16 = if has_files { 15 } else { 1 };

    Some(ListPanelLayout::new(fixed_rows, tree_rows))
}

pub fn split_inline_file_palette_area(session: &mut Session, area: Rect) -> (Rect, Option<Rect>) {
    if area.height == 0 || area.width == 0 {
        return (area, None);
    }

    let Some(layout) = file_palette_panel_layout(session) else {
        return (area, None);
    };

    layout.split(area)
}

pub fn render_file_palette(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    if !session.inline_lists_visible()
        || area.height == 0
        || area.width == 0
        || !session.file_palette_visible()
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

    let dim_style = default_style(session).add_modifier(Modifier::DIM);
    let border_style = session.core.styles.border_style();

    let instructions = file_palette_instructions(session, palette);
    let filter_query = palette.filter_query().to_owned();
    let tree_groups = palette.tree_groups().to_vec();

    let header_line = Line::from(Span::styled("Files".to_owned(), dim_style));
    let show_search = true;
    let show_divider = true;

    let mut constraints = Vec::new();
    constraints.push(Constraint::Length(1)); // header
    constraints.push(Constraint::Length(instructions.len() as u16)); // info
    if show_search {
        constraints.push(Constraint::Length(2)); // search
    }
    if show_divider {
        constraints.push(Constraint::Length(1)); // divider
    }
    constraints.push(Constraint::Min(1)); // tree

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut idx = 0;

    // Header
    frame.render_widget(Paragraph::new(header_line).style(dim_style), layout[idx]);
    idx += 1;

    // Info instructions
    frame.render_widget(
        Paragraph::new(Text::from(instructions)).style(dim_style),
        layout[idx],
    );
    idx += 1;

    // Search field
    if show_search {
        let search = SharedSearchField {
            label: "Search files".to_owned(),
            placeholder: Some("filename or path".to_owned()),
            query: filter_query,
        };
        let input_styles = input_styles_from_theme(&session.core.theme);
        render_shared_search_field(frame, layout[idx], &search, &input_styles);
        idx += 1;
    }

    // Divider
    if show_divider {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                ui::INLINE_BLOCK_HORIZONTAL.repeat(layout[idx].width as usize),
                border_style,
            )))
            .wrap(Wrap { trim: false }),
            layout[idx],
        );
        idx += 1;
    }

    // Tree
    let tree_area = layout[idx];
    if tree_area.height == 0 {
        return;
    }

    let tree_styles = file_tree_styles(session);
    let state = session
        .file_palette
        .as_mut()
        .expect("palette checked above")
        .tree_state_mut();
    let tree = Tree::default()
        .groups(tree_groups)
        .mode(Mode::None)
        .styles(tree_styles)
        .chevron_collapsed("\u{1F5C0}")
        .chevron_expanded("\u{231E}")
        .highlight_full_row(true);

    StatefulWidget::render(&tree, tree_area, frame.buffer_mut(), state);
}

fn file_tree_styles(session: &Session) -> TreeStyles {
    let default_style = default_style(session).bg(Color::Reset);
    let dim_style = default_style.add_modifier(Modifier::DIM);
    let highlight = modal_list_highlight_style(session).bg(Color::Reset);
    let accent = accent_style(session).bg(Color::Reset);

    TreeStyles {
        parent: default_style,
        child: dim_style,
        selected: highlight,
        chevron: dim_style,
        chevron_active: accent,
        chevron_dim: dim_style.add_modifier(Modifier::DIM),
        count: dim_style,
        icon: Style::default(),
    }
}

fn build_agent_palette_rows(
    session: &Session,
    palette: &AgentPalette,
) -> Vec<AgentPaletteRenderRow> {
    let mut rows = Vec::new();
    let default = default_style(session);

    for (_global_idx, entry, selected) in palette.current_page_items() {
        rows.push(AgentPaletteRenderRow {
            text: entry.display_name.clone(),
            subtitle: entry.description.clone(),
            style: default.add_modifier(Modifier::BOLD),
            selectable: true,
            selected,
        });
    }

    if rows.is_empty() {
        rows.push(AgentPaletteRenderRow {
            text: "No matching agents".to_owned(),
            subtitle: None,
            style: default.add_modifier(Modifier::DIM),
            selectable: false,
            selected: false,
        });
    }

    if palette.has_more_items() {
        let remaining = palette
            .total_items()
            .saturating_sub(palette.current_page_number() * 20);
        rows.push(AgentPaletteRenderRow {
            text: format!("... ({} more items)", remaining),
            subtitle: None,
            style: default.add_modifier(Modifier::DIM | Modifier::ITALIC),
            selectable: false,
            selected: false,
        });
    }

    rows
}

fn agent_palette_instructions(session: &Session, palette: &AgentPalette) -> Vec<Line<'static>> {
    let mut lines = vec![];

    if palette.is_empty() {
        lines.push(Line::from(Span::styled(
            "No agents found matching filter".to_owned(),
            default_style(session).add_modifier(Modifier::DIM),
        )));
    } else {
        let total = palette.total_items();
        let count_text = if total == 1 {
            "1 agent".to_owned()
        } else {
            format!("{} agents", total)
        };

        lines.push(Line::from(Span::styled(
            "↑↓ Navigate · PgUp/PgDn Page · Tab/Enter Select · Esc Close".to_owned(),
            default_style(session),
        )));

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

        lines.push(Line::from(vec![Span::styled(
            "↑↓ Navigate · ← → Expand/Collapse · Enter Select · Esc Close".to_owned(),
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
