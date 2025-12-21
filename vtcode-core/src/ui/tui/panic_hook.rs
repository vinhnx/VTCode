//! Panic hook implementation for VTCode TUI applications
//! This module provides a panic hook that restores terminal state when a panic occurs,
//! preventing terminal corruption, and provides better panic formatting for different build types.

use std::io::{self, Write};
use std::panic;
use std::sync::atomic::{AtomicBool, Ordering};

use better_panic::Settings;
use crossterm::{
    cursor::Show,
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, PopKeyboardEnhancementFlags,
    },
    execute,
    terminal::{LeaveAlternateScreen, disable_raw_mode},
};
use tracing;

static TUI_INITIALIZED: AtomicBool = AtomicBool::new(false);
static DEBUG_MODE: AtomicBool = AtomicBool::new(false);

/// Set whether the application is in debug mode
///
/// When debug mode is enabled, panics will show a detailed backtrace
/// using better-panic.
pub fn set_debug_mode(enabled: bool) {
    DEBUG_MODE.store(enabled, Ordering::SeqCst);
}

/// Initialize the panic hook to restore terminal state on panic and provide better formatting
///
/// This function should be called very early in the application lifecycle,
/// before any TUI operations begin.
///
/// Follows Ratatui recipe: https://ratatui.rs/recipes/apps/panic-hooks/
pub fn init_panic_hook() {
    // Store the original panic hook to chain to it later
    let original_hook = panic::take_hook();

    panic::set_hook(Box::new(move |panic_info| {
        let is_tui = TUI_INITIALIZED.load(Ordering::SeqCst);
        let is_debug = DEBUG_MODE.load(Ordering::SeqCst);

        // First, attempt to restore terminal state if TUI was active
        if is_tui {
            // Try to restore the terminal - ignore errors since we're in a panic state
            let _ = restore_tui();
        }

        if is_debug {
            // Show pretty backtrace (the "modal" look)
            Settings::debug()
                .most_recent_first(false)
                .lineno_suffix(true)
                .create_panic_handler()(panic_info);
        } else {
            if !is_tui {
                // On program CLI show on debug log
                // We use tracing if available, but also print a simple message
                tracing::debug!(panic = %panic_info, "VTCode encountered a critical error");
            }

            eprintln!("\nVTCode encountered a critical error and needs to shut down.");
            eprintln!("Error details: {}", panic_info);
            eprintln!("If you encounter this issue, please report it to the VTCode team.");
            eprintln!("Run with --debug for more information.\n");

            // Call the original hook to ensure standard panic reporting (like backtraces)
            original_hook(panic_info);
        }

        // Exit with failure code to ensure the entire process terminates
        std::process::exit(1);
    }));
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
    // Attempt to disable raw mode if it was enabled
    // We ignore errors here since raw mode might not have been enabled
    let _ = disable_raw_mode();

    // Get stdout for executing terminal commands
    let mut stdout = io::stdout();

    // Disable various terminal modes that might have been enabled by the TUI
    let _ = execute!(stdout, DisableBracketedPaste);
    let _ = execute!(stdout, DisableFocusChange);
    let _ = execute!(stdout, DisableMouseCapture);
    let _ = execute!(stdout, PopKeyboardEnhancementFlags);

    // Ensure cursor is visible
    let _ = execute!(stdout, Show);

    // Try to leave alternate screen to return to normal terminal
    // This might fail if we're not in alternate screen, which is fine
    let _ = execute!(stdout, LeaveAlternateScreen);

    // Additional flush to ensure all escape sequences are processed
    let _ = stdout.flush();

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
