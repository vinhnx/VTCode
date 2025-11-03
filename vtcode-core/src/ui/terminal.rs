//! Terminal utilities and helpers

use std::io::IsTerminal;
use std::io::Write;

/// Get the terminal width, fallback to 80 if unable to determine
pub fn get_terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(terminal_size::Width(w), _)| w as usize)
        .unwrap_or(80)
}

/// Flush stdout to ensure output is displayed immediately
pub fn flush_stdout() {
    std::io::stdout().flush().ok();
}

/// Read a line from stdin with proper error handling
pub fn read_line() -> std::io::Result<String> {
    let mut buffer = String::new();
    std::io::stdin().read_line(&mut buffer)?;
    Ok(buffer.trim().to_string())
}

/// Check if output is being piped (not a terminal)
pub fn is_piped_output() -> bool {
    !std::io::stdout().is_terminal()
}

/// Check if input is being piped (not a terminal)
pub fn is_piped_input() -> bool {
    !std::io::stdin().is_terminal()
}
