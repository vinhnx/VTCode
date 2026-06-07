// DRY helpers for user-facing CLI messages.
//
// Conventions:
//   error()   — red bold "Error: ..." for fatal problems
//   warn()    — yellow "Warning: ..." for non-fatal issues
//   info()    — cyan "Info: ..." for neutral guidance
//   ok()      — green "Done: ..." for success
//   hint()    — dim "  -> ..." for follow-up actions beneath an error/warn
//   config_hint() — renders a vtcode.toml snippet the user should add

use vtcode_core::utils::colors::style;

/// Red bold error prefix.
pub fn error(msg: &str) -> String {
    format!("{}", style(format!("Error: {msg}")).red().bold())
}

/// Yellow warning prefix.
pub fn warn(msg: &str) -> String {
    format!("{}", style(format!("Warning: {msg}")).yellow())
}

/// Cyan info prefix.
pub fn info(msg: &str) -> String {
    format!("{}", style(format!("Info: {msg}")).cyan())
}

/// Green success prefix.
pub fn ok(msg: &str) -> String {
    format!("{}", style(format!("Done: {msg}")).green())
}

/// Dim follow-up hint, indented with arrow.
pub fn hint(msg: &str) -> String {
    format!("{}", style(format!("  -> {msg}")).dim())
}

/// Render a vtcode.toml configuration snippet the user should add.
pub fn config_hint(section: &str, snippet: &str) -> String {
    format!(
        "Add the following to {}:\n\n  [{}]\n  {}",
        style("vtcode.toml").bold(),
        section,
        snippet
    )
}
