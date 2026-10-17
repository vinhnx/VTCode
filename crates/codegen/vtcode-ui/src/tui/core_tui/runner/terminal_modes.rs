use std::io;

use anyhow::Result;
use ratatui::crossterm::{
    cursor::{RestorePosition, SavePosition},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange,
        EnableMouseCapture,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode, is_raw_mode_enabled},
};

use crate::tui::options::FullscreenInteractionSettings;

/// Tracks terminal features changed while the TUI owns the terminal.
#[derive(Debug, Clone)]
pub(super) struct TerminalModeState {
    /// Whether VT Code enabled bracketed paste.
    bracketed_paste_enabled: bool,
    /// Whether VT Code enabled raw mode.
    raw_mode_enabled: bool,
    /// Whether VT Code enabled mouse capture.
    mouse_capture_enabled: bool,
    /// Whether VT Code enabled focus change events.
    focus_change_enabled: bool,
    /// Whether VT Code enabled Contour color-scheme change reports (unix only).
    color_scheme_reports_enabled: bool,
    /// Whether VT Code pushed keyboard enhancement flags.
    keyboard_enhancements_pushed: bool,
    /// Whether the cursor position was saved before entering fullscreen
    cursor_position_saved: bool,
    /// Whether the alternate screen buffer is active
    alternate_screen_active: bool,
}

impl TerminalModeState {
    /// Create a new TerminalModeState with all modes disabled (clean state)
    fn new() -> Self {
        Self {
            bracketed_paste_enabled: false,
            raw_mode_enabled: false,
            mouse_capture_enabled: false,
            focus_change_enabled: false,
            color_scheme_reports_enabled: false,
            keyboard_enhancements_pushed: false,
            cursor_position_saved: false,
            alternate_screen_active: false,
        }
    }

    pub(super) fn save_cursor_position(&mut self, stderr: &mut io::Stderr) {
        match execute!(stderr, SavePosition) {
            Ok(_) => {
                self.cursor_position_saved = true;
                crate::tui::ui::tui::panic_hook::mark_terminal_modified();
            }
            Err(error) => {
                tracing::debug!(%error, "failed to save cursor position for inline session");
            }
        }
    }

    pub(super) fn enter_alternate_screen(&mut self, stderr: &mut io::Stderr) -> Result<()> {
        execute!(stderr, EnterAlternateScreen)
            .map_err(|error| anyhow::anyhow!("failed to enter alternate inline screen: {error}"))?;
        self.alternate_screen_active = true;
        crate::tui::ui::tui::panic_hook::mark_terminal_modified();
        Ok(())
    }

    /// Enable unsolicited Contour color-scheme reports (`CSI ? 2031 h`).
    ///
    /// Unix only: the Windows backend consumes console input records instead
    /// of parsing VT byte streams, so DSR replies could surface as garbage
    /// input there. The canonical restore path disables the mode via the
    /// [`crate::tui::core_tui::panic_hook`] state flag.
    #[cfg(unix)]
    fn enable_color_scheme_reports(&mut self, stderr: &mut io::Stderr) {
        use std::io::Write as _;
        use vtcode_commons::ansi_codes::COLOR_SCHEME_REPORTS_ENABLE;

        match stderr
            .write_all(COLOR_SCHEME_REPORTS_ENABLE.as_bytes())
            .and_then(|()| stderr.flush())
        {
            Ok(()) => {
                self.color_scheme_reports_enabled = true;
                crate::tui::ui::tui::panic_hook::mark_color_scheme_reports_enabled(true);
                crate::tui::ui::tui::panic_hook::mark_terminal_modified();
            }
            Err(error) => {
                tracing::debug!(%error, "failed to enable color-scheme change reports");
            }
        }
    }

    pub(super) fn push_keyboard_enhancement_flags(
        &mut self,
        stderr: &mut io::Stderr,
        keyboard_flags: crossterm::event::KeyboardEnhancementFlags,
    ) {
        use ratatui::crossterm::event::PushKeyboardEnhancementFlags;

        if keyboard_flags.is_empty() {
            return;
        }

        match execute!(stderr, PushKeyboardEnhancementFlags(keyboard_flags)) {
            Ok(_) => {
                self.keyboard_enhancements_pushed = true;
                crate::tui::ui::tui::panic_hook::mark_keyboard_enhancements_pushed(true);
                crate::tui::ui::tui::panic_hook::mark_terminal_modified();
            }
            Err(error) => {
                tracing::debug!(%error, "failed to push keyboard enhancement flags");
            }
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
    fullscreen: &FullscreenInteractionSettings,
) -> Result<TerminalModeState> {
    let mut state = TerminalModeState::new();

    // Capture existing raw-mode state before changing any terminal mode so the
    // canonical restore path can return the terminal to its original state.
    match is_raw_mode_enabled() {
        Ok(prev_enabled) => {
            crate::tui::ui::tui::panic_hook::state::mark_raw_mode_was_enabled(prev_enabled);
            if !prev_enabled {
                match enable_raw_mode() {
                    Ok(_) => {
                        state.raw_mode_enabled = true;
                        crate::tui::ui::tui::panic_hook::mark_terminal_modified();
                    }
                    Err(error) => {
                        return Err(anyhow::anyhow!("failed to enable raw mode: {error}"));
                    }
                }
            }
        }
        Err(error) => {
            tracing::debug!(%error, "failed to query raw mode; attempting to enable raw mode");
            match enable_raw_mode() {
                Ok(_) => {
                    state.raw_mode_enabled = true;
                    crate::tui::ui::tui::panic_hook::mark_terminal_modified();
                }
                Err(error) => {
                    return Err(anyhow::anyhow!("failed to enable raw mode: {error}"));
                }
            }
        }
    }

    // Enable bracketed paste
    match execute!(stderr, EnableBracketedPaste) {
        Ok(_) => {
            state.bracketed_paste_enabled = true;
            crate::tui::ui::tui::panic_hook::mark_terminal_modified();
        }
        Err(error) => {
            tracing::warn!(%error, "failed to enable bracketed paste");
        }
    }

    if fullscreen.mouse_capture {
        match execute!(stderr, EnableMouseCapture) {
            Ok(_) => {
                state.mouse_capture_enabled = true;
                crate::tui::ui::tui::panic_hook::mark_terminal_modified();
            }
            Err(error) => {
                tracing::warn!(%error, "failed to enable mouse capture");
            }
        }
    }

    // Enable focus change events
    match execute!(stderr, EnableFocusChange) {
        Ok(_) => {
            state.focus_change_enabled = true;
            crate::tui::ui::tui::panic_hook::mark_terminal_modified();
        }
        Err(error) => {
            tracing::debug!(%error, "failed to enable focus change events");
        }
    }

    // Enable unsolicited Contour color-scheme change reports (dark/light).
    // Reports are parsed by the vendored crossterm fork and ignored unless
    // the session opted into automatic color-scheme following.
    #[cfg(unix)]
    state.enable_color_scheme_reports(stderr);

    Ok(state)
}

/// Restore terminal modes when the TUI task ends.
///
/// Delegates to the canonical restore but deliberately **keeps raw mode**: the
/// TUI task ends as soon as the session does, while the runloop still has
/// teardown to run (archive write, MCP shutdown, TUI join). A cooked tty during
/// that window echoes late input — most visibly the kitty-protocol key-release
/// report for the exiting Ctrl+C — onto the screen and into the shell's input.
/// The exit postamble owns the final transition to cooked mode
/// ([`finish_deferred_raw_mode_restore`]); every other restore path (panic hook,
/// emergency exit, error report, host backstop) goes through `restore_tui()`,
/// which forces the transition, so a non-graceful exit can never leave the tty
/// in raw mode.
pub(super) fn restore_terminal_modes(_state: &TerminalModeState) -> Result<()> {
    crate::tui::ui::tui::panic_hook::restore_tui_keep_raw_mode()?;
    Ok(())
}
