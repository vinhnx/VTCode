//! ANSI escape sequence constants and utilities
//!
//! Re-exports from vtcode-commons for backward compatibility.
//! See `docs/reference/ansi-in-vtcode.md` for workspace usage guidance.

pub use vtcode_commons::ansi_codes::*;

pub fn notify_attention_with_terminal_method(
    default_enabled: bool,
    message: Option<&str>,
    method: vtcode_config::TerminalNotificationMethod,
) {
    let override_mode = match method {
        vtcode_config::TerminalNotificationMethod::Auto => NotifyMethodOverride::Auto,
        vtcode_config::TerminalNotificationMethod::Bel => NotifyMethodOverride::Bell,
        vtcode_config::TerminalNotificationMethod::Osc9 => NotifyMethodOverride::Osc9,
    };

    notify_attention_with_mode(default_enabled, message, override_mode);
}
