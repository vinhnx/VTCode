use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, List, ListDirection, ListItem, ListState, Paragraph, Wrap,
};

use super::SelectionEntry;

const CONTROLS_HINT: &str =
    "↑/↓ j/k to move  •  Home/End to jump  •  Enter/Tab confirm  •  Esc cancel";
const NUMBER_JUMP_HINT: &str = "Tip: Type number to jump";

mod styles {
    use ratatui::style::{Color, Modifier, Style};

    pub const ITEM_NUMBER: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    pub const DESCRIPTION: Style = Style::new().fg(Color::DarkGray);
    pub const DEFAULT_TEXT: Style = Style::new().fg(Color::White);
    pub const HIGHLIGHT: Style = Style::new()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD.union(Modifier::REVERSED));
}

pub(super) fn draw_selection_ui(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stderr>>,
    title: &str,
    instructions: &str,
    entries: &[SelectionEntry],
    selected_index: usize,
    list_state: &mut ListState,
) -> Result<()> {
    list_state.select(Some(selected_index));
    terminal
        .draw(|frame| {
            let area = frame.area();
            let instruction_lines = instructions.lines().count().max(1) as u16;
            let instruction_height = instruction_lines.saturating_add(2);
            let footer_height: u16 = 4;
            let layout = Layout::vertical([
                Constraint::Length(
                    instruction_height.min(area.height.saturating_sub(footer_height + 5)),
                ),
                Constraint::Min(5),
                Constraint::Length(footer_height),
            ])
            .spacing(-1)
            .margin(1)
            .vertical_margin(1)
            .split(area);

            if layout.len() < 3 {
                return;
            }

            let instructions_widget = Paragraph::new(instructions)
                .block(
                    Block::bordered()
                        .title("Instructions")
                        .border_type(BorderType::Rounded),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(instructions_widget, layout[0]);

            let items: Vec<ListItem> = entries
                .iter()
                .enumerate()
                .map(|(idx, entry)| {
                    let mut lines = vec![Line::from(vec![
                        Span::styled(format!("{:2}. ", idx + 1), styles::ITEM_NUMBER),
                        Span::raw(entry.title.as_str()),
                    ])];
                    if let Some(description) = entry.description.as_ref()
                        && !description.is_empty()
                        && description != &entry.title
                    {
                        lines.push(Line::from(Span::styled(
                            format!("    {}", description),
                            styles::DESCRIPTION,
                        )));
                    }
                    ListItem::new(lines)
                })
                .collect();

            let list = List::new(items)
                .block(
                    Block::bordered()
                        .title(title)
                        .border_type(BorderType::Rounded),
                )
                .style(styles::DEFAULT_TEXT)
                .highlight_style(styles::HIGHLIGHT)
                .highlight_symbol("> ")
                .repeat_highlight_symbol(true)
                .direction(ListDirection::TopToBottom)
                .scroll_padding(1);
            frame.render_stateful_widget(list, layout[1], list_state);

            let current = match entries.get(selected_index) {
                Some(entry) => entry,
                None => {
                    tracing::warn!("Selected index {selected_index} out of bounds");
                    return;
                }
            };

            let mut summary_lines = vec![Line::from(Span::styled(
                current.title.as_str(),
                Style::default().add_modifier(Modifier::BOLD),
            ))];

            if let Some(description) = current.description.as_ref()
                && !description.is_empty()
                && description != &current.title
            {
                summary_lines.push(Line::from(Span::styled(
                    format!("  {}", description),
                    styles::DESCRIPTION,
                )));
            }

            summary_lines.push(Line::from(""));
            summary_lines.push(Line::from(CONTROLS_HINT));
            summary_lines.push(Line::from(Span::styled(
                NUMBER_JUMP_HINT,
                styles::DESCRIPTION,
            )));

            let footer = Paragraph::new(summary_lines)
                .block(
                    Block::bordered()
                        .title("Selection")
                        .border_type(BorderType::Rounded),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(footer, layout[2]);
        })
        .with_context(|| format!("Failed to draw {title} selector UI"))?;

    Ok(())
}
