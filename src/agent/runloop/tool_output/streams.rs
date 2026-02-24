#![allow(clippy::too_many_arguments)]
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
//! - `MAX_CODE_LINES: 500`: For code fence blocks (still truncated by tokens upstream)
//!
//! Full output is spooled to `.vtcode/tool-output/` for later review.
//! For very large outputs, files are saved to `~/.vtcode/tmp/<session_hash>/call_<id>.output`
//! with a notification displayed to the client.

use std::borrow::Cow;

use anyhow::Result;
use smallvec::SmallVec;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::files::{colorize_diff_summary_line, format_diff_content_lines, truncate_text_safe};
use super::styles::{GitStyles, LsStyles, select_line_style};
#[path = "streams_helpers.rs"]
mod streams_helpers;
use streams_helpers::{
    build_markdown_code_block, looks_like_diff_content, select_stream_lines_streaming,
    should_render_as_code_block, spool_output_if_needed, tail_lines_streaming,
};
pub(crate) use streams_helpers::{
    render_code_fence_blocks, resolve_stdout_tail_limit, spool_output_with_notification,
    strip_ansi_codes,
};

/// Maximum number of lines to display in inline mode before truncating
const INLINE_STREAM_MAX_LINES: usize = 30;
/// Maximum number of lines for run-command output in TUI
const RUN_COMMAND_MAX_LINES: usize = 12;
/// Maximum line length before truncation to prevent TUI hang
const MAX_LINE_LENGTH: usize = 150;
/// Size threshold (bytes) below which output is displayed inline vs. spooled
const DEFAULT_SPOOL_THRESHOLD: usize = 50_000; // 50KB — UI render truncation
/// Maximum number of lines to display in code fence blocks
const MAX_CODE_LINES: usize = 500;
/// Preview window lines used by large-output previewing (kept local)
#[allow(dead_code)]
const PREVIEW_HEAD_LINES: usize = 20;
/// Preview tail lines used by large-output previewing (kept local)
#[allow(dead_code)]
const PREVIEW_TAIL_LINES: usize = 10;
/// Size threshold (bytes) at which to show minimal preview instead of full output
const LARGE_OUTPUT_THRESHOLD_MB: usize = 1_000_000;
/// Size threshold (bytes) at which to show fewer preview lines
const VERY_LARGE_OUTPUT_THRESHOLD_MB: usize = 500_000;
/// Size threshold (bytes) at which to skip preview entirely
const EXTREME_OUTPUT_THRESHOLD_MB: usize = 2_000_000;
/// Size threshold (bytes) for using new large output handler with hashed directories
const LARGE_OUTPUT_NOTIFICATION_THRESHOLD: usize = 50_000; // 50KB — triggers spool-to-file for UI

/// Determine preview line count based on content size
fn calculate_preview_lines(content_size: usize) -> usize {
    match content_size {
        size if size > LARGE_OUTPUT_THRESHOLD_MB => 3,
        size if size > VERY_LARGE_OUTPUT_THRESHOLD_MB => 5,
        _ => 10,
    }
}

fn format_hidden_lines_summary(hidden: usize) -> String {
    if hidden == 1 {
        "… +1 line".to_string()
    } else {
        format!("… +{} lines", hidden)
    }
}

fn resolve_run_command_tail_limit(tail_limit: usize) -> usize {
    tail_limit.clamp(1, RUN_COMMAND_MAX_LINES)
}

#[cfg_attr(
    feature = "profiling",
    tracing::instrument(
        skip(renderer, content, git_styles, ls_styles, config),
        level = "debug"
    )
)]
pub(crate) async fn render_stream_section(
    renderer: &mut AnsiRenderer,
    title: &str,
    content: &str,
    mode: ToolOutputMode,
    tail_limit: usize,
    tool_name: Option<&str>,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
    fallback_style: MessageStyle,
    allow_ansi: bool,
    disable_spool: bool,
    config: Option<&VTCodeConfig>,
) -> Result<()> {
    use std::fmt::Write as FmtWrite;

    let is_run_command = matches!(
        tool_name,
        Some(vtcode_core::config::constants::tools::RUN_PTY_CMD)
            | Some(vtcode_core::config::constants::tools::UNIFIED_EXEC)
    );
    let run_command_tail_limit = resolve_run_command_tail_limit(tail_limit);
    let allow_ansi_for_tool = allow_ansi && !is_run_command;
    let apply_line_styles = !is_run_command;
    let force_tail_mode = is_run_command;
    let stripped_for_diff = strip_ansi_codes(content);
    let is_diff_content = apply_line_styles && looks_like_diff_content(stripped_for_diff.as_ref());
    let normalized_content = if allow_ansi_for_tool {
        Cow::Borrowed(content)
    } else {
        strip_ansi_codes(content)
    };

    // Token budget logic removed - use normalized content as-is
    let effective_normalized_content = normalized_content.clone();
    let was_truncated_by_tokens = false;

    if !disable_spool
        && let Some(tool) = tool_name
        && let Ok(Some(log_path)) =
            spool_output_if_needed(effective_normalized_content.as_ref(), tool, config).await
    {
        // For very large output, show minimal preview to avoid TUI hang
        let preview_lines = calculate_preview_lines(effective_normalized_content.len());

        // Skip preview entirely for extremely large output
        if effective_normalized_content.len() > EXTREME_OUTPUT_THRESHOLD_MB {
            let mut msg_buffer = String::with_capacity(256);
            let _ = write!(
                &mut msg_buffer,
                "Command output too large ({} bytes), spooled to: {}",
                effective_normalized_content.len(),
                log_path.display()
            );
            renderer.line(MessageStyle::ToolDetail, &msg_buffer)?;
            renderer.line(MessageStyle::ToolDetail, "(Preview skipped due to size)")?;
            return Ok(());
        }

        let (tail, total) =
            tail_lines_streaming(effective_normalized_content.as_ref(), preview_lines);

        let mut msg_buffer = String::with_capacity(256);
        if !is_run_command {
            let uppercase_title = if title.is_empty() {
                Cow::Borrowed("OUTPUT")
            } else {
                Cow::Owned(title.to_ascii_uppercase())
            };
            let _ = write!(
                &mut msg_buffer,
                "[{}] Output too large ({} bytes, {} lines), spooled to: {}",
                <Cow<'_, str> as AsRef<str>>::as_ref(&uppercase_title),
                effective_normalized_content.len(),
                total,
                log_path.display()
            );
        } else {
            let _ = write!(
                &mut msg_buffer,
                "Command output too large ({} bytes, {} lines), spooled to: {}",
                effective_normalized_content.len(),
                total,
                log_path.display()
            );
        }
        renderer.line(MessageStyle::ToolDetail, &msg_buffer)?;
        renderer.line(
            MessageStyle::ToolDetail,
            &format!("Last {} lines:", preview_lines),
        )?;

        msg_buffer.clear();
        msg_buffer.reserve(128);

        let hidden = total.saturating_sub(tail.len());
        if hidden > 0 {
            if is_run_command {
                renderer.line(
                    MessageStyle::ToolDetail,
                    &format_hidden_lines_summary(hidden),
                )?;
            } else {
                msg_buffer.clear();
                msg_buffer.push_str("[... ");
                msg_buffer.push_str(&hidden.to_string());
                msg_buffer.push_str(" line");
                if hidden != 1 {
                    msg_buffer.push('s');
                }
                msg_buffer.push_str(" truncated ...]");
                renderer.line(MessageStyle::ToolDetail, &msg_buffer)?;
            }
        }

        if should_render_as_code_block(fallback_style) && !apply_line_styles {
            let markdown = build_markdown_code_block(&tail);
            renderer.render_markdown_output(fallback_style, &markdown)?;
        } else {
            for line in &tail {
                // Truncate very long lines to prevent TUI hang
                let display_line = if line.len() > MAX_LINE_LENGTH {
                    let truncated = truncate_text_safe(line, MAX_LINE_LENGTH);
                    Cow::Owned(format!("{}...", truncated))
                } else {
                    Cow::Borrowed(*line)
                };

                if display_line.is_empty() {
                    // Skip empty lines to avoid extra line breaks in TUI rendering
                    continue;
                } else {
                    msg_buffer.clear();
                    msg_buffer.push_str(&display_line);
                }
                if apply_line_styles
                    && let Some(style) =
                        select_line_style(tool_name, &display_line, git_styles, ls_styles)
                {
                    renderer.line_with_override_style(fallback_style, style, &msg_buffer)?;
                    continue;
                }
                renderer.line_with_override_style(
                    fallback_style,
                    fallback_style.style(),
                    &msg_buffer,
                )?;
            }
        }
        return Ok(());
    }

    if is_diff_content {
        let diff_lines = format_diff_content_lines(stripped_for_diff.as_ref());
        let total = diff_lines.len();
        let effective_limit = if force_tail_mode {
            run_command_tail_limit
        } else if renderer.prefers_untruncated_output() || matches!(mode, ToolOutputMode::Full) {
            tail_limit.max(1000)
        } else {
            tail_limit
        };
        let (lines_slice, truncated) = if force_tail_mode || total > effective_limit {
            let start = total.saturating_sub(effective_limit);
            (&diff_lines[start..], total > effective_limit)
        } else {
            (&diff_lines[..], false)
        };

        if truncated {
            let hidden = total.saturating_sub(lines_slice.len());
            if hidden > 0 {
                if is_run_command {
                    renderer.line(
                        MessageStyle::ToolDetail,
                        &format_hidden_lines_summary(hidden),
                    )?;
                } else {
                    renderer.line(
                        MessageStyle::ToolDetail,
                        &format!("[... {} lines truncated ...]", hidden),
                    )?;
                }
            }
        }

        let mut display_buffer = String::with_capacity(128);
        for line in lines_slice {
            if line.is_empty() {
                continue;
            }
            display_buffer.clear();
            if line.len() > MAX_LINE_LENGTH {
                let truncated = truncate_text_safe(line, MAX_LINE_LENGTH);
                display_buffer.push_str(truncated);
                display_buffer.push_str("...");
            } else {
                display_buffer.push_str(line);
            }

            if let Some(summary_line) = colorize_diff_summary_line(
                &display_buffer,
                renderer.capabilities().supports_color(),
            ) {
                renderer.line_with_override_style(
                    fallback_style,
                    fallback_style.style(),
                    &summary_line,
                )?;
                continue;
            }

            if let Some(style) =
                select_line_style(tool_name, &display_buffer, git_styles, ls_styles)
            {
                renderer.line_with_override_style(fallback_style, style, &display_buffer)?;
            } else {
                renderer.line_with_override_style(
                    fallback_style,
                    fallback_style.style(),
                    &display_buffer,
                )?;
            }
        }

        return Ok(());
    }

    // If content was already token-truncated, use that content; otherwise use the original normalized content
    let (lines_vec, total, truncated_flag) = if was_truncated_by_tokens {
        // Content was already truncated by tokens, so we need to process it differently
        // Split the truncated content by lines and use that
        let lines: SmallVec<[&str; 32]> = effective_normalized_content.lines().collect();
        let total_lines = effective_normalized_content.lines().count();
        (lines, total_lines, true) // Always mark as truncated if token-based truncation was applied
    } else if force_tail_mode {
        let (tail, total) =
            tail_lines_streaming(normalized_content.as_ref(), run_command_tail_limit);
        let truncated = total > tail.len();
        (tail, total, truncated)
    } else {
        let prefer_full = renderer.prefers_untruncated_output();
        let (mut lines, total, mut truncated) = select_stream_lines_streaming(
            normalized_content.as_ref(),
            mode,
            tail_limit,
            prefer_full,
        );
        if prefer_full && lines.len() > INLINE_STREAM_MAX_LINES {
            let drop = lines.len() - INLINE_STREAM_MAX_LINES;
            lines.drain(..drop);
            truncated = true;
        }
        (lines, total, truncated)
    };

    let truncated = truncated_flag || was_truncated_by_tokens;

    if lines_vec.is_empty() {
        return Ok(());
    }

    let mut format_buffer = String::with_capacity(64);

    let hidden = if truncated {
        total.saturating_sub(lines_vec.len())
    } else {
        0
    };
    if hidden > 0 {
        format_buffer.clear();
        if was_truncated_by_tokens {
            format_buffer.push_str("[... content truncated by token budget ...]");
        } else if is_run_command {
            format_buffer.push_str(&format_hidden_lines_summary(hidden));
        } else {
            format_buffer.push_str("[... ");
            format_buffer.push_str(&hidden.to_string());
            format_buffer.push_str(" line");
            if hidden != 1 {
                format_buffer.push('s');
            }
            format_buffer.push_str(" truncated ...]");
        }
        renderer.line(MessageStyle::ToolDetail, &format_buffer)?;
    }

    let mut display_buffer = String::with_capacity(128);

    if should_render_as_code_block(fallback_style) && !apply_line_styles {
        let markdown = build_markdown_code_block(&lines_vec);
        renderer.render_markdown_output(fallback_style, &markdown)?;
    } else {
        for line in &lines_vec {
            if line.is_empty() {
                // Skip empty lines to avoid extra line breaks in TUI rendering
                continue;
            } else {
                display_buffer.clear();
                display_buffer.push_str(line);
            }

            if apply_line_styles
                && let Some(style) = select_line_style(tool_name, line, git_styles, ls_styles)
            {
                renderer.line_with_override_style(fallback_style, style, &display_buffer)?;
                continue;
            }
            renderer.line_with_override_style(
                fallback_style,
                fallback_style.style(),
                &display_buffer,
            )?;
        }
    }

    Ok(())
}
