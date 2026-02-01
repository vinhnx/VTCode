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
use std::io::Write as IoWrite;
use std::path::PathBuf;

use anyhow::{Context, Result};
use smallvec::SmallVec;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::defaults;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::files::truncate_text_safe;
use super::large_output::{LargeOutputConfig, spool_large_output};
use super::styles::{GitStyles, LsStyles, select_line_style};
use crate::agent::runloop::text_tools::CodeFenceBlock;

/// Maximum number of lines to display in inline mode before truncating
const INLINE_STREAM_MAX_LINES: usize = 30;
/// Maximum line length before truncation to prevent TUI hang
const MAX_LINE_LENGTH: usize = 150;
/// Size threshold (bytes) below which output is displayed inline vs. spooled
const DEFAULT_SPOOL_THRESHOLD: usize = 4_000; // 4KB for token efficiency
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
const LARGE_OUTPUT_NOTIFICATION_THRESHOLD: usize = 4_000; // 4KB for token efficiency

/// Determine preview line count based on content size
fn calculate_preview_lines(content_size: usize) -> usize {
    match content_size {
        size if size > LARGE_OUTPUT_THRESHOLD_MB => 3,
        size if size > VERY_LARGE_OUTPUT_THRESHOLD_MB => 5,
        _ => 10,
    }
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
    config: Option<&VTCodeConfig>,
) -> Result<()> {
    use std::fmt::Write as FmtWrite;

    let is_run_command = matches!(
        tool_name,
        Some(vtcode_core::config::constants::tools::RUN_PTY_CMD)
    );
    let allow_ansi_for_tool = allow_ansi && !is_run_command;
    let apply_line_styles = !is_run_command;
    let force_tail_mode = is_run_command;
    let normalized_content = if allow_ansi_for_tool {
        Cow::Borrowed(content)
    } else {
        strip_ansi_codes(content)
    };

    // Token budget logic removed - use normalized content as-is
    let effective_normalized_content = normalized_content.clone();
    let was_truncated_by_tokens = false;

    if let Some(tool) = tool_name
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
                    renderer.line_with_style(style, &msg_buffer)?;
                    continue;
                }
                renderer.line(fallback_style, &msg_buffer)?;
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
        let (tail, total) = tail_lines_streaming(normalized_content.as_ref(), tail_limit);
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
                renderer.line_with_style(style, &display_buffer)?;
                continue;
            }
            renderer.line(fallback_style, &display_buffer)?;
        }
    }

    Ok(())
}

pub(crate) fn render_code_fence_blocks(
    renderer: &mut AnsiRenderer,
    blocks: &[CodeFenceBlock],
) -> Result<()> {
    for (index, block) in blocks.iter().enumerate() {
        if block.lines.is_empty() {
            renderer.line(MessageStyle::ToolDetail, "(no content)")?;
        } else {
            let total_lines = block.lines.len();
            let truncated = total_lines > MAX_CODE_LINES;
            let display_lines = if truncated {
                &block.lines[..MAX_CODE_LINES]
            } else {
                &block.lines[..]
            };

            let lang = block.language.as_deref().unwrap_or("");
            let mut markdown = format!("```{lang}\n");
            for line in display_lines {
                markdown.push_str(line);
                markdown.push('\n');
            }
            markdown.push_str("```");

            renderer.render_markdown_output(MessageStyle::ToolDetail, &markdown)?;

            if truncated {
                renderer.line(
                    MessageStyle::ToolDetail,
                    &format!(
                        "... ({} more lines truncated, view full output in tool logs)",
                        total_lines - MAX_CODE_LINES
                    ),
                )?;
            }
        }

        if index + 1 < blocks.len() {
            renderer.line(MessageStyle::ToolDetail, "")?;
        }
    }

    Ok(())
}

fn should_render_as_code_block(style: MessageStyle) -> bool {
    matches!(style, MessageStyle::ToolOutput | MessageStyle::Output)
}

fn build_markdown_code_block(lines: &[&str]) -> String {
    let mut markdown = String::with_capacity(lines.len() * 80 + 16);
    markdown.push_str("```\n");
    for line in lines {
        let display_line = if line.len() > MAX_LINE_LENGTH {
            let truncated = truncate_text_safe(line, MAX_LINE_LENGTH);
            Cow::Owned(format!("{}...", truncated))
        } else {
            Cow::Borrowed(*line)
        };
        markdown.push_str(&display_line);
        markdown.push('\n');
    }
    markdown.push_str("```");
    markdown
}

pub(crate) fn resolve_stdout_tail_limit(config: Option<&VTCodeConfig>) -> usize {
    config
        .map(|cfg| {
            if cfg.ui.tool_output_max_lines > 0 {
                cfg.ui.tool_output_max_lines
            } else {
                cfg.pty.stdout_tail_lines
            }
        })
        .filter(|&lines| lines > 0)
        .unwrap_or(defaults::DEFAULT_PTY_STDOUT_TAIL_LINES)
}

/// Spool large output to a file with improved directory structure and notifications
///
/// For outputs exceeding the threshold, saves to `~/.vtcode/tmp/<session_hash>/call_<id>.output`
/// and returns the path. The session hash groups related outputs together for easier cleanup.
///
/// This function also supports the legacy spool directory for backwards compatibility.
pub(crate) async fn spool_output_if_needed(
    content: &str,
    tool_name: &str,
    config: Option<&VTCodeConfig>,
) -> Result<Option<PathBuf>> {
    let threshold = config
        .map(|cfg| cfg.ui.tool_output_spool_bytes)
        .unwrap_or(DEFAULT_SPOOL_THRESHOLD);

    if content.len() < threshold {
        return Ok(None);
    }

    // For very large outputs, use the new large output handler with hashed directories
    // This provides cleaner notifications and better organization
    if content.len() >= LARGE_OUTPUT_NOTIFICATION_THRESHOLD {
        let large_output_config =
            LargeOutputConfig::default().with_threshold(LARGE_OUTPUT_NOTIFICATION_THRESHOLD);

        if let Ok(Some(result)) = spool_large_output(content, tool_name, &large_output_config) {
            return Ok(Some(result.file_path));
        }
    }

    // Fall back to legacy spool directory for smaller outputs or if new handler fails
    let spool_dir = config
        .and_then(|cfg| cfg.ui.tool_output_spool_dir.as_deref())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".vtcode/tool-output"));

    let content_owned = content.to_string();
    let tool_name_owned = tool_name.to_string();
    let spool_dir_clone = spool_dir.clone();

    // Run blocking write in the tokio blocking pool since callers are usually async.
    let join_result = tokio::task::spawn_blocking(move || -> Result<PathBuf> {
        std::fs::create_dir_all(&spool_dir_clone).with_context(|| {
            format!(
                "Failed to create spool directory: {}",
                spool_dir_clone.display()
            )
        })?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let filename = format!("{}-{}.log", tool_name_owned.replace('/', "-"), timestamp);
        let log_path = spool_dir_clone.join(filename);

        let mut file = std::fs::File::create(&log_path)
            .with_context(|| format!("Failed to create spool file: {}", log_path.display()))?;
        file.write_all(content_owned.as_bytes())
            .with_context(|| format!("Failed to write to spool file: {}", log_path.display()))?;

        Ok(log_path)
    })
    .await
    .map_err(|_| anyhow::anyhow!("Spool thread panicked"))?;

    // join_result is inner Result<PathBuf, anyhow::Error>
    let path = join_result?;
    Ok(Some(path))
}

/// Spool large output with full SpoolResult for agent access
///
/// Returns the SpoolResult which is the SOURCE OF TRUTH for large outputs.
/// The SpoolResult provides:
/// - `file_path`: Where the full output is stored
/// - `read_full_content()`: Read the entire output
/// - `read_lines(start, end)`: Read specific line ranges
/// - `get_preview()`: Get head+tail preview
/// - `to_agent_response()`: Get formatted response for agent
#[allow(dead_code)]
pub(crate) fn spool_output_with_notification(
    content: &str,
    tool_name: &str,
    session_id: Option<&str>,
) -> Result<Option<super::large_output::SpoolResult>> {
    if content.len() < LARGE_OUTPUT_NOTIFICATION_THRESHOLD {
        return Ok(None);
    }

    let config = LargeOutputConfig::default()
        .with_threshold(LARGE_OUTPUT_NOTIFICATION_THRESHOLD)
        .with_session_id(session_id.unwrap_or("default").to_string());

    spool_large_output(content, tool_name, &config)
}

pub(crate) fn tail_lines_streaming<'a>(
    text: &'a str,
    limit: usize,
) -> (SmallVec<[&'a str; 32]>, usize) {
    if text.is_empty() {
        return (SmallVec::new(), 0);
    }
    if limit == 0 {
        return (SmallVec::new(), text.lines().count());
    }

    let mut buffer: SmallVec<[&'a str; 32]> = SmallVec::with_capacity(limit);
    let mut total = 0usize;
    let mut write_idx = 0usize;

    for line in text.lines() {
        if buffer.len() < limit {
            buffer.push(line);
        } else {
            buffer[write_idx] = line;
            write_idx = (write_idx + 1) % limit;
        }
        total += 1;
    }

    if total > limit {
        buffer.rotate_left(write_idx);
    }

    (buffer, total)
}

pub(crate) fn select_stream_lines_streaming(
    content: &str,
    mode: ToolOutputMode,
    tail_limit: usize,
    prefer_full: bool,
) -> (SmallVec<[&str; 32]>, usize, bool) {
    if content.is_empty() {
        return (SmallVec::new(), 0, false);
    }

    let effective_limit = if prefer_full || matches!(mode, ToolOutputMode::Full) {
        tail_limit.max(1000)
    } else {
        tail_limit
    };

    let (tail, total) = tail_lines_streaming(content, effective_limit);
    let truncated = total > tail.len();
    (tail, total, truncated)
}

pub(crate) fn strip_ansi_codes(input: &str) -> Cow<'_, str> {
    if !input.contains('\x1b') {
        return Cow::Borrowed(input);
    }
    Cow::Owned(vtcode_core::utils::ansi_parser::strip_ansi(input))
}

#[cfg(test)]
mod ansi_stripping_tests {
    use super::*;

    #[test]
    fn test_no_ansi_codes() {
        let input = "Plain text without codes";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "Plain text without codes");
    }

    #[test]
    fn test_simple_color_code() {
        let input =
            "warning: function \u{1b}[1;33mcheck_prompt_reference_trigger\u{1b}[0m is never used";
        let result = strip_ansi_codes(input);
        assert_eq!(
            result,
            "warning: function check_prompt_reference_trigger is never used"
        );
    }

    #[test]
    fn test_multiple_color_codes() {
        let input = "\u{1b}[0m\u{1b}[1;32m✓\u{1b}[0m Test \u{1b}[1;31mFailed\u{1b}[0m";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "✓ Test Failed");
    }

    #[test]
    fn test_cargo_check_output() {
        let input =
            "\u{1b}[0m\u{1b}[1;32m Finished\u{1b}[0m dev [unoptimized + debuginfo] target(s)";
        let result = strip_ansi_codes(input);
        assert_eq!(result, " Finished dev [unoptimized + debuginfo] target(s)");
    }

    #[test]
    fn test_bold_text() {
        let input = "\u{1b}[1mBold text\u{1b}[0m normal";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "Bold text normal");
    }

    #[test]
    fn test_rgb_color_codes() {
        // 256-color mode: \x1b[38;5;196m (red)
        let input = "Error: \u{1b}[38;5;196msomething failed\u{1b}[0m";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "Error: something failed");
    }

    #[test]
    fn test_true_color_codes() {
        // True color (24-bit): \x1b[38;2;255;0;0m (red)
        let input = "Alert: \u{1b}[38;2;255;0;0mCritical\u{1b}[0m";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "Alert: Critical");
    }

    #[test]
    fn test_cursor_movement() {
        // Cursor up: \x1b[A, Cursor down: \x1b[B, etc.
        let input = "Line1\u{1b}[ALine2";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "Line1Line2");
    }

    #[test]
    fn test_clear_screen() {
        // Clear screen: \x1b[2J
        let input = "Before\u{1b}[2JAfter";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "BeforeAfter");
    }

    #[test]
    fn test_osc_hyperlink() {
        // OSC hyperlink: \x1b]8;;http://example.com\x07text\x1b]8;;\x07
        let input = "Click \u{1b}]8;;http://example.com\u{1b}\\here\u{1b}]8;;\u{1b}\\ for more";
        let result = strip_ansi_codes(input);
        // Should preserve text but remove OSC sequences
        assert!(result.contains("here"));
        assert!(!result.contains("\u{1b}"));
    }

    #[test]
    fn test_osc_bel_terminator() {
        let input = "alert \u{1b}]9;ping\u{07}done";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "alert done");
    }

    #[test]
    fn test_csi_colon_parameters() {
        let input = "color \u{1b}[38:2:255:0:0mred\u{1b}[0m ready";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "color red ready");
    }

    #[test]
    fn test_sos_and_pm_sequences() {
        let input = "pre\u{1b}Xignored\u{1b}\\mid\u{1b}^more\u{1b}\\post";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "premidpost");
    }

    #[test]
    fn test_consecutive_codes() {
        let input = "\u{1b}[1m\u{1b}[31m\u{1b}[4mText\u{1b}[0m";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "Text");
    }

    #[test]
    fn test_incomplete_code_at_end() {
        // String ends with incomplete ANSI code (defensive)
        let input = "Text\u{1b}[";
        let result = strip_ansi_codes(input);
        // Should safely handle incomplete sequences
        assert!(result.starts_with("Text"));
    }

    #[test]
    fn test_empty_string() {
        let input = "";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_only_ansi_codes() {
        let input = "\u{1b}[31m\u{1b}[0m";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_unicode_with_ansi() {
        let input = "✓ \u{1b}[32mSuccess\u{1b}[0m ✗ \u{1b}[31mFailed\u{1b}[0m";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "✓ Success ✗ Failed");
    }

    #[test]
    fn test_newlines_preserved() {
        let input = "Line1\n\u{1b}[31mLine2\u{1b}[0m\nLine3";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "Line1\nLine2\nLine3");
    }
}

#[cfg(test)]
fn select_stream_lines(
    content: &str,
    mode: ToolOutputMode,
    tail_limit: usize,
    prefer_full: bool,
) -> (Vec<&str>, usize, bool) {
    let (lines, total, truncated) =
        select_stream_lines_streaming(content, mode, tail_limit, prefer_full);
    (lines.into_vec(), total, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_mode_truncates_when_not_inline() {
        let content = (1..=50)
            .map(|index| format!("line-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (lines, total, truncated) =
            select_stream_lines(&content, ToolOutputMode::Compact, 10, false);
        assert_eq!(total, 50);
        assert_eq!(lines.len(), 10);
        assert!(truncated);
        assert_eq!(lines.first().copied(), Some("line-41"));
    }

    #[test]
    fn inline_rendering_preserves_full_scrollback() {
        let content = (1..=30)
            .map(|index| format!("row-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (lines, total, truncated) =
            select_stream_lines(&content, ToolOutputMode::Compact, 5, true);
        assert_eq!(total, 30);
        assert_eq!(lines.len(), 30);
        assert!(!truncated);
        assert_eq!(lines.first().copied(), Some("row-1"));
        assert_eq!(lines.last().copied(), Some("row-30"));
    }
}
