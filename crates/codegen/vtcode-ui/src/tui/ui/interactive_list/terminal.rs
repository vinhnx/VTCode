use std::io::{self, Write};

use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::{MoveToColumn, RestorePosition, SavePosition, SetCursorStyle, Show};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

pub(super) struct TerminalModeGuard {
    label: String,
    raw_mode_enabled: bool,
    alternate_screen: bool,
    cursor_hidden: bool,
    cursor_position_saved: bool,
}

impl TerminalModeGuard {
    pub(super) fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            raw_mode_enabled: false,
            alternate_screen: false,
            cursor_hidden: false,
            cursor_position_saved: false,
        }
    }

    pub(super) fn save_cursor_position(&mut self, stderr: &mut io::Stderr) {
        match execute!(stderr, SavePosition) {
            Ok(_) => {
                self.cursor_position_saved = true;
                crate::tui::ui::tui::panic_hook::mark_terminal_modified();
            }
            Err(error) => {
                tracing::debug!(%error, selector = %self.label, "failed to save cursor position");
            }
        }
    }

    pub(super) fn enable_raw_mode(&mut self) -> Result<()> {
        enable_raw_mode().with_context(|| format!("Failed to enable raw mode for {} selector", self.label))?;
        self.raw_mode_enabled = true;
        crate::tui::ui::tui::panic_hook::mark_terminal_modified();
        Ok(())
    }

    pub(super) fn enter_alternate_screen(&mut self, stderr: &mut io::Stderr) -> Result<()> {
        execute!(stderr, EnterAlternateScreen)
            .with_context(|| format!("Failed to enter alternate screen for {} selector", self.label))?;
        self.alternate_screen = true;
        crate::tui::ui::tui::panic_hook::mark_terminal_modified();
        Ok(())
    }

    pub(super) fn hide_cursor(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stderr>>) -> Result<()> {
        terminal
            .hide_cursor()
            .with_context(|| format!("Failed to hide cursor for {} selector", self.label))?;
        self.cursor_hidden = true;
        crate::tui::ui::tui::panic_hook::mark_terminal_modified();
        Ok(())
    }

    pub(super) fn restore_with_terminal(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stderr>>,
    ) -> Result<()> {
        crate::tui::core_tui::runner::terminal_io::drain_terminal_events();

        let _ = execute!(io::stderr(), MoveToColumn(0), Clear(ClearType::CurrentLine));

        if self.alternate_screen {
            // Purge the alternate viewport before leaving so the picker frame
            // cannot leak into the main scrollback (mirrors the canonical
            // `panic_hook::restore_tui` ordering).
            let _ = execute!(terminal.backend_mut(), Clear(ClearType::All));
            execute!(terminal.backend_mut(), LeaveAlternateScreen)
                .with_context(|| format!("Failed to leave alternate screen after {} selector", self.label))?;
            self.alternate_screen = false;
        }

        if self.raw_mode_enabled {
            disable_raw_mode().with_context(|| format!("Failed to disable raw mode after {} selector", self.label))?;
            self.raw_mode_enabled = false;
        }

        if self.cursor_hidden {
            terminal
                .show_cursor()
                .with_context(|| format!("Failed to show cursor after {} selector", self.label))?;
            self.cursor_hidden = false;
        }

        let _ = execute!(terminal.backend_mut(), SetCursorStyle::DefaultUserShape);
        if self.cursor_position_saved {
            let _ = execute!(terminal.backend_mut(), RestorePosition);
            self.cursor_position_saved = false;
        }

        terminal.backend_mut().flush().ok();
        io::stderr().flush().ok();
        self.mark_restored_if_standalone();
        Ok(())
    }

    /// The selector restored the terminal itself. If it ran outside a TUI
    /// session, nothing else will restore the terminal, so clear the global
    /// modified flag to keep later error reports clean.
    fn mark_restored_if_standalone(&self) {
        if !crate::tui::ui::tui::panic_hook::is_tui_initialized() {
            crate::tui::ui::tui::panic_hook::mark_terminal_restored();
        }
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        crate::tui::core_tui::runner::terminal_io::drain_terminal_events();

        let _ = execute!(io::stderr(), MoveToColumn(0), Clear(ClearType::CurrentLine));

        if self.alternate_screen {
            let mut stderr = io::stderr();
            // Best-effort purge before leaving (mirrors the canonical
            // `panic_hook::restore_tui` ordering).
            let _ = execute!(stderr, Clear(ClearType::All));
            let _ = execute!(stderr, LeaveAlternateScreen);
            self.alternate_screen = false;
        }

        if self.raw_mode_enabled {
            let _ = disable_raw_mode();
            self.raw_mode_enabled = false;
        }

        if self.cursor_hidden {
            let mut stderr = io::stderr();
            let _ = execute!(stderr, SetCursorStyle::DefaultUserShape, Show);
            let _ = stderr.flush();
            self.cursor_hidden = false;
        }

        if self.cursor_position_saved {
            let mut stderr = io::stderr();
            let _ = execute!(stderr, RestorePosition);
            let _ = stderr.flush();
            self.cursor_position_saved = false;
        }

        let _ = io::stderr().flush();
        self.mark_restored_if_standalone();
    }
}
