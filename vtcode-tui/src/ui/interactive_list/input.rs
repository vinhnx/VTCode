use anyhow::Result;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use super::SelectionInterrupted;

pub(super) enum SelectionAction {
    Continue,
    Select,
    Cancel,
}

pub(super) fn handle_event(
    event: Event,
    total: usize,
    selected_index: &mut usize,
    number_buffer: &mut String,
) -> Result<SelectionAction> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected_index == 0 {
                    *selected_index = total - 1;
                } else {
                    *selected_index -= 1;
                }
                number_buffer.clear();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                *selected_index = (*selected_index + 1) % total;
                number_buffer.clear();
            }
            KeyCode::Home => {
                *selected_index = 0;
                number_buffer.clear();
            }
            KeyCode::End => {
                *selected_index = total - 1;
                number_buffer.clear();
            }
            KeyCode::PageUp => {
                let step = 5.min(total - 1);
                if *selected_index < step {
                    *selected_index = 0;
                } else {
                    *selected_index -= step;
                }
                number_buffer.clear();
            }
            KeyCode::PageDown => {
                let step = 5.min(total - 1);
                *selected_index = (*selected_index + step).min(total - 1);
                number_buffer.clear();
            }
            KeyCode::Enter | KeyCode::Tab => return Ok(SelectionAction::Select),
            KeyCode::Esc => return Ok(SelectionAction::Cancel),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Err(SelectionInterrupted.into());
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                number_buffer.push(c);
                if let Ok(index) = number_buffer.parse::<usize>()
                    && (1..=total).contains(&index)
                {
                    *selected_index = index - 1;
                }
                if number_buffer.len() >= total.to_string().len() {
                    number_buffer.clear();
                }
            }
            KeyCode::Backspace => {
                number_buffer.pop();
            }
            _ => {}
        },
        Event::Resize(_, _) => {
            number_buffer.clear();
        }
        _ => {}
    }

    Ok(SelectionAction::Continue)
}
