//! Markdown rendering utilities for terminal output with syntax highlighting support.

use crate::config::loader::SyntaxHighlightingConfig;
use crate::ui::syntax_highlight::{find_syntax_by_token, load_theme, syntax_set};
use crate::ui::theme::{self, ThemeStyles};
use anstyle::Style;
use anstyle_syntect::to_anstyle;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::cmp::max;
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxReference;
use syntect::util::LinesWithEndings;
use tracing::warn;
use unicode_width::UnicodeWidthStr;

const LIST_INDENT_WIDTH: usize = 2;
const CODE_EXTRA_INDENT: &str = "    ";

#[derive(Clone, Debug)]
enum MarkdownEvent {
    Start(MarkdownTag),
    End(MarkdownTag),
    Text(String),
    Code(String),
    Html(String),
    SoftBreak,
    HardBreak,
    Rule,
    TaskListMarker(bool),
    FootnoteReference(String),
}

#[derive(Clone, Debug)]
enum MarkdownTag {
    Paragraph,
    Heading(HeadingLevel),
    BlockQuote,
    List(Option<usize>),
    Item,
    Emphasis,
    Strong,
    Strikethrough,
    Link,
    Image,
    CodeBlock(CodeBlockKind),
    Table,
    TableHead,
    TableRow,
    TableCell,
    FootnoteDefinition,
    HtmlBlock,
}

impl From<Tag<'_>> for MarkdownTag {
    fn from(tag: Tag) -> Self {
        match tag {
            Tag::Paragraph => MarkdownTag::Paragraph,
            Tag::Heading { level, .. } => MarkdownTag::Heading(heading_level_from_u8(level as u8)),
            Tag::BlockQuote(_) => MarkdownTag::BlockQuote,
            Tag::CodeBlock(kind) => MarkdownTag::CodeBlock(kind.into()),
            Tag::HtmlBlock => MarkdownTag::HtmlBlock,
            Tag::List(start) => MarkdownTag::List(start.map(|n| n as usize)),
            Tag::Item => MarkdownTag::Item,
            Tag::FootnoteDefinition(_) => MarkdownTag::FootnoteDefinition,
            Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {
                MarkdownTag::Paragraph
            }
            Tag::Table(_) => MarkdownTag::Table,
            Tag::TableHead => MarkdownTag::TableHead,
            Tag::TableRow => MarkdownTag::TableRow,
            Tag::TableCell => MarkdownTag::TableCell,
            Tag::Emphasis => MarkdownTag::Emphasis,
            Tag::Strong => MarkdownTag::Strong,
            Tag::Strikethrough => MarkdownTag::Strikethrough,
            Tag::Superscript | Tag::Subscript => MarkdownTag::Emphasis,
            Tag::Link { .. } => MarkdownTag::Link,
            Tag::Image { .. } => MarkdownTag::Image,
            Tag::MetadataBlock(_) => MarkdownTag::Paragraph, // fallback
        }
    }
}

impl From<pulldown_cmark::CodeBlockKind<'_>> for CodeBlockKind {
    fn from(kind: pulldown_cmark::CodeBlockKind) -> Self {
        match kind {
            pulldown_cmark::CodeBlockKind::Fenced(info) => {
                CodeBlockKind::Fenced(info.into_string())
            }
            pulldown_cmark::CodeBlockKind::Indented => CodeBlockKind::Indented,
        }
    }
}

impl From<TagEnd> for MarkdownTag {
    fn from(tag_end: TagEnd) -> Self {
        match tag_end {
            TagEnd::Paragraph => MarkdownTag::Paragraph,
            TagEnd::Heading(level) => MarkdownTag::Heading(heading_level_from_u8(level as u8)),
            TagEnd::BlockQuote(_) => MarkdownTag::BlockQuote,
            TagEnd::CodeBlock => MarkdownTag::CodeBlock(CodeBlockKind::Indented), // doesn't matter for end
            TagEnd::HtmlBlock => MarkdownTag::HtmlBlock,
            TagEnd::List(_) => MarkdownTag::List(None), // doesn't matter for end
            TagEnd::Item => MarkdownTag::Item,
            TagEnd::FootnoteDefinition => MarkdownTag::FootnoteDefinition,
            TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition => MarkdownTag::Paragraph,
            TagEnd::Table => MarkdownTag::Table,
            TagEnd::TableHead => MarkdownTag::TableHead,
            TagEnd::TableRow => MarkdownTag::TableRow,
            TagEnd::TableCell => MarkdownTag::TableCell,
            TagEnd::Emphasis => MarkdownTag::Emphasis,
            TagEnd::Strong => MarkdownTag::Strong,
            TagEnd::Strikethrough => MarkdownTag::Strikethrough,
            TagEnd::Superscript | TagEnd::Subscript => MarkdownTag::Emphasis,
            TagEnd::Link => MarkdownTag::Link,
            TagEnd::Image => MarkdownTag::Image,
            TagEnd::MetadataBlock(_) => MarkdownTag::Paragraph, // fallback
        }
    }
}

#[derive(Clone, Debug)]
enum CodeBlockKind {
    Fenced(String),
    Indented,
}

#[derive(Clone, Copy, Debug)]
enum HeadingLevel {
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
}

fn heading_level_from_u8(level: u8) -> HeadingLevel {
    match level {
        1 => HeadingLevel::H1,
        2 => HeadingLevel::H2,
        3 => HeadingLevel::H3,
        4 => HeadingLevel::H4,
        5 => HeadingLevel::H5,
        _ => HeadingLevel::H6,
    }
}

/// A styled text segment.
#[derive(Clone, Debug)]
pub struct MarkdownSegment {
    pub style: Style,
    pub text: String,
}

impl MarkdownSegment {
    pub(crate) fn new(style: Style, text: impl Into<String>) -> Self {
        Self {
            style,
            text: text.into(),
        }
    }
}

/// A rendered line composed of styled segments.
#[derive(Clone, Debug, Default)]
pub struct MarkdownLine {
    pub segments: Vec<MarkdownSegment>,
}

impl MarkdownLine {
    fn push_segment(&mut self, style: Style, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.segments.last_mut()
            && last.style == style
        {
            last.text.push_str(text);
            return;
        }
        self.segments.push(MarkdownSegment::new(style, text));
    }

    fn prepend_segments(&mut self, segments: &[PrefixSegment]) {
        if segments.is_empty() {
            return;
        }
        let mut prefixed = Vec::with_capacity(segments.len() + self.segments.len());
        for segment in segments {
            prefixed.push(MarkdownSegment::new(segment.style, segment.text.clone()));
        }
        prefixed.append(&mut self.segments);
        self.segments = prefixed;
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.segments
            .iter()
            .all(|segment| segment.text.trim().is_empty())
    }

    fn width(&self) -> usize {
        self.segments
            .iter()
            .map(|seg| UnicodeWidthStr::width(seg.text.as_str()))
            .sum()
    }
}

#[derive(Debug, Default)]
struct TableBuffer {
    headers: Vec<MarkdownLine>,
    rows: Vec<Vec<MarkdownLine>>,
    current_row: Vec<MarkdownLine>,
    in_head: bool,
}

#[derive(Clone, Debug)]
struct PrefixSegment {
    style: Style,
    text: String,
}

impl PrefixSegment {
    fn new(style: Style, text: impl Into<String>) -> Self {
        Self {
            style,
            text: text.into(),
        }
    }
}

#[derive(Clone, Debug)]
struct CodeBlockState {
    language: Option<String>,
    buffer: String,
}

#[derive(Clone, Debug)]
struct ListState {
    kind: ListKind,
    depth: usize,
    continuation: String,
}

#[derive(Clone, Debug)]
enum ListKind {
    Unordered,
    Ordered { next: usize },
}

/// Render markdown text to styled lines that can be written to the terminal renderer.
pub fn render_markdown_to_lines(
    source: &str,
    base_style: Style,
    theme_styles: &ThemeStyles,
    highlight_config: Option<&SyntaxHighlightingConfig>,
) -> Vec<MarkdownLine> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);

    let parser = Parser::new_ext(source, options);
    let events = collect_markdown_events(parser);

    let mut lines = Vec::new();
    let mut current_line = MarkdownLine::default();
    let mut style_stack = vec![base_style];
    let mut blockquote_depth = 0usize;
    let mut list_stack: Vec<ListState> = Vec::new();
    let mut pending_list_prefix: Option<String> = None;
    let mut code_block: Option<CodeBlockState> = None;
    let mut active_table: Option<TableBuffer> = None;
    let mut table_cell_index: usize = 0;

    for event in events {
        if let Some(state) = code_block.as_mut() {
            match event {
                MarkdownEvent::Text(text) => {
                    state.buffer.push_str(&text);
                    continue;
                }
                MarkdownEvent::End(MarkdownTag::CodeBlock(_)) => {
                    flush_current_line(
                        &mut lines,
                        &mut current_line,
                        blockquote_depth,
                        &list_stack,
                        &mut pending_list_prefix,
                        theme_styles,
                        base_style,
                    );
                    let prefix = build_prefix_segments(
                        blockquote_depth,
                        &list_stack,
                        theme_styles,
                        base_style,
                    );
                    let highlighted = highlight_code_block(
                        &state.buffer,
                        state.language.as_deref(),
                        highlight_config,
                        theme_styles,
                        base_style,
                        &prefix,
                    );
                    lines.extend(highlighted);
                    push_blank_line(&mut lines);
                    code_block = None;
                    continue;
                }
                _ => {}
            }
        }

        match event {
            MarkdownEvent::Start(tag) => {
                let mut context = MarkdownContext {
                    style_stack: &mut style_stack,
                    blockquote_depth: &mut blockquote_depth,
                    list_stack: &mut list_stack,
                    pending_list_prefix: &mut pending_list_prefix,
                    lines: &mut lines,
                    current_line: &mut current_line,
                    theme_styles,
                    base_style,
                    code_block: &mut code_block,
                    active_table: &mut active_table,
                    table_cell_index: &mut table_cell_index,
                };
                handle_start_tag(tag, &mut context);
            }
            MarkdownEvent::End(tag) => {
                let mut context = MarkdownContext {
                    style_stack: &mut style_stack,
                    blockquote_depth: &mut blockquote_depth,
                    list_stack: &mut list_stack,
                    pending_list_prefix: &mut pending_list_prefix,
                    lines: &mut lines,
                    current_line: &mut current_line,
                    theme_styles,
                    base_style,
                    code_block: &mut code_block,
                    active_table: &mut active_table,
                    table_cell_index: &mut table_cell_index,
                };
                handle_end_tag(tag, &mut context);
            }
            MarkdownEvent::Text(text) => {
                let mut context = MarkdownContext {
                    style_stack: &mut style_stack,
                    blockquote_depth: &mut blockquote_depth,
                    list_stack: &mut list_stack,
                    pending_list_prefix: &mut pending_list_prefix,
                    lines: &mut lines,
                    current_line: &mut current_line,
                    theme_styles,
                    base_style,
                    code_block: &mut code_block,
                    active_table: &mut active_table,
                    table_cell_index: &mut table_cell_index,
                };
                append_text(&text, &mut context);
            }
            MarkdownEvent::Code(code_text) => {
                ensure_prefix(
                    &mut current_line,
                    blockquote_depth,
                    &list_stack,
                    &mut pending_list_prefix,
                    theme_styles,
                    base_style,
                );
                current_line.push_segment(inline_code_style(theme_styles, base_style), &code_text);
            }
            MarkdownEvent::SoftBreak => {
                let mut context = MarkdownContext {
                    style_stack: &mut style_stack,
                    blockquote_depth: &mut blockquote_depth,
                    list_stack: &mut list_stack,
                    pending_list_prefix: &mut pending_list_prefix,
                    lines: &mut lines,
                    current_line: &mut current_line,
                    theme_styles,
                    base_style,
                    code_block: &mut code_block,
                    active_table: &mut active_table,
                    table_cell_index: &mut table_cell_index,
                };
                append_text(" ", &mut context);
            }
            MarkdownEvent::HardBreak => {
                flush_current_line(
                    &mut lines,
                    &mut current_line,
                    blockquote_depth,
                    &list_stack,
                    &mut pending_list_prefix,
                    theme_styles,
                    base_style,
                );
            }
            MarkdownEvent::Rule => {
                flush_current_line(
                    &mut lines,
                    &mut current_line,
                    blockquote_depth,
                    &list_stack,
                    &mut pending_list_prefix,
                    theme_styles,
                    base_style,
                );
                let mut line = MarkdownLine::default();
                let rule_style = theme_styles.secondary.bold();
                line.push_segment(rule_style, "―".repeat(32).as_str());
                lines.push(line);
                push_blank_line(&mut lines);
            }
            MarkdownEvent::TaskListMarker(checked) => {
                ensure_prefix(
                    &mut current_line,
                    blockquote_depth,
                    &list_stack,
                    &mut pending_list_prefix,
                    theme_styles,
                    base_style,
                );
                let marker = if checked { "[x] " } else { "[ ] " };
                current_line.push_segment(base_style, marker);
            }
            MarkdownEvent::Html(html) => {
                let mut context = MarkdownContext {
                    style_stack: &mut style_stack,
                    blockquote_depth: &mut blockquote_depth,
                    list_stack: &mut list_stack,
                    pending_list_prefix: &mut pending_list_prefix,
                    lines: &mut lines,
                    current_line: &mut current_line,
                    theme_styles,
                    base_style,
                    code_block: &mut code_block,
                    active_table: &mut active_table,
                    table_cell_index: &mut table_cell_index,
                };
                append_text(&html, &mut context);
            }
            MarkdownEvent::FootnoteReference(reference) => {
                let mut context = MarkdownContext {
                    style_stack: &mut style_stack,
                    blockquote_depth: &mut blockquote_depth,
                    list_stack: &mut list_stack,
                    pending_list_prefix: &mut pending_list_prefix,
                    lines: &mut lines,
                    current_line: &mut current_line,
                    theme_styles,
                    base_style,
                    code_block: &mut code_block,
                    active_table: &mut active_table,
                    table_cell_index: &mut table_cell_index,
                };
                append_text(&format!("[^{}]", reference), &mut context);
            }
        }
    }

    if let Some(state) = code_block {
        flush_current_line(
            &mut lines,
            &mut current_line,
            blockquote_depth,
            &list_stack,
            &mut pending_list_prefix,
            theme_styles,
            base_style,
        );
        let prefix = build_prefix_segments(blockquote_depth, &list_stack, theme_styles, base_style);
        let highlighted = highlight_code_block(
            &state.buffer,
            state.language.as_deref(),
            highlight_config,
            theme_styles,
            base_style,
            &prefix,
        );
        lines.extend(highlighted);
    }

    if !current_line.segments.is_empty() {
        lines.push(current_line);
    }

    trim_trailing_blank_lines(&mut lines);
    lines
}

/// Convenience helper that renders markdown using the active theme without emitting output.
///
/// Returns the styled lines so callers can perform custom handling or assertions in tests.
pub fn render_markdown(source: &str) -> Vec<MarkdownLine> {
    let styles = theme::active_styles();
    render_markdown_to_lines(source, Style::default(), &styles, None)
}

fn collect_markdown_events<'a>(parser: Parser<'a>) -> Vec<MarkdownEvent> {
    parser
        .map(|event| {
            #[allow(unreachable_patterns)]
            match event {
                Event::Start(tag) => MarkdownEvent::Start(tag.into()),
                Event::End(tag_end) => MarkdownEvent::End(tag_end.into()),
                Event::Text(text) => MarkdownEvent::Text(text.into_string()),
                Event::Code(code) => MarkdownEvent::Code(code.into_string()),
                Event::Html(html) => MarkdownEvent::Html(html.into_string()),
                Event::FootnoteReference(ref_str) => {
                    MarkdownEvent::FootnoteReference(ref_str.into_string())
                }
                Event::SoftBreak => MarkdownEvent::SoftBreak,
                Event::HardBreak => MarkdownEvent::HardBreak,
                Event::Rule => MarkdownEvent::Rule,
                Event::TaskListMarker(checked) => MarkdownEvent::TaskListMarker(checked),
                Event::InlineHtml(html) => MarkdownEvent::Html(html.into_string()),
                Event::InlineMath(math) => MarkdownEvent::Text(format!("${}$", math.into_string())),
                Event::DisplayMath(math) => {
                    MarkdownEvent::Text(format!("$$\n{}\n$$", math.into_string()))
                }
                other => {
                    warn!(?other, "Unhandled pulldown-cmark event variant");
                    MarkdownEvent::Text(String::new())
                }
            }
        })
        .collect()
}

struct MarkdownContext<'a> {
    style_stack: &'a mut Vec<Style>,
    blockquote_depth: &'a mut usize,
    list_stack: &'a mut Vec<ListState>,
    pending_list_prefix: &'a mut Option<String>,
    lines: &'a mut Vec<MarkdownLine>,
    current_line: &'a mut MarkdownLine,
    theme_styles: &'a ThemeStyles,
    base_style: Style,
    code_block: &'a mut Option<CodeBlockState>,
    active_table: &'a mut Option<TableBuffer>,
    table_cell_index: &'a mut usize,
}

fn handle_start_tag(tag: MarkdownTag, context: &mut MarkdownContext<'_>) {
    match tag {
        MarkdownTag::Paragraph => {}
        MarkdownTag::Heading(level) => {
            context.style_stack.push(heading_style(
                level,
                context.theme_styles,
                context.base_style,
            ));
        }
        MarkdownTag::BlockQuote => {
            *context.blockquote_depth += 1;
        }
        MarkdownTag::List(start) => {
            let depth = context.list_stack.len();
            let kind = start
                .map(|value| ListKind::Ordered {
                    next: max(1, value),
                })
                .unwrap_or(ListKind::Unordered);
            context.list_stack.push(ListState {
                kind,
                depth,
                continuation: String::new(),
            });
        }
        MarkdownTag::Item => {
            if let Some(state) = context.list_stack.last_mut() {
                let indent = " ".repeat(state.depth * LIST_INDENT_WIDTH);
                match &mut state.kind {
                    ListKind::Unordered => {
                        // Use better bullet character: • (U+2022) for nested, then ◦, ▪
                        let bullet_char = match state.depth % 3 {
                            0 => "•",
                            1 => "◦",
                            _ => "▪",
                        };
                        let bullet = format!("{}{} ", indent, bullet_char);
                        state.continuation = format!("{}  ", indent);
                        *context.pending_list_prefix = Some(bullet);
                    }
                    ListKind::Ordered { next } => {
                        let bullet = format!("{}{}. ", indent, *next);
                        let width = bullet.len().saturating_sub(indent.len());
                        state.continuation = format!("{}{}", indent, " ".repeat(width));
                        *context.pending_list_prefix = Some(bullet);
                        *next += 1;
                    }
                }
            }
        }
        MarkdownTag::Emphasis => {
            let style = context
                .style_stack
                .last()
                .copied()
                .unwrap_or(context.base_style)
                .italic();
            context.style_stack.push(style);
        }
        MarkdownTag::Strong => {
            let style = context
                .style_stack
                .last()
                .copied()
                .unwrap_or(context.base_style)
                .bold();
            context.style_stack.push(style);
        }
        MarkdownTag::Strikethrough => {
            let style = context
                .style_stack
                .last()
                .copied()
                .unwrap_or(context.base_style)
                .strikethrough();
            context.style_stack.push(style);
        }
        MarkdownTag::Link | MarkdownTag::Image => {
            let style = context
                .style_stack
                .last()
                .copied()
                .unwrap_or(context.base_style)
                .underline();
            context.style_stack.push(style);
        }
        MarkdownTag::CodeBlock(kind) => {
            let language = match kind {
                CodeBlockKind::Fenced(info) => info
                    .split_whitespace()
                    .next()
                    .filter(|lang| !lang.is_empty())
                    .map(|lang| lang.to_owned()),
                CodeBlockKind::Indented => None,
            };
            *context.code_block = Some(CodeBlockState {
                language,
                buffer: String::new(),
            });
        }
        MarkdownTag::Table => {
            // Begin table rendering - initialize buffer
            // First flush any pending content
            flush_current_line(
                context.lines,
                context.current_line,
                *context.blockquote_depth,
                context.list_stack,
                context.pending_list_prefix,
                context.theme_styles,
                context.base_style,
            );
            push_blank_line(context.lines);
            *context.active_table = Some(TableBuffer::default());
            *context.table_cell_index = 0;
        }
        MarkdownTag::TableRow => {
            // New row
            if let Some(table) = context.active_table {
                table.current_row.clear();
            } else {
                // Fallback if not in table mode (shouldn't happen with valid markdown)
                flush_current_line(
                    context.lines,
                    context.current_line,
                    *context.blockquote_depth,
                    context.list_stack,
                    context.pending_list_prefix,
                    context.theme_styles,
                    context.base_style,
                );
            }
            *context.table_cell_index = 0;
        }
        MarkdownTag::TableHead => {
            if let Some(table) = context.active_table {
                table.in_head = true;
            }
        }
        MarkdownTag::TableCell => {
            // Ensure current line is clear for capturing cell content
            // We do NOT write separators here anymore, we do it in render_table
            if context.active_table.is_none() {
                ensure_prefix(
                    context.current_line,
                    *context.blockquote_depth,
                    context.list_stack,
                    context.pending_list_prefix,
                    context.theme_styles,
                    context.base_style,
                );
            } else {
                // Just clear the line so we capture fresh content
                // If there's garbage in current_line, it should have been flushed by previous tags
                context.current_line.segments.clear();
            }
            *context.table_cell_index += 1;
        }
        MarkdownTag::FootnoteDefinition | MarkdownTag::HtmlBlock => {}
    }
}

fn handle_end_tag(tag: MarkdownTag, context: &mut MarkdownContext<'_>) {
    match tag {
        MarkdownTag::Paragraph => {
            flush_current_line(
                context.lines,
                context.current_line,
                *context.blockquote_depth,
                context.list_stack,
                context.pending_list_prefix,
                context.theme_styles,
                context.base_style,
            );
            push_blank_line(context.lines);
        }
        MarkdownTag::Heading(..) => {
            flush_current_line(
                context.lines,
                context.current_line,
                *context.blockquote_depth,
                context.list_stack,
                context.pending_list_prefix,
                context.theme_styles,
                context.base_style,
            );
            if !context.style_stack.is_empty() {
                context.style_stack.pop();
            }
            push_blank_line(context.lines);
        }
        MarkdownTag::BlockQuote => {
            flush_current_line(
                context.lines,
                context.current_line,
                *context.blockquote_depth,
                context.list_stack,
                context.pending_list_prefix,
                context.theme_styles,
                context.base_style,
            );
            if *context.blockquote_depth > 0 {
                *context.blockquote_depth -= 1;
            }
        }
        MarkdownTag::List(_) => {
            flush_current_line(
                context.lines,
                context.current_line,
                *context.blockquote_depth,
                context.list_stack,
                context.pending_list_prefix,
                context.theme_styles,
                context.base_style,
            );
            if context.list_stack.pop().is_some() {
                if let Some(state) = context.list_stack.last() {
                    context
                        .pending_list_prefix
                        .replace(state.continuation.clone());
                } else {
                    context.pending_list_prefix.take();
                }
            }
            push_blank_line(context.lines);
        }
        MarkdownTag::Item => {
            flush_current_line(
                context.lines,
                context.current_line,
                *context.blockquote_depth,
                context.list_stack,
                context.pending_list_prefix,
                context.theme_styles,
                context.base_style,
            );
            if let Some(state) = context.list_stack.last() {
                context
                    .pending_list_prefix
                    .replace(state.continuation.clone());
            }
        }
        MarkdownTag::Emphasis
        | MarkdownTag::Strong
        | MarkdownTag::Strikethrough
        | MarkdownTag::Link
        | MarkdownTag::Image => {
            context.style_stack.pop();
        }
        MarkdownTag::CodeBlock(_) => {}
        MarkdownTag::Table => {
            // End table rendering - render buffered table
            if let Some(mut table) = context.active_table.take() {
                // Validate if current_row has data (unlikely for Table end, usually TableRow end)
                if !table.current_row.is_empty() {
                    table.rows.push(std::mem::take(&mut table.current_row));
                }

                let rendered_lines = render_table(
                    &table,
                    context.theme_styles,
                    context.base_style,
                    *context.blockquote_depth,
                    context.list_stack,
                    context.pending_list_prefix.clone(),
                );
                context.lines.extend(rendered_lines);
            }

            push_blank_line(context.lines);
            *context.table_cell_index = 0;
        }
        MarkdownTag::TableRow => {
            // End of row
            if let Some(table) = context.active_table {
                if table.in_head {
                    // Collect into headers
                    // We assume table head has only one row usually, but if multiple, we might overwrite or append?
                    // Standard markdown table has one header row.
                    // If current_row is not empty, move it to headers
                    table.headers = std::mem::take(&mut table.current_row);
                } else {
                    table.rows.push(std::mem::take(&mut table.current_row));
                }
            } else {
                flush_current_line(
                    context.lines,
                    context.current_line,
                    *context.blockquote_depth,
                    context.list_stack,
                    context.pending_list_prefix,
                    context.theme_styles,
                    context.base_style,
                );
            }
            *context.table_cell_index = 0;
        }
        MarkdownTag::TableCell => {
            // End of cell - capture content
            if let Some(table) = context.active_table {
                table.current_row.push(std::mem::take(context.current_line));
            }
        }
        MarkdownTag::TableHead => {
            if let Some(table) = context.active_table {
                table.in_head = false;
            }
        }
        MarkdownTag::FootnoteDefinition | MarkdownTag::HtmlBlock => {}
    }
}

fn render_table(
    table: &TableBuffer,
    theme_styles: &ThemeStyles,
    base_style: Style,
    blockquote_depth: usize,
    list_stack: &[ListState],
    pending_list_prefix: Option<String>,
) -> Vec<MarkdownLine> {
    let mut lines = Vec::new();
    let mut pending_prefix = pending_list_prefix; // local mutable copy

    if table.headers.is_empty() && table.rows.is_empty() {
        return lines;
    }

    // Calculate column widths
    let mut col_widths: Vec<usize> = Vec::new();

    // Check headers
    for (i, cell) in table.headers.iter().enumerate() {
        if i >= col_widths.len() {
            col_widths.push(0);
        }
        col_widths[i] = max(col_widths[i], cell.width());
    }

    // Check rows
    for row in &table.rows {
        for (i, cell) in row.iter().enumerate() {
            if i >= col_widths.len() {
                col_widths.push(0);
            }
            col_widths[i] = max(col_widths[i], cell.width());
        }
    }

    let border_style = theme_styles.secondary.dimmed(); // faint-ish border

    // Render Headers
    if !table.headers.is_empty() {
        let cells = &table.headers;
        let mut line = MarkdownLine::default();

        ensure_prefix(
            &mut line,
            blockquote_depth,
            list_stack,
            &mut pending_prefix,
            theme_styles,
            base_style,
        );

        line.push_segment(border_style, "│ ");

        for (i, width) in col_widths.iter().enumerate() {
            let cell = cells.get(i);
            let cell_width = cell.map(|c| c.width()).unwrap_or(0);
            let padding = width.saturating_sub(cell_width);

            if let Some(c) = cell {
                for seg in &c.segments {
                    line.push_segment(seg.style.bold(), &seg.text);
                }
            }

            if padding > 0 {
                line.push_segment(base_style, &" ".repeat(padding));
            }

            line.push_segment(border_style, " │ ");
        }
        lines.push(line);

        // Render Separator
        let mut sep_line = MarkdownLine::default();
        ensure_prefix(
            &mut sep_line,
            blockquote_depth,
            list_stack,
            &mut pending_prefix,
            theme_styles,
            base_style,
        );

        sep_line.push_segment(border_style, "├─");
        for (i, width) in col_widths.iter().enumerate() {
            let dash_count = *width;
            sep_line.push_segment(border_style, &"─".repeat(dash_count));

            if i < col_widths.len() - 1 {
                sep_line.push_segment(border_style, "─┼─");
            } else {
                sep_line.push_segment(border_style, "─┤");
            }
        }
        lines.push(sep_line);
    }

    // Render Rows
    for row in &table.rows {
        let cells = row;
        let mut line = MarkdownLine::default();

        ensure_prefix(
            &mut line,
            blockquote_depth,
            list_stack,
            &mut pending_prefix,
            theme_styles,
            base_style,
        );

        line.push_segment(border_style, "│ ");

        for (i, width) in col_widths.iter().enumerate() {
            let cell = cells.get(i);
            let cell_width = cell.map(|c| c.width()).unwrap_or(0);
            let padding = width.saturating_sub(cell_width);

            if let Some(c) = cell {
                for seg in &c.segments {
                    line.push_segment(seg.style, &seg.text);
                }
            }

            if padding > 0 {
                line.push_segment(base_style, &" ".repeat(padding));
            }

            line.push_segment(border_style, " │ ");
        }
        lines.push(line);
    }

    lines
}

fn append_text(text: &str, context: &mut MarkdownContext<'_>) {
    let style = context
        .style_stack
        .last()
        .copied()
        .unwrap_or(context.base_style);

    let mut start = 0usize;
    let mut chars = text.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch == '\n' {
            let segment = &text[start..idx];
            if !segment.is_empty() {
                ensure_prefix(
                    context.current_line,
                    *context.blockquote_depth,
                    context.list_stack,
                    context.pending_list_prefix,
                    context.theme_styles,
                    context.base_style,
                );
                context.current_line.push_segment(style, segment);
            }
            context.lines.push(std::mem::take(context.current_line));
            start = idx + ch.len_utf8();

            // Skip consecutive newlines to prevent multiple blank lines
            // Each sequence of consecutive newlines should result in only one blank line
            while let Some(&(_, next_ch)) = chars.peek() {
                if next_ch != '\n' {
                    break;
                }
                chars.next(); // consume the additional newline
                start += next_ch.len_utf8(); // increment start by character length
            }

            // After skipping consecutive newlines, if there are more consecutive newlines,
            // we need to ensure we don't add multiple blank lines.
            // The first \n already added a line (above), so we only need one blank line per sequence
        }
    }

    if start < text.len() {
        let remaining = &text[start..];
        if !remaining.is_empty() {
            ensure_prefix(
                context.current_line,
                *context.blockquote_depth,
                context.list_stack,
                context.pending_list_prefix,
                context.theme_styles,
                context.base_style,
            );
            context.current_line.push_segment(style, remaining);
        }
    }
}

fn ensure_prefix(
    current_line: &mut MarkdownLine,
    blockquote_depth: usize,
    list_stack: &[ListState],
    pending_list_prefix: &mut Option<String>,
    theme_styles: &ThemeStyles,
    base_style: Style,
) {
    if !current_line.segments.is_empty() {
        return;
    }

    for _ in 0..blockquote_depth {
        current_line.push_segment(theme_styles.secondary.italic(), "│ ");
    }

    if let Some(prefix) = pending_list_prefix.take() {
        current_line.push_segment(base_style, &prefix);
    } else if !list_stack.is_empty() {
        let mut continuation = String::new();
        for state in list_stack {
            continuation.push_str(&state.continuation);
        }
        if !continuation.is_empty() {
            current_line.push_segment(base_style, &continuation);
        }
    }
}

fn flush_current_line(
    lines: &mut Vec<MarkdownLine>,
    current_line: &mut MarkdownLine,
    blockquote_depth: usize,
    list_stack: &[ListState],
    pending_list_prefix: &mut Option<String>,
    theme_styles: &ThemeStyles,
    base_style: Style,
) {
    if current_line.segments.is_empty() && pending_list_prefix.is_some() {
        ensure_prefix(
            current_line,
            blockquote_depth,
            list_stack,
            pending_list_prefix,
            theme_styles,
            base_style,
        );
    }

    if !current_line.segments.is_empty() {
        lines.push(std::mem::take(current_line));
    }
}

fn push_blank_line(lines: &mut Vec<MarkdownLine>) {
    if lines
        .last()
        .map(|line| line.segments.is_empty())
        .unwrap_or(false)
    {
        return;
    }
    lines.push(MarkdownLine::default());
}

fn trim_trailing_blank_lines(lines: &mut Vec<MarkdownLine>) {
    while lines
        .last()
        .map(|line| line.segments.is_empty())
        .unwrap_or(false)
    {
        lines.pop();
    }
}

fn inline_code_style(theme_styles: &ThemeStyles, base_style: Style) -> Style {
    let fg = theme_styles
        .secondary
        .get_fg_color()
        .or_else(|| base_style.get_fg_color());
    let mut style = base_style;
    if let Some(fg_color) = fg {
        style = style.fg_color(Some(fg_color));
    }
    style.bold()
}

fn heading_style(level: HeadingLevel, theme_styles: &ThemeStyles, base_style: Style) -> Style {
    match level {
        HeadingLevel::H1 => theme_styles.primary.bold().underline(),
        HeadingLevel::H2 => theme_styles.primary.bold(),
        HeadingLevel::H3 => theme_styles.secondary.bold(),
        _ => base_style.bold(),
    }
}

fn build_prefix_segments(
    blockquote_depth: usize,
    list_stack: &[ListState],
    theme_styles: &ThemeStyles,
    base_style: Style,
) -> Vec<PrefixSegment> {
    let mut segments = Vec::new();
    for _ in 0..blockquote_depth {
        segments.push(PrefixSegment::new(theme_styles.secondary.italic(), "│ "));
    }
    if !list_stack.is_empty() {
        let mut continuation = String::new();
        for state in list_stack {
            continuation.push_str(&state.continuation);
        }
        if !continuation.is_empty() {
            segments.push(PrefixSegment::new(base_style, continuation));
        }
    }
    segments
}

fn highlight_code_block(
    code: &str,
    language: Option<&str>,
    highlight_config: Option<&SyntaxHighlightingConfig>,
    theme_styles: &ThemeStyles,
    base_style: Style,
    prefix_segments: &[PrefixSegment],
) -> Vec<MarkdownLine> {
    let mut lines = Vec::new();
    let mut augmented_prefix = prefix_segments.to_vec();
    augmented_prefix.push(PrefixSegment::new(base_style, CODE_EXTRA_INDENT));

    // Always normalize indentation first, regardless of highlighting
    let normalized_code = normalize_code_indentation(code, language);
    let code_to_display = &normalized_code;

    if let Some(config) = highlight_config.filter(|cfg| cfg.enabled)
        && let Some(highlighted) = try_highlight(code_to_display, language, config)
    {
        for segments in highlighted {
            let mut line = MarkdownLine::default();
            line.prepend_segments(&augmented_prefix);
            for (style, text) in segments {
                line.push_segment(style, &text);
            }
            lines.push(line);
        }
        return lines;
    }

    // Fallback: render without syntax highlighting, but still with normalized indentation
    for raw_line in LinesWithEndings::from(code_to_display) {
        let trimmed = raw_line.trim_end_matches('\n');
        let mut line = MarkdownLine::default();
        line.prepend_segments(&augmented_prefix);
        if !trimmed.is_empty() {
            line.push_segment(code_block_style(theme_styles, base_style), trimmed);
        }
        lines.push(line);
    }

    if code_to_display.ends_with('\n') {
        let mut line = MarkdownLine::default();
        line.prepend_segments(&augmented_prefix);
        lines.push(line);
    }

    lines
}

fn code_block_style(theme_styles: &ThemeStyles, base_style: Style) -> Style {
    let fg = theme_styles
        .output
        .get_fg_color()
        .or_else(|| base_style.get_fg_color());
    let mut style = base_style;
    if let Some(color) = fg {
        style = style.fg_color(Some(color));
    }
    style
}

/// Normalize indentation in code blocks.
///
/// This function strips common leading indentation when ALL non-empty lines have at least
/// that much indentation. It preserves the relative indentation structure within the code block.
fn normalize_code_indentation(code: &str, language: Option<&str>) -> String {
    // Check if we should normalize based on language hint
    let has_language_hint = language.is_some_and(|hint| {
        matches!(
            hint.to_lowercase().as_str(),
            "rust"
                | "rs"
                | "python"
                | "py"
                | "javascript"
                | "js"
                | "jsx"
                | "typescript"
                | "ts"
                | "tsx"
                | "go"
                | "golang"
                | "java"
                | "bash"
                | "sh"
        )
    });

    // Always normalize indentation regardless of language (applies to plain text code too)
    // This ensures consistently formatted output
    if !has_language_hint && language.is_some() {
        // If there's a language hint but it's not supported, don't normalize
        return code.to_string();
    }

    let lines: Vec<&str> = code.lines().collect();

    // Get minimum indentation across all non-empty lines
    // Find the longest common leading whitespace prefix across all non-empty lines
    let min_indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| &line[..line.len() - line.trim_start().len()])
        .reduce(|acc, p| {
            let mut len = 0;
            for (c1, c2) in acc.chars().zip(p.chars()) {
                if c1 != c2 {
                    break;
                }
                len += c1.len_utf8();
            }
            &acc[..len]
        })
        .map(|s| s.len())
        .unwrap_or(0);

    // Remove the common leading indentation from all lines, preserving relative indentation
    let normalized = lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                line // preserve empty lines as-is
            } else if line.len() >= min_indent {
                &line[min_indent..] // remove common indentation
            } else {
                line // line is shorter than min_indent, return as-is
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Preserve trailing newline if original had one
    if code.ends_with('\n') {
        format!("{normalized}\n")
    } else {
        normalized
    }
}

/// Highlight a single line of code for diff preview.
/// Returns a vector of (style, text) segments, or None if highlighting fails.
pub fn highlight_line_for_diff(line: &str, language: Option<&str>) -> Option<Vec<(Style, String)>> {
    let syntax = select_syntax(language);
    let theme = get_theme("base16-ocean.dark", true);
    let mut highlighter = HighlightLines::new(syntax, &theme);

    let ranges = highlighter.highlight_line(line, syntax_set()).ok()?;
    let mut segments = Vec::new();
    for (style, part) in ranges {
        if part.is_empty() {
            continue;
        }
        let mut anstyle = to_anstyle(style);
        anstyle = anstyle.bg_color(None);
        segments.push((anstyle, part.to_owned()));
    }
    Some(segments)
}

fn try_highlight(
    code: &str,
    language: Option<&str>,
    config: &SyntaxHighlightingConfig,
) -> Option<Vec<Vec<(Style, String)>>> {
    let max_bytes = config.max_file_size_mb.saturating_mul(1024 * 1024);
    if max_bytes > 0 && code.len() > max_bytes {
        return None;
    }

    if let Some(lang) = language {
        let enabled = config
            .enabled_languages
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(lang));
        if !enabled {
            return None;
        }
    }

    let syntax = select_syntax(language);
    let theme = get_theme(&config.theme, config.cache_themes);
    let mut highlighter = HighlightLines::new(syntax, &theme);
    let mut rendered = Vec::new();

    let mut ends_with_newline = false;
    for line in LinesWithEndings::from(code) {
        ends_with_newline = line.ends_with('\n');
        let trimmed = line.trim_end_matches('\n');
        let ranges = highlighter.highlight_line(trimmed, syntax_set()).ok()?;
        let mut segments = Vec::new();
        for (style, part) in ranges {
            if part.is_empty() {
                continue;
            }
            let mut anstyle = to_anstyle(style);
            // Strip background color to avoid filled backgrounds in terminal
            anstyle = anstyle.bg_color(None);
            segments.push((anstyle, part.to_owned()));
        }
        rendered.push(segments);
    }

    if ends_with_newline {
        rendered.push(Vec::new());
    }

    Some(rendered)
}

fn select_syntax(language: Option<&str>) -> &'static SyntaxReference {
    language
        .map(|lang| find_syntax_by_token(lang))
        .unwrap_or_else(|| syntax_set().find_syntax_plain_text())
}

fn get_theme(theme_name: &str, cache: bool) -> Theme {
    load_theme(theme_name, cache)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_unordered_list_bullets() {
        let markdown = r#"
- Item 1
- Item 2
  - Nested 1
  - Nested 2
- Item 3
"#;

        let lines = render_markdown(markdown);
        let output: String = lines
            .iter()
            .map(|line| {
                line.segments
                    .iter()
                    .map(|seg| seg.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Check for bullet characters (• for depth 0, ◦ for depth 1, etc.)
        assert!(
            output.contains("•") || output.contains("◦") || output.contains("▪"),
            "Should use Unicode bullet characters instead of dashes"
        );
    }

    #[test]
    fn test_markdown_table_box_drawing() {
        let markdown = r#"
| Header 1 | Header 2 |
|----------|----------|
| Cell 1   | Cell 2   |
| Cell 3   | Cell 4   |
"#;

        let lines = render_markdown(markdown);
        let output: String = lines
            .iter()
            .map(|line| {
                line.segments
                    .iter()
                    .map(|seg| seg.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Check for box-drawing character (│ instead of |)
        assert!(
            output.contains("│"),
            "Should use box-drawing character (│) for table cells instead of pipe"
        );
    }

    #[test]
    fn test_code_indentation_normalization_removes_common_indent() {
        let code_with_indent = "    fn hello() {\n        println!(\"world\");\n    }";
        let expected = "fn hello() {\n    println!(\"world\");\n}";
        let result = normalize_code_indentation(code_with_indent, Some("rust"));
        assert_eq!(result, expected);
    }

    #[test]
    fn test_code_indentation_preserves_already_normalized() {
        let code = "fn hello() {\n    println!(\"world\");\n}";
        let result = normalize_code_indentation(code, Some("rust"));
        assert_eq!(result, code);
    }

    #[test]
    fn test_code_indentation_without_language_hint() {
        // Without language hint, normalization still happens - common indent is stripped
        let code = "    some code";
        let result = normalize_code_indentation(code, None);
        assert_eq!(result, "some code");
    }

    #[test]
    fn test_code_indentation_preserves_relative_indentation() {
        let code = "    line1\n        line2\n    line3";
        let expected = "line1\n    line2\nline3";
        let result = normalize_code_indentation(code, Some("python"));
        assert_eq!(result, expected);
    }

    #[test]
    fn test_code_indentation_mixed_whitespace_preserves_indent() {
        // Mixed tabs and spaces - common prefix should be empty if they differ
        let code = "    line1\n\tline2";
        let result = normalize_code_indentation(code, None);
        // Should preserve original content rather than stripping incorrectly
        assert_eq!(result, code);
    }

    #[test]
    fn test_code_indentation_common_prefix_mixed() {
        // Common prefix is present ("    ")
        let code = "    line1\n    \tline2";
        let expected = "line1\n\tline2";
        let result = normalize_code_indentation(code, None);
        assert_eq!(result, expected);
    }
}
