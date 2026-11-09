//! TTY detection and capability utilities using Rust's `IsTerminal` trait.
//!
//! The canonical [`TtyExt`] trait lives in [`vtcode_commons::tty`] and is
//! re-exported here so existing `vtcode_core::utils::tty::*` import paths
//! keep resolving. [`TtyCapabilities`], [`is_interactive_session`], and
//! [`terminal_size()`] remain core-side helpers built on that trait.
//!
//! # Example
//!
//! ```rust
//! use vtcode_core::utils::tty::TtyExt;
//! use std::io;
//!
//! // Check if stdout is a TTY
//! if io::stdout().is_tty_ext() {
//!     // Apply terminal-specific features
//! }
//! ```

pub use vtcode_commons::tty::TtyExt;

use std::io;
use std::io::IsTerminal;

/// TTY capabilities that can be queried for feature detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TtyCapabilities {
    /// Whether the terminal supports ANSI color codes.
    pub color: bool,
    /// Whether the terminal supports cursor movement and manipulation.
    pub cursor: bool,
    /// Whether the terminal supports bracketed paste mode.
    pub bracketed_paste: bool,
    /// Whether the terminal supports focus change events.
    pub focus_events: bool,
    /// Whether the terminal supports mouse input.
    pub mouse: bool,
    /// Whether the terminal supports keyboard enhancement flags.
    pub keyboard_enhancement: bool,
}

impl TtyCapabilities {
    /// Detect the capabilities of the current terminal.
    ///
    /// This function queries the terminal to determine which features
    /// are available. It should be called once at application startup
    /// and the results cached for later use.
    ///
    /// # Returns
    ///
    /// Returns `Some(TtyCapabilities)` if stderr is a TTY, otherwise `None`.
    pub fn detect() -> Option<Self> {
        let stderr = io::stderr();
        if !stderr.is_terminal() {
            return None;
        }

        Some(Self {
            color: stderr.supports_color(),
            cursor: true,               // All TTYs support basic cursor movement
            bracketed_paste: true,      // Assume support, will fail gracefully if not
            focus_events: true,         // Assume support, will fail gracefully if not
            mouse: true,                // Assume support, will fail gracefully if not
            keyboard_enhancement: true, // Assume support, will fail gracefully if not
        })
    }

    /// Returns `true` if the terminal supports all advanced features.
    pub fn is_fully_featured(&self) -> bool {
        self.color
            && self.cursor
            && self.bracketed_paste
            && self.focus_events
            && self.mouse
            && self.keyboard_enhancement
    }

    /// Returns `true` if the terminal supports basic TUI features.
    pub fn is_basic_tui(&self) -> bool {
        self.color && self.cursor
    }
}

/// Check if the application is running in an interactive TTY context.
///
/// This is useful for deciding whether to use rich terminal features
/// or fall back to plain text output.
pub fn is_interactive_session() -> bool {
    io::stderr().is_terminal() && io::stdin().is_terminal()
}

/// Get the current terminal dimensions.
///
/// Returns `Some((width, height))` if the terminal size can be determined,
/// otherwise `None`.
pub fn terminal_size() -> Option<(u16, u16)> {
    terminal_size::terminal_size().map(|(terminal_size::Width(w), terminal_size::Height(h))| (w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tty_detection() {
        // These tests verify the TTY detection logic works
        // Note: In actual test environments, these may vary
        let stdout = io::stdout();
        let stderr = io::stderr();
        let stdin = io::stdin();

        // Just verify the methods don't panic
        let _ = stdout.is_terminal();
        let _ = stderr.is_terminal();
        let _ = stdin.is_terminal();
    }

    #[test]
    fn test_capabilities_detection() {
        // Test that capability detection doesn't panic
        let caps = TtyCapabilities::detect();
        // In a test environment, this might be None
        // Just verify the method works
        let _ = caps.is_some() || caps.is_none();
    }

    #[test]
    fn test_interactive_session() {
        // Test interactive session detection
        let _ = is_interactive_session();
    }
}
