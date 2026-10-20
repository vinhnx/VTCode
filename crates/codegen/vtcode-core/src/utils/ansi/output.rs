//! Plain output lines, modals, and screen management.

use super::*;

use crate::utils::transcript;

use crate::ui::markdown::{MarkdownLine, RenderMarkdownOptions, render_markdown_to_lines_with_options};
use crate::ui::theme;
use crate::ui::tui::{InlineListItem, InlineListSearchConfig, InlineListSelection, InlineMessageKind};
use crate::utils::ansi_capabilities::AnsiCapabilities;
use anstyle::{Reset, Style};
use anyhow::{Result, anyhow};
use std::io::Write;
use unicode_width::UnicodeWidthStr;

const RECENT_DIAGNOSTIC_LIMIT: usize = 128;

impl AnsiRenderer {
    pub fn set_table_max_width(&mut self, max_width: Option<usize>) {
        if let Some(sink) = &mut self.sink {
            sink.table_max_width = max_width;
            sink.table_max_width_override = max_width;
        }
    }

    pub(super) fn should_render_style(&self, style: MessageStyle) -> bool {
        self.reasoning_visible || !matches!(style, MessageStyle::Reasoning | MessageStyle::ReasoningEmphasis)
    }

    /// Install the canonical event boundary after a thread sink exists.
    pub fn set_diagnostic_sink(&mut self, sink: DiagnosticSink) {
        self.diagnostic_sink = Some(sink);
        self.diagnostic_history.clear();
    }

    /// Display an operational diagnostic. Payloads must also be sanitized before event persistence.
    pub fn diagnostic(&mut self, diagnostic: &vtcode_commons::diagnostics::Diagnostic) -> Result<()> {
        self.display_diagnostic(diagnostic, true)
    }

    /// Restore an already-durable incident without emitting another event.
    pub fn restore_diagnostic(&mut self, diagnostic: &vtcode_commons::diagnostics::Diagnostic) -> Result<()> {
        self.display_diagnostic(diagnostic, false)
    }

    fn display_diagnostic(
        &mut self,
        diagnostic: &vtcode_commons::diagnostics::Diagnostic,
        persist: bool,
    ) -> Result<()> {
        use vtcode_commons::diagnostics::{DiagnosticSeverity, redact_diagnostic};
        self.flush_compact_command_group();
        let mut diagnostic = redact_diagnostic(diagnostic);
        let already_persisted = self.diagnostic_history.iter().any(|previous| previous == &diagnostic);
        let persistence_error = if !persist || already_persisted {
            None
        } else {
            self.diagnostic_sink.as_ref().and_then(|sink| sink(&diagnostic).err())
        };
        if !already_persisted && persistence_error.is_none() && self.diagnostic_sink.is_some() {
            self.diagnostic_history
                .retain(|previous| previous.correlation_id != diagnostic.correlation_id);
            if self.diagnostic_history.len() == RECENT_DIAGNOSTIC_LIMIT {
                let _ = self.diagnostic_history.pop_front();
            }
            self.diagnostic_history.push_back(diagnostic.clone());
        }
        if let Some(error) = &persistence_error {
            if !diagnostic.agent_details.is_empty() {
                diagnostic.agent_details.push_str("\n\n");
            }
            diagnostic
                .agent_details
                .push_str("Diagnostic persistence failed; this message may not be saved.\n");
            diagnostic
                .agent_details
                .push_str(&vtcode_commons::sanitizer::sanitize_provider_diagnostic(error.to_string()));
        }
        let style = match diagnostic.severity {
            DiagnosticSeverity::Info => MessageStyle::Info,
            DiagnosticSeverity::Warning => MessageStyle::Warning,
            DiagnosticSeverity::Error => MessageStyle::Error,
        };
        if let Some(sink) = &self.sink {
            if persist && self.diagnostic_sink.is_none() {
                // Secondary renderers retain their owning handle's canonical observer.
                sink.handle.diagnostic(diagnostic.clone());
            } else {
                sink.handle.present_diagnostic(diagnostic.clone());
            }
            transcript::append(&diagnostic.text());
        } else {
            self.render_markdown(style, &diagnostic.text())?;
        }
        if let Some(error) = persistence_error {
            return Err(error);
        }
        Ok(())
    }

    #[track_caller]
    fn legacy_diagnostic(&mut self, style: MessageStyle, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }
        use vtcode_commons::diagnostics::DiagnosticSeverity;
        let severity = if matches!(style, MessageStyle::Warning) {
            DiagnosticSeverity::Warning
        } else {
            DiagnosticSeverity::Error
        };
        self.diagnostic(&vtcode_commons::diagnostics::legacy_ui_diagnostic(severity, text))
    }

    pub(super) fn indent_for_style(&self, style: MessageStyle) -> &'static str {
        if self.screen_reader_mode && matches!(style, MessageStyle::Reasoning | MessageStyle::ReasoningEmphasis) {
            "  [reasoning] "
        } else {
            style.indent()
        }
    }

    /// Get the terminal's detected ANSI capabilities
    pub fn capabilities(&self) -> &AnsiCapabilities {
        &self.capabilities
    }

    /// Return the width available to a rendered diff row after the logical
    /// message indent and, for inline UI output, the transcript frame.
    ///
    /// Diff rows use this width only to extend their background with spaces;
    /// when terminal sizing is unavailable callers retain their bounded
    /// fallback behavior.
    pub fn diff_content_width(&self, style: MessageStyle) -> Option<usize> {
        let indent_width = UnicodeWidthStr::width(self.indent_for_style(style));
        let kind = Self::message_kind(style);
        let frame_width = self
            .sink
            .as_ref()
            .map(|sink| transcript_table_frame_width(kind, sink.handle.agent_label_frame_width()))
            .unwrap_or_default();
        // An explicit test/configuration width wins. Otherwise prefer a fresh
        // terminal measurement so a later diff preview follows a resize even
        // when an earlier Markdown render cached the previous width.
        let terminal_width = self
            .sink
            .as_ref()
            .and_then(|sink| {
                sink.table_max_width_override
                    .or_else(|| crossterm::terminal::size().ok().map(|(width, _)| usize::from(width)))
                    .or(sink.table_max_width)
            })
            .or_else(|| crossterm::terminal::size().ok().map(|(width, _)| usize::from(width)));

        terminal_width.map(|width| width.saturating_sub(indent_width).saturating_sub(frame_width))
    }

    /// Check if unicode should be used for formatting (tables, boxes, etc.)
    pub fn should_use_unicode_formatting(&self) -> bool {
        self.capabilities.should_use_unicode_boxes()
    }

    /// Check if 256-color output is supported
    pub fn supports_256_colors(&self) -> bool {
        self.capabilities.supports_256_colors()
    }

    /// Check if true color (24-bit) output is supported
    pub fn supports_true_color(&self) -> bool {
        self.capabilities.supports_true_color()
    }

    /// Check if should use unicode characters based on terminal capabilities
    pub fn should_use_unicode(&self) -> bool {
        self.capabilities.unicode_support
    }

    pub fn show_list_modal(
        &mut self,
        title: &str,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
    ) {
        if let Some(sink) = &self.sink {
            sink.show_list_modal(title.into(), lines, items, selected, search);
        }
    }

    pub fn show_list_modal_with_footer(
        &mut self,
        title: &str,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
        footer_hint: Option<String>,
    ) {
        self.show_list_modal_with_status(title, lines, items, selected, search, footer_hint, None);
    }

    /// Show a list modal with an optional status strip (last action feedback).
    pub fn show_list_modal_with_status(
        &mut self,
        title: &str,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
        footer_hint: Option<String>,
        status: Option<vtcode_commons::ui_protocol::InlineStatus>,
    ) {
        if let Some(sink) = &self.sink {
            sink.show_list_modal_with_status(title.into(), lines, items, selected, search, footer_hint, status);
        }
    }

    pub fn show_secure_prompt_modal(&mut self, title: &str, lines: Vec<String>, prompt_label: String) {
        if let Some(sink) = &self.sink {
            sink.show_secure_prompt_modal(title.into(), lines, prompt_label);
        }
    }

    pub fn close_modal(&mut self) {
        if let Some(sink) = &self.sink {
            sink.close_modal();
        }
    }

    pub fn clear_screen(&mut self) {
        self.flush_compact_command_group();
        if let Some(sink) = &self.sink {
            sink.handle.clear_screen();
        }
    }

    /// Push text into the buffer
    pub fn push(&mut self, text: &str) {
        self.buffer.push_str(text);
    }

    /// Flush the buffer with the given style
    #[track_caller]
    pub fn flush(&mut self, style: MessageStyle) -> Result<()> {
        self.flush_compact_command_group();
        if !self.should_render_style(style) {
            self.buffer.clear();
            return Ok(());
        }
        if matches!(style, MessageStyle::Error | MessageStyle::ToolError | MessageStyle::Warning) {
            let text = std::mem::take(&mut self.buffer);
            return self.legacy_diagnostic(style, &text);
        }
        let indent = self.indent_for_style(style);
        if let Some(sink) = &mut self.sink {
            // Track if this line is empty
            self.last_line_was_empty = self.buffer.is_empty() && indent.is_empty();
            sink.write_line(style.style(), indent, &self.buffer, Self::message_kind(style))?;
            self.buffer.clear();
            return Ok(());
        }
        let style = style.style();
        if self.color {
            writeln!(self.writer, "{style}{}{Reset}", self.buffer)?;
        } else {
            writeln!(self.writer, "{}", self.buffer)?;
        }
        self.writer.flush()?;
        transcript::append(&self.buffer);
        // Track if this line is empty
        self.last_line_was_empty = self.buffer.is_empty();
        self.buffer.clear();
        Ok(())
    }

    /// Convenience for writing a single line
    #[track_caller]
    pub fn line(&mut self, style: MessageStyle, text: &str) -> Result<()> {
        self.flush_compact_command_group();
        if !self.should_render_style(style) {
            return Ok(());
        }
        if matches!(style, MessageStyle::Error | MessageStyle::ToolError | MessageStyle::Warning) {
            return self.legacy_diagnostic(style, text);
        }
        if matches!(style, MessageStyle::Response | MessageStyle::Reasoning | MessageStyle::ReasoningEmphasis) {
            return self.render_markdown(style, text);
        }
        if matches!(style, MessageStyle::Output | MessageStyle::ToolOutput) {
            let stripped = crate::utils::ansi_parser::strip_ansi(text);
            self.buffer.clear();
            if looks_like_diff(&stripped) {
                self.buffer.push_str("```diff\n");
            } else {
                self.buffer.push_str("```\n");
            }
            self.buffer.push_str(&stripped);
            self.buffer.push_str("\n```");
            let fenced = std::mem::take(&mut self.buffer);
            return self.render_markdown(style, &fenced);
        }
        if matches!(style, MessageStyle::ToolDetail) {
            if contains_markdown_fence(text) {
                let stripped = crate::utils::ansi_parser::strip_ansi(text);
                return self.render_markdown(style, &stripped);
            }
            if looks_like_diff(text) {
                let stripped = crate::utils::ansi_parser::strip_ansi(text);
                self.buffer.clear();
                self.buffer.push_str("```diff\n");
                self.buffer.push_str(&stripped);
                self.buffer.push_str("\n```");
                let fenced = std::mem::take(&mut self.buffer);
                return self.render_markdown(style, &fenced);
            }
        }
        let indent = style.indent();
        let dont_split = matches!(style, MessageStyle::Tool | MessageStyle::ToolDetail);

        if let Some(sink) = &mut self.sink {
            sink.write_multiline_with_transcript(style.style(), indent, text, Self::message_kind(style), true, None)?;
            return Ok(());
        }

        if text.contains('\n') && !dont_split {
            for line in text.lines() {
                self.buffer.clear();
                if !indent.is_empty() && !line.is_empty() {
                    self.buffer.push_str(indent);
                }
                self.buffer.push_str(line);
                self.flush(style)?;
            }
            Ok(())
        } else {
            self.buffer.clear();
            if !indent.is_empty() && !text.is_empty() {
                self.buffer.push_str(indent);
            }
            self.buffer.push_str(text);
            self.flush(style)
        }
    }

    /// Write a continuation line that joins an existing Pty block.
    ///
    /// Sends the text as `InlineMessageKind::Pty` through
    /// `write_multiline_with_transcript` so the TUI reflow renders it
    /// with the same 2-space block prefix and styling as the PTY output.
    pub fn pty_continuation_line(&mut self, text: &str) -> Result<()> {
        self.flush_compact_command_group();
        let style = MessageStyle::ToolOutput;
        let indent = style.indent();
        let kind = Self::message_kind(style);
        if let Some(sink) = &mut self.sink {
            sink.write_multiline_with_transcript(style.style(), indent, text, kind, true, None)?;
            return Ok(());
        }
        self.buffer.clear();
        if !indent.is_empty() && !text.is_empty() {
            self.buffer.push_str(indent);
        }
        self.buffer.push_str(text);
        self.flush(style)
    }

    /// Write a URL as a full, clickable line using OSC 8 hyperlinks.
    ///
    /// The URL is rendered on its own line so that terminal emulators can
    /// detect and activate it for click-to-open behaviour.
    pub fn hyperlink_line(&mut self, style: MessageStyle, url: &str) -> Result<()> {
        self.flush_compact_command_group();
        if !self.should_render_style(style) {
            return Ok(());
        }
        let indent = style.indent();
        if let Some(sink) = &mut self.sink {
            let linked = format!(
                "{}{}{}",
                vtcode_commons::ansi_codes::hyperlink_open(url),
                url,
                vtcode_commons::ansi_codes::hyperlink_close(),
            );
            sink.write_multiline_with_transcript(
                style.style(),
                indent,
                &linked,
                Self::message_kind(style),
                true,
                None,
            )?;
            self.last_line_was_empty = false;
            return Ok(());
        }
        self.buffer.clear();
        if !indent.is_empty() {
            self.buffer.push_str(indent);
        }
        self.buffer.push_str(&vtcode_commons::ansi_codes::hyperlink_open(url));
        self.buffer.push_str(url);
        self.buffer.push_str(&vtcode_commons::ansi_codes::hyperlink_close());
        let ansi_style = style.style();
        if self.color {
            writeln!(self.writer, "{ansi_style}{}{Reset}", self.buffer)?;
        } else {
            writeln!(self.writer, "{}", self.buffer)?;
        }
        self.writer.flush()?;
        transcript::append(url);
        self.last_line_was_empty = false;
        self.buffer.clear();
        Ok(())
    }

    /// Append a large pasted user message as a placeholder in inline UI.
    pub fn append_paste_placeholder(&mut self, message: &str, line_count: usize) -> Result<()> {
        self.flush_compact_command_group();
        if let Some(sink) = &self.sink {
            sink.handle
                .append_pasted_message(InlineMessageKind::User, message.to_string(), line_count);
            transcript::append(message);
            self.last_line_was_empty = message.trim().is_empty();
            return Ok(());
        }
        self.line(MessageStyle::User, message)
    }

    /// Write styled text without a trailing newline
    pub fn inline_with_style(&mut self, style: MessageStyle, text: &str) -> Result<()> {
        self.flush_compact_command_group();
        if !self.should_render_style(style) {
            return Ok(());
        }
        if let Some(sink) = &mut self.sink {
            sink.write_inline(style.style(), text, Self::message_kind(style));
            return Ok(());
        }
        let ansi_style = style.style();
        if self.color {
            write!(self.writer, "{ansi_style}{text}{Reset}")?;
        } else {
            write!(self.writer, "{text}")?;
        }
        self.writer.flush()?;
        Ok(())
    }

    /// Write a line with an explicit style
    pub fn line_with_style(&mut self, style: Style, text: &str) -> Result<()> {
        self.line_with_override_style(MessageStyle::Info, style, text)
    }

    /// Write a line with a custom style while preserving the logical message kind.
    #[track_caller]
    pub fn line_with_override_style(&mut self, fallback: MessageStyle, style: Style, text: &str) -> Result<()> {
        self.flush_compact_command_group();
        let tool_output_id = self.pending_tool_output_anchor.take();
        if !self.should_render_style(fallback) {
            return Ok(());
        }
        if matches!(fallback, MessageStyle::Error | MessageStyle::ToolError | MessageStyle::Warning) {
            return self.legacy_diagnostic(fallback, text);
        }
        let kind = Self::message_kind(fallback);
        let indent = self.indent_for_style(fallback);
        if let Some(sink) = &mut self.sink {
            sink.write_multiline_with_transcript(style, indent, text, kind, true, tool_output_id)?;
            self.last_line_was_empty = text.trim().is_empty();
            return Ok(());
        }
        let mut combined;
        let display = if !indent.is_empty() && !text.is_empty() {
            combined = String::with_capacity(indent.len() + text.len());
            combined.push_str(indent);
            combined.push_str(text);
            combined.as_str()
        } else {
            text
        };
        if self.color {
            writeln!(self.writer, "{style}{display}{Reset}")?;
        } else {
            writeln!(self.writer, "{display}")?;
        }
        self.writer.flush()?;
        transcript::append(display);
        self.last_line_was_empty = text.trim().is_empty();
        Ok(())
    }

    /// Write an empty line only if the previous line was not empty
    pub fn line_if_not_empty(&mut self, style: MessageStyle) -> Result<()> {
        if !self.was_previous_line_empty() {
            self.line(style, "")
        } else {
            Ok(())
        }
    }

    /// Write a raw line without styling
    pub fn raw_line(&mut self, text: &str) -> Result<()> {
        self.flush_compact_command_group();
        writeln!(self.writer, "{text}")?;
        self.writer.flush()?;
        transcript::append(text);
        Ok(())
    }

    pub fn render_token_delta(&mut self, delta: &str) -> Result<()> {
        self.inline_with_style(MessageStyle::Response, delta)
    }

    pub fn stream_markdown_response(&mut self, text: &str, previous_line_count: usize) -> Result<usize> {
        // Strip ANSI codes from agent response to prevent interference with markdown rendering
        let text = crate::utils::ansi_parser::strip_ansi(text);
        let text = &text;

        let styles = theme::active_styles();
        let style = MessageStyle::Response;
        let base_style = style.style();
        let indent = style.indent();
        if let Some(sink) = &mut self.sink {
            // Read terminal width fresh so tables adapt to resizes.
            if sink.table_max_width_override.is_none()
                && let Ok((w, _)) = crossterm::terminal::size()
            {
                sink.table_max_width = Some(w as usize);
            }
            let table_max_width = sink.table_content_width(Self::message_kind(style), indent);
            let (prepared, plain_lines, last_empty) =
                sink.prepare_markdown_lines_with_table_width(text, indent, base_style, true, true, table_max_width);
            let line_count = prepared.len();
            sink.replace_inline_lines(previous_line_count, prepared, &plain_lines, Self::message_kind(style));
            self.last_line_was_empty = last_empty;
            return Ok(line_count);
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
            RenderMarkdownOptions::default(),
        );
        if lines.is_empty() {
            lines.push(MarkdownLine::default());
        }

        Err(anyhow!("stream_markdown_response requires an inline sink"))
    }
}
