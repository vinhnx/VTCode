//! Panic hook implementation for terminal UI applications
//! This module provides a panic hook that restores terminal state when a panic occurs,
//! preventing terminal corruption, and provides enhanced panic formatting for different build types.

use std::io::{self, Write};
use std::panic;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

use better_panic::{Settings as BetterPanicSettings, Verbosity as BetterPanicVerbosity};
use human_panic::{Metadata as HumanPanicMetadata, handle_dump as human_panic_dump, print_msg};
use ratatui::crossterm::{
    cursor::{MoveToColumn, RestorePosition, SetCursorStyle, Show},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, PopKeyboardEnhancementFlags,
    },
    execute,
    terminal::{Clear, ClearType, LeaveAlternateScreen, disable_raw_mode},
};

static TUI_INITIALIZED: AtomicBool = AtomicBool::new(false);
static DEBUG_MODE: AtomicBool = AtomicBool::new(cfg!(debug_assertions));
static COLOR_EYRE_ENABLED: AtomicBool = AtomicBool::new(cfg!(debug_assertions));
static SHOW_DIAGNOSTICS: AtomicBool = AtomicBool::new(false);
static PANIC_HOOK_ONCE: Once = Once::new();
static COLOR_EYRE_SETUP_ONCE: Once = Once::new();
#[cfg(debug_assertions)]
static COLOR_EYRE_PANIC_HOOK: std::sync::OnceLock<color_eyre::config::PanicHook> =
    std::sync::OnceLock::new();
static APP_METADATA: std::sync::OnceLock<AppMetadata> = std::sync::OnceLock::new();

#[derive(Clone, Debug)]
struct AppMetadata {
    name: &'static str,
    version: &'static str,
    authors: &'static str,
    repository: Option<&'static str>,
}

impl AppMetadata {
    fn default_for_tui_crate() -> Self {
        Self {
            name: env!("CARGO_PKG_NAME"),
            version: env!("CARGO_PKG_VERSION"),
            authors: env!("CARGO_PKG_AUTHORS"),
            repository: Some(env!("CARGO_PKG_REPOSITORY")).filter(|value| !value.is_empty()),
        }
    }
}

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

/// Set whether color-eyre formatting should be used for debug panic/error reporting.
pub fn set_color_eyre_enabled(enabled: bool) {
    COLOR_EYRE_ENABLED.store(enabled, Ordering::SeqCst);
}

/// Get whether color-eyre formatting is enabled for debug panic/error reporting.
fn is_color_eyre_enabled() -> bool {
    COLOR_EYRE_ENABLED.load(Ordering::SeqCst)
}

/// Install color-eyre's eyre hook for richer top-level error rendering in dev/debug mode.
fn maybe_prepare_color_eyre_hooks() {
    if !cfg!(debug_assertions) || !is_color_eyre_enabled() {
        return;
    }

    #[cfg(debug_assertions)]
    COLOR_EYRE_SETUP_ONCE.call_once(|| {
        let hooks = color_eyre::config::HookBuilder::default().try_into_hooks();
        match hooks {
            Ok((panic_hook, eyre_hook)) => {
                let _ = COLOR_EYRE_PANIC_HOOK.set(panic_hook);
                if let Err(error) = eyre_hook.install() {
                    eprintln!("warning: failed to install color-eyre hook: {error}");
                }
            }
            Err(error) => {
                eprintln!("warning: failed to prepare color-eyre hook: {error}");
            }
        }
    });
}

/// Print an application error using color-eyre when enabled, otherwise fallback formatting.
pub fn print_error_report(error: anyhow::Error) {
    if cfg!(debug_assertions) && is_color_eyre_enabled() {
        #[cfg(debug_assertions)]
        {
            maybe_prepare_color_eyre_hooks();
            let report = color_eyre::eyre::eyre!("{error:#}");
            eprintln!("{report:?}");
            return;
        }
    }

    eprintln!("Error: {error:?}");
}

/// Set whether diagnostics (ERROR-level logs, warnings) should be displayed in the TUI.
/// Driven by `ui.show_diagnostics_in_transcript` in vtcode.toml.
pub fn set_show_diagnostics(enabled: bool) {
    SHOW_DIAGNOSTICS.store(enabled, Ordering::SeqCst);
}

/// Get whether diagnostics should be displayed in the TUI
pub fn show_diagnostics() -> bool {
    SHOW_DIAGNOSTICS.load(Ordering::SeqCst)
}

/// Set application metadata used by release panic reports.
///
/// If this is not set, metadata from the `vtcode-tui` crate is used.
pub fn set_app_metadata(
    name: &'static str,
    version: &'static str,
    authors: &'static str,
    repository: Option<&'static str>,
) {
    let _ = APP_METADATA.set(AppMetadata {
        name,
        version,
        authors,
        repository: repository.filter(|value| !value.is_empty()),
    });
}

fn app_metadata() -> AppMetadata {
    APP_METADATA
        .get()
        .cloned()
        .unwrap_or_else(AppMetadata::default_for_tui_crate)
}

/// Initialize the panic hook to restore terminal state on panic and provide better formatting
///
/// This function should be called very early in the application lifecycle,
/// before any TUI operations begin.
///
/// Follows Ratatui recipe: https://ratatui.rs/recipes/apps/better-panic/
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

            if cfg!(debug_assertions) && is_debug {
                if is_color_eyre_enabled() {
                    #[cfg(debug_assertions)]
                    {
                        maybe_prepare_color_eyre_hooks();
                        if let Some(panic_hook) = COLOR_EYRE_PANIC_HOOK.get() {
                            eprintln!("{}", panic_hook.panic_report(panic_info));
                            return;
                        }
                    }
                }

                better_panic_hook(panic_info);
                // In debug/dev mode, preserve normal panic semantics (unwind/abort by profile)
                // rather than forcing immediate process exit from inside the hook.
                return;
            }

            {
                let metadata = app_metadata();
                let mut report_metadata = HumanPanicMetadata::new(metadata.name, metadata.version)
                    .authors(format!("authored by {}", metadata.authors));

                if let Some(repository) = metadata.repository {
                    report_metadata = report_metadata
                        .support(format!("Open a support request at {}", repository));
                }

                let file_path = human_panic_dump(&report_metadata, panic_info);
                if let Err(error) = print_msg(file_path, &report_metadata) {
                    eprintln!("\nVT Code encountered a critical error and needs to shut down.");
                    eprintln!("Failed to print crash report details: {}", error);
                    original_hook(panic_info);
                }
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
    mark_tui_deinitialized();
    let mut first_error: Option<io::Error> = None;

    // 1. Drain any pending crossterm events to prevent them from leaking to the shell
    // This is a best-effort drain with a zero timeout
    while let Ok(true) = crossterm::event::poll(std::time::Duration::from_millis(0)) {
        let _ = crossterm::event::read();
    }

    // Get stderr for executing terminal commands
    let mut stderr = io::stderr();

    // 2. Clear current line to remove any echoed ^C characters from rapid Ctrl+C presses
    if let Err(error) = execute!(stderr, MoveToColumn(0), Clear(ClearType::CurrentLine)) {
        first_error.get_or_insert(error);
    }

    // 3. Leave alternate screen FIRST (if we were in one)
    // This is the most critical operation for visual restoration
    if let Err(error) = execute!(stderr, LeaveAlternateScreen) {
        first_error.get_or_insert(error);
    }

    // 4. Disable various terminal modes that might have been enabled by the TUI
    if let Err(error) = execute!(stderr, DisableBracketedPaste) {
        first_error.get_or_insert(error);
    }
    if let Err(error) = execute!(stderr, DisableFocusChange) {
        first_error.get_or_insert(error);
    }
    if let Err(error) = execute!(stderr, DisableMouseCapture) {
        first_error.get_or_insert(error);
    }
    if let Err(error) = execute!(stderr, PopKeyboardEnhancementFlags) {
        first_error.get_or_insert(error);
    }

    // Ensure cursor state is restored
    if let Err(error) = execute!(
        stderr,
        SetCursorStyle::DefaultUserShape,
        Show,
        RestorePosition
    ) {
        first_error.get_or_insert(error);
    }

    // 5. Disable raw mode LAST to ensure all cleanup commands are sent properly
    if let Err(error) = disable_raw_mode() {
        first_error.get_or_insert(error);
    }

    // Additional flush to ensure all escape sequences are processed
    if let Err(error) = stderr.flush() {
        first_error.get_or_insert(error);
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
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

    #[test]
    fn test_color_eyre_toggle() {
        set_color_eyre_enabled(false);
        assert!(!is_color_eyre_enabled());

        set_color_eyre_enabled(true);
        assert!(is_color_eyre_enabled());
    }
}
