//! Tool output rendering with token-aware truncation
//!
//! This module handles formatting and displaying tool output to the user.
//! It uses a **token-based truncation strategy** instead of naive line limits,
//! which aligns with how LLMs consume context.
//!
//! ## Truncation Strategy
//!
//! Instead of hard line limits (e.g., "show first 128 + last 128 lines"), we use:
//! - **Token budget**: 25,000 tokens max per tool response
//! - **Head+Tail preservation**: Keep first ~50% and last ~50% of tokens
//! - **Token-aware**: Uses heuristic approximation for token counting
//!   (1 token ≈ 3.5 chars for regular content)
//!
//! ### Why Token-Based?
//!
//! 1. **Aligns with reality**: Tokens matter for context window, not lines
//!    - 256 short lines (~1-2k tokens) < 100 long lines (~10k tokens)
//!
//! 2. **Better for incomplete outputs**: Long build logs or test results often have
//!    critical info at the end (errors, summaries). Head+tail preserves both.
//!
//! 3. **Fewer tool calls needed**: Model can absorb more meaningful information
//!    per call instead of making multiple sequential calls to work around limits.
//!
//! 4. **Consistent across tools**: All tool outputs use the same token budget,
//!    not arbitrary per-tool line limits.
//!
//! ### UI Display Limits (Separate Layer)
//!
//! The token limit applies to what we *send to the model*. Display rendering has
//! separate safeguards to prevent UI lag:
//! - `MAX_LINE_LENGTH: 150`: Prevents extremely long lines from hanging the TUI
//! - `INLINE_STREAM_MAX_LINES: 30`: Limits visible output in inline mode
//! - `MAX_CODE_LINES: 30`: For code fence blocks (full output in spool files)
//!
//! Full output is spooled to workspace `.vtcode/tool-output/` for later review.
//! For very large outputs, files are saved to the user cache's
//! `large-output/<session_hash>/call_<id>.output`
//! with a notification displayed to the client.
const INLINE_STREAM_MAX_LINES: usize = 30;
/// Number of head lines to show for run-command output previews
const RUN_COMMAND_HEAD_PREVIEW_LINES: usize = 3;
/// Number of tail lines to show for run-command output previews
const RUN_COMMAND_TAIL_PREVIEW_LINES: usize = 3;
/// Maximum line length before truncation to prevent TUI hang
const MAX_LINE_LENGTH: usize = 150;
/// Safety cap for inline-TUI diff rows that opt into reflow wrapping.
///
/// Rows under this width are emitted whole so transcript reflow can word-wrap
/// them with a hanging gutter indent. Pathological minified lines above the
/// cap still ellipsis-truncate so a single row cannot flood the transcript.
const DIFF_WRAP_SOURCE_MAX_WIDTH: usize = 2_000;
/// Size threshold (bytes) below which output is displayed inline vs. spooled
const DEFAULT_SPOOL_THRESHOLD: usize = 50_000; // 50KB — UI render truncation
/// Maximum number of lines to display in code fence blocks before truncating.
/// Kept low to prevent TUI flooding — full output is in spool files.
const MAX_CODE_LINES: usize = 30;

/// Visible stdin/stdout rows kept for an exec-session call.
///
/// `write_stdin` and the session readers re-render the session's captured
/// output on every poll or wait, so a long build would otherwise repeat its
/// whole log at each step. Ten rows plus the trailing `… +N lines` notice keep
/// the transcript scannable; the complete capture stays in the `Ctrl+T` session
/// viewer and the spool file, and the model still receives the full result.
const EXEC_SESSION_OUTPUT_MAX_LINES: usize = 10;

/// Size threshold (bytes) at which to skip preview entirely
const EXTREME_OUTPUT_THRESHOLD_MB: usize = 2_000_000;
/// Size threshold (bytes) for using new large output handler with hashed directories
const LARGE_OUTPUT_NOTIFICATION_THRESHOLD: usize = 50_000; // 50KB — triggers spool-to-file for UI

use std::borrow::Cow;

use anstyle::{AnsiColor, Effects, Reset, Style as AnsiStyle};
use anyhow::Result;
use smallvec::SmallVec;
use vtcode_commons::preview::{
    display_width, excerpt_text_lines, format_hidden_lines_summary as shared_hidden_lines_summary,
    truncate_with_ellipsis,
};
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::tools::tool_intent;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_diff::{
    DiffDisplayKind, DiffDisplayLine, SideBySideRow, bounded_display_lines, diff_display_line_number_width,
    diff_gutter_fits, diff_gutter_width, diff_side_by_side_fits, display_lines_from_unified_diff, side_by_side_rows,
};

use super::files::colorize_diff_summary_line;
use super::styles::{GitStyles, LsStyles, select_line_style};
use vtcode_commons::diff_paths::{is_prose_language_hint, parse_diff_git_path, parse_diff_marker_path};

#[path = "../streams_helpers.rs"]
mod streams_helpers;
pub(crate) use streams_helpers::{
    build_markdown_code_block, render_code_fence_blocks, resolve_stdout_tail_limit, strip_ansi_codes,
};
use streams_helpers::{
    looks_like_diff_content, select_stream_lines_streaming, should_render_as_code_block, spool_output_if_needed,
};

mod diff_blocks;
mod diff_format;
mod diff_paths;
mod notices;
mod preview;
mod stream_section;

#[cfg(test)]
pub(crate) use diff_blocks::format_side_by_side_row_ansi;
pub(crate) use diff_blocks::{
    render_diff_content_block, render_diff_content_block_with_language, select_line_style_for_kind,
};
#[cfg(test)]
pub(crate) use diff_format::format_diff_line_with_gutter_and_syntax;
#[cfg(test)]
pub(crate) use diff_format::syntax_segments_for_diff_body;
pub(crate) use diff_format::{
    diff_display_text, format_diff_line_with_gutter_and_syntax_to_width, highlight_diff_body_with_syntax,
    highlight_diff_content, select_render_line_style, semantic_diff_line_style, should_show_diff_gutter,
};
pub(crate) use diff_paths::{
    diff_language_hint_from_content, is_generic_diff_review_label, is_pretruncated_diff_content,
    language_hint_for_display_line, line_exceeds_wrap_safety_cap, parse_omitted_line_count, resolve_diff_review_path,
};
pub(crate) use notices::{HiddenLinesNoticeKind, hidden_lines_notice, hidden_lines_notice_with};
#[cfg(test)]
pub(crate) use preview::collect_run_command_preview;
pub(crate) use preview::{render_preview_line, render_run_command_preview, trim_to_tail};
#[cfg(test)]
pub(crate) use stream_section::is_exec_session_tool;
pub(crate) use stream_section::render_stream_section;

#[cfg(test)]
mod tests;
