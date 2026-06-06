//! Diff utilities for generating structured diffs.
//!
//! Delegates to `vtcode_design::diff` for the canonical implementations.

pub use vtcode_commons::diff::*;

/// Format a unified diff without ANSI color codes.
pub fn format_unified_diff(old: &str, new: &str, options: DiffOptions<'_>) -> String {
    vtcode_design::diff::format_unified_diff(old, new, options)
}

/// Compute a structured diff bundle using the default theme-aware formatter.
pub fn compute_diff_with_theme(old: &str, new: &str, options: DiffOptions<'_>) -> DiffBundle {
    vtcode_design::diff::compute_diff_with_theme(old, new, options)
}

/// Format diff hunks with standard ANSI colors for terminal display.
pub fn format_colored_diff(hunks: &[DiffHunk], options: &DiffOptions<'_>) -> String {
    vtcode_design::diff::format_colored_diff(hunks, options)
}
