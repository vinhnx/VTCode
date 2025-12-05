//! Local tools and utilities for the CLI.

pub mod context_helper;
pub mod ripgrep_installer;

pub use context_helper::{ContextStatus, format_token_usage, suggest_actions};
pub use ripgrep_installer::RipgrepStatus;
