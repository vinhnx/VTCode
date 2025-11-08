use std::borrow::Cow;
use std::io::Write as IoWrite;
use std::path::PathBuf;

use anyhow::{Context, Result};
use smallvec::SmallVec;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::defaults;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::panels::{PanelContentLine, clamp_panel_text, render_panel};
use super::styles::{GitStyles, LsStyles, select_line_style};
use crate::agent::runloop::text_tools::CodeFenceBlock;

const INLINE_STREAM_MAX_LINES: usize = 30;

#[cfg_attr(
    feature = "profiling",
    tracing::instrument(
        skip(renderer, content, git_styles, ls_styles, config),
        level = "debug"
    )
)]
pub(crate) fn render_stream_section(
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

    if let Some(tool) = tool_name
        && let Ok(Some(log_path)) =
            spool_output_if_needed(normalized_content.as_ref(), tool, config)
    {
        // For very large output, show minimal preview to avoid TUI hang
        let preview_lines = if content.len() > 1_000_000 {
            3
        } else if content.len() > 500_000 {
            5
        } else {
            10
        };

        // Skip preview entirely for extremely large output
        if content.len() > 2_000_000 {
            let mut msg_buffer = String::with_capacity(256);
            let _ = write!(
                &mut msg_buffer,
                "Command output too large ({} bytes), spooled to: {}",
                content.len(),
                log_path.display()
            );
            renderer.line(MessageStyle::Info, &msg_buffer)?;
            renderer.line(MessageStyle::Info, "(Preview skipped due to size)")?;
            return Ok(());
        }

        let (tail, total) = tail_lines_streaming(normalized_content.as_ref(), preview_lines);

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
                content.len(),
                total,
                log_path.display()
            );
        } else {
            let _ = write!(
                &mut msg_buffer,
                "Command output too large ({} bytes, {} lines), spooled to: {}",
                content.len(),
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

        const MAX_LINE_LENGTH: usize = 150;
        for line in &tail {
            // Truncate very long lines to prevent TUI hang
            let display_line = if line.len() > MAX_LINE_LENGTH {
                // Fast byte-based truncation for ASCII-heavy content
                let truncate_at = line.len().min(MAX_LINE_LENGTH);
                let safe_truncate = if line.is_char_boundary(truncate_at) {
                    truncate_at
                } else {
                    // Find previous char boundary
                    (0..truncate_at)
                        .rev()
                        .find(|&i| line.is_char_boundary(i))
                        .unwrap_or(0)
                };
                Cow::Owned(format!("{}...", &line[..safe_truncate]))
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

    let (lines_vec, total, truncated_flag) = if force_tail_mode {
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

    let truncated = truncated_flag;

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
        format_buffer.push_str("[... ");
        format_buffer.push_str(&hidden.to_string());
        format_buffer.push_str(" line");
        if hidden != 1 {
            format_buffer.push('s');
        }
        format_buffer.push_str(" truncated ...]");
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
    const MAX_CONTENT_WIDTH: usize = 96;
    let content_limit = MAX_CONTENT_WIDTH.saturating_sub(4);
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
            const MAX_CODE_LINES: usize = 200;
            let total_lines = block.lines.len();
            for (idx, line) in block.lines.iter().enumerate() {
                if idx >= MAX_CODE_LINES {
                    lines.push(PanelContentLine::new(
                        format!(
                            "    ... ({} more lines truncated)",
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
        .unwrap_or(200_000);

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
