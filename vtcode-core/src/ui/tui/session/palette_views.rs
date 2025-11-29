use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
};

use super::super::style::ratatui_color_from_ansi;
use super::super::types::InlineTheme;
use super::terminal_capabilities;
use super::file_palette::FilePalette;
use super::modal::{ModalListLayout, compute_modal_area};
use super::prompt_palette::PromptPalette;

pub(super) fn render_file_palette(
    frame: &mut Frame<'_>,
    viewport: Rect,
    palette: &FilePalette,
    theme: &InlineTheme,
) {
    if viewport.height == 0 || viewport.width == 0 {
        return;
    }

    if !palette.has_files() {
        render_file_palette_loading(frame, viewport, theme);
        return;
    }

    let items = palette.current_page_items();
    if items.is_empty() && palette.filter_query().is_empty() {
        return;
    }

    let mut width_hint = 40u16;
    for (_, entry, _) in &items {
        width_hint = width_hint.max(entry.relative_path.len() as u16 + 4);
    }

    let instructions = file_palette_instructions(palette, theme);
    let has_continuation = palette.has_more_items();
    let modal_height = items.len() + instructions.len() + 2 + if has_continuation { 1 } else { 0 };
    let area = compute_modal_area(viewport, width_hint, modal_height, 0, 0, true);

    frame.render_widget(Clear, area);
    let title = " 📁 File Browser ";
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(terminal_capabilities::get_border_type())
        .border_style(border_style(theme));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let layout = ModalListLayout::new(inner, instructions.len());
    if let Some(text_area) = layout.text_area {
        let instructions_paragraph =
            ratatui::widgets::Paragraph::new(instructions).style(default_style(theme));
        frame.render_widget(instructions_paragraph, text_area);
    }

    let default_style_val = default_style(theme);
    let mut list_items: Vec<ListItem> = items
        .iter()
        .map(|(_index, entry, is_selected)| {
            let display_path = if entry.is_dir {
                format!("{}/", entry.relative_path)
            } else {
                entry.relative_path.clone()
            };

            let icon = if entry.is_dir { "📁" } else { "📄" };

            let mut content_style = default_style_val;
            if *is_selected {
                content_style = content_style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
                if let Some(primary) = theme.primary.or(theme.foreground) {
                    content_style = content_style.fg(ratatui_color_from_ansi(primary));
                }
            }

            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", icon), content_style),
                Span::styled(display_path, content_style),
            ]))
        })
        .collect();

    if palette.has_more_items() {
        let more_indicator = format!(
            "   {} more files available...",
            palette.total_items() - palette.current_page_items().len()
        );
        list_items.push(ListItem::new(Line::from(Span::styled(
            more_indicator,
            default_style_val.add_modifier(Modifier::DIM | Modifier::ITALIC),
        ))));
    }

    let list = List::new(list_items).style(default_style_val);
    frame.render_widget(list, layout.list_area);
}

fn render_file_palette_loading(frame: &mut Frame<'_>, viewport: Rect, theme: &InlineTheme) {
    let width_hint = 40u16;
    let modal_height = 3;
    let area = compute_modal_area(viewport, width_hint, modal_height, 0, 0, true);

    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" 📁 File Browser ")
        .borders(Borders::ALL)
        .border_type(terminal_capabilities::get_border_type())
        .border_style(border_style(theme));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height > 0 && inner.width > 0 {
        let loading_text = Line::from(Span::styled(
            "Loading files...",
            default_style(theme).add_modifier(Modifier::DIM),
        ));
        let loading_paragraph =
            ratatui::widgets::Paragraph::new(loading_text).style(default_style(theme));
        frame.render_widget(loading_paragraph, inner);
    }
}

fn file_palette_instructions(palette: &FilePalette, theme: &InlineTheme) -> Vec<Line<'static>> {
    let mut lines = vec![];

    if palette.is_empty() {
        lines.push(Line::from(Span::styled(
            "No files found matching filter".to_owned(),
            default_style(theme).add_modifier(Modifier::DIM),
        )));
    } else {
        lines.push(Line::from(vec![Span::styled(
            "↑↓ Navigate · Enter/Tab Select · Esc Close",
            default_style(theme),
        )]));

        let total = palette.total_items();
        let count_text = if total == 1 {
            "1 file".to_owned()
        } else {
            format!("{} files", total)
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!("Showing {}", count_text),
                default_style(theme).add_modifier(Modifier::DIM),
            ),
            Span::styled(
                if !palette.filter_query().is_empty() {
                    format!(" matching '{}'", palette.filter_query())
                } else {
                    String::new()
                },
                accent_style(theme),
            ),
        ]));
    }

    lines
}

pub(super) fn render_prompt_palette(
    frame: &mut Frame<'_>,
    viewport: Rect,
    palette: &PromptPalette,
    theme: &InlineTheme,
) {
    if viewport.height == 0 || viewport.width == 0 {
        return;
    }

    if !palette.has_prompts() {
        render_prompt_palette_loading(frame, viewport, theme);
        return;
    }

    let items = palette.current_page_items();
    if items.is_empty() && palette.filter_query().is_empty() {
        return;
    }

    let mut width_hint = 40u16;
    for (_, entry, _) in &items {
        width_hint = width_hint.max(entry.name.len() as u16 + 4);
    }

    let instructions = prompt_palette_instructions(palette, theme);
    let has_continuation = palette.has_more_items();
    let modal_height = items.len() + instructions.len() + 2 + if has_continuation { 1 } else { 0 };
    let area = compute_modal_area(viewport, width_hint, modal_height, 0, 0, true);

    frame.render_widget(Clear, area);
    let title = " 💬 Custom Prompts ";
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(terminal_capabilities::get_border_type())
        .border_style(border_style(theme));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let layout = ModalListLayout::new(inner, instructions.len());
    if let Some(text_area) = layout.text_area {
        let instructions_paragraph =
            ratatui::widgets::Paragraph::new(instructions).style(default_style(theme));
        frame.render_widget(instructions_paragraph, text_area);
    }

    let default_style_val = default_style(theme);
    let mut list_items: Vec<ListItem> = items
        .iter()
        .map(|(_index, entry, is_selected)| {
            let mut content_style = default_style_val;
            if *is_selected {
                content_style = content_style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
                if let Some(primary) = theme.primary.or(theme.foreground) {
                    content_style = content_style.fg(ratatui_color_from_ansi(primary));
                }
            }

            ListItem::new(Line::from(Span::styled(entry.name.clone(), content_style)))
        })
        .collect();

    if palette.has_more_items() {
        let more_indicator = format!(
            "   {} more prompts available...",
            palette.total_items() - palette.current_page_items().len()
        );
        list_items.push(ListItem::new(Line::from(Span::styled(
            more_indicator,
            default_style_val.add_modifier(Modifier::DIM | Modifier::ITALIC),
        ))));
    }

    let list = List::new(list_items).style(default_style_val);
    frame.render_widget(list, layout.list_area);
}

fn render_prompt_palette_loading(frame: &mut Frame<'_>, viewport: Rect, theme: &InlineTheme) {
    let width_hint = 40u16;
    let modal_height = 3;
    let area = compute_modal_area(viewport, width_hint, modal_height, 0, 0, true);

    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" 💬 Custom Prompts ")
        .borders(Borders::ALL)
        .border_type(terminal_capabilities::get_border_type())
        .border_style(border_style(theme));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height > 0 && inner.width > 0 {
        let loading_text = Line::from(Span::styled(
            "Loading prompts...",
            default_style(theme).add_modifier(Modifier::DIM),
        ));
        let loading_paragraph =
            ratatui::widgets::Paragraph::new(loading_text).style(default_style(theme));
        frame.render_widget(loading_paragraph, inner);
    }
}

fn prompt_palette_instructions(palette: &PromptPalette, theme: &InlineTheme) -> Vec<Line<'static>> {
    let mut lines = vec![];

    if palette.is_empty() {
        lines.push(Line::from(Span::styled(
            "No prompts found matching filter".to_owned(),
            default_style(theme).add_modifier(Modifier::DIM),
        )));
    } else {
        let total = palette.total_items();
        let count_text = if total == 1 {
            "1 prompt".to_owned()
        } else {
            format!("{} prompts", total)
        };

        lines.push(Line::from(vec![Span::styled(
            "↑↓ Navigate · Enter/Tab Select · Esc Close",
            default_style(theme),
        )]));

        lines.push(Line::from(vec![
            Span::styled(
                format!("Showing {}", count_text),
                default_style(theme).add_modifier(Modifier::DIM),
            ),
            Span::styled(
                if !palette.filter_query().is_empty() {
                    format!(" matching '{}'", palette.filter_query())
                } else {
                    String::new()
                },
                accent_style(theme),
            ),
        ]));
    }

    lines
}

fn default_style(theme: &InlineTheme) -> Style {
    let mut style = Style::default();
    if let Some(foreground) = theme.foreground.map(ratatui_color_from_ansi) {
        style = style.fg(foreground);
    }
    style
}

fn accent_style(theme: &InlineTheme) -> Style {
    let mut style = Style::default();
    if let Some(primary) = theme
        .primary
        .or(theme.foreground)
        .map(ratatui_color_from_ansi)
    {
        style = style.fg(primary);
    }
    style
}

fn border_style(theme: &InlineTheme) -> Style {
    let mut style = Style::default();
    if let Some(secondary) = theme
        .secondary
        .or(theme.foreground)
        .map(ratatui_color_from_ansi)
    {
        style = style.fg(secondary);
    }
    style.add_modifier(Modifier::DIM)
}
