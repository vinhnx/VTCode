use crate::config::loader::SyntaxHighlightingConfig;
use crate::ui::markdown::{MarkdownLine, MarkdownSegment, render_markdown_to_lines};
use crate::ui::theme;
use crate::ui::tui::{
    InlineHandle, InlineListItem, InlineListSearchConfig, InlineListSelection, InlineMessageKind,
    InlineSegment, InlineTextStyle, SecurePromptConfig, convert_style as convert_to_inline_style,
    theme_from_styles,
};
use crate::utils::transcript;
use anstream::{AutoStream, ColorChoice};
use anstyle::{Ansi256Color, AnsiColor, Color as AnsiColorEnum, Reset, RgbColor, Style};
use anstyle_query::{clicolor, clicolor_force, no_color, term_supports_color};
use anyhow::{Result, anyhow};
use ratatui::style::{Color as RatColor, Modifier as RatModifier, Style as RatatuiStyle};
use std::io::{self, Write};
use tui_markdown::from_str as parse_markdown_text;

/// Styles available for rendering messages
#[derive(Clone, Copy)]
pub enum MessageStyle {
    Info,
    Error,
    Output,
    Response,
    Tool,
    ToolDetail,
    Status,
    McpStatus,
    User,
    Reasoning,
}

impl MessageStyle {
    pub fn style(self) -> Style {
        let styles = theme::active_styles();
        match self {
            Self::Info => styles.info,
            Self::Error => styles.error,
            Self::Output => styles.output,
            Self::Response => styles.response,
            Self::Tool => styles.tool,
            Self::ToolDetail => styles.tool_detail,
            Self::Status => styles.status,
            Self::McpStatus => styles.mcp,
            Self::User => styles.user,
            Self::Reasoning => styles.reasoning,
        }
    }

    pub fn indent(self) -> &'static str {
        match self {
            Self::Response | Self::Tool | Self::Reasoning => "  ",
            Self::ToolDetail => "    ",
            _ => "",
        }
    }
}

/// Renderer with deferred output buffering
pub struct AnsiRenderer {
    writer: AutoStream<io::Stdout>,
    buffer: String,
    color: bool,
    sink: Option<InlineSink>,
    last_line_was_empty: bool,
    highlight_config: SyntaxHighlightingConfig,
}

impl AnsiRenderer {
    /// Create a new renderer for stdout
    pub fn stdout() -> Self {
        let color =
            clicolor_force() || (!no_color() && clicolor().unwrap_or_else(term_supports_color));
        let choice = if color {
            ColorChoice::Auto
        } else {
            ColorChoice::Never
        };
        Self {
            writer: AutoStream::new(std::io::stdout(), choice),
            buffer: String::new(),
            color,
            sink: None,
            last_line_was_empty: false,
            highlight_config: SyntaxHighlightingConfig::default(),
        }
    }

    /// Create a renderer that forwards output to the inline UI session handle
    pub fn with_inline_ui(
        handle: InlineHandle,
        highlight_config: SyntaxHighlightingConfig,
    ) -> Self {
        let mut renderer = Self::stdout();
        renderer.highlight_config = highlight_config;
        renderer.sink = Some(InlineSink::new(handle));
        renderer.last_line_was_empty = false;
        renderer
    }

    /// Override the syntax highlighting configuration.
    pub fn set_highlight_config(&mut self, config: SyntaxHighlightingConfig) {
        self.highlight_config = config;
    }

    /// Check if the last line rendered was empty
    pub fn was_previous_line_empty(&self) -> bool {
        self.last_line_was_empty
    }

    fn message_kind(style: MessageStyle) -> InlineMessageKind {
        match style {
            MessageStyle::Info => InlineMessageKind::Info,
            MessageStyle::Error => InlineMessageKind::Error,
            MessageStyle::Output => InlineMessageKind::Pty,
            MessageStyle::Response => InlineMessageKind::Agent,
            MessageStyle::Tool | MessageStyle::ToolDetail => InlineMessageKind::Tool,
            MessageStyle::Status | MessageStyle::McpStatus => InlineMessageKind::Info,
            MessageStyle::User => InlineMessageKind::User,
            MessageStyle::Reasoning => InlineMessageKind::Policy,
        }
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

    pub fn show_list_modal(
        &mut self,
        title: &str,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
    ) {
        if let Some(sink) = &self.sink {
            sink.show_list_modal(title.to_string(), lines, items, selected, search);
        }
    }

    pub fn show_secure_prompt_modal(
        &mut self,
        title: &str,
        lines: Vec<String>,
        prompt_label: String,
    ) {
        if let Some(sink) = &self.sink {
            sink.show_secure_prompt_modal(title.to_string(), lines, prompt_label);
        }
    }

    pub fn close_modal(&mut self) {
        if let Some(sink) = &self.sink {
            sink.close_modal();
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
            let line = self.buffer.clone();
            // Track if this line is empty
            self.last_line_was_empty = line.is_empty() && indent.is_empty();
            sink.write_line(style.style(), indent, &line, Self::message_kind(style))?;
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
        if matches!(style, MessageStyle::Response) {
            return self.render_markdown(style, text);
        }
        let indent = style.indent();

        if let Some(sink) = &mut self.sink {
            sink.write_multiline(style.style(), indent, text, Self::message_kind(style))?;
            return Ok(());
        }

        if text.contains('\n') {
            let trailing_newline = text.ends_with('\n');
            for line in text.lines() {
                self.buffer.clear();
                if !indent.is_empty() && !line.is_empty() {
                    self.buffer.push_str(indent);
                }
                self.buffer.push_str(line);
                self.flush(style)?;
            }
            if trailing_newline {
                self.buffer.clear();
                if !indent.is_empty() {
                    self.buffer.push_str(indent);
                }
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
        if let Some(sink) = &mut self.sink {
            sink.write_multiline(style, "", text, kind)?;
            self.last_line_was_empty = text.trim().is_empty();
            return Ok(());
        }
        if self.color {
            writeln!(self.writer, "{style}{}{Reset}", text)?;
        } else {
            writeln!(self.writer, "{}", text)?;
        }
        self.writer.flush()?;
        transcript::append(text);
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

    fn render_markdown(&mut self, style: MessageStyle, text: &str) -> Result<()> {
        let styles = theme::active_styles();
        let base_style = style.style();
        let indent = style.indent();
        if let Some(sink) = &mut self.sink {
            let last_empty =
                sink.write_markdown(text, indent, base_style, Self::message_kind(style))?;
            self.last_line_was_empty = last_empty;
            return Ok(());
        }
        let highlight_cfg = if self.highlight_config.enabled {
            Some(&self.highlight_config)
        } else {
            None
        };
        let mut lines = render_markdown_to_lines(text, base_style, &styles, highlight_cfg);
        if lines.is_empty() {
            lines.push(MarkdownLine::default());
        }
        for line in lines {
            self.write_markdown_line(style, indent, line)?;
        }
        Ok(())
    }

    pub fn stream_markdown_response(
        &mut self,
        text: &str,
        previous_line_count: usize,
    ) -> Result<usize> {
        let styles = theme::active_styles();
        let style = MessageStyle::Response;
        let base_style = style.style();
        let indent = style.indent();
        if let Some(sink) = &mut self.sink {
            let (prepared, plain_lines, last_empty) =
                sink.prepare_markdown_lines(text, indent, base_style);
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
        let mut lines = render_markdown_to_lines(text, base_style, &styles, highlight_cfg);
        if lines.is_empty() {
            lines.push(MarkdownLine::default());
        }

        Err(anyhow!("stream_markdown_response requires an inline sink"))
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

struct InlineSink {
    handle: InlineHandle,
}

impl InlineSink {
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
            RatColor::Gray => Some(AnsiColorEnum::Ansi(AnsiColor::White)),
            RatColor::DarkGray => Some(AnsiColorEnum::Ansi(AnsiColor::BrightBlack)),
            RatColor::LightRed => Some(AnsiColorEnum::Ansi(AnsiColor::BrightRed)),
            RatColor::LightGreen => Some(AnsiColorEnum::Ansi(AnsiColor::BrightGreen)),
            RatColor::LightYellow => Some(AnsiColorEnum::Ansi(AnsiColor::BrightYellow)),
            RatColor::LightBlue => Some(AnsiColorEnum::Ansi(AnsiColor::BrightBlue)),
            RatColor::LightMagenta => Some(AnsiColorEnum::Ansi(AnsiColor::BrightMagenta)),
            RatColor::LightCyan => Some(AnsiColorEnum::Ansi(AnsiColor::BrightCyan)),
            RatColor::White => Some(AnsiColorEnum::Ansi(AnsiColor::BrightWhite)),
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
        let removed = style.sub_modifier;

        if added.contains(RatModifier::BOLD) {
            resolved.bold = true;
        } else if removed.contains(RatModifier::BOLD) {
            resolved.bold = false;
        }

        if added.contains(RatModifier::ITALIC) {
            resolved.italic = true;
        } else if removed.contains(RatModifier::ITALIC) {
            resolved.italic = false;
        }

        resolved
    }

    fn prepare_markdown_lines(
        &self,
        text: &str,
        indent: &str,
        base_style: Style,
    ) -> (Vec<Vec<InlineSegment>>, Vec<String>, bool) {
        let fallback = self.resolve_fallback_style(base_style);
        let parsed = parse_markdown_text(text);
        let mut prepared = Vec::new();
        let mut plain = Vec::new();

        for line in parsed.lines.into_iter() {
            let mut segments = Vec::new();
            let mut plain_line = String::new();
            let line_style = RatatuiStyle::default()
                .patch(parsed.style)
                .patch(line.style);

            for span in line.spans.into_iter() {
                let content = span.content.into_owned();
                if content.is_empty() {
                    continue;
                }
                let span_style = line_style.patch(span.style);
                let inline_style = self.inline_style_from_ratatui(span_style, &fallback);
                plain_line.push_str(&content);
                segments.push(InlineSegment {
                    text: content,
                    style: inline_style,
                });
            }

            if !indent.is_empty() && !plain_line.is_empty() {
                segments.insert(
                    0,
                    InlineSegment {
                        text: indent.to_string(),
                        style: fallback.clone(),
                    },
                );
                plain_line.insert_str(0, indent);
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
    ) -> Result<bool> {
        let (prepared, plain, last_empty) = self.prepare_markdown_lines(text, indent, base_style);
        for (segments, line) in prepared.into_iter().zip(plain.iter()) {
            if segments.is_empty() {
                self.handle.append_line(kind, Vec::new());
            } else {
                self.handle.append_line(kind, segments);
            }
            crate::utils::transcript::append(line);
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
        crate::utils::transcript::replace_last(count, plain);
    }

    fn new(handle: InlineHandle) -> Self {
        Self { handle }
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
            }),
        );
    }

    fn close_modal(&self) {
        self.handle.close_modal();
    }

    fn resolve_fallback_style(&self, style: Style) -> InlineTextStyle {
        let mut text_style = convert_to_inline_style(style);
        if text_style.color.is_none() {
            let theme = theme_from_styles(&theme::active_styles());
            text_style = text_style.merge_color(theme.foreground);
        }
        text_style
    }

    fn style_to_segment(&self, style: Style, text: &str) -> InlineSegment {
        let text_style = self.resolve_fallback_style(style);
        InlineSegment {
            text: text.to_string(),
            style: text_style,
        }
    }

    fn convert_plain_lines(
        &self,
        text: &str,
        fallback: &InlineTextStyle,
    ) -> (Vec<Vec<InlineSegment>>, Vec<String>) {
        if text.is_empty() {
            return (vec![Vec::new()], vec![String::new()]);
        }

        let mut converted_lines = Vec::new();
        let mut plain_lines = Vec::new();

        for line in text.split('\n') {
            let mut segments = Vec::new();
            if !line.is_empty() {
                segments.push(InlineSegment {
                    text: line.to_string(),
                    style: fallback.clone(),
                });
            }
            converted_lines.push(segments);
            plain_lines.push(line.to_string());
        }

        if text.ends_with('\n') {
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
        if text.is_empty() {
            self.handle.append_line(kind, Vec::new());
            crate::utils::transcript::append("");
            return Ok(());
        }

        let fallback = self.resolve_fallback_style(style);
        let (converted_lines, plain_lines) = self.convert_plain_lines(text, &fallback);

        for (mut segments, mut plain) in converted_lines.into_iter().zip(plain_lines.into_iter()) {
            if !indent.is_empty() && !plain.is_empty() {
                segments.insert(
                    0,
                    InlineSegment {
                        text: indent.to_string(),
                        style: fallback.clone(),
                    },
                );
                plain.insert_str(0, indent);
            }

            if segments.is_empty() {
                self.handle.append_line(kind, Vec::new());
            } else {
                self.handle.append_line(kind, segments);
            }
            crate::utils::transcript::append(&plain);
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
        let (converted_lines, _) = self.convert_plain_lines(text, &fallback);
        let line_count = converted_lines.len();

        for (index, segments) in converted_lines.into_iter().enumerate() {
            let has_next = index + 1 < line_count;
            if segments.is_empty() {
                if has_next {
                    self.handle.inline(
                        kind,
                        InlineSegment {
                            text: "\n".to_string(),
                            style: fallback.clone(),
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
        crate::utils::transcript::append(&plain);
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
}
