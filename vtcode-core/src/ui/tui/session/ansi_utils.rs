//! ANSI utilities for TUI session
//!
//! This module re-exports the canonical ANSI stripping utilities from `utils::ansi_parser`.
//! Use `crate::utils::ansi_parser::strip_ansi` for string output or
//! `super::text_utils::strip_ansi_codes` for Cow<str> output with early return optimization.

/// Re-export the canonical strip_ansi function for use within the TUI session module.
/// For new code, prefer using `crate::utils::ansi_parser::strip_ansi` directly.
#[allow(dead_code)]
#[inline]
pub(super) fn strip_ansi_codes(text: &str) -> String {
    crate::utils::ansi_parser::strip_ansi(text)
}
