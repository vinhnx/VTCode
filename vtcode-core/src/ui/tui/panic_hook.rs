//! Panic hook implementation for VTCode TUI applications
//! This module provides a panic hook that restores terminal state when a panic occurs,
//! preventing terminal corruption, and provides better panic formatting for different build types.

use std::io::{self, Write};
use std::panic;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::{
    execute,
    terminal::{LeaveAlternateScreen, disable_raw_mode},
};

static TUI_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Initialize the panic hook to restore terminal state on panic and provide better formatting
///
/// This function should be called very early in the application lifecycle,
/// before any TUI operations begin.
pub fn init_panic_hook() {
    // Store the original panic hook to potentially chain to it later if needed
    let original_hook = panic::take_hook();

    panic::set_hook(Box::new(move |panic_info| {
        // First, attempt to restore terminal state if TUI was active
        if TUI_INITIALIZED.load(Ordering::SeqCst) {
            // Try to restore the terminal - ignore errors since we're in a panic state
            let _ = restore_terminal_on_panic();
        }

        // For debug builds, use better-panic for developer-friendly stack traces
        #[cfg(debug_assertions)]
        {
            better_panic::Settings::auto()
                .most_recent_first(false) // Show most recent frames first for easier debugging
                .lineno_suffix(true) // Include line numbers for precise location info
                .verbosity(better_panic::Verbosity::Full) // Maximum detail for debugging
                .create_panic_handler()(panic_info);
        }

        // For release builds, provide a cleaner panic message without full stack trace
        #[cfg(not(debug_assertions))]
        {
            eprintln!("VTCode encountered a critical error and needs to shut down.");
            eprintln!("Error details: {}", panic_info);
            eprintln!("If you encounter this issue, please report it to the VTCode team.");
        }

        // Call the original hook as well, in case other code has registered hooks
        original_hook(panic_info);

        // Exit with failure code
        std::process::exit(libc::EXIT_FAILURE);
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
fn restore_terminal_on_panic() -> io::Result<()> {
    // Attempt to disable raw mode if it was enabled
    // We ignore errors here since raw mode might not have been enabled
    let _ = disable_raw_mode();

    // Get stdout for executing terminal commands
    let mut stdout = io::stdout();

    // Ensure cursor is visible
    let _ = execute!(stdout, crossterm::cursor::Show);

    // Try to leave alternate screen to return to normal terminal
    // This might fail if we're not in alternate screen, which is fine
    let _ = execute!(stdout, LeaveAlternateScreen);

    // Additional flush to ensure all escape sequences are processed
    stdout.flush()?;

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
        let result = restore_terminal_on_panic();
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
