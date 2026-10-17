use super::*;
use crate::tui::config::constants::ui;
use crate::tui::core_tui::session::inline_list::{InlineListRow, list_cursor};
use crate::tui::core_tui::session::list_panel::{
    ListPanelLayout, SharedListPanelSections, SharedListPanelStyles, SharedSearchField, StaticRowsListPanelModel,
    fixed_section_rows, fixed_section_rows_with_divider, input_styles_from_theme, render_shared_list_panel,
    rows_to_u16,
};
use ratatui::widgets::{Clear, Paragraph, Wrap};

#[derive(Clone)]
struct AgentPaletteRenderRow {
    text: String,
    subtitle: Option<String>,
    style: Style,
    selectable: bool,
    selected: bool,
    global_index: usize,
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
    let search_rows = if palette.has_agents() { 1 } else { 0 };
    let fixed_rows = fixed_section_rows(1, info_rows, search_rows);
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
    if !session.inline_lists_visible() || area.height == 0 || area.width == 0 || !session.agent_palette_visible() {
        return;
    }

    let Some(palette) = session.agent_palette.as_ref() else {
        return;
    };

    frame.render_widget(Clear, area);

    if !palette.has_agents() {
        let loading = Paragraph::new(Line::from(Span::styled(
            "Loading subagents...".to_owned(),
            session.core.styles.muted_text_style(),
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

    let base_style = default_style(session);
    // Muted by explicit color (theme secondary), never `Modifier::DIM`:
    // DIM doubles as a sticky area style in this pipeline and renders
    // near-invisible on several terminals.
    let muted_style = session.core.styles.muted_text_style();
    let highlight_style = modal_list_highlight_style(session);

    let selected = rows.iter().position(|row| row.selectable && row.selected);
    let mut global_indices = Vec::new();
    let rendered_rows = rows
        .into_iter()
        .enumerate()
        .map(|(idx, row)| {
            global_indices.push(row.global_index);
            let is_selected = selected == Some(idx);
            let cursor = list_cursor(is_selected);
            let cursor_style = if is_selected { highlight_style } else { muted_style };
            let name_style = if is_selected { highlight_style } else { row.style };
            let mut spans = vec![
                Span::styled(cursor, cursor_style),
                Span::styled(" ", cursor_style),
                Span::styled(row.text, name_style),
            ];
            if let Some(subtitle) = row.subtitle {
                let sub_style = if is_selected { highlight_style } else { muted_style };
                spans.push(Span::styled(format!("  {subtitle}"), sub_style));
            }

            (
                InlineListRow::single(
                    Line::from(spans),
                    if row.selectable {
                        muted_style
                    } else {
                        muted_style.add_modifier(Modifier::ITALIC)
                    },
                ),
                1_u16,
            )
        })
        .collect::<Vec<_>>();

    let sections = SharedListPanelSections {
        header: vec![Line::from(Span::styled("Agents".to_owned(), highlight_style))],
        info: instructions,
        search: Some(SharedSearchField {
            label: String::new(),
            placeholder: Some("name or description".to_owned()),
            query: palette.filter_query().to_owned(),
        }),
    };
    let scroll_offset = palette.scroll_offset();
    let mut model = StaticRowsListPanelModel {
        rows: rendered_rows,
        selected,
        offset: scroll_offset,
        visible_rows: 0,
    };

    render_shared_list_panel(
        frame,
        area,
        sections,
        SharedListPanelStyles {
            base_style,
            selected_style: Some(highlight_style),
            text_style: base_style,
            divider_style: None,
            input_styles: input_styles_from_theme(&session.core.theme),
            show_divider: false,
        },
        &mut model,
    );

    if let Some(palette) = session.agent_palette.as_mut() {
        if let Some(selected) = model.selected {
            let global_idx = global_indices.get(selected).copied();
            if let Some(global_idx) = global_idx {
                palette.select_index(global_idx);
            } else {
                palette.set_selected(None);
            }
        } else {
            palette.set_selected(None);
        }
        palette.set_scroll_offset(model.offset);
    }
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
    let search_rows = if has_files { 1 } else { 0 };
    let fixed_rows = fixed_section_rows_with_divider(1, info_rows, search_rows, true);
    let list_rows: u16 = if has_files {
        palette.total_items().min(ui::INLINE_LIST_MAX_ROWS) as u16
    } else {
        1
    };

    Some(ListPanelLayout::new(fixed_rows, list_rows))
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
    if !session.inline_lists_visible() || area.height == 0 || area.width == 0 || !session.file_palette_visible() {
        return;
    }

    let Some(palette) = session.file_palette.as_ref() else {
        return;
    };

    frame.render_widget(Clear, area);

    if !palette.has_files() {
        // Distinguish "index still loading" from "the current directory is
        // genuinely empty" so the user does not wait on a finished load.
        let message = if palette.is_search_mode() && !palette.search_index_loaded() {
            "Indexing workspace files…".to_owned()
        } else {
            "No files here".to_owned()
        };
        let loading = Paragraph::new(Line::from(Span::styled(message, session.core.styles.muted_text_style())))
            .wrap(Wrap { trim: true });
        frame.render_widget(loading, area);
        return;
    }

    let base_style = default_style(session);
    // Muted by explicit color (theme secondary), never `Modifier::DIM`: DIM
    // doubles as a sticky area style in this pipeline and renders
    // near-invisible on several terminals.
    let muted_style = session.core.styles.muted_text_style();
    let highlight_style = modal_list_highlight_style(session);
    let accent = accent_style(session);

    let warning_style = session.core.styles.warning_style();
    let search_mode = palette.is_search_mode();
    let selected = palette.selected_index();
    let rendered_rows: Vec<(InlineListRow, u16)> = palette
        .list_entries()
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            let is_selected = selected == Some(idx);
            let cursor = list_cursor(is_selected);
            let cursor_style = if is_selected { highlight_style } else { muted_style };

            let broken = entry.symlink_broken;
            let name_style = if entry.is_parent {
                cursor_style.add_modifier(Modifier::ITALIC)
            } else if broken {
                // A dangling symlink is actionable but currently unusable.
                warning_style
            } else if entry.is_dir {
                if is_selected {
                    highlight_style.add_modifier(Modifier::BOLD)
                } else {
                    accent.add_modifier(Modifier::BOLD)
                }
            } else if is_selected {
                highlight_style
            } else {
                palette
                    .style_for_entry(entry)
                    .map(crate::tui::core_tui::style::ratatui_style_from_ansi)
                    .unwrap_or(muted_style)
            };

            // Glyph communicates row kind at a glance: `↑` ascends, `▸` opens a
            // directory, `▧`/`⚙` mark images/executables, and code/other files
            // keep a blank cell so the filename column stays aligned.
            let glyph = if entry.is_parent {
                ui::INLINE_FILE_PICKER_PARENT_PREFIX.trim_end().to_owned()
            } else {
                entry.kind.glyph().to_owned()
            };

            let mut spans = vec![
                Span::styled(cursor, cursor_style),
                Span::styled(" ", cursor_style),
                Span::styled(format!("{glyph} "), cursor_style),
            ];

            // Search rows carry the full relative path; split it so the parent
            // directory reads as muted hierarchy and the basename as the target.
            // Browse rows already show a basename only.
            if search_mode && !entry.is_parent {
                if let Some((dir, base)) = entry.display_name.rsplit_once('/') {
                    spans.push(Span::styled(format!("{dir}/"), muted_style));
                    spans.push(Span::styled(base.to_owned(), name_style));
                } else {
                    spans.push(Span::styled(entry.display_name.clone(), name_style));
                }
            } else {
                spans.push(Span::styled(entry.display_name.clone(), name_style));
            }

            if let Some(target) = &entry.symlink_target {
                let arrow_style = if broken { warning_style } else { muted_style };
                spans.push(Span::styled(format!(" → {}", target.display()), arrow_style));
                if broken {
                    spans.push(Span::styled(" (broken)".to_owned(), warning_style));
                }
            }

            (InlineListRow::single(Line::from(spans), muted_style), 1_u16)
        })
        .collect();

    // A `+N` suffix surfaces when the listing is longer than the panel's visible
    // window, so the user knows scrolling continues past the fold. The hidden
    // count uses the real visible-row budget, not a fixed constant.
    let overflow_suffix = if palette.has_more_items() {
        let visible_rows = file_palette_panel_layout(session).map_or(0, |layout| layout.visible_list_rows(area));
        if visible_rows > 0 && palette.total_items() > visible_rows {
            Some(format!("  +{} more", palette.total_items() - visible_rows))
        } else {
            None
        }
    } else {
        None
    };

    let header = if palette.is_search_mode() {
        // The active query is already visible in the search field, so the header
        // only needs to label the panel — repeating `(search: '…')` here is clutter.
        let mut spans = vec![Span::styled("Files", highlight_style)];
        if let Some(suffix) = &overflow_suffix {
            spans.push(Span::styled(suffix.clone(), muted_style));
        }
        vec![Line::from(spans)]
    } else {
        let mut spans = vec![
            Span::styled("Files", highlight_style),
            Span::styled(format!("  {}", palette.breadcrumb()), muted_style),
        ];
        if let Some(suffix) = &overflow_suffix {
            spans.push(Span::styled(suffix.clone(), muted_style));
        }
        vec![Line::from(spans)]
    };

    let sections = SharedListPanelSections {
        header,
        info: file_palette_instructions(session, palette),
        search: Some(SharedSearchField {
            label: String::new(),
            placeholder: Some("filename or path".to_owned()),
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
            base_style,
            selected_style: Some(highlight_style),
            text_style: base_style,
            divider_style: Some(session.core.styles.border_style()),
            input_styles: input_styles_from_theme(&session.core.theme),
            show_divider: true,
        },
        &mut model,
    );

    // The shared panel may clamp the selection (e.g. after filtering); keep the
    // palette's selection state authoritative across frames.
    if let Some(palette) = session.file_palette.as_mut() {
        palette.set_selected(model.selected);
    }
}

fn build_agent_palette_rows(session: &Session, palette: &AgentPalette) -> Vec<AgentPaletteRenderRow> {
    let mut rows = Vec::new();
    let default = default_style(session);
    let muted = session.core.styles.muted_text_style();

    for (global_idx, entry, selected) in palette.current_page_items() {
        rows.push(AgentPaletteRenderRow {
            text: entry.display_name.clone(),
            subtitle: entry.description.clone(),
            style: default.add_modifier(Modifier::BOLD),
            selectable: true,
            selected,
            global_index: global_idx,
        });
    }

    if rows.is_empty() {
        rows.push(AgentPaletteRenderRow {
            text: "No matching agents".to_owned(),
            subtitle: None,
            style: muted,
            selectable: false,
            selected: false,
            global_index: 0,
        });
    }

    if palette.has_more_items() {
        let remaining = palette
            .total_items()
            .saturating_sub(palette.current_page_number() * palette.page_size());
        rows.push(AgentPaletteRenderRow {
            text: format!("... ({remaining} more items)"),
            subtitle: None,
            style: muted.add_modifier(Modifier::ITALIC),
            selectable: false,
            selected: false,
            global_index: 0,
        });
    }

    rows
}

fn agent_palette_instructions(session: &Session, palette: &AgentPalette) -> Vec<Line<'static>> {
    let mut lines = vec![];

    if palette.is_empty() {
        lines.push(Line::from(Span::styled("No agents found matching filter".to_owned(), default_style(session))));
    } else {
        lines.push(Line::from(Span::styled(
            "↑↓ Navigate · Enter select · Esc close".to_owned(),
            default_style(session),
        )));
    }

    lines
}

fn file_palette_instructions(session: &Session, palette: &FilePalette) -> Vec<Line<'static>> {
    let mut lines = vec![];

    if palette.is_empty() {
        let message = if palette.is_search_mode() && !palette.search_index_loaded() {
            "Indexing workspace files…"
        } else if palette.is_search_mode() {
            "No files match — refine the filter or clear it to browse"
        } else {
            "Empty directory — press ← to go up"
        };
        lines.push(Line::from(Span::styled(message.to_owned(), default_style(session))));
    } else {
        let nav_hint = if palette.is_search_mode() {
            "↑↓ Navigate · Enter select · ← up · Type to refine · Esc close"
        } else {
            "↑↓ Navigate · Enter open · Alt+Enter folder · ← up · Type to filter · Esc close"
        };

        lines.push(Line::from(Span::styled(nav_hint.to_owned(), default_style(session))));
    }

    lines
}
