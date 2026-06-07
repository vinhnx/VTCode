// DRY helpers for user-facing CLI messages.
//
// Conventions:
//   error()   — red bold "Error: ..." for fatal problems
//   warn()    — yellow "Warning: ..." for non-fatal issues
//   ok()      — green "Done: ..." for success
//   hint()    — dim "  -> ..." for follow-up actions beneath an error/warn

use vtcode_core::utils::colors::style;

/// Red bold error prefix.
pub fn error(msg: &str) -> String {
    format!("{}", style(format!("Error: {msg}")).red().bold())
}

/// Yellow warning prefix.
pub fn warn(msg: &str) -> String {
    format!("{}", style(format!("Warning: {msg}")).yellow())
}

/// Green success prefix.
pub fn ok(msg: &str) -> String {
    format!("{}", style(format!("Done: {msg}")).green())
}

/// Dim follow-up hint, indented with arrow.
pub fn hint(msg: &str) -> String {
    format!("{}", style(format!("  -> {msg}")).dim())
}
