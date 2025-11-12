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
//! - **Tokenizer-aware**: Uses HuggingFace tokenizers for accuracy, falls back to
//!   character-based estimation (1 token ≈ 3.5 chars)
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

use std::borrow::Cow;
use std::io::Write as IoWrite;
use std::path::PathBuf;

use anyhow::{Context, Result};
use smallvec::SmallVec;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::defaults;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::token_budget::{MAX_TOOL_RESPONSE_TOKENS, TokenBudgetManager};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::files::truncate_text_safe;
use super::panels::{PanelContentLine, clamp_panel_text, render_panel};
use super::styles::{GitStyles, LsStyles, select_line_style};
use crate::agent::runloop::text_tools::CodeFenceBlock;
use vtcode_core::core::token_constants::{
    CODE_DETECTION_THRESHOLD, CODE_HEAD_RATIO_PERCENT, CODE_INDICATOR_CHARS, LOG_HEAD_RATIO_PERCENT,
    TOKENS_PER_CHARACTER,
};

/// Maximum number of lines to display in inline mode before truncating
const INLINE_STREAM_MAX_LINES: usize = 30;
/// Maximum content width percentage for panel rendering
const MAX_CONTENT_WIDTH_PERCENT: usize = 96;
/// Maximum line length before truncation to prevent TUI hang
const MAX_LINE_LENGTH: usize = 150;
/// Size threshold (bytes) below which output is displayed inline vs. spooled
const DEFAULT_SPOOL_THRESHOLD: usize = 200_000;
/// Maximum number of lines to display in code fence blocks
const MAX_CODE_LINES: usize = 500;
/// Size threshold (bytes) at which to show minimal preview instead of full output
const LARGE_OUTPUT_THRESHOLD_MB: usize = 1_000_000;
/// Size threshold (bytes) at which to show fewer preview lines
const VERY_LARGE_OUTPUT_THRESHOLD_MB: usize = 500_000;
/// Size threshold (bytes) at which to skip preview entirely
const EXTREME_OUTPUT_THRESHOLD_MB: usize = 2_000_000;

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
        skip(renderer, content, git_styles, ls_styles, config, token_budget),
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
    token_budget: Option<&TokenBudgetManager>,
) -> Result<()> {
    use std::fmt::Write as FmtWrite;

    let is_mcp_tool = tool_name.is_some_and(|name| name.starts_with("mcp_"));
    let is_run_command = matches!(
        tool_name,
        Some(vtcode_core::config::constants::tools::RUN_COMMAND)
    );
    let force_tail_mode = is_run_command;
    let normalized_content = if allow_ansi {
        Cow::Borrowed(content)
    } else {
        strip_ansi_codes(content)
    };

    // Apply token-based truncation if TokenBudgetManager is available
    let (effective_normalized_content, was_truncated_by_tokens) = if let Some(budget) = token_budget
    {
        let (truncated_content, truncated_flag) = truncate_content_by_tokens(
            normalized_content.as_ref(),
            MAX_TOOL_RESPONSE_TOKENS,
            budget,
        )
        .await;
        (Cow::Owned(truncated_content), truncated_flag)
    } else {
        (normalized_content.clone(), false)
    };

    if let Some(tool) = tool_name
        && let Ok(Some(log_path)) =
            spool_output_if_needed(effective_normalized_content.as_ref(), tool, config)
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
            renderer.line(MessageStyle::Info, &msg_buffer)?;
            renderer.line(MessageStyle::Info, "(Preview skipped due to size)")?;
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
                uppercase_title.as_ref(),
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
        renderer.line(MessageStyle::Info, &msg_buffer)?;
        renderer.line(
            MessageStyle::Info,
            &format!("Last {} lines:", preview_lines),
        )?;

        msg_buffer.clear();
        msg_buffer.reserve(128);
        let prefix = if is_mcp_tool { "" } else { "  " };

        let hidden = total.saturating_sub(tail.len());
        if hidden > 0 {
            msg_buffer.clear();
            msg_buffer.push_str(prefix);
            msg_buffer.push_str("[... ");
            msg_buffer.push_str(&hidden.to_string());
            msg_buffer.push_str(" line");
            if hidden != 1 {
                msg_buffer.push('s');
            }
            msg_buffer.push_str(" truncated ...]");
            renderer.line(MessageStyle::Info, &msg_buffer)?;
        }

        for line in &tail {
            // Truncate very long lines to prevent TUI hang
            let display_line = if line.len() > MAX_LINE_LENGTH {
                let truncated = truncate_text_safe(line, MAX_LINE_LENGTH);
                Cow::Owned(format!("{}...", truncated))
            } else {
                Cow::Borrowed(*line)
            };

            if display_line.is_empty() {
                msg_buffer.clear();
            } else {
                msg_buffer.clear();
                msg_buffer.push_str(prefix);
                msg_buffer.push_str(&display_line);
            }
            if let Some(style) = select_line_style(tool_name, &display_line, git_styles, ls_styles)
            {
                renderer.line_with_style(style, &msg_buffer)?;
                continue;
            }
            renderer.line(fallback_style, &msg_buffer)?;
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
        let prefix = if is_mcp_tool { "" } else { "  " };
        format_buffer.clear();
        format_buffer.push_str(prefix);
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
        renderer.line(MessageStyle::Info, &format_buffer)?;
    }

    if !is_mcp_tool && !is_run_command && !title.is_empty() {
        format_buffer.clear();
        format_buffer.push('[');
        for ch in title.chars() {
            format_buffer.push(ch.to_ascii_uppercase());
        }
        format_buffer.push(']');
        renderer.line(MessageStyle::Info, &format_buffer)?;
    }

    let mut display_buffer = String::with_capacity(128);
    let prefix = if is_mcp_tool { "" } else { "  " };

    for line in &lines_vec {
        if line.is_empty() {
            display_buffer.clear();
        } else {
            display_buffer.clear();
            display_buffer.push_str(prefix);
            display_buffer.push_str(line);
        }

        if let Some(style) = select_line_style(tool_name, line, git_styles, ls_styles) {
            renderer.line_with_style(style, &display_buffer)?;
            continue;
        }
        renderer.line(fallback_style, &display_buffer)?;
    }

    Ok(())
}

pub(crate) fn render_code_fence_blocks(
    renderer: &mut AnsiRenderer,
    blocks: &[CodeFenceBlock],
) -> Result<()> {
    let content_limit = MAX_CONTENT_WIDTH_PERCENT.saturating_sub(4);
    for (index, block) in blocks.iter().enumerate() {
        let header = describe_code_fence_header(block.language.as_deref());

        let mut lines = Vec::new();

        if block.lines.is_empty() {
            lines.push(PanelContentLine::new(
                clamp_panel_text("    (no content)", content_limit),
                MessageStyle::Info,
            ));
        } else {
            lines.push(PanelContentLine::new(String::new(), MessageStyle::Response));
            // Use reasonable limit to prevent UI hang, but note that semantic content is
            // truncated at token level (25k tokens in render_stream_section)
            let total_lines = block.lines.len();
            for (idx, line) in block.lines.iter().enumerate() {
                if idx >= MAX_CODE_LINES {
                    lines.push(PanelContentLine::new(
                        format!(
                            "    ... ({} more lines truncated, view full output in tool logs)",
                            total_lines - MAX_CODE_LINES
                        ),
                        MessageStyle::Info,
                    ));
                    break;
                }
                let display = format!("    {}", line);
                lines.push(PanelContentLine::new(
                    clamp_panel_text(&display, content_limit),
                    MessageStyle::Response,
                ));
            }
        }

        render_panel(
            renderer,
            Some(clamp_panel_text(&header, content_limit)),
            lines,
            MessageStyle::Response,
        )?;

        if index + 1 < blocks.len() {
            renderer.line(MessageStyle::Response, "")?;
        }
    }

    Ok(())
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

pub(crate) fn spool_output_if_needed(
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

    let spool_dir = config
        .and_then(|cfg| cfg.ui.tool_output_spool_dir.as_deref())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".vtcode/tool-output"));

    let content_owned = content.to_string();
    let tool_name_owned = tool_name.to_string();
    let spool_dir_clone = spool_dir.clone();

    // Use spawn_blocking to avoid blocking the async runtime
    let result = std::thread::spawn(move || -> Result<PathBuf> {
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
    .join()
    .map_err(|_| anyhow::anyhow!("Spool thread panicked"))?;

    result.map(Some)
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

    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    // CSI: ESC [ ... letter
                    chars.next();
                    for next in chars.by_ref() {
                        if next.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') | Some('P') | Some('^') | Some('_') => {
                    // OSC/DCS/PM/APC: ESC X ... ST (where ST = ESC \ or BEL)
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\x07' {
                            break;
                        }
                        if next == '\x1b' && matches!(chars.peek(), Some('\\')) {
                            chars.next();
                            break;
                        }
                    }
                }
                Some('(') | Some(')') | Some('*') | Some('+') => {
                    // Character set: ESC ( X, ESC ) X, etc.
                    chars.next();
                    chars.next();
                }
                Some('7') | Some('8') | Some('=') | Some('>') | Some('c') | Some('D')
                | Some('E') | Some('H') | Some('M') | Some('Z') => {
                    // Single char sequences: save/restore cursor, reset, etc.
                    chars.next();
                }
                _ => {}
            }
            continue;
        }
        output.push(ch);
    }
    Cow::Owned(output)
}

/// Truncate content by tokens, keeping head + tail to preserve context
///
/// This function implements token-aware truncation instead of naive line-based limits.
/// It preserves the most important parts of the output:
/// - Head: Shows initial state, setup, early context (40% for logs, 50% for code)
/// - Tail: Shows final state, errors, completion status (60% for logs, 50% for code)
///
/// Smart allocation: For non-code output (logs, test results), we bias toward the tail
/// (40/60 split) since errors and summaries typically appear at the end. For code blocks,
/// we use balanced allocation (50/50) since logic can be distributed throughout.
///
/// Why token-based is better than line-based:
/// 1. Tokens are what matter for LLM context window, not lines
/// 2. Preserves both beginning context AND final results
/// 3. Better for outputs where errors appear at the end (test failures, build logs)
/// 4. More robust: 100 lines of code ≈ 2k tokens, 100 lines of prose ≈ 500 tokens
///
/// Fallback: If tokenization fails, uses character-based estimation (1 token ≈ 3.5 chars)
async fn truncate_content_by_tokens(
    content: &str,
    max_tokens: usize,
    token_budget: &TokenBudgetManager,
) -> (String, bool) {
    // Count total tokens in content
    let total_tokens = match token_budget.count_tokens(content).await {
        Ok(count) => count,
        Err(_) => {
            // If tokenization fails, fall back to character-based estimation
            // More conservative than naive 1:4 ratio to account for punctuation
            (content.len() as f64 / TOKENS_PER_CHARACTER).ceil() as usize
        }
    };

    if total_tokens <= max_tokens {
        return (content.to_owned(), false);
    }

    // Calculate how many tokens to take from head and tail
    // Smart ratio: bias toward tail since most important info (errors, final state)
    // appears at the end. Use LOG_HEAD_RATIO_PERCENT split for logs.
    // Exception: if content is code, use CODE_HEAD_RATIO_PERCENT since logic can be anywhere.
    // Detect code by checking for high density of brackets/operators
    let char_count = content.len();
    let bracket_count: usize = content
        .chars()
        .filter(|c| CODE_INDICATOR_CHARS.contains(*c))
        .count();
    let is_code = bracket_count > (char_count / CODE_DETECTION_THRESHOLD);
    let head_ratio = if is_code {
        CODE_HEAD_RATIO_PERCENT // Code: keep balanced context
    } else {
        LOG_HEAD_RATIO_PERCENT // Logs/output: bias toward errors at end
    };

    let head_tokens = (max_tokens * head_ratio) / 100;
    let tail_tokens = max_tokens - head_tokens;

    // Split content into lines to process token by token
    let lines: Vec<&str> = content.lines().collect();

    // For head: collect lines until we reach the token limit
    // Use fast fallback estimation for most lines to avoid async overhead
    let mut head_lines = Vec::new();
    let mut current_tokens = 0;

    for line in &lines {
        if current_tokens >= head_tokens {
            break;
        }

        // Fast path: estimate tokens using character count (avoids async call)
        let line_tokens = (line.len() as f64 / TOKENS_PER_CHARACTER).ceil() as usize;
        if current_tokens + line_tokens <= head_tokens || head_lines.is_empty() {
            head_lines.push(line);
            current_tokens += line_tokens;
        } else {
            break;
        }
    }

    let head_line_idx = head_lines.len();
    let head_content = if head_lines.is_empty() {
        String::new()
    } else {
        // Pre-allocate based on estimated content size
        let estimated_size = head_lines.iter().map(|l| l.len() + 1).sum::<usize>();
        let mut buf = String::with_capacity(estimated_size);
        for line in head_lines {
            buf.push_str(line);
            buf.push('\n');
        }
        buf
    };

    // For tail: collect lines from the end until we reach the token limit
    // Use fast fallback estimation to avoid async overhead
    let mut tail_lines = Vec::new();
    let mut current_tokens = 0;
    let mut tail_start_idx = lines.len();

    for line in lines.iter().rev() {
        if current_tokens >= tail_tokens {
            break;
        }

        // Fast path: estimate tokens using character count (avoids async call)
        let line_tokens = (line.len() as f64 / TOKENS_PER_CHARACTER).ceil() as usize;
        if current_tokens + line_tokens <= tail_tokens || tail_lines.is_empty() {
            tail_lines.push(*line);
            current_tokens += line_tokens;
            tail_start_idx -= 1;
        } else {
            break;
        }
    }

    // Reverse tail_lines to restore original order
    tail_lines.reverse();
    let tail_content = if tail_lines.is_empty() {
        String::new()
    } else {
        tail_lines.join("\n")
    };

    // Combine head and tail
    if tail_start_idx > head_line_idx {
        // No overlap, safe to combine
        let truncated_lines = tail_start_idx - head_line_idx;
        if tail_start_idx < lines.len() {
            // Pre-calculate result size
            let truncation_msg = format!("[... {} lines truncated ...]\n", truncated_lines);
            let result_size = head_content.len() + 1 + truncation_msg.len() + tail_content.len();
            let mut result = String::with_capacity(result_size);
            result.push_str(head_content.trim_end());
            result.push('\n');
            result.push_str(&truncation_msg);
            result.push_str(&tail_content);
            (result.trim_end().to_string(), true)
        } else {
            (head_content.trim_end().to_string(), true)
        }
    } else {
        // Overlap, just return head
        (head_content.trim_end().to_string(), true)
    }
}

fn describe_code_fence_header(language: Option<&str>) -> String {
    let Some(lang) = language
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return "Code block".to_string();
    };

    let lower = lang.to_ascii_lowercase();
    match lower.as_str() {
        "sh" | "bash" | "zsh" | "shell" | "pwsh" | "powershell" | "cmd" | "batch" | "bat" => {
            format!("Shell ({lower})")
        }
        _ => {
            let mut chars = lower.chars();
            let Some(first) = chars.next() else {
                return "Code block".to_string();
            };
            let mut label = first.to_uppercase().collect::<String>();
            label.extend(chars);
            format!("{label} block")
        }
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

    #[test]
    fn describes_shell_code_fence_as_shell_header() {
        let header = describe_code_fence_header(Some("bash"));
        assert_eq!(header, "Shell (bash)");

        let rust_header = describe_code_fence_header(Some("rust"));
        assert_eq!(rust_header, "Rust block");

        let empty_header = describe_code_fence_header(None);
        assert_eq!(empty_header, "Code block");
    }
}
