//! Panic hook implementation for VT Code TUI applications
//! This module provides a panic hook that restores terminal state when a panic occurs,
//! preventing terminal corruption, and provides enhanced panic formatting for different build types.

use std::io::{self, Write};
use std::panic;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

use better_panic::{Settings as BetterPanicSettings, Verbosity as BetterPanicVerbosity};
use ratatui::crossterm::{
    cursor::Show,
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, PopKeyboardEnhancementFlags,
    },
    execute,
    terminal::{LeaveAlternateScreen, disable_raw_mode},
};

static TUI_INITIALIZED: AtomicBool = AtomicBool::new(false);
static DEBUG_MODE: AtomicBool = AtomicBool::new(cfg!(debug_assertions));
static PANIC_HOOK_ONCE: Once = Once::new();

/// Set whether the application is in debug mode
///
/// When debug mode is enabled, panics will show a detailed backtrace.
pub fn set_debug_mode(enabled: bool) {
    DEBUG_MODE.store(enabled, Ordering::SeqCst);
}

/// Get whether the application is in debug mode
pub fn is_debug_mode() -> bool {
    DEBUG_MODE.load(Ordering::SeqCst)
}

/// Initialize the panic hook to restore terminal state on panic and provide better formatting
///
/// This function should be called very early in the application lifecycle,
/// before any TUI operations begin.
///
/// Follows Ratatui recipe: https://ratatui.rs/recipes/apps/panic-hooks/
pub fn init_panic_hook() {
    PANIC_HOOK_ONCE.call_once(|| {
        // Keep original hook for concise non-debug panic output.
        let original_hook = panic::take_hook();

        // Better panic formatting for debug-mode crashes.
        let better_panic_hook = BetterPanicSettings::new()
            .verbosity(BetterPanicVerbosity::Full)
            .most_recent_first(false)
            .lineno_suffix(true)
            .create_panic_handler();

        panic::set_hook(Box::new(move |panic_info| {
            let is_tui = TUI_INITIALIZED.load(Ordering::SeqCst);
            let is_debug = DEBUG_MODE.load(Ordering::SeqCst);

            // Ratatui recipe: always restore terminal before panic reporting.
            if is_tui {
                // Intentionally ignore restoration failures during panic unwind.
                let _ = restore_tui();
            }

            if is_debug {
                better_panic_hook(panic_info);
            } else {
                eprintln!("\nVTCode encountered a critical error and needs to shut down.");
                eprintln!("If this keeps happening, please report it with a backtrace.");
                eprintln!("Hint: run with --debug and set RUST_BACKTRACE=1.\n");
                original_hook(panic_info);
            }

            // Keep current behavior: terminate process after unrecoverable panic.
            std::process::exit(1);
        }));
    });
}

/// Mark that TUI has been initialized so panic hook knows to restore terminal
pub fn mark_tui_initialized() {
    TUI_INITIALIZED.store(true, Ordering::SeqCst);
}

/// Mark that TUI has been deinitialized to prevent further restoration attempts
pub fn mark_tui_deinitialized() {
    TUI_INITIALIZED.store(false, Ordering::SeqCst);
}

/// Restore terminal to a usable state after a panic
///
/// This function attempts to restore the terminal to its original state
/// by disabling raw mode and leaving alternate screen if they were active.
/// It handles all errors internally to ensure cleanup happens even if individual
/// operations fail.
///
/// Follows Ratatui recipe: https://ratatui.rs/recipes/apps/panic-hooks/
pub fn restore_tui() -> io::Result<()> {
    // 1. Drain any pending crossterm events to prevent them from leaking to the shell
    // This is a best-effort drain with a zero timeout
    while let Ok(true) = ratatui::crossterm::event::poll(std::time::Duration::from_millis(0)) {
        let _ = ratatui::crossterm::event::read();
    }

    // Get stderr for executing terminal commands
    let mut stderr = io::stderr();

    // 2. Clear current line to remove any echoed ^C characters from rapid Ctrl+C presses
    // \r returns to start of line, \x1b[K clears to end of line
    let _ = stderr.write_all(b"\r\x1b[K");

    // 3. Leave alternate screen FIRST (if we were in one)
    // This is the most critical operation for visual restoration
    let _ = execute!(stderr, LeaveAlternateScreen);

    // 4. Disable various terminal modes that might have been enabled by the TUI
    let _ = execute!(stderr, DisableBracketedPaste);
    let _ = execute!(stderr, DisableFocusChange);
    let _ = execute!(stderr, DisableMouseCapture);
    let _ = execute!(stderr, PopKeyboardEnhancementFlags);

    // Ensure cursor is visible
    let _ = execute!(stderr, Show);

    // 5. Disable raw mode LAST to ensure all cleanup commands are sent properly
    let _ = disable_raw_mode();

    // Additional flush to ensure all escape sequences are processed
    let _ = stderr.flush();

    Ok(())
}

/// A guard struct that automatically registers and unregisters TUI state
/// with the panic hook system.
///
/// This ensures that terminal restoration only happens when the TUI was actually active.
pub struct TuiPanicGuard;

impl TuiPanicGuard {
    /// Create a new guard and mark TUI as initialized
    ///
    /// This should be called when a TUI session begins.
    pub fn new() -> Self {
        mark_tui_initialized();
        Self
    }
}

impl Default for TuiPanicGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TuiPanicGuard {
    fn drop(&mut self) {
        mark_tui_deinitialized();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn test_panic_guard_initialization() {
        // Reset state for test
        TUI_INITIALIZED.store(false, Ordering::SeqCst);

        {
            let _guard = TuiPanicGuard::new();
            assert_eq!(
                TUI_INITIALIZED.load(Ordering::SeqCst),
                true,
                "TUI should be marked as initialized"
            );

            // Drop happens automatically when leaving scope
        }

        assert_eq!(
            TUI_INITIALIZED.load(Ordering::SeqCst),
            false,
            "TUI should be marked as deinitialized after guard drops"
        );
    }

    #[test]
    fn test_restore_terminal_no_panic_when_not_initialized() {
        // Test that restore does not panic when TUI is not initialized
        TUI_INITIALIZED.store(false, Ordering::SeqCst);

        // This should not cause issues even if terminal is not in expected state
        let result = restore_tui();
        // Should return Ok or Err but not panic
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_guard_lifecycle() {
        TUI_INITIALIZED.store(false, Ordering::SeqCst);

        // Create guard in a separate scope to test drop behavior
        {
            let _guard = TuiPanicGuard::new();
            assert_eq!(
                TUI_INITIALIZED.load(Ordering::SeqCst),
                true,
                "Guard should mark TUI as initialized"
            );
        }

        assert_eq!(
            TUI_INITIALIZED.load(Ordering::SeqCst),
            false,
            "Drop should mark TUI as deinitialized"
        );
    }
}
