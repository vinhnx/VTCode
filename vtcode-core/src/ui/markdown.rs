//! Markdown rendering utilities for terminal output with syntax highlighting support.

use crate::config::loader::SyntaxHighlightingConfig;
use crate::ui::syntax_highlight::{find_syntax_by_token, load_theme, syntax_set};
use crate::ui::theme::{self, ThemeStyles};
use anstyle::Style;
use anstyle_syntect::to_anstyle;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::cmp::max;
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxReference;
use syntect::util::LinesWithEndings;
use unicode_width::UnicodeWidthStr;

const LIST_INDENT_WIDTH: usize = 2;
const CODE_EXTRA_INDENT: &str = "    ";

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
    let options = Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;

    let parser = Parser::new_ext(source, options);

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
            Event::SoftBreak => append_text(" ", &mut ctx),
            Event::HardBreak => ctx.flush_line(),
            Event::Rule => {
                ctx.flush_line();
                let mut line = MarkdownLine::default();
                line.push_segment(theme_styles.secondary.bold(), &"―".repeat(32));
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
            ctx.style_stack
                .push(heading_style(*level, ctx.theme_styles, ctx.base_style));
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
        Tag::Strong => ctx.push_style(Style::bold),
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
    theme_styles: &ThemeStyles,
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

    let border_style = theme_styles.secondary.dimmed();

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
) -> Vec<MarkdownSegment> {
    let mut segments = Vec::new();
    for _ in 0..blockquote_depth {
        segments.push(MarkdownSegment::new(theme_styles.secondary.italic(), "│ "));
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
) -> Vec<MarkdownLine> {
    let mut lines = Vec::new();
    let mut augmented_prefix = prefix_segments.to_vec();
    augmented_prefix.push(MarkdownSegment::new(base_style, CODE_EXTRA_INDENT));

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
