use std::io;

use crate::utils::tty::TtyExt;
use anyhow::{Context, Result, anyhow};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event;
use ratatui::widgets::ListState;

mod input;
mod render;
mod terminal;

#[derive(Debug, Clone)]
pub struct SelectionEntry {
    pub title: String,
    pub description: Option<String>,
}

impl SelectionEntry {
    pub fn new(title: impl Into<String>, description: Option<String>) -> Self {
        Self {
            title: title.into(),
            description,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("selection interrupted by Ctrl+C")]
pub struct SelectionInterrupted;

pub fn run_interactive_selection(
    title: &str,
    instructions: &str,
    entries: &[SelectionEntry],
    default_index: usize,
) -> Result<Option<usize>> {
    if entries.is_empty() {
        return Err(anyhow!("No options available for selection"));
    }

    if !io::stderr().is_tty_ext() {
        return Err(anyhow!("Terminal UI is unavailable"));
    }

    let mut stderr = io::stderr();
    let mut terminal_guard = TerminalModeGuard::new(title);
    terminal_guard.save_cursor_position(&mut stderr);
    terminal_guard.enable_raw_mode()?;
    terminal_guard.enter_alternate_screen(&mut stderr)?;

    let backend = CrosstermBackend::new(stderr);
    let mut terminal = Terminal::new(backend)
        .with_context(|| format!("Failed to initialize Ratatui terminal for {title} selector"))?;
    terminal_guard.hide_cursor(&mut terminal)?;

    let selection_result = (|| -> Result<Option<usize>> {
        let total = entries.len();
        let mut selected_index = default_index.min(total.saturating_sub(1));
        let mut number_buffer = String::new();
        let mut list_state = ListState::default();

        loop {
            render::draw_selection_ui(
                &mut terminal,
                title,
                instructions,
                entries,
                selected_index,
                &mut list_state,
            )?;

            let event = event::read()
                .with_context(|| format!("Failed to read terminal input for {title} selector"))?;
            match input::handle_event(event, total, &mut selected_index, &mut number_buffer)? {
                input::SelectionAction::Continue => {}
                input::SelectionAction::Select => return Ok(Some(selected_index)),
                input::SelectionAction::Cancel => return Ok(None),
            }
        }
    })();

    let cleanup_result = terminal_guard.restore_with_terminal(&mut terminal);
    cleanup_result?;
    selection_result
}

use terminal::TerminalModeGuard;
