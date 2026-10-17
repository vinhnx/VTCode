use anyhow::Result;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use super::{SelectionEntry, SelectionInterrupted, SelectionListState};

pub(super) enum SelectionAction {
    Continue,
    Select,
    Cancel,
}

fn is_vim_nav(ch: char) -> bool {
    matches!(ch, 'j' | 'k')
}

pub(super) fn handle_event(
    event: Event,
    entries: &[SelectionEntry],
    state: &mut SelectionListState,
    number_buffer: &mut String,
) -> Result<SelectionAction> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            // Filter field first. Digits are reserved for jump-while-empty;
            // j/k stay vim navigation while the query is empty so the
            // documented control is not stolen by typing. Once the user is
            // filtering, every printable (including j/k/digits) edits the query.
            if state.search_enabled() {
                match key.code {
                    KeyCode::Char(ch)
                        if !key.modifiers.contains(KeyModifiers::CONTROL)
                            && !key.modifiers.contains(KeyModifiers::ALT)
                            && (state.is_filtering() || !(ch.is_ascii_digit() || is_vim_nav(ch))) =>
                    {
                        state.push_char(ch, entries);
                        number_buffer.clear();
                        return Ok(SelectionAction::Continue);
                    }
                    KeyCode::Backspace if state.is_filtering() => {
                        state.backspace(entries);
                        number_buffer.clear();
                        return Ok(SelectionAction::Continue);
                    }
                    KeyCode::Esc if state.is_filtering() => {
                        state.clear_query(entries);
                        number_buffer.clear();
                        return Ok(SelectionAction::Continue);
                    }
                    _ => {}
                }
            }

            let total = state.visible_indices().len();
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    state.move_selection(-1);
                    number_buffer.clear();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.move_selection(1);
                    number_buffer.clear();
                }
                KeyCode::Home => {
                    state.set_selection_pos(0);
                    number_buffer.clear();
                }
                KeyCode::End => {
                    state.set_selection_pos(total.saturating_sub(1));
                    number_buffer.clear();
                }
                KeyCode::PageUp => {
                    state.page_selection(-5);
                    number_buffer.clear();
                }
                KeyCode::PageDown => {
                    state.page_selection(5);
                    number_buffer.clear();
                }
                KeyCode::Enter | KeyCode::Tab => {
                    if state.selected_entry(entries).is_some() {
                        return Ok(SelectionAction::Select);
                    }
                    // Empty filter result: Enter is a no-op.
                }
                KeyCode::Esc => return Ok(SelectionAction::Cancel),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Err(SelectionInterrupted.into());
                }
                KeyCode::Char(c) if c.is_ascii_digit() && !state.is_filtering() => {
                    number_buffer.push(c);
                    if let Ok(index) = number_buffer.parse::<usize>()
                        && (1..=entries.len()).contains(&index)
                    {
                        state.jump_to_original(index - 1);
                    }
                    if number_buffer.len() >= entries.len().to_string().len() {
                        number_buffer.clear();
                    }
                }
                KeyCode::Backspace => {
                    number_buffer.pop();
                }
                _ => {}
            }
        }
        Event::Resize(_, _) => {
            number_buffer.clear();
        }
        _ => {}
    }

    Ok(SelectionAction::Continue)
}
