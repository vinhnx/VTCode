use super::*;

/// Generic palette rendering helper to avoid duplication
struct PaletteRenderParams<F>
where
    F: for<'a> Fn(&Session, &'a str, bool) -> ListItem<'static>,
{
    is_active: bool,
    title: String,
    items: Vec<(usize, String, bool)>, // (index, display_text, is_selected)
    instructions: Vec<Line<'static>>,
    has_more: bool,
    more_text: String,
    render_item: F,
}

fn render_palette_generic<F>(
    session: &mut Session,
    frame: &mut Frame<'_>,
    viewport: Rect,
    params: PaletteRenderParams<F>,
) where
    F: for<'a> Fn(&Session, &'a str, bool) -> ListItem<'static>,
{
    if !params.is_active || viewport.height == 0 || viewport.width == 0 || session.modal.is_some() {
        return;
    }

    if params.items.is_empty() {
        return;
    }

    let modal_height =
        params.items.len() + params.instructions.len() + 2 + if params.has_more { 1 } else { 0 };
    let area = compute_modal_area(viewport, modal_height, 0, 0, true);

    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title(params.title)
        .border_type(terminal_capabilities::get_border_type())
        .style(default_style(session))
        .border_style(border_style(session));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let layout = ModalListLayout::new(inner, params.instructions.len());
    if let Some(text_area) = layout.text_area {
        let paragraph = Paragraph::new(params.instructions).wrap(Wrap { trim: true });
        frame.render_widget(paragraph, text_area);
    }

    let mut list_items: Vec<ListItem> = params
        .items
        .iter()
        .map(|(_, display_text, is_selected)| {
            (params.render_item)(session, display_text.as_str(), *is_selected)
        })
        .collect();

    if params.has_more {
        let continuation_style =
            default_style(session).add_modifier(Modifier::DIM | Modifier::ITALIC);
        list_items.push(ListItem::new(Line::from(Span::styled(
            params.more_text,
            continuation_style,
        ))));
    }

    let list = List::new(list_items)
        .style(default_style(session))
        .highlight_symbol(ui::MODAL_LIST_HIGHLIGHT_FULL)
        .repeat_highlight_symbol(true);
    frame.render_widget(list, layout.list_area);
}

pub fn render_file_palette(session: &mut Session, frame: &mut Frame<'_>, viewport: Rect) {
    if !session.file_palette_active {
        return;
    }

    let Some(palette) = session.file_palette.as_ref() else {
        return;
    };

    if viewport.height == 0 || viewport.width == 0 || session.modal.is_some() {
        return;
    }

    // Show loading state if no files loaded yet
    if !palette.has_files() {
        render_file_palette_loading(session, frame, viewport);
        return;
    }

    let items = palette.current_page_items();
    if items.is_empty() && palette.filter_query().is_empty() {
        return;
    }

    // Convert items to generic format
    let generic_items: Vec<(usize, String, bool)> = items
        .iter()
        .map(|(idx, entry, selected)| {
            let display = if entry.is_dir {
                format!("{}/ ", entry.display_name)
            } else {
                entry.display_name.clone()
            };
            (*idx, display, *selected)
        })
        .collect();

    let title = format!(
        "File Browser (Page {}/{})",
        palette.current_page_number(),
        palette.total_pages()
    );

    let instructions = file_palette_instructions(session, palette);
    let has_more = palette.has_more_items();
    let more_text = format!(
        "  ... ({} more items)",
        palette
            .total_items()
            .saturating_sub(palette.current_page_number() * 20)
    );

    // Render using generic helper
    render_palette_generic(
        session,
        frame,
        viewport,
        PaletteRenderParams {
            is_active: true, // is_active already checked above
            title,
            items: generic_items,
            instructions,
            has_more,
            more_text,
            render_item: |session, display_text: &str, is_selected| {
                let base_style = if is_selected {
                    modal_list_highlight_style(session)
                } else {
                    default_style(session)
                };

                // Apply file-specific styling
                let mut style = base_style;

                // Add icon prefix based on file type
                let (prefix, is_dir) = if display_text.ends_with("/ ") {
                    ("↳  ", true)
                } else {
                    ("  · ", false)
                };

                if is_dir {
                    style = style.add_modifier(Modifier::BOLD);
                }

                let display = format!("{}{}", prefix, display_text.trim_end_matches("/ "));
                ListItem::new(Line::from(display).style(style))
            },
        },
    );
}

fn render_file_palette_loading(session: &Session, frame: &mut Frame<'_>, viewport: Rect) {
    let modal_height = 3;
    let area = compute_modal_area(viewport, modal_height, 0, 0, true);

    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title("File Browser")
        .border_type(terminal_capabilities::get_border_type())
        .style(default_style(session))
        .border_style(border_style(session));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height > 0 && inner.width > 0 {
        let loading_text = vec![Line::from(Span::styled(
            "Loading workspace files...".to_owned(),
            default_style(session).add_modifier(Modifier::DIM),
        ))];
        let paragraph = Paragraph::new(loading_text).wrap(Wrap { trim: true });
        frame.render_widget(paragraph, inner);
    }
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

pub(super) fn has_input_status(session: &Session) -> bool {
    let left_present = session
        .input_status_left
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
    if left_present {
        return true;
    }
    session
        .input_status_right
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty())
}
