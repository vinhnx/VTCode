use std::panic;

use better_panic::{Settings as BetterPanicSettings, Verbosity as BetterPanicVerbosity};
use human_panic::{Metadata as HumanPanicMetadata, handle_dump as human_panic_dump, print_msg};

use super::restore::restore_tui;
use super::state::{self, AppMetadata};

static PANIC_HOOK_ONCE: std::sync::Once = std::sync::Once::new();

/// Initialize the panic hook to restore terminal state on panic and provide better formatting.
pub fn init_panic_hook() {
    PANIC_HOOK_ONCE.call_once(|| {
        let original_hook = panic::take_hook();

        let better_panic_hook = BetterPanicSettings::new()
            .verbosity(BetterPanicVerbosity::Full)
            .most_recent_first(false)
            .lineno_suffix(true)
            .create_panic_handler();

        panic::set_hook(Box::new(move |panic_info| {
            let is_debug = state::is_debug_mode();

            if state::is_tui_initialized() {
                let _ = restore_tui();
            }

            if cfg!(debug_assertions) && is_debug {
                if state::is_color_eyre_enabled() {
                    state::maybe_prepare_color_eyre_hooks();
                    if let Some(panic_hook) = state::COLOR_EYRE_PANIC_HOOK.get() {
                        vtcode_commons::diagnostics::stderr(format_args!("{}", panic_hook.panic_report(panic_info)));
                        return;
                    }
                }

                better_panic_hook(panic_info);
                return;
            }

            // Release mode: human-panic dump + user-facing message
            let metadata = state::app_metadata();
            let mut report_metadata = HumanPanicMetadata::new(metadata.name, metadata.version)
                .authors(format!("authored by {}", metadata.authors));

            if let Some(repository) = metadata.repository {
                report_metadata = report_metadata.support(format!("Open a support request at {repository}"));
            }

            let file_path = human_panic_dump(&report_metadata, panic_info);
            if let Err(error) = print_msg(file_path, &report_metadata) {
                vtcode_commons::diagnostics::stderr(format_args!(
                    "\nVT Code encountered a critical error and needs to shut down."
                ));
                vtcode_commons::diagnostics::stderr(format_args!("Failed to print crash report details: {error}"));
                original_hook(panic_info);
            }

            std::process::exit(1);
        }));
    });
}
