use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, List, ListDirection, ListItem, ListState, Paragraph, Wrap};

use super::{SelectionEntry, SelectionListState};
use crate::tui::ui::search::ListSearchFilter;

fn controls_hint() -> String {
    crate::design::keys::choice_hint()
}
const FILTER_HINT: &str = "Type to filter · number jumps when filter is empty";
const NUMBER_JUMP_HINT: &str = "Type a number to jump";
const NO_MATCHES: &str = "No matching options";

mod styles {
    use ratatui::style::{Color, Modifier, Style};

    pub const ITEM_NUMBER: Style = Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM);
    pub const DESCRIPTION: Style = Style::new().fg(Color::DarkGray);
    pub const DEFAULT_TEXT: Style = Style::new().fg(Color::Gray).add_modifier(Modifier::DIM);
    pub const HIGHLIGHT: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    pub const SEARCH_LABEL: Style = Style::new().fg(Color::Gray).add_modifier(Modifier::DIM);
    pub const SEARCH_QUERY: Style = Style::new().fg(Color::White);
}

/// Split the selector chrome into instruction / search / list / footer rows.
///
/// `try_layout::<N>` is const-generic on the constraint count: mismatched N
/// always errors and would leave the picker blank. Keep 3- and 4-row paths
/// separate (search-off vs search-on).
pub(super) fn selection_layout(area: Rect, search_enabled: bool, instruction_height: u16) -> (Rect, Rect, Rect, Rect) {
    let footer_height: u16 = 4;
    if search_enabled {
        let [instructions_area, search_area, list_area, footer_area] = area
            .try_layout(
                &Layout::vertical([
                    Constraint::Length(instruction_height.min(area.height.saturating_sub(footer_height + 6))),
                    Constraint::Length(1),
                    Constraint::Min(5),
                    Constraint::Length(footer_height),
                ])
                .spacing(-1)
                .margin(1)
                .vertical_margin(1),
            )
            .unwrap_or([Rect::ZERO; 4]);
        (instructions_area, search_area, list_area, footer_area)
    } else {
        let [instructions_area, list_area, footer_area] = area
            .try_layout(
                &Layout::vertical([
                    Constraint::Length(instruction_height.min(area.height.saturating_sub(footer_height + 5))),
                    Constraint::Min(5),
                    Constraint::Length(footer_height),
                ])
                .spacing(-1)
                .margin(1)
                .vertical_margin(1),
            )
            .unwrap_or([Rect::ZERO; 3]);
        (instructions_area, Rect::ZERO, list_area, footer_area)
    }
}

pub(super) fn draw_selection_ui(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stderr>>,
    title: &str,
    instructions: &str,
    entries: &[SelectionEntry],
    state: &SelectionListState,
    list_state: &mut ListState,
) -> Result<()> {
    let selected_index = state.selected_visible_index();
    list_state.select(selected_index);
    terminal
        .draw(|frame| {
            let area = frame.area();
            // Degenerate viewport (e.g. single-row terminal): the layout below
            // cannot be satisfied and every chunk would be zero-area, which
            // ratatui renders as a no-op. Return early so the empty selector
            // is explicit rather than a silent blank screen.
            if area.width == 0 || area.height == 0 {
                return;
            }
            let instruction_lines = instructions.lines().count().max(1) as u16;
            let instruction_height = instruction_lines.saturating_add(2);
            let (instructions_area, search_area, list_area, footer_area) =
                selection_layout(area, state.search_enabled(), instruction_height);

            let instructions_widget = Paragraph::new(instructions)
                .block(Block::bordered().title("Instructions").border_type(BorderType::Rounded))
                .wrap(Wrap { trim: true });
            frame.render_widget(instructions_widget, instructions_area);

            if state.search_enabled() {
                let query = state.query();
                let cursor = if query.is_empty() { "" } else { "▌" };
                let line = Line::from(vec![
                    Span::styled("Filter: ", styles::SEARCH_LABEL),
                    Span::styled(query.to_string(), styles::SEARCH_QUERY),
                    Span::styled(cursor.to_string(), styles::HIGHLIGHT),
                ]);
                frame.render_widget(Paragraph::new(line), search_area);
            }

            let visible = state.visible_indices();
            let mut items: Vec<ListItem> = Vec::with_capacity(visible.len().max(1));
            if visible.is_empty() {
                items.push(ListItem::new(Line::from(Span::styled(NO_MATCHES.to_string(), styles::DESCRIPTION))));
            } else {
                for &original_index in visible.iter() {
                    let Some(entry) = entries.get(original_index) else {
                        continue;
                    };
                    // Show the original 1-based number so digit jumps and
                    // long lists stay anchored to the unfiltered catalog.
                    let mut lines = vec![Line::from(vec![
                        Span::styled(format!("{:2}. ", original_index + 1), styles::ITEM_NUMBER),
                        Span::raw(entry.title.as_str()),
                    ])];
                    if let Some(description) = entry.description.as_ref()
                        && !description.is_empty()
                        && description != &entry.title
                    {
                        lines.push(Line::from(Span::styled(format!("    {description}"), styles::DESCRIPTION)));
                    }
                    // Shared rhythm: one blank separator row between entries so
                    // dense standalone pickers match the core modal lists.
                    lines.push(Line::default());
                    items.push(ListItem::new(lines));
                }
            }

            let list = List::new(items)
                .block(
                    Block::bordered()
                        .title(if state.is_filtering() {
                            format!("{title} (filtered)")
                        } else {
                            title.to_string()
                        })
                        .border_type(BorderType::Rounded),
                )
                .style(styles::DEFAULT_TEXT)
                .highlight_style(styles::HIGHLIGHT)
                .highlight_symbol("→ ")
                .direction(ListDirection::TopToBottom)
                .scroll_padding(1);
            frame.render_stateful_widget(list, list_area, list_state);

            let current = state.selected_entry(entries);
            let mut summary_lines = Vec::new();
            match current {
                Some(entry) => {
                    summary_lines.push(Line::from(Span::styled(
                        entry.title.as_str(),
                        Style::default().add_modifier(Modifier::BOLD),
                    )));
                    if let Some(description) = entry.description.as_ref()
                        && !description.is_empty()
                        && description != &entry.title
                    {
                        summary_lines.push(Line::from(Span::styled(format!("  {description}"), styles::DESCRIPTION)));
                    }
                }
                None => {
                    summary_lines.push(Line::from(Span::styled(NO_MATCHES, styles::DESCRIPTION)));
                }
            }

            summary_lines.push(Line::from(""));
            summary_lines.push(Line::from(controls_hint()));
            summary_lines.push(Line::from(Span::styled(
                if state.search_enabled() {
                    FILTER_HINT
                } else {
                    NUMBER_JUMP_HINT
                },
                styles::DESCRIPTION,
            )));

            let footer = Paragraph::new(summary_lines)
                .block(Block::bordered().title("Selection").border_type(BorderType::Rounded))
                .wrap(Wrap { trim: true });
            frame.render_widget(footer, footer_area);
        })
        .with_context(|| format!("Failed to draw {title} selector UI"))?;

    Ok(())
}

/// Shared haystack for one entry: title + description + keywords.
pub(super) fn entry_haystack(entry: &SelectionEntry) -> String {
    use crate::tui::ui::search::SearchCandidate;
    let keywords = entry.keywords.join(" ");
    SearchCandidate::new(entry.title.as_str())
        .with_description(entry.description.as_deref())
        .with_keywords(keywords.as_str())
        .haystack()
}

/// Filter entries with the shared list search filter.
pub(super) fn filter_entries(entries: &[SelectionEntry], query: &str, fuzzy: bool) -> Vec<usize> {
    let mut filter = ListSearchFilter::new(query, fuzzy);
    let haystacks: Vec<String> = entries.iter().map(entry_haystack).collect();
    filter.filter_haystacks(&haystacks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_layout_three_row_path_is_not_blank() {
        let area = Rect::new(0, 0, 80, 24);
        let (instructions, search, list, footer) = selection_layout(area, false, 3);
        assert_eq!(search, Rect::ZERO, "no search row when disabled");
        assert!(instructions.height > 0, "instructions visible: {instructions:?}");
        assert!(list.height > 0, "list visible: {list:?}");
        assert!(footer.height > 0, "footer visible: {footer:?}");
    }

    #[test]
    fn selection_layout_four_row_path_reserves_search_row() {
        let area = Rect::new(0, 0, 80, 24);
        let (instructions, search, list, footer) = selection_layout(area, true, 3);
        assert!(search.height > 0, "search row visible: {search:?}");
        assert!(instructions.height > 0 && list.height > 0 && footer.height > 0);
    }
}
