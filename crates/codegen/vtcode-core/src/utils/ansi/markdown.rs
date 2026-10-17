//! Markdown rendering for tool output.

use super::*;

use std::io::Write;

use crate::utils::transcript;

use crate::ui::markdown::{
    MarkdownLine, MarkdownSegment, RenderMarkdownOptions, render_markdown_to_lines_with_options,
};
use crate::ui::theme;
use anstyle::Reset;
use anyhow::Result;

impl AnsiRenderer {
    /// Render markdown content with proper syntax highlighting and indentation normalization.
    /// Use this for tool output that contains markdown code blocks.
    pub fn render_markdown_output(&mut self, style: MessageStyle, text: &str) -> Result<()> {
        self.flush_compact_command_group();
        self.render_markdown(style, text)
    }

    pub(super) fn render_markdown(&mut self, style: MessageStyle, text: &str) -> Result<()> {
        if !self.should_render_style(style) {
            return Ok(());
        }
        let styles = theme::active_styles();
        let base_style = style.style();
        let indent = self.indent_for_style(style);
        let preserve_code_indentation = matches!(
            style,
            MessageStyle::Output
                | MessageStyle::ToolOutput
                | MessageStyle::ToolDetail
                | MessageStyle::Response
                | MessageStyle::Reasoning
                | MessageStyle::ReasoningEmphasis
                | MessageStyle::User
        );

        // Strip ANSI codes from agent response to prevent interference with markdown rendering
        let text_storage;
        let text = if matches!(style, MessageStyle::Response) {
            text_storage = crate::utils::ansi_parser::strip_ansi(text);
            &text_storage
        } else {
            text
        };

        if let Some(sink) = &mut self.sink {
            // Read terminal width fresh so tables adapt to resizes.
            if sink.table_max_width_override.is_none()
                && let Ok((w, _)) = crossterm::terminal::size()
            {
                sink.table_max_width = Some(w as usize);
            }
            let last_empty =
                sink.write_markdown(text, indent, base_style, Self::message_kind(style), preserve_code_indentation)?;
            self.last_line_was_empty = last_empty;
            return Ok(());
        }
        let highlight_cfg = if self.highlight_config.enabled {
            Some(&self.highlight_config)
        } else {
            None
        };
        let mut lines = render_markdown_to_lines_with_options(
            text,
            base_style,
            &styles,
            highlight_cfg,
            RenderMarkdownOptions {
                preserve_code_indentation,
                disable_code_block_table_reparse: false,
                table_max_width: terminal_table_content_width(""),
            },
        );
        if lines.is_empty() {
            lines.push(MarkdownLine::default());
        }

        // Pre-allocate buffer for markdown output if rendering many lines
        if lines.len() > 10 {
            self.buffer.reserve(lines.len() * 80);
        }

        for line in lines {
            self.write_markdown_line(style, indent, line)?;
        }
        Ok(())
    }

    pub(super) fn write_markdown_line(
        &mut self,
        style: MessageStyle,
        indent: &str,
        mut line: MarkdownLine,
    ) -> Result<()> {
        if !indent.is_empty() && !line.segments.is_empty() {
            line.segments.insert(
                0,
                MarkdownSegment {
                    style: style.style(),
                    text: indent.to_string(),
                    link_target: None,
                },
            );
        }

        if let Some(sink) = &mut self.sink {
            sink.write_segments(&line.segments, Self::message_kind(style))?;
            self.last_line_was_empty = line.is_empty();
            return Ok(());
        }

        let mut plain = String::new();
        if self.color {
            for segment in &line.segments {
                let clickable_target = segment.link_target.as_deref().and_then(make_clickable_target);
                if let Some(target) = clickable_target.as_deref() {
                    write!(self.writer, "\u{1b}]8;;{target}\u{1b}\\")?;
                }
                write!(self.writer, "{style}{}{Reset}", segment.text, style = segment.style)?;
                if clickable_target.is_some() {
                    write!(self.writer, "\u{1b}]8;;\u{1b}\\")?;
                }
                plain.push_str(&segment.text);
            }
            writeln!(self.writer)?;
        } else {
            for segment in &line.segments {
                let clickable_target = segment.link_target.as_deref().and_then(make_clickable_target);
                if let Some(target) = clickable_target.as_deref() {
                    write!(self.writer, "\u{1b}]8;;{target}\u{1b}\\")?;
                }
                write!(self.writer, "{}", segment.text)?;
                if clickable_target.is_some() {
                    write!(self.writer, "\u{1b}]8;;\u{1b}\\")?;
                }
                plain.push_str(&segment.text);
            }
            writeln!(self.writer)?;
        }
        self.writer.flush()?;
        transcript::append(&plain);
        self.last_line_was_empty = plain.trim().is_empty();
        Ok(())
    }
}
