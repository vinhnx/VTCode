//! TTY detection and capability utilities using crossterm's IsTty trait.
//!
//! This module provides safe and convenient TTY detection across the codebase,
//! abstracting away platform differences for TTY detection.
//!
//! # Usage
//!
//! ```rust
//! use vtcode_tui::utils::tty::TtyExt;
//! use std::io;
//!
//! // Check if stdout is a TTY
//! if io::stdout().is_tty_ext() {
//!     // Apply terminal-specific features
//! }
//!
//! // Check if stdin is a TTY
//! if io::stdin().is_tty_ext() {
//!     // Interactive input available
//! }
//! ```

use crossterm::tty::IsTty;
use std::io;

/// Extension trait for TTY detection on standard I/O streams.
///
/// This trait extends crossterm's `IsTty` to provide convenient methods
/// for checking TTY capabilities with better error handling.
pub trait TtyExt {
    /// Returns `true` if this stream is connected to a terminal.
    ///
    /// This is a convenience wrapper around crossterm's `IsTty` trait
    /// that provides consistent behavior across the codebase.
    fn is_tty_ext(&self) -> bool;

    /// Returns `true` if this stream supports ANSI color codes.
    ///
    /// This checks both TTY status and common environment variables
    /// that might disable color output.
    fn supports_color(&self) -> bool;

    /// Returns `true` if this stream supports interactive features.
    ///
    /// Interactive features include cursor movement, color, and other
    /// terminal capabilities that require a real terminal.
    fn is_interactive(&self) -> bool;
}

impl TtyExt for io::Stdout {
    fn is_tty_ext(&self) -> bool {
        self.is_tty()
    }

    fn supports_color(&self) -> bool {
        if !self.is_tty() {
            return false;
        }

        // Check for NO_COLOR environment variable
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }

        // Check for FORCE_COLOR environment variable
        if std::env::var_os("FORCE_COLOR").is_some() {
            return true;
        }

        true
    }

    fn is_interactive(&self) -> bool {
        self.is_tty() && self.supports_color()
    }
}

impl TtyExt for io::Stderr {
    fn is_tty_ext(&self) -> bool {
        self.is_tty()
    }

    fn supports_color(&self) -> bool {
        if !self.is_tty() {
            return false;
        }

        // Check for NO_COLOR environment variable
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }

        // Check for FORCE_COLOR environment variable
        if std::env::var_os("FORCE_COLOR").is_some() {
            return true;
        }

        true
    }

    fn is_interactive(&self) -> bool {
        self.is_tty() && self.supports_color()
    }
}

impl TtyExt for io::Stdin {
    fn is_tty_ext(&self) -> bool {
        self.is_tty()
    }

    fn supports_color(&self) -> bool {
        // Stdin doesn't output color, but we check if it's interactive
        self.is_tty()
    }

    fn is_interactive(&self) -> bool {
        self.is_tty()
    }
}

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
        if !stderr.is_tty() {
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
    io::stderr().is_tty() && io::stdin().is_tty()
}

/// Get the current terminal dimensions.
///
/// Returns `Some((width, height))` if the terminal size can be determined,
/// otherwise `None`.
pub fn terminal_size() -> Option<(u16, u16)> {
    crossterm::terminal::size().ok()
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
        let _ = stdout.is_tty();
        let _ = stderr.is_tty();
        let _ = stdin.is_tty();
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
