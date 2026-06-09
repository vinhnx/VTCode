use std::io::{self, Write};
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::{
    Terminal,
    backend::Backend,
    crossterm::{cursor::SetCursorStyle, execute},
};

/// Mouse pointer shape states, mirroring standard text editor cursors.
///
/// Emitted as OSC 22 escape sequences, supported by xterm, kitty, foot,
/// WezTerm, iTerm2, and other modern terminal emulators.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MousePointerShape {
    #[default]
    Default,
    /// Hand cursor when hovering clickable links/files/URLs.
    Pointer,
    /// I-beam cursor during text selection or when a selection is active.
    Text,
}

impl MousePointerShape {
    fn as_osc22_name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Pointer => "pointer",
            Self::Text => "text",
        }
    }
}

/// Set the mouse pointer shape via OSC 22.
pub(crate) fn set_mouse_pointer_shape(shape: MousePointerShape) {
    let name = shape.as_osc22_name();
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "\x1b]22;{name}\x07");
    let _ = stderr.flush();
}

/// Reset the mouse pointer shape to the terminal default.
pub(crate) fn reset_mouse_pointer_shape() {
    set_mouse_pointer_shape(MousePointerShape::Default);
}

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
    reset_mouse_pointer_shape();
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
