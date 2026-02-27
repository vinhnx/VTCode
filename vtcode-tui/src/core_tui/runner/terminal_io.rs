use std::io;
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::{
    Terminal,
    backend::Backend,
    crossterm::{cursor::SetCursorStyle, execute},
};

pub(super) fn prepare_terminal<B: Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    terminal
        .hide_cursor()
        .map_err(|e| anyhow::anyhow!("failed to hide inline cursor: {}", e))?;
    terminal
        .clear()
        .map_err(|e| anyhow::anyhow!("failed to clear inline terminal: {}", e))?;
    Ok(())
}

pub(super) fn finalize_terminal<B: Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    execute!(io::stderr(), SetCursorStyle::DefaultUserShape)
        .context("failed to restore cursor style after inline session")?;
    terminal
        .show_cursor()
        .map_err(|e| anyhow::anyhow!("failed to show cursor after inline session: {}", e))?;
    terminal
        .clear()
        .map_err(|e| anyhow::anyhow!("failed to clear inline terminal after session: {}", e))?;
    terminal
        .flush()
        .map_err(|e| anyhow::anyhow!("failed to flush inline terminal after session: {}", e))?;
    Ok(())
}

/// Drain any pending crossterm events (e.g., resize, focus responses, or buffered keystrokes)
/// so they don't leak to the shell or interfere with next startup.
pub(super) fn drain_terminal_events() {
    use ratatui::crossterm::event;

    while event::poll(Duration::from_millis(0)).unwrap_or(false) {
        let _ = event::read();
    }
}
