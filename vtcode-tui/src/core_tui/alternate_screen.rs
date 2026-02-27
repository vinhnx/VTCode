use std::io::{self, Write};

use crate::utils::tty::TtyExt;
use anyhow::{Context, Result};
use ratatui::crossterm::{
    event::{DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange},
    execute,
    terminal::{
        self, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    },
};

/// Terminal state that needs to be preserved when entering alternate screen
#[derive(Debug)]
struct TerminalState {
    raw_mode_enabled: bool,
    bracketed_paste_enabled: bool,
    focus_change_enabled: bool,
}

/// Manages entering and exiting alternate screen with proper state preservation
///
/// This struct ensures that terminal state is properly saved before entering
/// alternate screen and restored when exiting, even in the presence of errors.
///
/// # Example
///
/// ```no_run
/// use vtcode_core::ui::tui::alternate_screen::AlternateScreenSession;
///
/// // Run a closure in alternate screen with automatic cleanup
/// let result = AlternateScreenSession::run(|| {
///     // Your code that runs in alternate screen
///     println!("Running in alternate screen!");
///     Ok(())
/// })?;
/// ```
pub struct AlternateScreenSession {
    /// Terminal state before entering alternate screen
    original_state: TerminalState,
    /// Whether we successfully entered alternate screen
    entered: bool,
}

impl AlternateScreenSession {
    /// Enter alternate screen, saving current terminal state
    ///
    /// This will:
    /// 1. Save the current terminal state
    /// 2. Enter alternate screen
    /// 3. Enable raw mode
    /// 4. Enable bracketed paste
    /// 5. Enable focus change events (if supported)
    /// 6. Push keyboard enhancement flags (if supported)
    ///
    /// # Errors
    ///
    /// Returns an error if any terminal operation fails.
    pub fn enter() -> Result<Self> {
        let mut stdout = io::stdout();

        // Check if stdout is a TTY before proceeding
        let is_tty = stdout.is_tty_ext();
        if !is_tty {
            tracing::warn!("stdout is not a TTY, alternate screen features may not work");
        }

        // Save current state
        let original_state = TerminalState {
            raw_mode_enabled: false, // We'll enable it fresh
            bracketed_paste_enabled: false,
            focus_change_enabled: false,
        };

        // Enter alternate screen first
        execute!(stdout, EnterAlternateScreen)
            .context("failed to enter alternate screen for terminal app")?;

        let mut session = Self {
            original_state,
            entered: true,
        };

        // Enable raw mode
        enable_raw_mode().context("failed to enable raw mode for terminal app")?;
        session.original_state.raw_mode_enabled = true;

        // Enable bracketed paste (only if TTY)
        if is_tty && execute!(stdout, EnableBracketedPaste).is_ok() {
            session.original_state.bracketed_paste_enabled = true;
        }

        // Enable focus change events (only if TTY)
        if is_tty && execute!(stdout, EnableFocusChange).is_ok() {
            session.original_state.focus_change_enabled = true;
        }

        Ok(session)
    }

    /// Exit alternate screen, restoring original terminal state
    ///
    /// This will:
    /// 1. Pop keyboard enhancement flags (if they were pushed)
    /// 2. Disable focus change events (if they were enabled)
    /// 3. Disable bracketed paste (if it was enabled)
    /// 4. Disable raw mode (if it was enabled)
    /// 5. Leave alternate screen
    ///
    /// # Errors
    ///
    /// Returns an error if any terminal operation fails. However, this method
    /// will attempt to restore as much state as possible even if some operations fail.
    pub fn exit(mut self) -> Result<()> {
        self.restore_state()?;
        self.entered = false; // Prevent Drop from trying again
        Ok(())
    }

    /// Run a closure in alternate screen with automatic cleanup
    ///
    /// This is a convenience method that handles entering and exiting alternate
    /// screen automatically, ensuring cleanup happens even if the closure panics.
    ///
    /// # Errors
    ///
    /// Returns an error if entering/exiting alternate screen fails, or if the
    /// closure returns an error.
    pub fn run<F, T>(f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        let session = Self::enter()?;
        let result = f();
        session.exit()?;
        result
    }

    /// Internal method to restore terminal state
    fn restore_state(&mut self) -> Result<()> {
        if !self.entered {
            return Ok(());
        }

        // Drain any pending crossterm events BEFORE leaving alternate screen and disabling raw mode
        // to prevent them from leaking to the shell.
        while let Ok(true) = ratatui::crossterm::event::poll(std::time::Duration::from_millis(0)) {
            let _ = ratatui::crossterm::event::read();
        }

        let mut stdout = io::stdout();

        // Clear current line to remove artifacts like ^C from rapid presses
        let _ = stdout.write_all(b"\r\x1b[K");

        let mut errors = Vec::new();

        // Restore in proper order to prevent leakage

        // 1. Leave alternate screen FIRST
        if let Err(e) = execute!(stdout, LeaveAlternateScreen) {
            tracing::warn!(%e, "failed to leave alternate screen");
            errors.push(format!("leave alternate screen: {}", e));
        }

        // 2. Disable focus change (if enabled and TTY)
        if self.original_state.focus_change_enabled
            && let Err(e) = execute!(stdout, DisableFocusChange)
        {
            tracing::warn!(%e, "failed to disable focus change");
            errors.push(format!("disable focus change: {}", e));
        }

        // 3. Disable bracketed paste (if enabled and TTY)
        if self.original_state.bracketed_paste_enabled
            && let Err(e) = execute!(stdout, DisableBracketedPaste)
        {
            tracing::warn!(%e, "failed to disable bracketed paste");
            errors.push(format!("disable bracketed paste: {}", e));
        }

        // 4. Disable raw mode LAST
        if self.original_state.raw_mode_enabled
            && let Err(e) = disable_raw_mode()
        {
            tracing::warn!(%e, "failed to disable raw mode");
            errors.push(format!("disable raw mode: {}", e));
        }

        // Flush to ensure all changes are applied
        if let Err(e) = stdout.flush() {
            tracing::warn!(%e, "failed to flush stdout");
            errors.push(format!("flush stdout: {}", e));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            tracing::warn!(
                errors = ?errors,
                "some terminal operations failed during restore"
            );
            // Don't fail the operation, just warn - terminal is likely already in a bad state
            Ok(())
        }
    }
}

impl Drop for AlternateScreenSession {
    fn drop(&mut self) {
        if self.entered {
            // Best effort cleanup - ignore errors in Drop
            let _ = self.restore_state();
        }
    }
}

/// Clear the alternate screen
///
/// This is useful when you want to clear the screen before running a terminal app.
pub fn clear_screen() -> Result<()> {
    execute!(io::stdout(), terminal::Clear(terminal::ClearType::All))
        .context("failed to clear alternate screen")
}

/// Get current terminal size
pub fn terminal_size() -> Result<(u16, u16)> {
    terminal::size().context("failed to get terminal size")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enter_exit_cycle() {
        // This test verifies that we can enter and exit alternate screen
        // without panicking. We can't easily verify the actual terminal state
        // in a unit test, but we can at least ensure the code doesn't crash.
        let session = AlternateScreenSession::enter();
        assert!(session.is_ok());

        if let Ok(session) = session {
            let result = session.exit();
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_run_with_closure() {
        let result = AlternateScreenSession::run(|| {
            // Simulate some work in alternate screen
            Ok(42)
        });

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_run_with_error() {
        let result: Result<()> = AlternateScreenSession::run(|| Err(anyhow::anyhow!("test error")));

        assert!(result.is_err());
    }

    #[test]
    fn test_drop_cleanup() {
        // Verify that Drop properly cleans up
        {
            let _session = AlternateScreenSession::enter();
            // Session dropped here
        }
        // If we get here without hanging, Drop worked
    }
}
