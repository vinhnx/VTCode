#![warn(missing_docs)]
#![expect(
    clippy::indexing_slicing,
    clippy::string_slice,
    reason = "validated diff indexes and UTF-8 token boundaries are structural invariants"
)]
//! Bounded structured text diffs and renderer-neutral preview rows.
//!
//! The presentation architecture and underlying ideas (unified and side-by-side
//! terminal previews with intraline emphasis) were informed by
//! [OpenAI Codex](https://github.com/openai/codex) (Apache-2.0). This crate is
//! an independent implementation for VT Code; no Codex source code was copied.

mod adapters;
mod compute;
mod display;
mod format;
mod intraline;
mod layout;
mod parse;
mod types;

pub use compute::compute_diff;
pub use display::{count_diff_changes, display_lines_from_hunks, display_lines_from_unified_diff, side_by_side_rows};
pub use format::{
    diff_display_line_number_width, format_hunk_header, format_numbered_unified_diff, format_unified_diff,
    format_unified_hunks,
};
pub use intraline::{annotate_word_level_diffs, compute_diff_chunks, word_level_changed_ranges};
pub use layout::{bounded_display_lines, layout_display_lines};
pub use types::*;

#[cfg(feature = "ratatui")]
pub use adapters::to_ratatui_lines;
#[cfg(feature = "ansi")]
pub use adapters::{AnsiDiffPalette, render_ansi_rows};

#[cfg(test)]
mod tests;
