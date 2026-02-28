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
use std::path::Path;

use anstyle::{AnsiColor, Effects, Reset, Style as AnsiStyle};
use anyhow::Result;
use smallvec::SmallVec;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::ui::markdown;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::files::{
    colorize_diff_summary_line, format_diff_content_lines_with_numbers, truncate_text_safe,
};
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
/// Number of head lines to show for run-command output previews
const RUN_COMMAND_HEAD_PREVIEW_LINES: usize = 3;
/// Number of tail lines to show for run-command output previews
const RUN_COMMAND_TAIL_PREVIEW_LINES: usize = 3;
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

fn parse_diff_git_path(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    if parts.next()? != "diff" || parts.next()? != "--git" {
        return None;
    }
    let _old = parts.next()?;
    let new_path = parts.next()?;
    Some(new_path.trim_start_matches("b/").to_string())
}

fn parse_diff_marker_path(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !(trimmed.starts_with("--- ") || trimmed.starts_with("+++ ")) {
        return None;
    }
    let path = trimmed.split_whitespace().nth(1)?;
    if path == "/dev/null" {
        return None;
    }
    Some(
        path.trim_start_matches("a/")
            .trim_start_matches("b/")
            .to_string(),
    )
}

fn language_hint_from_path(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty())
        .map(|ext| ext.to_ascii_lowercase())
}

fn split_numbered_diff_line(line: &str) -> Option<(char, &str, &str)> {
    let mut chars = line.char_indices();
    let (_, marker) = chars.next()?;
    if !matches!(marker, '+' | '-' | ' ') {
        return None;
    }

    let rest = line.get(1..)?;
    let first_digit = rest.find(|c: char| c.is_ascii_digit())?;
    if first_digit == 0 || !rest[..first_digit].chars().all(|c| c == ' ') {
        return None;
    }

    let digits = &rest[first_digit..];
    let digits_len = digits.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits_len == 0 {
        return None;
    }

    let after_digits = first_digit + digits_len;
    let separator = rest[after_digits..].chars().next()?;
    if separator != ' ' {
        return None;
    }

    let split_at = 1 + after_digits + separator.len_utf8();
    let line_no = rest[..after_digits].trim();
    if line_no.is_empty() {
        return None;
    }
    let content = line.get(split_at..)?;
    Some((marker, line_no, content))
}

fn highlight_diff_content(
    content: &str,
    language_hint: Option<&str>,
    bg: Option<anstyle::Color>,
) -> Option<String> {
    let leading_ws_len = content
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, _)| idx)
        .unwrap_or(content.len());
    let (leading_ws, code_content) = content.split_at(leading_ws_len);

    let segments = markdown::highlight_line_for_diff(code_content, language_hint)?;
    if segments.is_empty() {
        return None;
    }

    let mut out = String::with_capacity(content.len() + 16);
    if !leading_ws.is_empty() {
        out.push_str(leading_ws);
    }
    for (style, text) in segments {
        if text.is_empty() {
            continue;
        }
        let mut token_style = style;
        if token_style.get_bg_color().is_none() && bg.is_some() {
            token_style = token_style.bg_color(bg);
        }
        out.push_str(&token_style.render().to_string());
        out.push_str(&text);
        out.push_str(&Reset.to_string());
    }
    if out.is_empty() { None } else { Some(out) }
}

fn numbered_diff_line_width(lines: &[String]) -> usize {
    let max_digits = lines
        .iter()
        .filter_map(|line| split_numbered_diff_line(line).map(|(_, line_no, _)| line_no.len()))
        .max()
        .unwrap_or(4);
    max_digits.clamp(4, 6)
}

fn format_diff_line_with_gutter_and_syntax(
    line: &str,
    base_style: Option<AnsiStyle>,
    language_hint: Option<&str>,
    line_number_width: usize,
) -> String {
    let Some((marker, line_no, content)) = split_numbered_diff_line(line) else {
        return line.to_string();
    };

    let marker_text = match marker {
        '+' => "+",
        '-' => "-",
        _ => " ",
    };
    let bg = base_style.and_then(|style| style.get_bg_color());
    let marker_style = match marker {
        '+' => AnsiStyle::new()
            .fg_color(Some(anstyle::Color::Ansi(AnsiColor::BrightGreen)))
            .bg_color(bg)
            .effects(Effects::BOLD),
        '-' => AnsiStyle::new()
            .fg_color(Some(anstyle::Color::Ansi(AnsiColor::BrightRed)))
            .bg_color(bg)
            .effects(Effects::BOLD),
        _ => AnsiStyle::new()
            .fg_color(Some(anstyle::Color::Ansi(AnsiColor::BrightBlack)))
            .effects(Effects::DIMMED),
    };
    let gutter_style = match marker {
        '+' | '-' => AnsiStyle::new()
            .fg_color(Some(anstyle::Color::Ansi(AnsiColor::BrightBlack)))
            .bg_color(bg)
            .effects(Effects::BOLD),
        _ => AnsiStyle::new()
            .fg_color(Some(anstyle::Color::Ansi(AnsiColor::BrightBlack)))
            .effects(Effects::DIMMED),
    };
    let reset = anstyle::Reset;
    let mut out = String::with_capacity(line.len() + 32);
    out.push_str(&marker_style.render().to_string());
    out.push_str(marker_text);
    out.push(' ');
    out.push_str(&gutter_style.render().to_string());
    out.push_str(&format!("{line_no:>line_number_width$} "));
    if let Some(highlighted) = highlight_diff_content(content, language_hint, bg) {
        out.push_str(&highlighted);
    } else {
        if let Some(style) = base_style {
            out.push_str(&style.render().to_string());
            out.push_str(content);
        } else {
            out.push_str(content);
        }
    }
    out.push_str(&reset.to_string());
    out
}

fn collect_run_command_preview(content: &str) -> (SmallVec<[&str; 32]>, usize, usize) {
    let lines: SmallVec<[&str; 32]> = content.lines().collect();
    let total = lines.len();
    let preview_size = RUN_COMMAND_HEAD_PREVIEW_LINES + RUN_COMMAND_TAIL_PREVIEW_LINES;

    if total <= preview_size {
        return (lines, total, 0);
    }

    let mut preview: SmallVec<[&str; 32]> = SmallVec::with_capacity(preview_size);
    preview.extend_from_slice(&lines[..RUN_COMMAND_HEAD_PREVIEW_LINES]);
    preview.extend_from_slice(&lines[total - RUN_COMMAND_TAIL_PREVIEW_LINES..]);
    let hidden = total.saturating_sub(preview.len());

    (preview, total, hidden)
}

async fn render_run_command_preview(
    renderer: &mut AnsiRenderer,
    content: &str,
    tool_name: Option<&str>,
    fallback_style: MessageStyle,
    disable_spool: bool,
    config: Option<&VTCodeConfig>,
) -> Result<()> {
    let run_tool_name = tool_name.unwrap_or(vtcode_core::config::constants::tools::RUN_PTY_CMD);
    if !disable_spool
        && let Ok(Some(log_path)) = spool_output_if_needed(content, run_tool_name, config).await
    {
        let total = content.lines().count();
        renderer.line(
            MessageStyle::ToolDetail,
            &format!(
                "Command output too large ({} bytes, {} lines), spooled to: {}",
                content.len(),
                total,
                log_path.display()
            ),
        )?;
    }

    let (preview_lines, _total, hidden) = collect_run_command_preview(content);
    if preview_lines.is_empty() {
        return Ok(());
    }

    let mut display_buffer = String::with_capacity(192);
    let mut prefixed_line = String::with_capacity(200);
    for (idx, line) in preview_lines.iter().enumerate() {
        if hidden > 0 && idx == RUN_COMMAND_HEAD_PREVIEW_LINES {
            prefixed_line.clear();
            prefixed_line.push_str("    ");
            prefixed_line.push_str(&format_hidden_lines_summary(hidden));
            renderer.line(MessageStyle::ToolDetail, &prefixed_line)?;
        }

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

        prefixed_line.clear();
        if idx == 0 {
            prefixed_line.push_str("└ ");
        } else {
            prefixed_line.push_str("  ");
        }
        prefixed_line.push_str(&display_buffer);

        renderer.line_with_override_style(
            fallback_style,
            fallback_style.style(),
            &prefixed_line,
        )?;
    }

    Ok(())
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
    let allow_ansi_for_tool = allow_ansi && !is_run_command;
    let apply_line_styles = !is_run_command;
    let stripped_for_diff = strip_ansi_codes(content);
    let is_diff_content = apply_line_styles && looks_like_diff_content(stripped_for_diff.as_ref());
    let normalized_content = if allow_ansi_for_tool {
        Cow::Borrowed(content)
    } else {
        strip_ansi_codes(content)
    };

    if is_run_command {
        return render_run_command_preview(
            renderer,
            normalized_content.as_ref(),
            tool_name,
            fallback_style,
            disable_spool,
            config,
        )
        .await;
    }

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
        let diff_lines = format_diff_content_lines_with_numbers(stripped_for_diff.as_ref());
        let total = diff_lines.len();
        let effective_limit =
            if renderer.prefers_untruncated_output() || matches!(mode, ToolOutputMode::Full) {
                tail_limit.max(1000)
            } else {
                tail_limit
            };
        let (lines_slice, truncated) = if total > effective_limit {
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

        let mut display_buffer = String::with_capacity(256);
        let mut current_language_hint: Option<String> = None;
        let line_number_width = numbered_diff_line_width(lines_slice);
        for line in lines_slice {
            if line.is_empty() {
                continue;
            }
            if let Some(path) = parse_diff_git_path(line).or_else(|| parse_diff_marker_path(line)) {
                current_language_hint = language_hint_from_path(&path);
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

            let line_style = select_line_style(tool_name, &display_buffer, git_styles, ls_styles);
            let rendered = format_diff_line_with_gutter_and_syntax(
                &display_buffer,
                line_style,
                current_language_hint.as_deref(),
                line_number_width,
            );
            if rendered != display_buffer {
                let style = line_style.unwrap_or(fallback_style.style());
                renderer.line_with_override_style(fallback_style, style, &rendered)?;
                continue;
            }
            if let Some(style) = line_style {
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

#[cfg(test)]
mod tests {
    use anstyle::AnsiColor;

    use super::{
        collect_run_command_preview, format_diff_line_with_gutter_and_syntax,
        language_hint_from_path, numbered_diff_line_width, split_numbered_diff_line,
        strip_ansi_codes,
    };

    #[test]
    fn run_command_preview_uses_head_tail_three_lines() {
        let content = "l1\nl2\nl3\nl4\nl5\nl6\nl7\n";
        let (preview, total, hidden) = collect_run_command_preview(content);
        assert_eq!(total, 7);
        assert_eq!(hidden, 1);
        assert_eq!(preview.as_slice(), ["l1", "l2", "l3", "l5", "l6", "l7"]);
    }

    #[test]
    fn run_command_preview_keeps_short_output_unmodified() {
        let content = "l1\nl2\nl3\n";
        let (preview, total, hidden) = collect_run_command_preview(content);
        assert_eq!(total, 3);
        assert_eq!(hidden, 0);
        assert_eq!(preview.as_slice(), ["l1", "l2", "l3"]);
    }

    #[test]
    fn split_numbered_diff_line_parses_gutter() {
        let (marker, line_no, content) =
            split_numbered_diff_line("+  1377 syntax_highlight::foo").expect("expected gutter");
        assert_eq!(marker, '+');
        assert_eq!(line_no, "1377");
        assert_eq!(content, "syntax_highlight::foo");
    }

    #[test]
    fn split_numbered_diff_line_preserves_code_indentation() {
        let (_, _, content) =
            split_numbered_diff_line("+  1384     line,").expect("expected gutter");
        assert_eq!(content, "    line,");
    }

    #[test]
    fn format_diff_line_styles_gutter_for_additions() {
        let style = anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green)));
        let rendered =
            format_diff_line_with_gutter_and_syntax("+  1377 let x = 1;", Some(style), None, 5);
        assert!(rendered.contains("\u{1b}["));
        let stripped = strip_ansi_codes(&rendered);
        assert!(stripped.contains("+  1377 "));
        assert!(stripped.contains("let x = 1;"));
    }

    #[test]
    fn numbered_diff_line_width_uses_max_digits() {
        let lines = vec![
            "+    99 let a = 1;".to_string(),
            "  10420 let b = 2;".to_string(),
        ];
        assert_eq!(numbered_diff_line_width(&lines), 5);
    }

    #[test]
    fn language_hint_from_path_extracts_extension() {
        assert_eq!(
            language_hint_from_path("vtcode-tui/src/ui/markdown.rs").as_deref(),
            Some("rs")
        );
        assert_eq!(language_hint_from_path("Makefile"), None);
    }
}
