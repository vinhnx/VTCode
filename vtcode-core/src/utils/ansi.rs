use crate::ui::iocraft::{
    IocraftHandle, IocraftSegment, convert_style as convert_to_iocraft_style, theme_from_styles,
};
use crate::ui::theme;
use crate::utils::transcript;
use anstream::{AutoStream, ColorChoice};
use anstyle::{Reset, Style};
use anstyle_query::{clicolor, clicolor_force, no_color, term_supports_color};
use anyhow::Result;
use std::io::{self, Write};

/// Styles available for rendering messages
#[derive(Clone, Copy)]
pub enum MessageStyle {
    Info,
    Error,
    Output,
    Response,
    Tool,
    User,
    Reasoning,
}

impl MessageStyle {
    fn style(self) -> Style {
        let styles = theme::active_styles();
        match self {
            Self::Info => styles.info,
            Self::Error => styles.error,
            Self::Output => styles.output,
            Self::Response => styles.response,
            Self::Tool => styles.tool,
            Self::User => styles.user,
            Self::Reasoning => styles.reasoning,
        }
    }

    fn indent(self) -> &'static str {
        match self {
            Self::Response | Self::Tool | Self::Reasoning => "  ",
            _ => "",
        }
    }
}

/// Renderer with deferred output buffering
pub struct AnsiRenderer {
    writer: AutoStream<io::Stdout>,
    buffer: String,
    color: bool,
    sink: Option<IocraftSink>,
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
        }
    }

    /// Create a renderer that forwards output to an iocraft session handle
    pub fn with_iocraft(handle: IocraftHandle) -> Self {
        let mut renderer = Self::stdout();
        renderer.sink = Some(IocraftSink::new(handle));
        renderer
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
            sink.write_line(style.style(), indent, &line)?;
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
        self.buffer.clear();
        Ok(())
    }

    /// Convenience for writing a single line
    pub fn line(&mut self, style: MessageStyle, text: &str) -> Result<()> {
        let indent = style.indent();

        if let Some(sink) = &mut self.sink {
            sink.write_multiline(style.style(), indent, text)?;
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
    pub fn inline_with_style(&mut self, style: Style, text: &str) -> Result<()> {
        if let Some(sink) = &mut self.sink {
            sink.write_inline(style, text);
            return Ok(());
        }
        if self.color {
            write!(self.writer, "{style}{}{Reset}", text)?;
        } else {
            write!(self.writer, "{}", text)?;
        }
        self.writer.flush()?;
        Ok(())
    }

    /// Write a line with an explicit style
    pub fn line_with_style(&mut self, style: Style, text: &str) -> Result<()> {
        if let Some(sink) = &mut self.sink {
            sink.write_multiline(style, "", text)?;
            return Ok(());
        }
        if self.color {
            writeln!(self.writer, "{style}{}{Reset}", text)?;
        } else {
            writeln!(self.writer, "{}", text)?;
        }
        self.writer.flush()?;
        transcript::append(text);
        Ok(())
    }

    /// Write a raw line without styling
    pub fn raw_line(&mut self, text: &str) -> Result<()> {
        writeln!(self.writer, "{}", text)?;
        self.writer.flush()?;
        transcript::append(text);
        Ok(())
    }
}

struct IocraftSink {
    handle: IocraftHandle,
}

impl IocraftSink {
    fn new(handle: IocraftHandle) -> Self {
        Self { handle }
    }

    fn style_to_segment(&self, style: Style, text: &str) -> IocraftSegment {
        let mut text_style = convert_to_iocraft_style(style);
        if text_style.color.is_none() {
            let theme = theme_from_styles(&theme::active_styles());
            text_style = text_style.merge_color(theme.foreground);
        }
        IocraftSegment {
            text: text.to_string(),
            style: text_style,
        }
    }

    fn write_multiline(&mut self, style: Style, indent: &str, text: &str) -> Result<()> {
        if text.is_empty() {
            self.handle.append_line(Vec::new());
            crate::utils::transcript::append("");
            return Ok(());
        }

        let mut lines = text.split('\n').peekable();
        let ends_with_newline = text.ends_with('\n');

        while let Some(line) = lines.next() {
            let mut content = String::new();
            if !indent.is_empty() && !line.is_empty() {
                content.push_str(indent);
            }
            content.push_str(line);
            if content.is_empty() {
                self.handle.append_line(Vec::new());
                crate::utils::transcript::append("");
            } else {
                let segment = self.style_to_segment(style, &content);
                self.handle.append_line(vec![segment]);
                crate::utils::transcript::append(&content);
            }
        }

        if ends_with_newline {
            self.handle.append_line(Vec::new());
            crate::utils::transcript::append("");
        }

        Ok(())
    }

    fn write_line(&mut self, style: Style, indent: &str, text: &str) -> Result<()> {
        if text.is_empty() {
            self.handle.append_line(Vec::new());
            crate::utils::transcript::append("");
            return Ok(());
        }
        let mut content = String::new();
        if !indent.is_empty() {
            content.push_str(indent);
        }
        content.push_str(text);
        let segment = self.style_to_segment(style, &content);
        self.handle.append_line(vec![segment]);
        crate::utils::transcript::append(&content);
        Ok(())
    }

    fn write_inline(&mut self, style: Style, text: &str) {
        if text.is_empty() {
            return;
        }
        let segment = self.style_to_segment(style, text);
        self.handle.inline(segment);
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
