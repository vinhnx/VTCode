use std::borrow::Cow;
use std::io::Write as IoWrite;
use std::path::PathBuf;

use anyhow::{Context, Result};
use smallvec::SmallVec;
pub(super) use vtcode_commons::diff_paths::looks_like_diff_content;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::defaults;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::ansi_codes::ESC_CHAR;
use vtcode_core::utils::file_utils::ensure_dir_exists_sync;

use super::super::files::{display_width, truncate_text_safe};
use super::super::large_output::{LargeOutputConfig, spool_large_output};
use crate::agent::runloop::text_tools::CodeFenceBlock;

pub(crate) fn render_code_fence_blocks(
    renderer: &mut AnsiRenderer,
    blocks: &[CodeFenceBlock],
) -> Result<()> {
    for (index, block) in blocks.iter().enumerate() {
        if block.lines.is_empty() {
            renderer.line(MessageStyle::ToolDetail, "(no content)")?;
        } else {
            let total_lines = block.lines.len();
            let truncated = total_lines > super::MAX_CODE_LINES;
            let display_lines = if truncated {
                &block.lines[..super::MAX_CODE_LINES]
            } else {
                &block.lines[..]
            };
            let display_lines = display_lines.iter().map(String::as_str).collect::<Vec<_>>();
            let markdown =
                build_markdown_code_block(&display_lines, block.language.as_deref(), false);

            renderer.render_markdown_output(MessageStyle::ToolDetail, &markdown)?;

            if truncated {
                renderer.line(
                    MessageStyle::ToolDetail,
                    &format!(
                        "... ({} more lines truncated, view full output in tool logs)",
                        total_lines - super::MAX_CODE_LINES
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

pub(super) fn should_render_as_code_block(style: MessageStyle) -> bool {
    matches!(style, MessageStyle::ToolOutput | MessageStyle::Output)
}

pub(crate) fn build_markdown_code_block(
    lines: &[&str],
    language: Option<&str>,
    truncate_long_lines: bool,
) -> String {
    let mut markdown = String::with_capacity(lines.len() * 80 + 16);
    markdown.push_str("```");
    markdown.push_str(language.unwrap_or(""));
    markdown.push('\n');
    for line in lines {
        let display_line = if truncate_long_lines && display_width(line) > super::MAX_LINE_LENGTH {
            let truncated = truncate_text_safe(line, super::MAX_LINE_LENGTH);
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
pub(super) async fn spool_output_if_needed(
    content: &str,
    tool_name: &str,
    config: Option<&VTCodeConfig>,
) -> Result<Option<PathBuf>> {
    let threshold = config
        .map(|cfg| cfg.ui.tool_output_spool_bytes)
        .unwrap_or(super::DEFAULT_SPOOL_THRESHOLD);

    if content.len() < threshold {
        return Ok(None);
    }

    // For very large outputs, use the new large output handler with hashed directories
    // This provides cleaner notifications and better organization
    if content.len() >= super::LARGE_OUTPUT_NOTIFICATION_THRESHOLD {
        let large_output_config =
            LargeOutputConfig::default().with_threshold(super::LARGE_OUTPUT_NOTIFICATION_THRESHOLD);

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
        ensure_dir_exists_sync(&spool_dir_clone).with_context(|| {
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

pub(super) fn tail_lines_streaming<'a>(
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

pub(super) fn select_stream_lines_streaming(
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

#[cfg(test)]
mod markdown_block_tests {
    use tokio::sync::mpsc::UnboundedReceiver;
    use vtcode_core::ui::{InlineCommand, InlineHandle};

    use super::*;

    fn collect_inline_output(receiver: &mut UnboundedReceiver<InlineCommand>) -> String {
        let mut lines: Vec<String> = Vec::new();
        while let Ok(command) = receiver.try_recv() {
            match command {
                InlineCommand::AppendLine { segments, .. } => {
                    lines.push(
                        segments
                            .into_iter()
                            .map(|segment| segment.text)
                            .collect::<String>(),
                    );
                }
                InlineCommand::ReplaceLast {
                    lines: replacement_lines,
                    ..
                } => {
                    for line in replacement_lines {
                        lines.push(
                            line.into_iter()
                                .map(|segment| segment.text)
                                .collect::<String>(),
                        );
                    }
                }
                _ => {}
            }
        }
        lines.join("\n")
    }

    #[test]
    fn build_markdown_code_block_includes_language_header() {
        let markdown = build_markdown_code_block(&["fn main() {}"], Some("rs"), false);
        assert!(markdown.starts_with("```rs\n"));
        assert!(markdown.contains("fn main() {}"));
        assert!(markdown.ends_with("```"));
    }

    #[test]
    fn build_markdown_code_block_omits_language_header_when_absent() {
        let markdown = build_markdown_code_block(&["plain"], None, false);
        assert!(markdown.starts_with("```\n"));
        assert!(!markdown.starts_with("```plain"));
    }

    #[test]
    fn build_markdown_code_block_truncates_only_when_requested() {
        let long_line = "x".repeat(super::super::MAX_LINE_LENGTH + 5);

        let untruncated = build_markdown_code_block(&[long_line.as_str()], None, false);
        assert!(untruncated.contains(&long_line));

        let truncated = build_markdown_code_block(&[long_line.as_str()], None, true);
        assert!(truncated.contains("..."));
        assert!(!truncated.contains(&long_line));
    }

    #[test]
    fn render_code_fence_blocks_keeps_truncation_notice() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer =
            AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        let lines = (0..=super::super::MAX_CODE_LINES)
            .map(|idx| format!("line-{idx}"))
            .collect::<Vec<_>>();
        let blocks = vec![CodeFenceBlock {
            language: Some("rust".to_string()),
            lines,
        }];

        render_code_fence_blocks(&mut renderer, &blocks).expect("code fence blocks should render");

        let inline_output = collect_inline_output(&mut receiver);
        assert!(inline_output.contains("line-0"));
        assert!(inline_output.contains("more lines truncated, view full output in tool logs"));
    }
}

pub(crate) fn strip_ansi_codes(input: &str) -> Cow<'_, str> {
    if !input.contains(ESC_CHAR) {
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

    #[test]
    fn diff_detector_ignores_plus_minus_plain_text() {
        let plain = "+ enabled feature flag\n- disabled old path\n";
        assert!(!looks_like_diff_content(plain));
    }

    #[test]
    fn diff_detector_accepts_real_unified_diff() {
        let diff = "diff --git a/a.rs b/a.rs\n@@ -1 +1 @@\n-old\n+new\n";
        assert!(looks_like_diff_content(diff));
    }
}
