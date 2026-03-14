use super::code_blocks::CodeBlockState;
use super::links::{
    extract_hidden_location_suffix, label_segments_have_location_suffix,
    should_render_link_destination,
};
use super::tables::{TableBuffer, render_table};
use super::{LIST_INDENT_WIDTH, MarkdownLine};
use crate::ui::theme::ThemeStyles;
use anstyle::Style;
use pulldown_cmark::{CodeBlockKind, HeadingLevel, Tag, TagEnd};
use std::cmp::max;

#[derive(Clone, Debug)]
pub(crate) struct ListState {
    pub(crate) kind: ListKind,
    pub(crate) depth: usize,
    pub(crate) continuation: String,
}

#[derive(Clone, Debug)]
pub(crate) enum ListKind {
    Unordered,
    Ordered { next: usize },
}

#[derive(Clone, Debug)]
pub(crate) struct LinkState {
    pub(crate) destination: String,
    pub(crate) show_destination: bool,
    pub(crate) hidden_location_suffix: Option<String>,
    pub(crate) label_start_segment_idx: usize,
}

pub(crate) struct MarkdownContext<'a> {
    pub(crate) style_stack: &'a mut Vec<Style>,
    pub(crate) blockquote_depth: &'a mut usize,
    pub(crate) list_stack: &'a mut Vec<ListState>,
    pub(crate) list_continuation_prefix: &'a mut String,
    pub(crate) pending_list_prefix: &'a mut Option<String>,
    pub(crate) lines: &'a mut Vec<MarkdownLine>,
    pub(crate) current_line: &'a mut MarkdownLine,
    pub(crate) theme_styles: &'a ThemeStyles,
    pub(crate) base_style: Style,
    pub(crate) code_block: &'a mut Option<CodeBlockState>,
    pub(crate) active_table: &'a mut Option<TableBuffer>,
    pub(crate) link_state: &'a mut Option<LinkState>,
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

    pub(crate) fn flush_line(&mut self) {
        flush_current_line(
            self.lines,
            self.current_line,
            *self.blockquote_depth,
            self.list_continuation_prefix,
            self.pending_list_prefix,
            self.base_style,
        );
    }

    fn flush_paragraph(&mut self) {
        self.flush_line();
        push_blank_line(self.lines);
    }

    pub(crate) fn ensure_prefix(&mut self) {
        ensure_prefix(
            self.current_line,
            *self.blockquote_depth,
            self.list_continuation_prefix,
            self.pending_list_prefix,
            self.base_style,
        );
    }

    fn refresh_list_continuation_prefix(&mut self) {
        rebuild_list_continuation_prefix(self.list_stack, self.list_continuation_prefix);
    }

    fn set_pending_list_continuation(&mut self) {
        if let Some(state) = self.list_stack.last() {
            self.pending_list_prefix.replace(state.continuation.clone());
        }
    }

    pub(crate) fn active_link_target(&self) -> Option<String> {
        self.link_state
            .as_ref()
            .map(|link| link.destination.clone())
    }
}

pub(crate) fn handle_start_tag(tag: &Tag<'_>, ctx: &mut MarkdownContext<'_>) {
    match tag {
        Tag::Paragraph => {}
        Tag::Heading { level, .. } => {
            let style = heading_style(*level, ctx.theme_styles, ctx.base_style);
            ctx.style_stack.push(style);
            ctx.ensure_prefix();
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
            ctx.refresh_list_continuation_prefix();
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
                        let bullet = format!("{indent}{bullet_char} ");
                        state.continuation = format!("{indent}  ");
                        *ctx.pending_list_prefix = Some(bullet);
                    }
                    ListKind::Ordered { next } => {
                        let bullet = format!("{indent}{}. ", *next);
                        let width = bullet.len().saturating_sub(indent.len());
                        state.continuation = format!("{indent}{}", " ".repeat(width));
                        *ctx.pending_list_prefix = Some(bullet);
                        *next += 1;
                    }
                }
                ctx.refresh_list_continuation_prefix();
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
        Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
            let show_destination = should_render_link_destination(dest_url);
            let label_start_segment_idx = ctx.current_line.segments.len();
            *ctx.link_state = Some(LinkState {
                destination: dest_url.to_string(),
                show_destination,
                hidden_location_suffix: extract_hidden_location_suffix(dest_url),
                label_start_segment_idx,
            });
            ctx.push_style(Style::underline);
        }
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
        }
        Tag::TableRow => {
            if let Some(table) = ctx.active_table.as_mut() {
                table.current_row.clear();
            } else {
                ctx.flush_line();
            }
        }
        Tag::TableHead => {
            if let Some(table) = ctx.active_table.as_mut() {
                table.in_head = true;
                table.current_row.clear();
            }
        }
        Tag::TableCell => {
            if ctx.active_table.is_none() {
                ctx.ensure_prefix();
            } else {
                ctx.current_line.segments.clear();
            }
        }
        Tag::FootnoteDefinition(_)
        | Tag::HtmlBlock
        | Tag::MetadataBlock(_)
        | Tag::DefinitionList
        | Tag::DefinitionListTitle
        | Tag::DefinitionListDefinition => {}
    }
}

pub(crate) fn handle_end_tag(tag: TagEnd, ctx: &mut MarkdownContext<'_>) {
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
                ctx.refresh_list_continuation_prefix();
                if ctx.list_stack.is_empty() {
                    ctx.pending_list_prefix.take();
                } else {
                    ctx.set_pending_list_continuation();
                }
            }
            push_blank_line(ctx.lines);
        }
        TagEnd::Item => {
            ctx.flush_line();
            ctx.set_pending_list_continuation();
        }
        TagEnd::Emphasis
        | TagEnd::Strong
        | TagEnd::Strikethrough
        | TagEnd::Superscript
        | TagEnd::Subscript => {
            ctx.pop_style();
        }
        TagEnd::Link | TagEnd::Image => {
            if let Some(link) = ctx.link_state.take() {
                if link.show_destination {
                    ctx.current_line.push_segment_with_link(
                        ctx.current_style(),
                        " (",
                        Some(link.destination.clone()),
                    );
                    ctx.current_line.push_segment_with_link(
                        ctx.current_style(),
                        &link.destination,
                        Some(link.destination.clone()),
                    );
                    ctx.current_line.push_segment_with_link(
                        ctx.current_style(),
                        ")",
                        Some(link.destination.clone()),
                    );
                } else if let Some(location_suffix) = link.hidden_location_suffix.as_deref() {
                    let label_segments = ctx
                        .current_line
                        .segments
                        .get(link.label_start_segment_idx..)
                        .unwrap_or(&[]);

                    if !label_segments_have_location_suffix(label_segments) {
                        ctx.current_line.push_segment_with_link(
                            ctx.current_style(),
                            location_suffix,
                            Some(link.destination.clone()),
                        );
                    }
                }
            }
            ctx.pop_style();
        }
        TagEnd::CodeBlock => {}
        TagEnd::Table => {
            if let Some(mut table) = ctx.active_table.take() {
                if !table.current_row.is_empty() {
                    table.rows.push(std::mem::take(&mut table.current_row));
                }
                let rendered = render_table(&table, ctx.base_style);
                ctx.lines.extend(rendered);
            }
            push_blank_line(ctx.lines);
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
        }
        TagEnd::TableCell => {
            if let Some(table) = ctx.active_table.as_mut() {
                table.current_row.push(std::mem::take(ctx.current_line));
            }
        }
        TagEnd::TableHead => {
            if let Some(table) = ctx.active_table.as_mut() {
                if !table.current_row.is_empty() {
                    table.headers = std::mem::take(&mut table.current_row);
                }
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

pub(crate) fn append_text(text: &str, ctx: &mut MarkdownContext<'_>) {
    let style = ctx.current_style();
    let link_target = ctx.active_link_target();
    let mut start = 0usize;
    let mut chars = text.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if ch == '\n' {
            let segment = &text[start..idx];
            if !segment.is_empty() {
                ctx.ensure_prefix();
                ctx.current_line
                    .push_segment_with_link(style, segment, link_target.clone());
            }
            ctx.lines.push(std::mem::take(ctx.current_line));
            start = idx + 1;
            while chars.peek().is_some_and(|&(_, c)| c == '\n') {
                let Some((_, c)) = chars.next() else {
                    break;
                };
                start += c.len_utf8();
            }
        }
    }

    if start < text.len() {
        let remaining = &text[start..];
        if !remaining.is_empty() {
            ctx.ensure_prefix();
            ctx.current_line
                .push_segment_with_link(style, remaining, link_target);
        }
    }
}

pub(crate) fn flush_current_line(
    lines: &mut Vec<MarkdownLine>,
    current_line: &mut MarkdownLine,
    blockquote_depth: usize,
    list_continuation_prefix: &str,
    pending_list_prefix: &mut Option<String>,
    base_style: Style,
) {
    if current_line.segments.is_empty() && pending_list_prefix.is_some() {
        ensure_prefix(
            current_line,
            blockquote_depth,
            list_continuation_prefix,
            pending_list_prefix,
            base_style,
        );
    }

    if !current_line.segments.is_empty() {
        lines.push(std::mem::take(current_line));
    }
}

pub(crate) fn push_blank_line(lines: &mut Vec<MarkdownLine>) {
    if lines
        .last()
        .map(|line| line.segments.is_empty())
        .unwrap_or(false)
    {
        return;
    }
    lines.push(MarkdownLine::default());
}

pub(crate) fn trim_trailing_blank_lines(lines: &mut Vec<MarkdownLine>) {
    while lines
        .last()
        .map(|line| line.segments.is_empty())
        .unwrap_or(false)
    {
        lines.pop();
    }
}

pub(crate) fn inline_code_style(theme_styles: &ThemeStyles, base_style: Style) -> Style {
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

fn ensure_prefix(
    current_line: &mut MarkdownLine,
    blockquote_depth: usize,
    list_continuation_prefix: &str,
    pending_list_prefix: &mut Option<String>,
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
    } else if !list_continuation_prefix.is_empty() {
        current_line.push_segment(base_style, list_continuation_prefix);
    }
}

fn heading_style(_level: HeadingLevel, theme_styles: &ThemeStyles, base_style: Style) -> Style {
    markdown_bold_accent_style(base_style.bold(), theme_styles, base_style)
}

fn strong_style(current: Style, theme_styles: &ThemeStyles, base_style: Style) -> Style {
    markdown_bold_accent_style(current.bold(), theme_styles, base_style)
}

fn markdown_bold_accent_style(
    mut style: Style,
    theme_styles: &ThemeStyles,
    base_style: Style,
) -> Style {
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

fn rebuild_list_continuation_prefix(
    list_stack: &[ListState],
    list_continuation_prefix: &mut String,
) {
    list_continuation_prefix.clear();
    for state in list_stack {
        list_continuation_prefix.push_str(&state.continuation);
    }
}
