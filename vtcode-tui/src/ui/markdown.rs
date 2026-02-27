//! Markdown rendering utilities for terminal output with syntax highlighting support.

use crate::config::loader::SyntaxHighlightingConfig;
use crate::ui::syntax_highlight::{find_syntax_by_token, load_theme, syntax_set};
use crate::ui::theme::{self, ThemeStyles};
use anstyle::{Effects, Style};
use anstyle_syntect::to_anstyle;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::cmp::max;
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxReference;
use syntect::util::LinesWithEndings;
use unicode_width::UnicodeWidthStr;

use crate::utils::diff_styles::DiffColorPalette;

const LIST_INDENT_WIDTH: usize = 2;
const CODE_LINE_NUMBER_MIN_WIDTH: usize = 3;

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

    fn prepend_segments(&mut self, segments: &[MarkdownSegment]) {
        if segments.is_empty() {
            return;
        }
        let mut prefixed = Vec::with_capacity(segments.len() + self.segments.len());
        prefixed.extend(segments.iter().cloned());
        prefixed.append(&mut self.segments);
        self.segments = prefixed;
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
    let mut pending_list_prefix: Option<String> = None;
    let mut code_block: Option<CodeBlockState> = None;
    let mut active_table: Option<TableBuffer> = None;
    let mut table_cell_index: usize = 0;

    for event in parser {
        // Handle code block accumulation separately to avoid borrow conflicts
        if code_block.is_some() {
            match &event {
                Event::Text(text) => {
                    if let Some(state) = code_block.as_mut() {
                        state.buffer.push_str(text);
                    }
                    continue;
                }
                Event::End(TagEnd::CodeBlock) => {
                    flush_current_line(
                        &mut lines,
                        &mut current_line,
                        blockquote_depth,
                        &list_stack,
                        &mut pending_list_prefix,
                        theme_styles,
                        base_style,
                    );
                    if let Some(state) = code_block.take() {
                        // If the code block contains a GFM table (common when
                        // LLMs wrap tables in ```markdown fences), render the
                        // content as markdown so the table gets box-drawing
                        // treatment instead of plain code-block line numbers.
                        if !render_options.disable_code_block_table_reparse
                            && code_block_contains_table(&state.buffer, state.language.as_deref())
                        {
                            let table_lines = render_markdown_code_block_table(
                                &state.buffer,
                                base_style,
                                theme_styles,
                                highlight_config,
                                render_options,
                            );
                            lines.extend(table_lines);
                        } else {
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
                                render_options.preserve_code_indentation,
                            );
                            lines.extend(highlighted);
                        }
                        push_blank_line(&mut lines);
                    }
                    continue;
                }
                _ => {}
            }
        }

        let mut ctx = MarkdownContext {
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

        match event {
            Event::Start(ref tag) => handle_start_tag(tag, &mut ctx),
            Event::End(tag) => handle_end_tag(tag, &mut ctx),
            Event::Text(text) => append_text(&text, &mut ctx),
            Event::Code(code) => {
                ctx.ensure_prefix();
                ctx.current_line
                    .push_segment(inline_code_style(theme_styles, base_style), &code);
            }
            Event::SoftBreak => ctx.flush_line(),
            Event::HardBreak => ctx.flush_line(),
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

    // Handle unclosed code block at end of input
    if let Some(state) = code_block.take() {
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
            render_options.preserve_code_indentation,
        );
        lines.extend(highlighted);
    }

    if !current_line.segments.is_empty() {
        lines.push(current_line);
    }

    trim_trailing_blank_lines(&mut lines);
    lines
}

fn render_markdown_code_block_table(
    source: &str,
    base_style: Style,
    theme_styles: &ThemeStyles,
    highlight_config: Option<&SyntaxHighlightingConfig>,
    render_options: RenderMarkdownOptions,
) -> Vec<MarkdownLine> {
    let mut nested_options = render_options;
    nested_options.disable_code_block_table_reparse = true;
    render_markdown_to_lines_with_options(
        source,
        base_style,
        theme_styles,
        highlight_config,
        nested_options,
    )
}

/// Convenience helper that renders markdown using the active theme without emitting output.
pub fn render_markdown(source: &str) -> Vec<MarkdownLine> {
    let styles = theme::active_styles();
    render_markdown_to_lines(source, Style::default(), &styles, None)
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

impl MarkdownContext<'_> {
    fn current_style(&self) -> Style {
        self.style_stack.last().copied().unwrap_or(self.base_style)
    }

    fn push_style(&mut self, modifier: impl FnOnce(Style) -> Style) {
        self.style_stack.push(modifier(self.current_style()));
    }

    fn pop_style(&mut self) {
        self.style_stack.pop();
    }

    fn flush_line(&mut self) {
        flush_current_line(
            self.lines,
            self.current_line,
            *self.blockquote_depth,
            self.list_stack,
            self.pending_list_prefix,
            self.theme_styles,
            self.base_style,
        );
    }

    fn flush_paragraph(&mut self) {
        self.flush_line();
        push_blank_line(self.lines);
    }

    fn ensure_prefix(&mut self) {
        ensure_prefix(
            self.current_line,
            *self.blockquote_depth,
            self.list_stack,
            self.pending_list_prefix,
            self.theme_styles,
            self.base_style,
        );
    }
}

fn handle_start_tag(tag: &Tag<'_>, ctx: &mut MarkdownContext<'_>) {
    match tag {
        Tag::Paragraph => {}
        Tag::Heading { level, .. } => {
            let style = heading_style(*level, ctx.theme_styles, ctx.base_style);
            ctx.style_stack.push(style);
            ctx.ensure_prefix();
            // Don't add the heading marker symbols to the output - just apply the style
        }
        Tag::BlockQuote(_) => *ctx.blockquote_depth += 1,
        Tag::List(start) => {
            let depth = ctx.list_stack.len();
            let kind = start
                .map(|v| ListKind::Ordered {
                    next: max(1, v as usize),
                })
                .unwrap_or(ListKind::Unordered);
            ctx.list_stack.push(ListState {
                kind,
                depth,
                continuation: String::new(),
            });
        }
        Tag::Item => {
            if let Some(state) = ctx.list_stack.last_mut() {
                let indent = " ".repeat(state.depth * LIST_INDENT_WIDTH);
                match &mut state.kind {
                    ListKind::Unordered => {
                        let bullet_char = match state.depth % 3 {
                            0 => "•",
                            1 => "◦",
                            _ => "▪",
                        };
                        let bullet = format!("{}{} ", indent, bullet_char);
                        state.continuation = format!("{}  ", indent);
                        *ctx.pending_list_prefix = Some(bullet);
                    }
                    ListKind::Ordered { next } => {
                        let bullet = format!("{}{}. ", indent, *next);
                        let width = bullet.len().saturating_sub(indent.len());
                        state.continuation = format!("{}{}", indent, " ".repeat(width));
                        *ctx.pending_list_prefix = Some(bullet);
                        *next += 1;
                    }
                }
            }
        }
        Tag::Emphasis => ctx.push_style(Style::italic),
        Tag::Strong => {
            let theme_styles = ctx.theme_styles;
            let base_style = ctx.base_style;
            ctx.push_style(|style| strong_style(style, theme_styles, base_style));
        }
        Tag::Strikethrough => ctx.push_style(Style::strikethrough),
        Tag::Superscript | Tag::Subscript => ctx.push_style(Style::italic),
        Tag::Link { .. } | Tag::Image { .. } => ctx.push_style(Style::underline),
        Tag::CodeBlock(kind) => {
            let language = match kind {
                CodeBlockKind::Fenced(info) => info
                    .split_whitespace()
                    .next()
                    .filter(|lang| !lang.is_empty())
                    .map(|lang| lang.to_string()),
                CodeBlockKind::Indented => None,
            };
            *ctx.code_block = Some(CodeBlockState {
                language,
                buffer: String::new(),
            });
        }
        Tag::Table(_) => {
            ctx.flush_paragraph();
            *ctx.active_table = Some(TableBuffer::default());
            *ctx.table_cell_index = 0;
        }
        Tag::TableRow => {
            if let Some(table) = ctx.active_table.as_mut() {
                table.current_row.clear();
            } else {
                ctx.flush_line();
            }
            *ctx.table_cell_index = 0;
        }
        Tag::TableHead => {
            if let Some(table) = ctx.active_table.as_mut() {
                table.in_head = true;
            }
        }
        Tag::TableCell => {
            if ctx.active_table.is_none() {
                ctx.ensure_prefix();
            } else {
                ctx.current_line.segments.clear();
            }
            *ctx.table_cell_index += 1;
        }
        Tag::FootnoteDefinition(_)
        | Tag::HtmlBlock
        | Tag::MetadataBlock(_)
        | Tag::DefinitionList
        | Tag::DefinitionListTitle
        | Tag::DefinitionListDefinition => {}
    }
}

fn handle_end_tag(tag: TagEnd, ctx: &mut MarkdownContext<'_>) {
    match tag {
        TagEnd::Paragraph => ctx.flush_paragraph(),
        TagEnd::Heading(_) => {
            ctx.flush_line();
            ctx.pop_style();
            push_blank_line(ctx.lines);
        }
        TagEnd::BlockQuote(_) => {
            ctx.flush_line();
            *ctx.blockquote_depth = ctx.blockquote_depth.saturating_sub(1);
        }
        TagEnd::List(_) => {
            ctx.flush_line();
            if ctx.list_stack.pop().is_some() {
                if let Some(state) = ctx.list_stack.last() {
                    ctx.pending_list_prefix.replace(state.continuation.clone());
                } else {
                    ctx.pending_list_prefix.take();
                }
            }
            push_blank_line(ctx.lines);
        }
        TagEnd::Item => {
            ctx.flush_line();
            if let Some(state) = ctx.list_stack.last() {
                ctx.pending_list_prefix.replace(state.continuation.clone());
            }
        }
        TagEnd::Emphasis
        | TagEnd::Strong
        | TagEnd::Strikethrough
        | TagEnd::Superscript
        | TagEnd::Subscript
        | TagEnd::Link
        | TagEnd::Image => {
            ctx.pop_style();
        }
        TagEnd::CodeBlock => {}
        TagEnd::Table => {
            if let Some(mut table) = ctx.active_table.take() {
                if !table.current_row.is_empty() {
                    table.rows.push(std::mem::take(&mut table.current_row));
                }
                let rendered = render_table(&table, ctx.theme_styles, ctx.base_style);
                ctx.lines.extend(rendered);
            }
            push_blank_line(ctx.lines);
            *ctx.table_cell_index = 0;
        }
        TagEnd::TableRow => {
            if let Some(table) = ctx.active_table.as_mut() {
                if table.in_head {
                    table.headers = std::mem::take(&mut table.current_row);
                } else {
                    table.rows.push(std::mem::take(&mut table.current_row));
                }
            } else {
                ctx.flush_line();
            }
            *ctx.table_cell_index = 0;
        }
        TagEnd::TableCell => {
            if let Some(table) = ctx.active_table.as_mut() {
                table.current_row.push(std::mem::take(ctx.current_line));
            }
        }
        TagEnd::TableHead => {
            if let Some(table) = ctx.active_table.as_mut() {
                table.in_head = false;
            }
        }
        TagEnd::FootnoteDefinition
        | TagEnd::HtmlBlock
        | TagEnd::MetadataBlock(_)
        | TagEnd::DefinitionList
        | TagEnd::DefinitionListTitle
        | TagEnd::DefinitionListDefinition => {}
    }
}

fn render_table(
    table: &TableBuffer,
    _theme_styles: &ThemeStyles,
    base_style: Style,
) -> Vec<MarkdownLine> {
    let mut lines = Vec::new();
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

    let border_style = base_style.dimmed();

    let render_row = |cells: &[MarkdownLine], col_widths: &[usize], bold: bool| -> MarkdownLine {
        let mut line = MarkdownLine::default();
        line.push_segment(border_style, "│ ");
        for (i, width) in col_widths.iter().enumerate() {
            if let Some(c) = cells.get(i) {
                for seg in &c.segments {
                    let s = if bold { seg.style.bold() } else { seg.style };
                    line.push_segment(s, &seg.text);
                }
                let padding = width.saturating_sub(c.width());
                if padding > 0 {
                    line.push_segment(base_style, &" ".repeat(padding));
                }
            } else {
                line.push_segment(base_style, &" ".repeat(*width));
            }
            line.push_segment(border_style, " │ ");
        }
        line
    };

    // Render Headers
    if !table.headers.is_empty() {
        lines.push(render_row(&table.headers, &col_widths, true));

        // Separator line
        let mut sep = MarkdownLine::default();
        sep.push_segment(border_style, "├─");
        for (i, width) in col_widths.iter().enumerate() {
            sep.push_segment(border_style, &"─".repeat(*width));
            sep.push_segment(
                border_style,
                if i < col_widths.len() - 1 {
                    "─┼─"
                } else {
                    "─┤"
                },
            );
        }
        lines.push(sep);
    }

    // Render Rows
    for row in &table.rows {
        lines.push(render_row(row, &col_widths, false));
    }

    lines
}

fn append_text(text: &str, ctx: &mut MarkdownContext<'_>) {
    let style = ctx.current_style();
    let mut start = 0usize;
    let mut chars = text.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if ch == '\n' {
            let segment = &text[start..idx];
            if !segment.is_empty() {
                ctx.ensure_prefix();
                ctx.current_line.push_segment(style, segment);
            }
            ctx.lines.push(std::mem::take(ctx.current_line));
            start = idx + 1;
            // Skip consecutive newlines (one blank line per sequence)
            while chars.peek().is_some_and(|&(_, c)| c == '\n') {
                let (_, c) = chars.next().expect("peeked");
                start += c.len_utf8();
            }
        }
    }

    if start < text.len() {
        let remaining = &text[start..];
        if !remaining.is_empty() {
            ctx.ensure_prefix();
            ctx.current_line.push_segment(style, remaining);
        }
    }
}

fn ensure_prefix(
    current_line: &mut MarkdownLine,
    blockquote_depth: usize,
    list_stack: &[ListState],
    pending_list_prefix: &mut Option<String>,
    _theme_styles: &ThemeStyles,
    base_style: Style,
) {
    if !current_line.segments.is_empty() {
        return;
    }

    for _ in 0..blockquote_depth {
        current_line.push_segment(base_style.dimmed().italic(), "│ ");
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
    let mut style = base_style.bold();
    if should_apply_markdown_accent(base_style, theme_styles)
        && let Some(color) = choose_markdown_accent(
            base_style,
            &[
                theme_styles.secondary,
                theme_styles.primary,
                theme_styles.tool_detail,
                theme_styles.status,
            ],
        )
    {
        style = style.fg_color(Some(color));
    }
    style
}

fn heading_style(_level: HeadingLevel, theme_styles: &ThemeStyles, base_style: Style) -> Style {
    let mut style = base_style.bold();
    if should_apply_markdown_accent(base_style, theme_styles)
        && let Some(color) = choose_markdown_accent(
            base_style,
            &[
                theme_styles.primary,
                theme_styles.secondary,
                theme_styles.status,
                theme_styles.tool,
            ],
        )
    {
        style = style.fg_color(Some(color));
    }
    style
}

fn strong_style(current: Style, theme_styles: &ThemeStyles, base_style: Style) -> Style {
    let mut style = current.bold();
    if should_apply_markdown_accent(base_style, theme_styles)
        && let Some(color) = choose_markdown_accent(
            base_style,
            &[
                theme_styles.primary,
                theme_styles.secondary,
                theme_styles.status,
                theme_styles.tool,
            ],
        )
    {
        style = style.fg_color(Some(color));
    }
    style
}

fn should_apply_markdown_accent(base_style: Style, theme_styles: &ThemeStyles) -> bool {
    base_style == theme_styles.response
}

fn choose_markdown_accent(base_style: Style, candidates: &[Style]) -> Option<anstyle::Color> {
    let base_fg = base_style.get_fg_color();
    candidates.iter().find_map(|candidate| {
        candidate
            .get_fg_color()
            .filter(|color| base_fg != Some(*color))
    })
}

fn build_prefix_segments(
    blockquote_depth: usize,
    list_stack: &[ListState],
    _theme_styles: &ThemeStyles,
    base_style: Style,
) -> Vec<MarkdownSegment> {
    let mut segments = Vec::new();
    for _ in 0..blockquote_depth {
        segments.push(MarkdownSegment::new(base_style.dimmed().italic(), "│ "));
    }
    if !list_stack.is_empty() {
        let mut continuation = String::new();
        for state in list_stack {
            continuation.push_str(&state.continuation);
        }
        if !continuation.is_empty() {
            segments.push(MarkdownSegment::new(base_style, continuation));
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
    prefix_segments: &[MarkdownSegment],
    preserve_code_indentation: bool,
) -> Vec<MarkdownLine> {
    let mut lines = Vec::new();

    // Normalize indentation unless we're preserving raw tool output.
    let normalized_code = normalize_code_indentation(code, language, preserve_code_indentation);
    let code_to_display = &normalized_code;
    if is_diff_language(language)
        || (language.is_none() && looks_like_diff_content(code_to_display))
    {
        return render_diff_code_block(code_to_display, theme_styles, base_style, prefix_segments);
    }
    let use_line_numbers =
        language.is_some_and(|lang| !lang.trim().is_empty()) && !is_diff_language(language);

    if let Some(config) = highlight_config.filter(|cfg| cfg.enabled)
        && let Some(highlighted) = try_highlight(code_to_display, language, config)
    {
        let line_count = highlighted.len();
        let number_width = line_number_width(line_count);
        for (index, segments) in highlighted.into_iter().enumerate() {
            let mut line = MarkdownLine::default();
            let line_prefix = if use_line_numbers {
                line_prefix_segments(
                    prefix_segments,
                    theme_styles,
                    base_style,
                    index + 1,
                    number_width,
                )
            } else {
                prefix_segments.to_vec()
            };
            line.prepend_segments(&line_prefix);
            for (style, text) in segments {
                line.push_segment(style, &text);
            }
            lines.push(line);
        }
        return lines;
    }

    // Fallback: render without syntax highlighting, but still with normalized indentation
    let mut line_number = 1usize;
    let mut line_count = LinesWithEndings::from(code_to_display).count();
    if code_to_display.ends_with('\n') {
        line_count = line_count.saturating_add(1);
    }
    let number_width = line_number_width(line_count);

    for raw_line in LinesWithEndings::from(code_to_display) {
        let trimmed = raw_line.trim_end_matches('\n');
        let mut line = MarkdownLine::default();
        let line_prefix = if use_line_numbers {
            line_prefix_segments(
                prefix_segments,
                theme_styles,
                base_style,
                line_number,
                number_width,
            )
        } else {
            prefix_segments.to_vec()
        };
        line.prepend_segments(&line_prefix);
        if !trimmed.is_empty() {
            line.push_segment(code_block_style(theme_styles, base_style), trimmed);
        }
        lines.push(line);
        line_number = line_number.saturating_add(1);
    }

    if code_to_display.ends_with('\n') {
        let mut line = MarkdownLine::default();
        let line_prefix = if use_line_numbers {
            line_prefix_segments(
                prefix_segments,
                theme_styles,
                base_style,
                line_number,
                number_width,
            )
        } else {
            prefix_segments.to_vec()
        };
        line.prepend_segments(&line_prefix);
        lines.push(line);
    }

    lines
}

fn format_start_only_hunk_header(line: &str) -> Option<String> {
    let trimmed = line.trim_end();
    if !trimmed.starts_with("@@ -") {
        return None;
    }

    let rest = trimmed.strip_prefix("@@ -")?;
    let mut parts = rest.split_whitespace();
    let old_part = parts.next()?;
    let new_part = parts.next()?;

    if !new_part.starts_with('+') {
        return None;
    }

    let old_start = old_part.split(',').next()?.parse::<usize>().ok()?;
    let new_start = new_part
        .trim_start_matches('+')
        .split(',')
        .next()?
        .parse::<usize>()
        .ok()?;

    Some(format!("@@ -{} +{} @@", old_start, new_start))
}

fn parse_diff_git_path(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    if parts.next()? != "diff" {
        return None;
    }
    if parts.next()? != "--git" {
        return None;
    }
    let _old = parts.next()?;
    let new_path = parts.next()?;
    Some(new_path.trim_start_matches("b/").to_string())
}

fn parse_diff_marker_path(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !(trimmed.starts_with("--- ") || trimmed.starts_with("+++ ")) {
        return None;
    }
    let path = trimmed.split_whitespace().nth(1)?;
    if path == "/dev/null" {
        return None;
    }
    Some(
        path.trim_start_matches("a/")
            .trim_start_matches("b/")
            .to_string(),
    )
}

fn is_addition_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('+') && !trimmed.starts_with("+++")
}

fn is_deletion_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('-') && !trimmed.starts_with("---")
}

fn normalize_diff_lines(code: &str) -> Vec<String> {
    #[derive(Default)]
    struct DiffBlock {
        header: String,
        path: String,
        lines: Vec<String>,
        additions: usize,
        deletions: usize,
    }

    let mut preface = Vec::new();
    let mut blocks = Vec::new();
    let mut current: Option<DiffBlock> = None;

    for line in code.lines() {
        if let Some(path) = parse_diff_git_path(line) {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
            current = Some(DiffBlock {
                header: line.to_string(),
                path,
                lines: Vec::new(),
                additions: 0,
                deletions: 0,
            });
            continue;
        }

        let rewritten = format_start_only_hunk_header(line).unwrap_or_else(|| line.to_string());
        if let Some(block) = current.as_mut() {
            if is_addition_line(line) {
                block.additions += 1;
            } else if is_deletion_line(line) {
                block.deletions += 1;
            }
            block.lines.push(rewritten);
        } else {
            preface.push(rewritten);
        }
    }

    if let Some(block) = current {
        blocks.push(block);
    }

    if blocks.is_empty() {
        let mut additions = 0usize;
        let mut deletions = 0usize;
        let mut fallback_path: Option<String> = None;
        let mut summary_insert_index: Option<usize> = None;
        let mut lines: Vec<String> = Vec::new();

        for line in code.lines() {
            if fallback_path.is_none() {
                fallback_path = parse_diff_marker_path(line);
            }
            if summary_insert_index.is_none() && line.trim_start().starts_with("+++ ") {
                summary_insert_index = Some(lines.len());
            }
            if is_addition_line(line) {
                additions += 1;
            } else if is_deletion_line(line) {
                deletions += 1;
            }
            let rewritten = format_start_only_hunk_header(line).unwrap_or_else(|| line.to_string());
            lines.push(rewritten);
        }

        let path = fallback_path.unwrap_or_else(|| "file".to_string());
        let summary = format!("• Diff {} (+{} -{})", path, additions, deletions);

        let mut output = Vec::with_capacity(lines.len() + 1);
        if let Some(idx) = summary_insert_index {
            output.extend(lines[..=idx].iter().cloned());
            output.push(summary);
            output.extend(lines[idx + 1..].iter().cloned());
        } else {
            output.push(summary);
            output.extend(lines);
        }
        return output;
    }

    let mut output = Vec::new();
    output.extend(preface);
    for block in blocks {
        output.push(block.header);
        output.push(format!(
            "• Diff {} (+{} -{})",
            block.path, block.additions, block.deletions
        ));
        output.extend(block.lines);
    }
    output
}

fn render_diff_code_block(
    code: &str,
    theme_styles: &ThemeStyles,
    base_style: Style,
    prefix_segments: &[MarkdownSegment],
) -> Vec<MarkdownLine> {
    let mut lines = Vec::new();
    let palette = DiffColorPalette::default();
    let context_style = code_block_style(theme_styles, base_style);
    let header_style = palette.header_style();
    let added_style = palette.added_style();
    let removed_style = palette.removed_style();

    for line in normalize_diff_lines(code) {
        let trimmed = line.trim_end_matches('\n');
        let trimmed_start = trimmed.trim_start();
        if let Some((path, additions, deletions)) = parse_diff_summary_line(trimmed_start) {
            let leading_len = trimmed.len().saturating_sub(trimmed_start.len());
            let leading = &trimmed[..leading_len];
            let mut line = MarkdownLine::default();
            line.prepend_segments(prefix_segments);
            if !leading.is_empty() {
                line.push_segment(context_style, leading);
            }
            line.push_segment(context_style, &format!("• Diff {path} ("));
            line.push_segment(added_style, &format!("+{additions}"));
            line.push_segment(context_style, " ");
            line.push_segment(removed_style, &format!("-{deletions}"));
            line.push_segment(context_style, ")");
            lines.push(line);
            continue;
        }
        let style = if trimmed.is_empty() {
            context_style
        } else if is_diff_header_line(trimmed_start) {
            header_style
        } else if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
            added_style
        } else if trimmed.starts_with('-') && !trimmed.starts_with("---") {
            removed_style
        } else {
            context_style
        };

        let mut line = MarkdownLine::default();
        line.prepend_segments(prefix_segments);
        if !trimmed.is_empty() {
            line.push_segment(style, trimmed);
        }
        lines.push(line);
    }

    if code.ends_with('\n') {
        let mut line = MarkdownLine::default();
        line.prepend_segments(prefix_segments);
        lines.push(line);
    }

    lines
}

fn parse_diff_summary_line(line: &str) -> Option<(&str, usize, usize)> {
    let summary = line.strip_prefix("• Diff ")?;
    let (path, counts) = summary.rsplit_once(" (")?;
    let counts = counts.strip_suffix(')')?;
    let mut parts = counts.split_whitespace();
    let additions = parts.next()?.strip_prefix('+')?.parse().ok()?;
    let deletions = parts.next()?.strip_prefix('-')?.parse().ok()?;
    Some((path, additions, deletions))
}

fn is_diff_header_line(trimmed: &str) -> bool {
    trimmed.starts_with("diff --git ")
        || trimmed.starts_with("@@ ")
        || trimmed.starts_with("index ")
        || trimmed.starts_with("new file mode ")
        || trimmed.starts_with("deleted file mode ")
        || trimmed.starts_with("rename from ")
        || trimmed.starts_with("rename to ")
        || trimmed.starts_with("copy from ")
        || trimmed.starts_with("copy to ")
        || trimmed.starts_with("similarity index ")
        || trimmed.starts_with("dissimilarity index ")
        || trimmed.starts_with("old mode ")
        || trimmed.starts_with("new mode ")
        || trimmed.starts_with("Binary files ")
        || trimmed.starts_with("\\ No newline at end of file")
        || trimmed.starts_with("+++ ")
        || trimmed.starts_with("--- ")
}

fn looks_like_diff_content(code: &str) -> bool {
    code.lines()
        .any(|line| is_diff_header_line(line.trim_start()))
}

fn line_prefix_segments(
    prefix_segments: &[MarkdownSegment],
    _theme_styles: &ThemeStyles,
    base_style: Style,
    line_number: usize,
    width: usize,
) -> Vec<MarkdownSegment> {
    let mut segments = prefix_segments.to_vec();
    let number_text = format!("{:>width$}  ", line_number, width = width);
    segments.push(MarkdownSegment::new(base_style.dimmed(), number_text));
    segments
}

fn line_number_width(line_count: usize) -> usize {
    let digits = line_count.max(1).to_string().len();
    digits.max(CODE_LINE_NUMBER_MIN_WIDTH)
}

/// Check if a code block's content is primarily a GFM table.
///
/// LLMs frequently wrap markdown tables in fenced code blocks (e.g.
/// ````markdown` or ` ```text`), which causes them to render as plain code
/// with line numbers instead of formatted tables. This function detects that
/// pattern so the caller can render the content as markdown instead.
fn code_block_contains_table(content: &str, language: Option<&str>) -> bool {
    // Only consider code blocks with no language or markup-like languages.
    // Code blocks tagged with programming languages (rust, python, etc.)
    // should never be reinterpreted.
    if let Some(lang) = language {
        let lang_lower = lang.to_ascii_lowercase();
        if !matches!(
            lang_lower.as_str(),
            "markdown" | "md" | "text" | "txt" | "plaintext" | "plain"
        ) {
            return false;
        }
    }

    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Quick heuristic: a GFM table must have at least a header row and a
    // separator row. The separator row contains only `|`, `-`, `:`, and
    // whitespace.
    let mut has_pipe_line = false;
    let mut has_separator = false;
    for line in trimmed.lines().take(4) {
        let line = line.trim();
        if line.contains('|') {
            has_pipe_line = true;
        }
        if line.starts_with('|') && line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
            has_separator = true;
        }
    }
    if !has_pipe_line || !has_separator {
        return false;
    }

    // Verify with pulldown-cmark: parse the content and check for a Table event.
    let options = Options::ENABLE_TABLES;
    let parser = Parser::new_ext(trimmed, options);
    for event in parser {
        match event {
            Event::Start(Tag::Table(_)) => return true,
            Event::Start(Tag::Paragraph) | Event::Text(_) | Event::SoftBreak => continue,
            _ => return false,
        }
    }
    false
}

fn is_diff_language(language: Option<&str>) -> bool {
    language.is_some_and(|lang| {
        matches!(
            lang.to_ascii_lowercase().as_str(),
            "diff" | "patch" | "udiff" | "git"
        )
    })
}

fn code_block_style(theme_styles: &ThemeStyles, base_style: Style) -> Style {
    let base_fg = base_style.get_fg_color();
    let theme_fg = theme_styles.output.get_fg_color();
    let fg = if base_style.get_effects().contains(Effects::DIMMED) {
        base_fg.or(theme_fg)
    } else {
        theme_fg.or(base_fg)
    };
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
fn normalize_code_indentation(
    code: &str,
    language: Option<&str>,
    preserve_indentation: bool,
) -> String {
    if preserve_indentation {
        return code.to_string();
    }
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
                | "cpp"
                | "c"
                | "php"
                | "html"
                | "css"
                | "sql"
                | "csharp"
                | "bash"
                | "sh"
                | "swift"
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
        .map(find_syntax_by_token)
        .unwrap_or_else(|| syntax_set().find_syntax_plain_text())
}

fn get_theme(theme_name: &str, cache: bool) -> Theme {
    load_theme(theme_name, cache)
}

/// A highlighted line segment with style and text.
#[derive(Clone, Debug)]
pub struct HighlightedSegment {
    pub style: Style,
    pub text: String,
}

/// Highlight a code string and return styled segments per line.
///
/// This function applies syntax highlighting to the provided code and returns
/// a vector of lines, where each line contains styled segments.
pub fn highlight_code_to_segments(
    code: &str,
    language: Option<&str>,
    theme_name: &str,
) -> Vec<Vec<HighlightedSegment>> {
    let syntax = select_syntax(language);
    let theme = get_theme(theme_name, true);
    let mut highlighter = HighlightLines::new(syntax, &theme);
    let mut result = Vec::new();

    for line in LinesWithEndings::from(code) {
        let trimmed = line.trim_end_matches('\n');
        let segments = match highlighter.highlight_line(trimmed, syntax_set()) {
            Ok(ranges) => ranges
                .into_iter()
                .filter(|(_, text)| !text.is_empty())
                .map(|(style, text)| {
                    let mut anstyle = to_anstyle(style);
                    anstyle = anstyle.bg_color(None);
                    HighlightedSegment {
                        style: anstyle,
                        text: text.to_owned(),
                    }
                })
                .collect(),
            Err(_) => vec![HighlightedSegment {
                style: Style::new(),
                text: trimmed.to_owned(),
            }],
        };
        result.push(segments);
    }

    result
}

/// Highlight a code string and return ANSI-formatted strings per line.
///
/// This is a convenience function that renders highlighting directly to
/// ANSI escape sequences suitable for terminal output.
pub fn highlight_code_to_ansi(code: &str, language: Option<&str>, theme_name: &str) -> Vec<String> {
    let segments = highlight_code_to_segments(code, language, theme_name);
    segments
        .into_iter()
        .map(|line_segments| {
            let mut ansi_line = String::new();
            for seg in line_segments {
                let rendered = seg.style.render();
                ansi_line.push_str(&format!(
                    "{rendered}{text}{reset}",
                    text = seg.text,
                    reset = anstyle::Reset
                ));
            }
            ansi_line
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_to_text(lines: &[MarkdownLine]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.segments
                    .iter()
                    .map(|seg| seg.text.as_str())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn test_markdown_heading_renders_prefixes() {
        let markdown = "# Heading\n\n## Subheading\n";
        let lines = render_markdown(markdown);
        let text_lines = lines_to_text(&lines);
        assert!(text_lines.iter().any(|line| line == "# Heading"));
        assert!(text_lines.iter().any(|line| line == "## Subheading"));
    }

    #[test]
    fn test_markdown_blockquote_prefix() {
        let markdown = "> Quote line\n> Second line\n";
        let lines = render_markdown(markdown);
        let text_lines = lines_to_text(&lines);
        assert!(
            text_lines
                .iter()
                .any(|line| line.starts_with("│ ") && line.contains("Quote line"))
        );
        assert!(
            text_lines
                .iter()
                .any(|line| line.starts_with("│ ") && line.contains("Second line"))
        );
    }

    #[test]
    fn test_markdown_inline_code_strips_backticks() {
        let markdown = "Use `code` here.";
        let lines = render_markdown(markdown);
        let text_lines = lines_to_text(&lines);
        assert!(
            text_lines
                .iter()
                .any(|line| line.contains("Use code here."))
        );
    }

    #[test]
    fn test_markdown_soft_break_renders_line_break() {
        let markdown = "first line\nsecond line";
        let lines = render_markdown(markdown);
        let text_lines: Vec<String> = lines_to_text(&lines)
            .into_iter()
            .filter(|line| !line.is_empty())
            .collect();
        assert_eq!(
            text_lines,
            vec!["first line".to_string(), "second line".to_string()]
        );
    }

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
    fn test_table_inside_markdown_code_block_renders_as_table() {
        let markdown = "```markdown\n\
            | Module | Purpose |\n\
            |--------|----------|\n\
            | core   | Library  |\n\
            ```\n";

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

        assert!(
            output.contains("│"),
            "Table inside ```markdown code block should render with box-drawing characters, got: {output}"
        );
        // Should NOT contain code-block line numbers
        assert!(
            !output.contains("  1  "),
            "Table inside markdown code block should not have line numbers"
        );
    }

    #[test]
    fn test_table_inside_md_code_block_renders_as_table() {
        let markdown = "```md\n\
            | A | B |\n\
            |---|---|\n\
            | 1 | 2 |\n\
            ```\n";

        let lines = render_markdown(markdown);
        let output = lines_to_text(&lines).join("\n");

        assert!(
            output.contains("│"),
            "Table inside ```md code block should render as table: {output}"
        );
    }

    #[test]
    fn test_table_code_block_reparse_guard_can_disable_table_reparse() {
        let markdown = "```markdown\n\
            | Module | Purpose |\n\
            |--------|----------|\n\
            | core   | Library  |\n\
            ```\n";
        let options = RenderMarkdownOptions {
            preserve_code_indentation: false,
            disable_code_block_table_reparse: true,
        };
        let lines = render_markdown_to_lines_with_options(
            markdown,
            Style::default(),
            &theme::active_styles(),
            None,
            options,
        );
        let output = lines_to_text(&lines).join("\n");

        assert!(
            output.contains("| Module | Purpose |"),
            "Guarded render should keep code-block content literal: {output}"
        );
        assert!(
            output.contains("  1  "),
            "Guarded render should keep code-block line numbers: {output}"
        );
    }

    #[test]
    fn test_rust_code_block_with_pipes_not_treated_as_table() {
        let markdown = "```rust\n\
            | Header | Col |\n\
            |--------|-----|\n\
            | a      | b   |\n\
            ```\n";

        let lines = render_markdown(markdown);
        let output = lines_to_text(&lines).join("\n");

        // Rust code blocks should NOT be reinterpreted as tables
        assert!(
            output.contains("| Header |"),
            "Rust code block should keep raw pipe characters: {output}"
        );
    }

    #[test]
    fn test_markdown_code_block_with_language_renders_line_numbers() {
        let markdown = "```rust\nfn main() {}\n```\n";
        let lines = render_markdown(markdown);
        let text_lines = lines_to_text(&lines);
        let code_line = text_lines
            .iter()
            .find(|line| line.contains("fn main() {}"))
            .expect("code line exists");
        assert!(code_line.contains("  1  "));
    }

    #[test]
    fn test_markdown_code_block_without_language_skips_line_numbers() {
        let markdown = "```\nfn main() {}\n```\n";
        let lines = render_markdown(markdown);
        let text_lines = lines_to_text(&lines);
        let code_line = text_lines
            .iter()
            .find(|line| line.contains("fn main() {}"))
            .expect("code line exists");
        assert!(!code_line.contains("  1  "));
    }

    #[test]
    fn test_markdown_diff_code_block_strips_backgrounds() {
        let markdown = "```diff\n@@ -1 +1 @@\n- old\n+ new\n context\n```\n";
        let lines =
            render_markdown_to_lines(markdown, Style::default(), &theme::active_styles(), None);

        let added_line = lines
            .iter()
            .find(|line| line.segments.iter().any(|seg| seg.text.contains("+ new")))
            .expect("added line exists");
        assert!(
            added_line
                .segments
                .iter()
                .all(|seg| seg.style.get_bg_color().is_none())
        );

        let removed_line = lines
            .iter()
            .find(|line| line.segments.iter().any(|seg| seg.text.contains("- old")))
            .expect("removed line exists");
        assert!(
            removed_line
                .segments
                .iter()
                .all(|seg| seg.style.get_bg_color().is_none())
        );

        let context_line = lines
            .iter()
            .find(|line| {
                line.segments
                    .iter()
                    .any(|seg| seg.text.contains(" context"))
            })
            .expect("context line exists");
        assert!(
            context_line
                .segments
                .iter()
                .all(|seg| seg.style.get_bg_color().is_none())
        );
    }

    #[test]
    fn test_markdown_unlabeled_diff_code_block_detects_diff() {
        let markdown = "```\n@@ -1 +1 @@\n- old\n+ new\n```\n";
        let lines =
            render_markdown_to_lines(markdown, Style::default(), &theme::active_styles(), None);
        let added_line = lines
            .iter()
            .find(|line| line.segments.iter().any(|seg| seg.text.contains("+ new")))
            .expect("added line exists");
        assert!(
            added_line
                .segments
                .iter()
                .all(|seg| seg.style.get_bg_color().is_none())
        );
    }

    #[test]
    fn test_markdown_task_list_markers() {
        let markdown = "- [x] Done\n- [ ] Todo\n";
        let lines = render_markdown(markdown);
        let text_lines = lines_to_text(&lines);
        assert!(text_lines.iter().any(|line| line.contains("[x]")));
        assert!(text_lines.iter().any(|line| line.contains("[ ]")));
    }

    #[test]
    fn test_code_indentation_normalization_removes_common_indent() {
        let code_with_indent = "    fn hello() {\n        println!(\"world\");\n    }";
        let expected = "fn hello() {\n    println!(\"world\");\n}";
        let result = normalize_code_indentation(code_with_indent, Some("rust"), false);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_code_indentation_preserves_already_normalized() {
        let code = "fn hello() {\n    println!(\"world\");\n}";
        let result = normalize_code_indentation(code, Some("rust"), false);
        assert_eq!(result, code);
    }

    #[test]
    fn test_code_indentation_without_language_hint() {
        // Without language hint, normalization still happens - common indent is stripped
        let code = "    some code";
        let result = normalize_code_indentation(code, None, false);
        assert_eq!(result, "some code");
    }

    #[test]
    fn test_code_indentation_preserves_relative_indentation() {
        let code = "    line1\n        line2\n    line3";
        let expected = "line1\n    line2\nline3";
        let result = normalize_code_indentation(code, Some("python"), false);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_code_indentation_mixed_whitespace_preserves_indent() {
        // Mixed tabs and spaces - common prefix should be empty if they differ
        let code = "    line1\n\tline2";
        let result = normalize_code_indentation(code, None, false);
        // Should preserve original content rather than stripping incorrectly
        assert_eq!(result, code);
    }

    #[test]
    fn test_code_indentation_common_prefix_mixed() {
        // Common prefix is present ("    ")
        let code = "    line1\n    \tline2";
        let expected = "line1\n\tline2";
        let result = normalize_code_indentation(code, None, false);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_code_indentation_preserve_when_requested() {
        let code = "    line1\n        line2\n    line3\n";
        let result = normalize_code_indentation(code, Some("rust"), true);
        assert_eq!(result, code);
    }

    #[test]
    fn test_diff_summary_counts_function_signature_change() {
        // Test case matching the user's TODO scenario - function signature change
        let diff = "diff --git a/ask.rs b/ask.rs\n\
index 0000000..1111111 100644\n\
--- a/ask.rs\n\
+++ b/ask.rs\n\
@@ -172,7 +172,7 @@\n\
          blocks\n\
      }\n\
 \n\
-    fn select_best_code_block<'a>(blocks: &'a [CodeFenceBlock]) -> Option<&'a CodeFenceBlock> {\n\
+    fn select_best_code_block(blocks: &[CodeFenceBlock]) -> Option<&CodeFenceBlock> {\n\
          let mut best = None;\n\
          let mut best_score = (0usize, 0u8);\n\
          for block in blocks {";

        let lines = normalize_diff_lines(diff);

        // Find the summary line
        let summary_line = lines
            .iter()
            .find(|l| l.starts_with("• Diff "))
            .expect("should have summary line");

        // Should show (+1 -1) not (+0 -0)
        assert_eq!(summary_line, "• Diff ask.rs (+1 -1)");
    }
}
