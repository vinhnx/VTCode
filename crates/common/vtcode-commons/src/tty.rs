//! TTY detection capability shared across VT Code crates.
//!
//! This module owns the canonical [`TtyExt`] extension trait for standard I/O
//! streams. Both `vtcode-core` (`crate::utils::tty`) and `vtcode-ui`
//! (`crate::tui::utils::tty`) re-export it so historical import paths keep
//! resolving; extend the contract here instead of forking it per crate.
//!
//! # Example
//!
//! ```rust
//! use std::io;
//! use vtcode_commons::tty::TtyExt;
//!
//! if io::stdout().is_tty_ext() {
//!     // Terminal-specific features available.
//! }
//! ```

use std::io;
use std::io::IsTerminal;

use crate::color_policy::no_color_env_active;

/// Extension trait for TTY detection on standard I/O streams.
///
/// This trait extends [`IsTerminal`] with convenient methods for checking TTY
/// capabilities with consistent `NO_COLOR` / `FORCE_COLOR` handling.
pub trait TtyExt {
    /// Returns `true` if this stream is connected to a terminal.
    fn is_tty_ext(&self) -> bool;

    /// Returns `true` if this stream supports ANSI color codes.
    ///
    /// This checks both TTY status and the `NO_COLOR` / `FORCE_COLOR`
    /// environment variables.
    fn supports_color(&self) -> bool;

    /// Returns `true` if this stream supports interactive features.
    ///
    /// Interactive features include cursor movement, color, and other
    /// terminal capabilities that require a real terminal.
    fn is_interactive(&self) -> bool;
}

impl TtyExt for io::Stdout {
    fn is_tty_ext(&self) -> bool {
        self.is_terminal()
    }

    fn supports_color(&self) -> bool {
        if !self.is_terminal() {
            return false;
        }

        // Check NO_COLOR with strict non-empty semantics.
        if no_color_env_active() {
            return false;
        }

        // Check for FORCE_COLOR environment variable
        if std::env::var_os("FORCE_COLOR").is_some() {
            return true;
        }

        true
    }

    fn is_interactive(&self) -> bool {
        self.is_terminal() && self.supports_color()
    }
}

impl TtyExt for io::Stderr {
    fn is_tty_ext(&self) -> bool {
        self.is_terminal()
    }

    fn supports_color(&self) -> bool {
        if !self.is_terminal() {
            return false;
        }

        // Check NO_COLOR with strict non-empty semantics.
        if no_color_env_active() {
            return false;
        }

        // Check for FORCE_COLOR environment variable
        if std::env::var_os("FORCE_COLOR").is_some() {
            return true;
        }

        true
    }

    fn is_interactive(&self) -> bool {
        self.is_terminal() && self.supports_color()
    }
}

impl TtyExt for io::Stdin {
    fn is_tty_ext(&self) -> bool {
        self.is_terminal()
    }

    fn supports_color(&self) -> bool {
        // Stdin doesn't output color, but we check if it's interactive
        self.is_terminal()
    }

    fn is_interactive(&self) -> bool {
        self.is_terminal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_is_tty_and_color_for_output_streams() {
        // Absolute values depend on the environment (pty vs pipe), so assert
        // the contract relationship instead: interactive == tty && color.
        for (interactive, tty, color) in [
            (io::stdout().is_interactive(), io::stdout().is_tty_ext(), io::stdout().supports_color()),
            (io::stderr().is_interactive(), io::stderr().is_tty_ext(), io::stderr().supports_color()),
        ] {
            assert_eq!(interactive, tty && color);
        }
    }

    #[test]
    fn stdin_answers_are_pure_tty_checks() {
        // Stdin never emits color: its answers track raw TTY status rather
        // than consulting NO_COLOR / FORCE_COLOR like the output streams.
        assert_eq!(io::stdin().supports_color(), io::stdin().is_tty_ext());
        assert_eq!(io::stdin().is_interactive(), io::stdin().is_tty_ext());
    }
}
