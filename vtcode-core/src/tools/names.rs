use std::borrow::Cow;

use crate::config::constants::tools;

/// Normalize tool identifiers to their canonical registry names.
pub fn canonical_tool_name<'a>(name: &'a str) -> Cow<'a, str> {
    match name {
        "run_pty_cmd"
        | "run_terminal_cmd"
        | "run_terminal_command"
        | "run_cmd"
        | "runcommand"
        | "run"
        | "terminalrun"
        | "terminal_cmd"
        | "terminalcommand" => Cow::Borrowed(tools::RUN_COMMAND),
        _ => Cow::Borrowed(name),
    }
}

/// Return known aliases for a canonical tool name.
pub fn tool_aliases(name: &str) -> &'static [&'static str] {
    match name {
        tools::RUN_COMMAND => &[
            "run_pty_cmd",
            "run_terminal_cmd",
            "run_terminal_command",
            "run_cmd",
            "runcommand",
            "run",
            "terminalrun",
            "terminal_cmd",
            "terminalcommand",
        ],
        _ => &[],
    }
}
