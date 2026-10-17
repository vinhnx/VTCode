//! Markdown rendering utilities for terminal output with syntax highlighting support.

mod code_blocks;
mod links;
mod parsing;
mod tables;

use crate::tui::config::loader::SyntaxHighlightingConfig;
use crate::tui::ui::theme::{self, ThemeStyles};
use anstyle::Style;
use code_blocks::{CodeBlockRenderEnv, CodeBlockState, finalize_unclosed_code_block, handle_code_block_event};
use parsing::{
    LinkState, ListState, MarkdownContext, append_text, handle_end_tag, handle_start_tag, inline_code_style,
    push_blank_line, trim_trailing_blank_lines,
};
use pulldown_cmark::{Event, Options, Parser};
use tables::TableBuffer;
use unicode_width::UnicodeWidthStr;

pub(crate) use code_blocks::render_diff_content_segments;
pub use code_blocks::{
    HighlightedSegment, highlight_code_to_ansi, highlight_code_to_segments, highlight_line_for_diff,
};

pub(crate) const LIST_INDENT_WIDTH: usize = 2;
pub(crate) const CODE_LINE_NUMBER_MIN_WIDTH: usize = 3;

/// A styled text segment.
///
/// Keep field-compatible with the headless `vtcode_commons::ui_protocol::MarkdownSegment`
/// (this TUI variant is used when the `tui` feature is on); both are serialized into
/// the same inline-stream shapes.
#[derive(Clone, Debug)]
pub struct MarkdownSegment {
    pub style: Style,
    pub text: String,
    pub link_target: Option<Box<str>>,
}

impl MarkdownSegment {
    fn new(style: Style, text: impl Into<String>) -> Self {
        Self { style, text: text.into(), link_target: None }
    }

    fn with_link(style: Style, text: impl Into<String>, link_target: Option<impl Into<Box<str>>>) -> Self {
        Self {
            style,
            text: text.into(),
            link_target: link_target.map(Into::into),
        }
    }
}

/// A rendered line composed of styled segments.
#[derive(Clone, Debug, Default)]
pub struct MarkdownLine {
    pub segments: Vec<MarkdownSegment>,
    pub line_background: Option<anstyle::Color>,
}

impl MarkdownLine {
    fn set_line_background(&mut self, color: Option<anstyle::Color>) {
        self.line_background = color;
    }

    fn push_segment(&mut self, style: Style, text: &str) {
        self.push_segment_with_link(style, text, None::<String>);
    }

    fn push_segment_with_link(&mut self, style: Style, text: &str, link_target: Option<impl Into<Box<str>>>) {
        let link_target = link_target.map(Into::into);
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
        self.segments.push(MarkdownSegment::with_link(style, text, link_target));
    }

    pub fn is_empty(&self) -> bool {
        self.segments.iter().all(|segment| segment.text.trim().is_empty())
    }

    fn width(&self) -> usize {
        self.segments.iter().map(|seg| UnicodeWidthStr::width(seg.text.as_str())).sum()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RenderMarkdownOptions {
    pub preserve_code_indentation: bool,
    pub disable_code_block_table_reparse: bool,
    /// Available content width for tables. Headered tables keep a padded,
    /// width-aware grid while cells remain readable, then fall back to aligned
    /// labeled records when the grid becomes too cramped. Headerless tables
    /// retain their grid layout and scale columns to fit.
    pub table_max_width: Option<usize>,
}

/// Render markdown text to styled lines that can be written to the terminal renderer.
fn render_markdown_to_lines(
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
    // Plan wrappers are control markup rather than user-visible prose. Remove
    // them only outside fenced code blocks so ordinary markdown and code
    // examples remain lossless.
    let preprocessed = preprocess_plan_wrappers(source);
    let parser_options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_FOOTNOTES;

    let parser = Parser::new_ext(&preprocessed, parser_options);

    // Output lines track the source roughly 1:1, so size from the source line
    // count to avoid reallocations during per-message rendering.
    let mut lines = Vec::with_capacity(source.lines().count());
    let mut current_line = MarkdownLine::default();
    let mut style_stack = vec![base_style];
    let mut blockquote_depth = 0usize;
    // List nesting is shallow in practice; bound the stack up front.
    let mut list_stack: Vec<ListState> = Vec::with_capacity(4);
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
            table_max_width: render_options.table_max_width,
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
                ctx.current_line.push_segment(base_style, if checked { "[x] " } else { "[ ] " });
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                // Keep the event-level guard for split or oddly-cased tags;
                // other HTML is preserved as text.
                if !is_plan_markup_html(&html) {
                    append_text(&html, &mut ctx);
                }
            }
            Event::FootnoteReference(r) => append_text(&format!("[^{r}]"), &mut ctx),
            Event::InlineMath(m) => append_text(&format!("${m}$"), &mut ctx),
            Event::DisplayMath(m) => append_text(&format!("$$\n{m}\n$$"), &mut ctx),
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
pub(crate) fn render_markdown(source: &str) -> Vec<MarkdownLine> {
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

/// Plan wrappers that must never appear literally in rendered output.
const PLAN_MARKUP_TAGS: &[&str] = &["<proposed_plan>", "</proposed_plan>", "<plan>", "</plan>"];

fn is_plan_markup_html(html: &str) -> bool {
    let normalized: String = html.chars().filter(|ch| !ch.is_whitespace()).collect();
    let lowered = normalized.to_ascii_lowercase();
    PLAN_MARKUP_TAGS.iter().any(|tag| lowered.contains(tag))
}

fn strip_plan_markup_tags(text: &str, inline_code_ticks: &mut Option<usize>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;

    while cursor < text.len() {
        let remainder = &text[cursor..];
        if remainder.starts_with('`') {
            let run_length = remainder.bytes().take_while(|byte| *byte == b'`').count();
            out.push_str(&remainder[..run_length]);
            if inline_code_ticks.is_some_and(|ticks| ticks == run_length) {
                *inline_code_ticks = None;
            } else if inline_code_ticks.is_none() {
                *inline_code_ticks = Some(run_length);
            }
            cursor += run_length;
            continue;
        }

        if inline_code_ticks.is_none()
            && let Some(tag) = PLAN_MARKUP_TAGS.iter().find(|tag| {
                remainder
                    .get(..tag.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(tag))
            })
        {
            cursor += tag.len();
            continue;
        }

        let character = remainder.chars().next().expect("cursor is on a character boundary");
        out.push(character);
        cursor += character.len_utf8();
    }

    out
}

fn preprocess_plan_wrappers(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut may_have_plan_tags = None;
    let mut in_fenced_code = false;
    let mut inline_code_ticks = None;

    for (index, line) in source.lines().enumerate() {
        if index > 0 {
            out.push('\n');
        }

        let is_fence = is_fence_delimiter(line);
        if in_fenced_code || is_fence || !*may_have_plan_tags.get_or_insert_with(|| source.contains('<')) {
            out.push_str(line);
        } else {
            out.push_str(&strip_plan_markup_tags(line, &mut inline_code_ticks));
        }

        if is_fence {
            in_fenced_code = !in_fenced_code;
            inline_code_ticks = None;
        }
    }

    if source.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn is_fence_delimiter(line: &str) -> bool {
    vtcode_commons::formatting::is_markdown_fence_delimiter(line)
}

#[cfg(test)]
mod tests;
