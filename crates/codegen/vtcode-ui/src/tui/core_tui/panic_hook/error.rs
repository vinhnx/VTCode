use super::restore::restore_tui;
use super::state::{self, AppMetadata};

/// Print an application error using color-eyre when enabled, otherwise fallback formatting.
pub fn print_error_report(error: anyhow::Error) {
    let _ = restore_tui();

    if cfg!(debug_assertions) && state::is_color_eyre_enabled() {
        state::maybe_prepare_color_eyre_hooks();
        let report = color_eyre::eyre::eyre!("{error:#}");
        vtcode_commons::diagnostics::stderr(format_args!("{report:?}"));
        return;
    }

    vtcode_commons::diagnostics::stderr(format_args!("Error: {error:?}"));
}

/// A guard struct that automatically registers and unregisters TUI state
/// with the panic hook system.
///
/// This ensures that terminal restoration only happens when the TUI was actually active.
pub struct TuiPanicGuard;

impl TuiPanicGuard {
    pub(crate) fn new() -> Self {
        state::mark_tui_initialized();
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
        state::mark_tui_deinitialized();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panic_guard_initialization() {
        state::TUI_INITIALIZED.store(false, std::sync::atomic::Ordering::SeqCst);

        {
            let _guard = TuiPanicGuard::new();
            assert!(state::TUI_INITIALIZED.load(std::sync::atomic::Ordering::SeqCst));
        }

        assert!(!state::TUI_INITIALIZED.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_guard_lifecycle() {
        state::TUI_INITIALIZED.store(false, std::sync::atomic::Ordering::SeqCst);

        {
            let _guard = TuiPanicGuard::new();
            assert!(state::TUI_INITIALIZED.load(std::sync::atomic::Ordering::SeqCst));
        }

        assert!(!state::TUI_INITIALIZED.load(std::sync::atomic::Ordering::SeqCst));
    }
}
