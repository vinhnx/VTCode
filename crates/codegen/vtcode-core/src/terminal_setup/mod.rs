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
pub use terminals::iterm2::{
    TerminalIconCommand, ensure_profile_icon, parse_terminal_icon_command, profile_icon_instructions,
    run_profile_icon_install, run_profile_icon_reset, run_terminal_icon_show, run_terminal_icon_switch,
};
pub use wizard::run_terminal_setup_wizard;
