mod error;
mod hook;
mod restore;
pub(crate) mod state;

pub use self::error::{TuiPanicGuard, print_error_report};
pub use self::hook::init_panic_hook;
pub use self::restore::{
    drain_pending_terminal_input, ensure_raw_mode_disabled, finish_deferred_raw_mode_restore, restore_tui,
    restore_tui_keep_raw_mode,
};
pub use self::state::{set_app_metadata, set_color_eyre_enabled, set_debug_mode, set_show_diagnostics};

pub(crate) use self::state::{
    is_debug_mode, is_restore_claimed, is_terminal_modified, is_tui_initialized, lock_terminal_operations,
    mark_color_scheme_reports_enabled, mark_keyboard_enhancements_pushed, mark_terminal_modified,
    mark_terminal_restored, show_diagnostics,
};
