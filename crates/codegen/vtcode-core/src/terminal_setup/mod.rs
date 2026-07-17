//! Terminal setup wizard for configuring terminal emulators for optimal VT Code experience.
//!
//! This module provides an interactive wizard that configures 9+ terminal emulators with:
//! - Shift+Enter for multiline input
//! - Enhanced copy/paste integration
//! - Shell integration (working directory, command status)
//! - Theme synchronization with VT Code themes

pub mod backup;
pub mod config_writer;
pub mod detector;
pub mod features;
pub mod terminals;
pub mod wizard;

pub use detector::{TerminalFeature, TerminalType};
pub use wizard::run_terminal_setup_wizard;

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_structure() {
        // Smoke test to ensure module compiles
    }
}
