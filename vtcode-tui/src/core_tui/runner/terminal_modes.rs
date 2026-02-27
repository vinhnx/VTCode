use std::io;

use anyhow::Result;
use ratatui::crossterm::{
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};

/// Represents the state of terminal modes before TUI initialization.
///
/// This struct tracks which terminal features were enabled before we
/// modified them, allowing proper restoration on exit.
#[derive(Debug, Clone)]
pub(super) struct TerminalModeState {
    /// Whether bracketed paste was enabled (we enable it)
    bracketed_paste_enabled: bool,
    /// Whether raw mode was enabled (we enable it)
    raw_mode_enabled: bool,
    /// Whether mouse capture was enabled (we enable it)
    mouse_capture_enabled: bool,
    /// Whether focus change events were enabled (we enable them)
    focus_change_enabled: bool,
    /// Whether keyboard enhancement flags were pushed (we push them)
    keyboard_enhancements_pushed: bool,
}

impl TerminalModeState {
    /// Create a new TerminalModeState with all modes disabled (clean state)
    fn new() -> Self {
        Self {
            bracketed_paste_enabled: false,
            raw_mode_enabled: false,
            mouse_capture_enabled: false,
            focus_change_enabled: false,
            keyboard_enhancements_pushed: false,
        }
    }
}

impl Default for TerminalModeState {
    fn default() -> Self {
        Self::new()
    }
}

pub(super) fn enable_terminal_modes(
    stderr: &mut io::Stderr,
    keyboard_flags: ratatui::crossterm::event::KeyboardEnhancementFlags,
) -> Result<TerminalModeState> {
    use ratatui::crossterm::event::PushKeyboardEnhancementFlags;

    let mut state = TerminalModeState::new();

    // Enable bracketed paste
    match execute!(stderr, EnableBracketedPaste) {
        Ok(_) => state.bracketed_paste_enabled = true,
        Err(error) => {
            tracing::warn!(%error, "failed to enable bracketed paste");
        }
    }

    // Enable raw mode
    match enable_raw_mode() {
        Ok(_) => state.raw_mode_enabled = true,
        Err(error) => {
            return Err(anyhow::anyhow!("failed to enable raw mode: {}", error));
        }
    }

    // Enable mouse capture
    match execute!(stderr, EnableMouseCapture) {
        Ok(_) => state.mouse_capture_enabled = true,
        Err(error) => {
            tracing::warn!(%error, "failed to enable mouse capture");
        }
    }

    // Enable focus change events
    match execute!(stderr, EnableFocusChange) {
        Ok(_) => state.focus_change_enabled = true,
        Err(error) => {
            tracing::debug!(%error, "failed to enable focus change events");
        }
    }

    // Push keyboard enhancement flags
    if !keyboard_flags.is_empty() {
        match execute!(stderr, PushKeyboardEnhancementFlags(keyboard_flags)) {
            Ok(_) => state.keyboard_enhancements_pushed = true,
            Err(error) => {
                tracing::debug!(%error, "failed to push keyboard enhancement flags");
            }
        }
    }

    Ok(state)
}

pub(super) fn restore_terminal_modes(state: &TerminalModeState) -> Result<()> {
    use ratatui::crossterm::event::PopKeyboardEnhancementFlags;
    let mut stderr = io::stderr();

    let mut errors = Vec::new();

    // Restore in reverse order of enabling

    // 1. Pop keyboard enhancement flags (if they were pushed)
    if state.keyboard_enhancements_pushed
        && let Err(error) = execute!(stderr, PopKeyboardEnhancementFlags)
    {
        tracing::debug!(%error, "failed to pop keyboard enhancement flags");
        errors.push(format!("keyboard enhancements: {}", error));
    }

    // 2. Disable focus change events (if they were enabled)
    if state.focus_change_enabled
        && let Err(error) = execute!(stderr, DisableFocusChange)
    {
        tracing::debug!(%error, "failed to disable focus change events");
        errors.push(format!("focus change: {}", error));
    }

    // 3. Disable mouse capture (if it was enabled)
    if state.mouse_capture_enabled
        && let Err(error) = execute!(stderr, DisableMouseCapture)
    {
        tracing::debug!(%error, "failed to disable mouse capture");
        errors.push(format!("mouse capture: {}", error));
    }

    // 4. Disable bracketed paste (if it was enabled)
    if state.bracketed_paste_enabled
        && let Err(error) = execute!(stderr, DisableBracketedPaste)
    {
        tracing::debug!(%error, "failed to disable bracketed paste");
        errors.push(format!("bracketed paste: {}", error));
    }

    // 5. Disable raw mode LAST (if it was enabled)
    if state.raw_mode_enabled
        && let Err(error) = disable_raw_mode()
    {
        tracing::debug!(%error, "failed to disable raw mode");
        errors.push(format!("raw mode: {}", error));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        tracing::warn!(
            errors = ?errors,
            "some terminal modes failed to restore"
        );
        Ok(()) // Don't fail the operation, just warn
    }
}
