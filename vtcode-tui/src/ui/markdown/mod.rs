//! Markdown rendering utilities for terminal output with syntax highlighting support.

mod code_blocks;
mod links;
mod parsing;
mod tables;

use crate::config::loader::SyntaxHighlightingConfig;
use crate::ui::theme::{self, ThemeStyles};
use anstyle::Style;
use code_blocks::{
    CodeBlockRenderEnv, CodeBlockState, finalize_unclosed_code_block, handle_code_block_event,
};
use parsing::{
    LinkState, ListState, MarkdownContext, append_text, handle_end_tag, handle_start_tag,
    inline_code_style, push_blank_line, trim_trailing_blank_lines,
};
use pulldown_cmark::{Event, Options, Parser};
use tables::TableBuffer;
use unicode_width::UnicodeWidthStr;

pub use code_blocks::{
    HighlightedSegment, highlight_code_to_ansi, highlight_code_to_segments, highlight_line_for_diff,
};

pub(crate) const LIST_INDENT_WIDTH: usize = 2;
pub(crate) const CODE_LINE_NUMBER_MIN_WIDTH: usize = 3;

/// A styled text segment.
#[derive(Clone, Debug)]
pub struct MarkdownSegment {
    pub style: Style,
    pub text: String,
    pub link_target: Option<String>,
}

impl MarkdownSegment {
    pub(crate) fn new(style: Style, text: impl Into<String>) -> Self {
        Self {
            style,
            text: text.into(),
            link_target: None,
        }
    }

    pub(crate) fn with_link(
        style: Style,
        text: impl Into<String>,
        link_target: Option<String>,
    ) -> Self {
        Self {
            style,
            text: text.into(),
            link_target,
        }
    }
}

/// A rendered line composed of styled segments.
#[derive(Clone, Debug, Default)]
pub struct MarkdownLine {
    pub segments: Vec<MarkdownSegment>,
}

impl MarkdownLine {
    pub(crate) fn push_segment(&mut self, style: Style, text: &str) {
        self.push_segment_with_link(style, text, None);
    }

    pub(crate) fn push_segment_with_link(
        &mut self,
        style: Style,
        text: &str,
        link_target: Option<String>,
    ) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.segments.last_mut()
            && last.style == style
            && last.link_target == link_target
        {
            last.text.push_str(text);
            return;
        }
        self.segments
            .push(MarkdownSegment::with_link(style, text, link_target));
    }

    pub fn is_empty(&self) -> bool {
        self.segments
            .iter()
            .all(|segment| segment.text.trim().is_empty())
    }

    pub(crate) fn width(&self) -> usize {
        self.segments
            .iter()
            .map(|seg| UnicodeWidthStr::width(seg.text.as_str()))
            .sum()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RenderMarkdownOptions {
    pub preserve_code_indentation: bool,
    pub disable_code_block_table_reparse: bool,
}

/// Render markdown text to styled lines that can be written to the terminal renderer.
pub fn render_markdown_to_lines(
    source: &str,
    base_style: Style,
    theme_styles: &ThemeStyles,
    highlight_config: Option<&SyntaxHighlightingConfig>,
) -> Vec<MarkdownLine> {
    render_markdown_to_lines_with_options(
        source,
        base_style,
        theme_styles,
        highlight_config,
        RenderMarkdownOptions::default(),
    )
}

pub fn render_markdown_to_lines_with_options(
    source: &str,
    base_style: Style,
    theme_styles: &ThemeStyles,
    highlight_config: Option<&SyntaxHighlightingConfig>,
    render_options: RenderMarkdownOptions,
) -> Vec<MarkdownLine> {
    let parser_options = Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;

    let parser = Parser::new_ext(source, parser_options);

    let mut lines = Vec::new();
    let mut current_line = MarkdownLine::default();
    let mut style_stack = vec![base_style];
    let mut blockquote_depth = 0usize;
    let mut list_stack: Vec<ListState> = Vec::new();
    let mut list_continuation_prefix = String::new();
    let mut pending_list_prefix: Option<String> = None;
    let mut code_block: Option<CodeBlockState> = None;
    let mut active_table: Option<TableBuffer> = None;
    let mut link_state: Option<LinkState> = None;

    for event in parser {
        let mut code_block_env = code_block_render_env(
            &mut lines,
            &mut current_line,
            blockquote_depth,
            &list_continuation_prefix,
            &mut pending_list_prefix,
            base_style,
            theme_styles,
            highlight_config,
            render_options,
        );
        if handle_code_block_event(&event, &mut code_block, &mut code_block_env) {
            continue;
        }

        let mut ctx = MarkdownContext {
            style_stack: &mut style_stack,
            blockquote_depth: &mut blockquote_depth,
            list_stack: &mut list_stack,
            pending_list_prefix: &mut pending_list_prefix,
            list_continuation_prefix: &mut list_continuation_prefix,
            lines: &mut lines,
            current_line: &mut current_line,
            theme_styles,
            base_style,
            code_block: &mut code_block,
            active_table: &mut active_table,
            link_state: &mut link_state,
        };

        match event {
            Event::Start(ref tag) => handle_start_tag(tag, &mut ctx),
            Event::End(tag) => handle_end_tag(tag, &mut ctx),
            Event::Text(text) => append_text(&text, &mut ctx),
            Event::Code(code) => {
                ctx.ensure_prefix();
                ctx.current_line.push_segment_with_link(
                    inline_code_style(theme_styles, base_style),
                    &code,
                    ctx.active_link_target(),
                );
            }
            Event::SoftBreak | Event::HardBreak => ctx.flush_line(),
            Event::Rule => {
                ctx.flush_line();
                let mut line = MarkdownLine::default();
                line.push_segment(base_style.dimmed(), &"―".repeat(32));
                ctx.lines.push(line);
                push_blank_line(ctx.lines);
            }
            Event::TaskListMarker(checked) => {
                ctx.ensure_prefix();
                ctx.current_line
                    .push_segment(base_style, if checked { "[x] " } else { "[ ] " });
            }
            Event::Html(html) | Event::InlineHtml(html) => append_text(&html, &mut ctx),
            Event::FootnoteReference(r) => append_text(&format!("[^{}]", r), &mut ctx),
            Event::InlineMath(m) => append_text(&format!("${}$", m), &mut ctx),
            Event::DisplayMath(m) => append_text(&format!("$$\n{}\n$$", m), &mut ctx),
        }
    }

    let mut code_block_env = code_block_render_env(
        &mut lines,
        &mut current_line,
        blockquote_depth,
        &list_continuation_prefix,
        &mut pending_list_prefix,
        base_style,
        theme_styles,
        highlight_config,
        render_options,
    );
    finalize_unclosed_code_block(&mut code_block, &mut code_block_env);

    if !current_line.segments.is_empty() {
        lines.push(current_line);
    }

    trim_trailing_blank_lines(&mut lines);
    lines
}

/// Convenience helper that renders markdown using the active theme without emitting output.
pub fn render_markdown(source: &str) -> Vec<MarkdownLine> {
    let styles = theme::active_styles();
    render_markdown_to_lines(source, Style::default(), &styles, None)
}

fn code_block_render_env<'a>(
    lines: &'a mut Vec<MarkdownLine>,
    current_line: &'a mut MarkdownLine,
    blockquote_depth: usize,
    list_continuation_prefix: &'a str,
    pending_list_prefix: &'a mut Option<String>,
    base_style: Style,
    theme_styles: &'a ThemeStyles,
    highlight_config: Option<&'a SyntaxHighlightingConfig>,
    render_options: RenderMarkdownOptions,
) -> CodeBlockRenderEnv<'a> {
    CodeBlockRenderEnv {
        lines,
        current_line,
        blockquote_depth,
        list_continuation_prefix,
        pending_list_prefix,
        base_style,
        theme_styles,
        highlight_config,
        render_options,
    }
}

#[cfg(test)]
mod tests;
