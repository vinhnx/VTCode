use crate::config::loader::SyntaxHighlightingConfig;
use crate::ui::markdown::{
    MarkdownLine, MarkdownSegment, RenderMarkdownOptions, render_markdown_to_lines_with_options,
};
use crate::ui::theme;
use crate::ui::tui::{
    InlineHandle, InlineListItem, InlineListSearchConfig, InlineListSelection, InlineMessageKind,
    InlineSegment, InlineTextStyle, SecurePromptConfig, convert_style as convert_to_inline_style,
};
use crate::utils::ansi_capabilities::AnsiCapabilities;
pub use crate::utils::message_style::MessageStyle;
use crate::utils::transcript;
use ansi_to_tui::IntoText;
use anstream::{AutoStream, ColorChoice};
use anstyle::{Ansi256Color, AnsiColor, Color as AnsiColorEnum, Effects, Reset, RgbColor, Style};
use anyhow::{Result, anyhow};
use ratatui::style::{Color as RatColor, Modifier as RatModifier, Style as RatatuiStyle};
use std::io::{self, Write};
use std::sync::Arc;

/// Renderer with deferred output buffering
pub struct AnsiRenderer {
    writer: AutoStream<io::Stdout>,
    buffer: String,
    color: bool,
    sink: Option<InlineSink>,
    last_line_was_empty: bool,
    highlight_config: SyntaxHighlightingConfig,
    capabilities: AnsiCapabilities,
}

impl AnsiRenderer {
    /// Create a new renderer for stdout
    pub fn stdout() -> Self {
        let capabilities = AnsiCapabilities::detect();
        let color = capabilities.supports_color();
        let choice = if color {
            ColorChoice::Auto
        } else {
            ColorChoice::Never
        };
        Self {
            writer: AutoStream::new(std::io::stdout(), choice),
            buffer: String::with_capacity(1024),
            color,
            sink: None,
            last_line_was_empty: false,
            highlight_config: SyntaxHighlightingConfig::default(),
            capabilities,
        }
    }

    /// Create a renderer that forwards output to the inline UI session handle
    pub fn with_inline_ui(
        handle: InlineHandle,
        highlight_config: SyntaxHighlightingConfig,
    ) -> Self {
        let mut renderer = Self::stdout();
        renderer.highlight_config = highlight_config.clone();
        renderer.sink = Some(InlineSink::new(handle, highlight_config));
        renderer.last_line_was_empty = false;
        renderer
    }

    /// Override the syntax highlighting configuration.
    pub fn set_highlight_config(&mut self, config: SyntaxHighlightingConfig) {
        if let Some(sink) = &mut self.sink {
            sink.set_highlight_config(config.clone());
        }
        self.highlight_config = config;
    }

    /// Check if the last line rendered was empty
    pub fn was_previous_line_empty(&self) -> bool {
        self.last_line_was_empty
    }

    fn message_kind(style: MessageStyle) -> InlineMessageKind {
        style.message_kind()
    }

    pub fn supports_streaming_markdown(&self) -> bool {
        self.sink.is_some()
    }

    /// Determine whether the renderer is connected to the inline UI.
    ///
    /// Inline rendering uses the terminal session scrollback, so tool output should
    /// avoid truncation that would otherwise be applied in compact CLI mode.
    pub fn prefers_untruncated_output(&self) -> bool {
        self.sink.is_some()
    }

    pub fn supports_inline_ui(&self) -> bool {
        self.sink.is_some()
    }

    /// Get the terminal's detected ANSI capabilities
    pub fn capabilities(&self) -> &AnsiCapabilities {
        &self.capabilities
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

    pub fn show_secure_prompt_modal(
        &mut self,
        title: &str,
        lines: Vec<String>,
        prompt_label: String,
    ) {
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
        if let Some(sink) = &self.sink {
            sink.handle.clear_screen();
        }
    }

    /// Push text into the buffer
    pub fn push(&mut self, text: &str) {
        self.buffer.push_str(text);
    }

    /// Flush the buffer with the given style
    pub fn flush(&mut self, style: MessageStyle) -> Result<()> {
        if let Some(sink) = &mut self.sink {
            let indent = style.indent();
            // Track if this line is empty
            self.last_line_was_empty = self.buffer.is_empty() && indent.is_empty();
            sink.write_line(
                style.style(),
                indent,
                &self.buffer,
                Self::message_kind(style),
            )?;
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
    pub fn line(&mut self, style: MessageStyle, text: &str) -> Result<()> {
        if matches!(style, MessageStyle::Response | MessageStyle::Reasoning) {
            return self.render_markdown(style, text);
        }
        if matches!(style, MessageStyle::Output | MessageStyle::ToolOutput) {
            let stripped = crate::utils::ansi_parser::strip_ansi(text);
            let fenced = if looks_like_diff(&stripped) {
                format!("```diff\n{stripped}\n```")
            } else {
                format!("```\n{stripped}\n```")
            };
            return self.render_markdown(style, &fenced);
        }
        if matches!(style, MessageStyle::ToolDetail) {
            if contains_markdown_fence(text) {
                let stripped = crate::utils::ansi_parser::strip_ansi(text);
                return self.render_markdown(style, &stripped);
            }
            if looks_like_diff(text) {
                let stripped = crate::utils::ansi_parser::strip_ansi(text);
                let fenced = format!("```diff\n{stripped}\n```");
                return self.render_markdown(style, &fenced);
            }
        }
        let indent = style.indent();
        let dont_split = matches!(style, MessageStyle::Tool | MessageStyle::ToolDetail);

        if let Some(sink) = &mut self.sink {
            sink.write_multiline(style.style(), indent, text, Self::message_kind(style))?;
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

    /// Append a large pasted user message as a placeholder in inline UI.
    pub fn append_paste_placeholder(&mut self, message: &str, line_count: usize) -> Result<()> {
        if let Some(sink) = &self.sink {
            sink.handle.append_pasted_message(
                InlineMessageKind::User,
                message.to_string(),
                line_count,
            );
            transcript::append(message);
            self.last_line_was_empty = message.trim().is_empty();
            return Ok(());
        }
        self.line(MessageStyle::User, message)
    }

    /// Write styled text without a trailing newline
    pub fn inline_with_style(&mut self, style: MessageStyle, text: &str) -> Result<()> {
        if let Some(sink) = &mut self.sink {
            sink.write_inline(style.style(), text, Self::message_kind(style));
            return Ok(());
        }
        let ansi_style = style.style();
        if self.color {
            write!(self.writer, "{ansi_style}{}{Reset}", text)?;
        } else {
            write!(self.writer, "{}", text)?;
        }
        self.writer.flush()?;
        Ok(())
    }

    /// Write a line with an explicit style
    pub fn line_with_style(&mut self, style: Style, text: &str) -> Result<()> {
        self.line_with_override_style(MessageStyle::Info, style, text)
    }

    /// Write a line with a custom style while preserving the logical message kind.
    pub fn line_with_override_style(
        &mut self,
        fallback: MessageStyle,
        style: Style,
        text: &str,
    ) -> Result<()> {
        let kind = Self::message_kind(fallback);
        let indent = fallback.indent();
        if let Some(sink) = &mut self.sink {
            sink.write_multiline(style, indent, text, kind)?;
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
            writeln!(self.writer, "{style}{}{Reset}", display)?;
        } else {
            writeln!(self.writer, "{}", display)?;
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
        writeln!(self.writer, "{}", text)?;
        self.writer.flush()?;
        transcript::append(text);
        Ok(())
    }

    /// Render markdown content with proper syntax highlighting and indentation normalization.
    /// Use this for tool output that contains markdown code blocks.
    pub fn render_markdown_output(&mut self, style: MessageStyle, text: &str) -> Result<()> {
        self.render_markdown(style, text)
    }

    fn render_markdown(&mut self, style: MessageStyle, text: &str) -> Result<()> {
        let styles = theme::active_styles();
        let base_style = style.style();
        let indent = style.indent();
        let preserve_code_indentation = matches!(
            style,
            MessageStyle::Output
                | MessageStyle::ToolOutput
                | MessageStyle::ToolDetail
                | MessageStyle::Response
                | MessageStyle::Reasoning
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
            let last_empty = sink.write_markdown(
                text,
                indent,
                base_style,
                Self::message_kind(style),
                preserve_code_indentation,
            )?;
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

    pub fn render_token_delta(&mut self, delta: &str) -> Result<()> {
        self.inline_with_style(MessageStyle::Response, delta)
    }

    pub fn render_reasoning_delta(&mut self, delta: &str) -> Result<()> {
        self.inline_with_style(MessageStyle::Reasoning, delta)
    }

    pub fn stream_markdown_response(
        &mut self,
        text: &str,
        previous_line_count: usize,
    ) -> Result<usize> {
        // Strip ANSI codes from agent response to prevent interference with markdown rendering
        let text = crate::utils::ansi_parser::strip_ansi(text);
        let text = &text;

        let styles = theme::active_styles();
        let style = MessageStyle::Response;
        let base_style = style.style();
        let indent = style.indent();
        if let Some(sink) = &mut self.sink {
            let (prepared, plain_lines, last_empty) =
                sink.prepare_markdown_lines(text, indent, base_style, true, true);
            let line_count = prepared.len();
            sink.replace_inline_lines(
                previous_line_count,
                prepared,
                &plain_lines,
                Self::message_kind(style),
            );
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

    pub fn render_reasoning_stream(
        &mut self,
        lines: &[String],
        previous_line_count: &mut usize,
    ) -> Result<()> {
        if lines.is_empty() {
            return Ok(());
        }

        let style = MessageStyle::Reasoning;
        let indent = style.indent();
        let kind = Self::message_kind(style);
        let base_style = style.style();

        if let Some(sink) = &mut self.sink {
            let fallback = sink.resolve_fallback_style(base_style);
            let fallback_arc = Arc::new(fallback.clone());
            let mut prepared: Vec<Vec<InlineSegment>> = Vec::new();
            let mut plain_lines: Vec<String> = Vec::new();

            for (line_idx, line) in lines.iter().enumerate() {
                let (converted, plain) = sink.convert_plain_lines(line, &fallback);
                for (segment_idx, (mut segments, mut plain_line)) in
                    converted.into_iter().zip(plain.into_iter()).enumerate()
                {
                    // Add "Thinking:" prefix to the very first line only
                    if *previous_line_count == 0
                        && line_idx == 0
                        && segment_idx == 0
                        && !plain_line.trim().is_empty()
                    {
                        segments.insert(
                            0,
                            InlineSegment {
                                text: "Thinking: ".to_owned(),
                                style: Arc::clone(&fallback_arc),
                            },
                        );
                        plain_line.insert_str(0, "Thinking: ");
                    }

                    if !indent.is_empty() && !plain_line.is_empty() {
                        segments.insert(
                            0,
                            InlineSegment {
                                text: indent.to_owned(),
                                style: Arc::clone(&fallback_arc),
                            },
                        );
                        plain_line.insert_str(0, indent);
                    }
                    prepared.push(segments);
                    plain_lines.push(plain_line);
                }
            }

            if *previous_line_count == 0 {
                for (segments, plain_line) in prepared.iter().zip(plain_lines.iter()) {
                    if segments.is_empty() {
                        sink.handle.append_line(kind, Vec::new());
                    } else {
                        sink.handle.append_line(kind, segments.clone());
                    }
                    crate::utils::transcript::append(plain_line);
                }
            } else {
                sink.replace_inline_lines(
                    *previous_line_count,
                    prepared.clone(),
                    &plain_lines,
                    kind,
                );
            }

            *previous_line_count = plain_lines.len();
            self.last_line_was_empty = plain_lines
                .last()
                .map(|line| line.trim().is_empty())
                .unwrap_or(true);

            return Ok(());
        }

        if *previous_line_count == 0 {
            for (idx, line) in lines.iter().enumerate() {
                if idx == 0 && !line.trim().is_empty() {
                    // Prepend "Thinking:" to first line
                    let prefixed = format!("Thinking: {}", line);
                    self.line(style, &prefixed)?;
                } else {
                    self.line(style, line)?;
                }
            }
        } else if let Some(last) = lines.last() {
            self.line(style, last)?;
        }

        *previous_line_count = lines.len();
        Ok(())
    }

    fn write_markdown_line(
        &mut self,
        style: MessageStyle,
        indent: &str,
        mut line: MarkdownLine,
    ) -> Result<()> {
        if !indent.is_empty() && !line.segments.is_empty() {
            line.segments
                .insert(0, MarkdownSegment::new(style.style(), indent));
        }

        if let Some(sink) = &mut self.sink {
            sink.write_segments(&line.segments, Self::message_kind(style))?;
            self.last_line_was_empty = line.is_empty();
            return Ok(());
        }

        let mut plain = String::new();
        if self.color {
            for segment in &line.segments {
                write!(
                    self.writer,
                    "{style}{}{Reset}",
                    segment.text,
                    style = segment.style
                )?;
                plain.push_str(&segment.text);
            }
            writeln!(self.writer)?;
        } else {
            for segment in &line.segments {
                write!(self.writer, "{}", segment.text)?;
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

fn contains_markdown_fence(text: &str) -> bool {
    text.contains("```") || text.contains("~~~")
}

fn looks_like_diff(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("diff --git ")
            || trimmed.starts_with("@@ ")
            || trimmed.starts_with("+++ ")
            || trimmed.starts_with("--- ")
            || trimmed.starts_with("index ")
            || trimmed.starts_with("new file mode ")
            || trimmed.starts_with("deleted file mode ")
    })
}

struct InlineSink {
    handle: InlineHandle,
    highlight_config: SyntaxHighlightingConfig,
}

impl InlineSink {
    fn should_record_transcript(kind: InlineMessageKind) -> bool {
        kind != InlineMessageKind::Pty
    }
    fn ansi_from_ratatui_color(color: RatColor) -> Option<AnsiColorEnum> {
        match color {
            RatColor::Reset => None,
            RatColor::Black => Some(AnsiColorEnum::Ansi(AnsiColor::Black)),
            RatColor::Red => Some(AnsiColorEnum::Ansi(AnsiColor::Red)),
            RatColor::Green => Some(AnsiColorEnum::Ansi(AnsiColor::Green)),
            RatColor::Yellow => Some(AnsiColorEnum::Ansi(AnsiColor::Yellow)),
            RatColor::Blue => Some(AnsiColorEnum::Ansi(AnsiColor::Blue)),
            RatColor::Magenta => Some(AnsiColorEnum::Ansi(AnsiColor::Magenta)),
            RatColor::Cyan => Some(AnsiColorEnum::Ansi(AnsiColor::Cyan)),
            RatColor::Gray => Some(AnsiColorEnum::Rgb(RgbColor(0x88, 0x88, 0x88))),
            RatColor::DarkGray => Some(AnsiColorEnum::Rgb(RgbColor(0x66, 0x66, 0x66))),
            RatColor::LightRed => Some(AnsiColorEnum::Ansi(AnsiColor::Red)),
            RatColor::LightGreen => Some(AnsiColorEnum::Ansi(AnsiColor::Green)),
            RatColor::LightYellow => Some(AnsiColorEnum::Ansi(AnsiColor::Yellow)),
            RatColor::LightBlue => Some(AnsiColorEnum::Ansi(AnsiColor::Blue)),
            RatColor::LightMagenta => Some(AnsiColorEnum::Ansi(AnsiColor::Magenta)),
            RatColor::LightCyan => Some(AnsiColorEnum::Ansi(AnsiColor::Cyan)),
            RatColor::White => Some(AnsiColorEnum::Ansi(AnsiColor::White)),
            RatColor::Rgb(r, g, b) => Some(AnsiColorEnum::Rgb(RgbColor(r, g, b))),
            RatColor::Indexed(value) => Some(AnsiColorEnum::Ansi256(Ansi256Color(value))),
        }
    }

    fn inline_style_from_ratatui(
        &self,
        style: RatatuiStyle,
        fallback: &InlineTextStyle,
    ) -> InlineTextStyle {
        let mut resolved = fallback.clone();
        if let Some(color) = style.fg.and_then(Self::ansi_from_ratatui_color) {
            resolved.color = Some(color);
        }

        let added = style.add_modifier;

        if added.contains(RatModifier::BOLD) {
            resolved.effects |= Effects::BOLD;
        }

        if added.contains(RatModifier::ITALIC) {
            resolved.effects |= Effects::ITALIC;
        }

        resolved
    }

    fn prepare_markdown_lines(
        &self,
        text: &str,
        indent: &str,
        base_style: Style,
        preserve_blank_lines: bool,
        preserve_code_indentation: bool,
    ) -> (Vec<Vec<InlineSegment>>, Vec<String>, bool) {
        let fallback = self.resolve_fallback_style(base_style);
        let fallback_arc = Arc::new(fallback.clone());
        let theme_styles = theme::active_styles();
        let highlight_cfg = self
            .highlight_config
            .enabled
            .then_some(&self.highlight_config);
        let mut rendered = render_markdown_to_lines_with_options(
            text,
            base_style,
            &theme_styles,
            highlight_cfg,
            RenderMarkdownOptions {
                preserve_code_indentation,
                disable_code_block_table_reparse: false,
            },
        );
        if preserve_blank_lines {
            let mut cleaned = Vec::with_capacity(rendered.len());
            let mut last_blank = false;
            for line in rendered {
                let is_blank = line.is_empty();
                if is_blank {
                    if last_blank {
                        continue;
                    }
                    last_blank = true;
                } else {
                    last_blank = false;
                }
                cleaned.push(line);
            }
            rendered = cleaned;
        } else {
            // TUI space is constrained; drop blank lines to keep transcripts compact.
            rendered.retain(|line| !line.is_empty());
        }
        if rendered.is_empty() {
            rendered.push(MarkdownLine::default());
        }

        let mut prepared = Vec::with_capacity(rendered.len());
        let mut plain = Vec::with_capacity(rendered.len());

        for line in rendered {
            // Pre-allocate segments and plain text with estimated capacity
            let mut segments = Vec::with_capacity(line.segments.len());
            let mut plain_line = String::with_capacity(120);

            let has_content = line
                .segments
                .iter()
                .any(|segment| !segment.text.trim().is_empty());

            if !indent.is_empty() && has_content {
                segments.push(InlineSegment {
                    text: indent.to_string(),
                    style: Arc::clone(&fallback_arc),
                });
                plain_line.push_str(indent);
            }

            for segment in line.segments {
                if segment.text.is_empty() {
                    continue;
                }
                let converted = convert_to_inline_style(segment.style);
                let mut inline_style = fallback.clone();
                if let Some(color) = converted.color {
                    inline_style.color = Some(color);
                }
                if let Some(bg) = converted.bg_color {
                    inline_style.bg_color = Some(bg);
                }
                inline_style.effects = converted.effects | fallback.effects;
                plain_line.push_str(&segment.text);
                segments.push(InlineSegment {
                    text: segment.text,
                    style: Arc::new(inline_style),
                });
            }

            prepared.push(segments);
            plain.push(plain_line);
        }

        if prepared.is_empty() {
            prepared.push(Vec::new());
            plain.push(String::new());
        }

        let last_empty = plain
            .last()
            .map(|line| line.trim().is_empty())
            .unwrap_or(true);

        (prepared, plain, last_empty)
    }

    fn write_markdown(
        &mut self,
        text: &str,
        indent: &str,
        base_style: Style,
        kind: InlineMessageKind,
        preserve_code_indentation: bool,
    ) -> Result<bool> {
        let record_transcript = Self::should_record_transcript(kind);
        let (prepared, plain, last_empty) =
            self.prepare_markdown_lines(text, indent, base_style, true, preserve_code_indentation);
        for (segments, line) in prepared.into_iter().zip(plain.iter()) {
            if segments.is_empty() {
                self.handle.append_line(kind, Vec::new());
            } else {
                self.handle.append_line(kind, segments);
            }
            if record_transcript {
                crate::utils::transcript::append(line);
            }
        }
        Ok(last_empty)
    }

    fn replace_inline_lines(
        &mut self,
        count: usize,
        lines: Vec<Vec<InlineSegment>>,
        plain: &[String],
        kind: InlineMessageKind,
    ) {
        self.handle.replace_last(count, kind, lines);
        if Self::should_record_transcript(kind) {
            crate::utils::transcript::replace_last(count, plain);
        }
    }

    fn new(handle: InlineHandle, highlight_config: SyntaxHighlightingConfig) -> Self {
        Self {
            handle,
            highlight_config,
        }
    }

    fn set_highlight_config(&mut self, highlight_config: SyntaxHighlightingConfig) {
        self.highlight_config = highlight_config;
    }

    fn show_list_modal(
        &self,
        title: String,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
    ) {
        self.handle
            .show_list_modal(title, lines, items, selected, search);
    }

    fn show_secure_prompt_modal(&self, title: String, lines: Vec<String>, prompt_label: String) {
        self.handle.show_modal(
            title,
            lines,
            Some(SecurePromptConfig {
                label: prompt_label,
                placeholder: None,
                mask_input: true,
            }),
        );
    }

    fn close_modal(&self) {
        self.handle.close_modal();
    }

    #[allow(dead_code)]
    fn clear_screen(&self) {
        self.handle.clear_screen();
    }

    fn resolve_fallback_style(&self, style: Style) -> InlineTextStyle {
        let mut text_style = convert_to_inline_style(style);
        if text_style.color.is_none() {
            let active = theme::active_styles();
            text_style = text_style.merge_color(Some(active.foreground));
        }
        text_style
    }

    fn style_to_segment(&self, style: Style, text: &str) -> InlineSegment {
        let text_style = self.resolve_fallback_style(style);
        InlineSegment {
            text: text.to_string(),
            style: Arc::new(text_style),
        }
    }

    fn convert_plain_lines(
        &self,
        text: &str,
        fallback: &InlineTextStyle,
    ) -> (Vec<Vec<InlineSegment>>, Vec<String>) {
        let fallback_arc = Arc::new(fallback.clone());
        if text.is_empty() {
            return (vec![Vec::new()], vec![String::new()]);
        }

        let had_trailing_newline = text.ends_with('\n');
        let line_count_estimate = text.as_bytes().iter().filter(|&&b| b == b'\n').count() + 1;

        // Attempt to parse ANSI codes, with fallback to plain text
        // Note: ansi-to-tui may have issues with UTF-8 multi-byte chars mixed with ANSI codes,
        // so we validate UTF-8 integrity after parsing
        if let Ok(parsed) = text.as_bytes().into_text() {
            let mut converted_lines =
                Vec::with_capacity(parsed.lines.len().max(line_count_estimate));
            let mut plain_lines = Vec::with_capacity(parsed.lines.len().max(line_count_estimate));
            let base_style = RatatuiStyle::default().patch(parsed.style);

            for line in &parsed.lines {
                // Pre-allocate segments based on typical span count (3-5 spans per line)
                let mut segments = Vec::with_capacity(line.spans.len());
                let mut plain_line = String::with_capacity(80);
                let line_style = base_style.patch(line.style);

                for span in &line.spans {
                    // Use as_ref() to avoid unnecessary clone - Cow is already optimized
                    let content: &str = &span.content;
                    if content.is_empty() {
                        continue;
                    }

                    let span_style = line_style.patch(span.style);
                    let inline_style = self.inline_style_from_ratatui(span_style, fallback);
                    plain_line.push_str(content);
                    segments.push(InlineSegment {
                        text: content.to_string(),
                        style: Arc::new(inline_style),
                    });
                }

                converted_lines.push(segments);
                plain_lines.push(plain_line);
            }

            let needs_placeholder_line = if converted_lines.is_empty() {
                true
            } else {
                had_trailing_newline && plain_lines.last().is_none_or(|line| !line.is_empty())
            };
            if needs_placeholder_line {
                converted_lines.push(Vec::new());
                plain_lines.push(String::new());
            }

            return (converted_lines, plain_lines);
        }

        // Fallback: Process as plain text without ANSI parsing
        let line_count_estimate = text.as_bytes().iter().filter(|&&b| b == b'\n').count() + 1;
        let mut converted_lines = Vec::with_capacity(line_count_estimate);
        let mut plain_lines = Vec::with_capacity(line_count_estimate);

        for line in text.split('\n') {
            let mut segments = Vec::new();
            if !line.is_empty() {
                segments.push(InlineSegment {
                    text: line.to_string(),
                    style: Arc::clone(&fallback_arc),
                });
            }
            converted_lines.push(segments);
            plain_lines.push(line.to_string());
        }

        if had_trailing_newline {
            converted_lines.push(Vec::new());
            plain_lines.push(String::new());
        }

        if converted_lines.is_empty() {
            converted_lines.push(Vec::new());
            plain_lines.push(String::new());
        }

        (converted_lines, plain_lines)
    }

    fn write_multiline(
        &mut self,
        style: Style,
        indent: &str,
        text: &str,
        kind: InlineMessageKind,
    ) -> Result<()> {
        let text_storage;
        let text = if kind == InlineMessageKind::Agent {
            text_storage = crate::utils::ansi_parser::strip_ansi(text);
            &text_storage
        } else {
            text
        };

        if text.is_empty() {
            self.handle.append_line(kind, Vec::new());
            return Ok(());
        }

        let fallback = self.resolve_fallback_style(style);
        let fallback_arc = Arc::new(fallback.clone());
        let (converted_lines, plain_lines) = self.convert_plain_lines(text, &fallback);
        let record_transcript = Self::should_record_transcript(kind);

        // Combine multiple lines into a single append for User and Tool to avoid
        // creating a separate inline entry for each line. This prevents the
        // UI from showing a separate line per original line of tool output.
        if kind == InlineMessageKind::User || kind == InlineMessageKind::Tool {
            let mut combined_segments = Vec::new();
            let mut combined_plain = String::new();

            for (mut segments, plain) in converted_lines.into_iter().zip(plain_lines.into_iter()) {
                if !combined_segments.is_empty() {
                    combined_segments.push(InlineSegment {
                        text: "\n".to_owned(),
                        style: Arc::clone(&fallback_arc),
                    });
                    combined_plain.push('\n');
                }

                if !indent.is_empty() && !plain.is_empty() {
                    segments.insert(
                        0,
                        InlineSegment {
                            text: indent.to_string(),
                            style: Arc::clone(&fallback_arc),
                        },
                    );
                    combined_plain.insert_str(0, indent);
                } else if !indent.is_empty() && plain.is_empty() {
                    segments.insert(
                        0,
                        InlineSegment {
                            text: indent.to_string(),
                            style: Arc::clone(&fallback_arc),
                        },
                    );
                }

                combined_segments.extend(segments);
                combined_plain.push_str(&plain);
            }

            self.handle.append_line(kind, combined_segments);
            if record_transcript {
                crate::utils::transcript::append(&combined_plain);
            }
        } else {
            let fallback_arc_opt = if !indent.is_empty() {
                Some(Arc::new(fallback.clone()))
            } else {
                None
            };
            for (mut segments, mut plain) in
                converted_lines.into_iter().zip(plain_lines.into_iter())
            {
                if let Some(ref style_arc) = fallback_arc_opt
                    && !plain.is_empty()
                {
                    segments.insert(
                        0,
                        InlineSegment {
                            text: indent.to_string(),
                            style: Arc::clone(style_arc),
                        },
                    );
                    plain.insert_str(0, indent);
                }

                if segments.is_empty() {
                    self.handle.append_line(kind, Vec::new());
                } else {
                    self.handle.append_line(kind, segments);
                }
                if record_transcript {
                    crate::utils::transcript::append(&plain);
                }
            }
        }

        Ok(())
    }

    fn write_line(
        &mut self,
        style: Style,
        indent: &str,
        text: &str,
        kind: InlineMessageKind,
    ) -> Result<()> {
        self.write_multiline(style, indent, text, kind)
    }

    fn write_inline(&mut self, style: Style, text: &str, kind: InlineMessageKind) {
        if text.is_empty() {
            return;
        }
        let fallback = self.resolve_fallback_style(style);
        let fallback_arc = Arc::new(fallback.clone());
        let (converted_lines, _) = self.convert_plain_lines(text, &fallback);
        let line_count = converted_lines.len();

        for (index, segments) in converted_lines.into_iter().enumerate() {
            let has_next = index + 1 < line_count;
            if segments.is_empty() {
                if has_next {
                    self.handle.inline(
                        kind,
                        InlineSegment {
                            text: "\n".to_owned(),
                            style: Arc::clone(&fallback_arc),
                        },
                    );
                }
                continue;
            }

            for mut segment in segments {
                if has_next {
                    segment.text.push('\n');
                }
                self.handle.inline(kind, segment);
            }
        }
    }

    fn write_segments(
        &mut self,
        segments: &[MarkdownSegment],
        kind: InlineMessageKind,
    ) -> Result<()> {
        let converted = self.convert_segments(segments);
        let plain = segments
            .iter()
            .map(|segment| segment.text.clone())
            .collect::<String>();
        self.handle.append_line(kind, converted);
        if Self::should_record_transcript(kind) {
            crate::utils::transcript::append(&plain);
        }
        Ok(())
    }

    fn convert_segments(&self, segments: &[MarkdownSegment]) -> Vec<InlineSegment> {
        if segments.is_empty() {
            return Vec::new();
        }

        let mut converted = Vec::with_capacity(segments.len());
        for segment in segments {
            if segment.text.is_empty() {
                continue;
            }
            converted.push(self.style_to_segment(segment.style, &segment.text));
        }
        converted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_styles_construct() {
        let info = MessageStyle::Info.style();
        assert_eq!(info, MessageStyle::Info.style());
        let resp = MessageStyle::Response.style();
        assert_eq!(resp, MessageStyle::Response.style());
        let tool = MessageStyle::Tool.style();
        assert_eq!(tool, MessageStyle::Tool.style());
        let reasoning = MessageStyle::Reasoning.style();
        assert_eq!(reasoning, MessageStyle::Reasoning.style());
    }

    #[test]
    fn test_renderer_buffer() {
        let mut r = AnsiRenderer::stdout();
        r.push("hello");
        assert_eq!(r.buffer, "hello");
    }

    #[test]
    fn convert_plain_lines_preserves_ansi_styles() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let sink = InlineSink::new(InlineHandle { sender }, SyntaxHighlightingConfig::default());
        let fallback = InlineTextStyle {
            color: Some(AnsiColorEnum::Ansi(AnsiColor::Green)),
            bg_color: None,
            effects: Effects::new(),
        };

        let (converted, plain) =
            sink.convert_plain_lines("\u{1b}[31mred\u{1b}[0m plain", &fallback);

        assert_eq!(plain, vec!["red plain".to_owned()]);
        assert_eq!(converted.len(), 1);
        let segments = &converted[0];
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "red");
        assert_eq!(
            segments[0].style.color,
            Some(AnsiColorEnum::Ansi(AnsiColor::Red))
        );
        assert_eq!(segments[1].text, " plain");
        assert_eq!(segments[1].style.color, fallback.color);
    }

    #[test]
    fn convert_plain_lines_retains_trailing_newline() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let sink = InlineSink::new(InlineHandle { sender }, SyntaxHighlightingConfig::default());
        let fallback = InlineTextStyle::default();

        let (converted, plain) = sink.convert_plain_lines("hello\n", &fallback);

        assert_eq!(plain, vec!["hello".to_owned(), String::new()]);
        assert_eq!(converted.len(), 2);
        assert!(!converted[0].is_empty());
        assert!(converted[1].is_empty());
    }

    #[test]
    fn write_multiline_combines_tool_lines() {
        use crate::ui::InlineCommand;
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut sink =
            InlineSink::new(InlineHandle { sender }, SyntaxHighlightingConfig::default());
        let style = InlineTextStyle::default();
        // Use Tool kind to verify that multiple lines are combined into a single AppendLine command
        let kind = InlineMessageKind::Tool;
        let text = "one\ntwo\nthree";
        sink.write_multiline(style.to_ansi_style(None), "", text, kind)
            .unwrap();

        // We should receive exactly one AppendLine command
        let mut count = 0;
        while let Ok(command) = receiver.try_recv() {
            if let InlineCommand::AppendLine { .. } = command {
                count += 1;
            }
        }
        assert_eq!(count, 1);
    }

    #[test]
    fn prepare_markdown_lines_uses_syntax_highlighting_config() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut config = SyntaxHighlightingConfig::default();
        config.enabled = true;
        config.enabled_languages = vec!["rust".to_string()];
        let sink = InlineSink::new(InlineHandle { sender }, config);
        let base_style = MessageStyle::Response.style();
        let markdown = "```rust\nlet value = 1;\n```";

        let (prepared, plain, _) =
            sink.prepare_markdown_lines(markdown, "", base_style, true, false);

        let (segments, plain_line) = prepared
            .iter()
            .zip(plain.iter())
            .find(|(_, line)| line.contains("let value = 1;"))
            .expect("code line exists");

        assert!(
            segments.len() > 2,
            "expected highlighted segments, got {}, line: {}",
            segments.len(),
            plain_line
        );
    }

    #[test]
    fn line_function_no_trailing_empty_line() {
        use crate::utils::ansi_capabilities::AnsiCapabilities;
        use anstream::{AutoStream, ColorChoice};

        // Create a renderer that doesn't output to stdout
        let choice = ColorChoice::Never;
        let mut renderer = AnsiRenderer {
            writer: AutoStream::new(std::io::stdout(), choice),
            buffer: String::new(),
            color: false,
            sink: None,
            last_line_was_empty: false,
            highlight_config: SyntaxHighlightingConfig::default(),
            capabilities: AnsiCapabilities::detect(),
        };

        // This should not create an extra empty line after "line 2"
        renderer
            .line(MessageStyle::Tool, "line 1\nline 2\n")
            .unwrap();

        // Previously, this would have added an extra empty line due to the trailing \n
        // With our fix, it should only process the actual content lines
    }
}
