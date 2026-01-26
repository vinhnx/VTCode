/// Maximum number of bytes allowed in diff preview inputs
pub const MAX_PREVIEW_BYTES: usize = 200_000;

/// Number of context lines to include around changes in unified diff output
pub const CONTEXT_RADIUS: usize = 3;

/// Maximum number of diff lines to keep in preview output before condensation
pub const MAX_PREVIEW_LINES: usize = 160;

/// Number of leading diff lines to retain when condensing previews
pub const HEAD_LINE_COUNT: usize = 96;

/// Number of trailing diff lines to retain when condensing previews
pub const TAIL_LINE_COUNT: usize = 32;

/// Maximum number of files to show inline diffs for before suppression
pub const MAX_INLINE_DIFF_FILES: usize = 10;

/// Maximum total diff lines across all files before suppression
pub const MAX_TOTAL_DIFF_LINES: usize = 500;

/// Maximum additions + deletions in a single file before suppression
pub const MAX_SINGLE_FILE_CHANGES: usize = 200;

/// Maximum number of files to list in suppression summary
pub const MAX_FILES_IN_SUMMARY: usize = 20;

/// Message shown when inline diffs are suppressed
pub const SUPPRESSION_MESSAGE: &str =
    "Inline diffs have been suppressed for recent changes because there are too many to display.";

/// Hint message shown with suppressed diffs
pub const SUPPRESSION_HINT: &str = "Tip: Use `git diff` to view the full changes.";
