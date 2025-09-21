//! Markdown rendering utilities for terminal output with syntax highlighting support.

use crate::config::loader::SyntaxHighlightingConfig;
use crate::ui::theme::ThemeStyles;
use anstyle::Style;
use anstyle_syntect::to_anstyle;
use once_cell::sync::Lazy;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag};
use std::cmp::max;
use std::collections::HashMap;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use tracing::warn;

const LIST_INDENT_WIDTH: usize = 2;
const CODE_EXTRA_INDENT: &str = "    ";
const MAX_THEME_CACHE_SIZE: usize = 32;

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME_CACHE: Lazy<parking_lot::RwLock<HashMap<String, Theme>>> = Lazy::new(|| {
    let defaults = ThemeSet::load_defaults();
    let mut entries: Vec<(String, Theme)> = defaults.themes.into_iter().collect();
    if entries.len() > MAX_THEME_CACHE_SIZE {
        entries.truncate(MAX_THEME_CACHE_SIZE);
    }
    let themes: HashMap<_, _> = entries.into_iter().collect();
    parking_lot::RwLock::new(themes)
});

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
        if let Some(last) = self.segments.last_mut() {
            if last.style == style {
                last.text.push_str(text);
                return;
            }
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

    for event in parser {
        if let Some(state) = code_block.as_mut() {
            match event {
                Event::Text(text) => {
                    state.buffer.push_str(&text);
                    continue;
                }
                Event::End(Tag::CodeBlock(_)) => {
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
            Event::Start(tag) => handle_start_tag(
                tag,
                &mut style_stack,
                &mut blockquote_depth,
                &mut list_stack,
                &mut pending_list_prefix,
                theme_styles,
                base_style,
                &mut code_block,
            ),
            Event::End(tag) => handle_end_tag(
                tag,
                &mut style_stack,
                &mut blockquote_depth,
                &mut list_stack,
                &mut pending_list_prefix,
                &mut lines,
                &mut current_line,
            ),
            Event::Text(text) => append_text(
                &text,
                &mut current_line,
                &mut lines,
                &style_stack,
                blockquote_depth,
                &list_stack,
                &mut pending_list_prefix,
                theme_styles,
                base_style,
            ),
            Event::Code(code_text) => {
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
            Event::SoftBreak => {
                append_text(
                    " ",
                    &mut current_line,
                    &mut lines,
                    &style_stack,
                    blockquote_depth,
                    &list_stack,
                    &mut pending_list_prefix,
                    theme_styles,
                    base_style,
                );
            }
            Event::HardBreak => {
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
            Event::Rule => {
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
            Event::TaskListMarker(checked) => {
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
            Event::Html(html) => append_text(
                &html,
                &mut current_line,
                &mut lines,
                &style_stack,
                blockquote_depth,
                &list_stack,
                &mut pending_list_prefix,
                theme_styles,
                base_style,
            ),
            Event::FootnoteReference(reference) => append_text(
                &format!("[^{}]", reference),
                &mut current_line,
                &mut lines,
                &style_stack,
                blockquote_depth,
                &list_stack,
                &mut pending_list_prefix,
                theme_styles,
                base_style,
            ),
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

fn handle_start_tag(
    tag: Tag,
    style_stack: &mut Vec<Style>,
    blockquote_depth: &mut usize,
    list_stack: &mut Vec<ListState>,
    pending_list_prefix: &mut Option<String>,
    theme_styles: &ThemeStyles,
    base_style: Style,
    code_block: &mut Option<CodeBlockState>,
) {
    match tag {
        Tag::Paragraph => {}
        Tag::Heading(level, ..) => {
            style_stack.push(heading_style(level, theme_styles, base_style));
        }
        Tag::BlockQuote => {
            *blockquote_depth += 1;
        }
        Tag::List(start) => {
            let depth = list_stack.len();
            let kind = start
                .map(|value| ListKind::Ordered {
                    next: max(1, value as usize),
                })
                .unwrap_or(ListKind::Unordered);
            list_stack.push(ListState {
                kind,
                depth,
                continuation: String::new(),
            });
        }
        Tag::Item => {
            if let Some(state) = list_stack.last_mut() {
                let indent = " ".repeat(state.depth * LIST_INDENT_WIDTH);
                match &mut state.kind {
                    ListKind::Unordered => {
                        let bullet = format!("{}- ", indent);
                        state.continuation = format!("{}  ", indent);
                        *pending_list_prefix = Some(bullet);
                    }
                    ListKind::Ordered { next } => {
                        let bullet = format!("{}{}. ", indent, *next);
                        let width = bullet.len().saturating_sub(indent.len());
                        state.continuation = format!("{}{}", indent, " ".repeat(width));
                        *pending_list_prefix = Some(bullet);
                        *next += 1;
                    }
                }
            }
        }
        Tag::Emphasis => {
            let style = style_stack.last().copied().unwrap_or(base_style).italic();
            style_stack.push(style);
        }
        Tag::Strong => {
            let style = style_stack.last().copied().unwrap_or(base_style).bold();
            style_stack.push(style);
        }
        Tag::Strikethrough => {
            let style = style_stack
                .last()
                .copied()
                .unwrap_or(base_style)
                .strikethrough();
            style_stack.push(style);
        }
        Tag::Link { .. } | Tag::Image { .. } => {
            let style = style_stack
                .last()
                .copied()
                .unwrap_or(base_style)
                .underline();
            style_stack.push(style);
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
            *code_block = Some(CodeBlockState {
                language,
                buffer: String::new(),
            });
        }
        _ => {}
    }
}

fn handle_end_tag(
    tag: Tag,
    style_stack: &mut Vec<Style>,
    blockquote_depth: &mut usize,
    list_stack: &mut Vec<ListState>,
    pending_list_prefix: &mut Option<String>,
    lines: &mut Vec<MarkdownLine>,
    current_line: &mut MarkdownLine,
) {
    match tag {
        Tag::Paragraph => {
            if !current_line.segments.is_empty() {
                lines.push(std::mem::take(current_line));
            }
            push_blank_line(lines);
        }
        Tag::Heading(..) => {
            if !current_line.segments.is_empty() {
                lines.push(std::mem::take(current_line));
            }
            push_blank_line(lines);
            style_stack.pop();
        }
        Tag::BlockQuote => {
            if *blockquote_depth > 0 {
                *blockquote_depth -= 1;
            }
        }
        Tag::List(_) => {
            list_stack.pop();
            *pending_list_prefix = None;
            if !current_line.segments.is_empty() {
                lines.push(std::mem::take(current_line));
            }
            push_blank_line(lines);
        }
        Tag::Item => {
            if !current_line.segments.is_empty() {
                lines.push(std::mem::take(current_line));
            }
            *pending_list_prefix = None;
        }
        Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } | Tag::Image { .. } => {
            style_stack.pop();
        }
        Tag::CodeBlock(_) => {}
        Tag::Table(_)
        | Tag::TableHead
        | Tag::TableRow
        | Tag::TableCell
        | Tag::FootnoteDefinition(_) => {}
    }
}

fn append_text(
    text: &str,
    current_line: &mut MarkdownLine,
    lines: &mut Vec<MarkdownLine>,
    style_stack: &[Style],
    blockquote_depth: usize,
    list_stack: &[ListState],
    pending_list_prefix: &mut Option<String>,
    theme_styles: &ThemeStyles,
    base_style: Style,
) {
    let style = style_stack.last().copied().unwrap_or(base_style);

    let mut start = 0usize;
    let mut chars = text.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch == '\n' {
            let segment = &text[start..idx];
            if !segment.is_empty() {
                ensure_prefix(
                    current_line,
                    blockquote_depth,
                    list_stack,
                    pending_list_prefix,
                    theme_styles,
                    base_style,
                );
                current_line.push_segment(style, segment);
            }
            lines.push(std::mem::take(current_line));
            start = idx + ch.len_utf8();
        }
    }

    if start < text.len() {
        let remaining = &text[start..];
        ensure_prefix(
            current_line,
            blockquote_depth,
            list_stack,
            pending_list_prefix,
            theme_styles,
            base_style,
        );
        current_line.push_segment(style, remaining);
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
    if current_line.segments.is_empty() {
        if pending_list_prefix.is_some() {
            ensure_prefix(
                current_line,
                blockquote_depth,
                list_stack,
                pending_list_prefix,
                theme_styles,
                base_style,
            );
        }
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
    let bg = Some(theme_styles.background.into());
    let mut style = base_style;
    if let Some(fg_color) = fg {
        style = style.fg_color(Some(fg_color));
    }
    style.bg_color(bg).bold()
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

    if let Some(config) = highlight_config.filter(|cfg| cfg.enabled) {
        if let Some(highlighted) = try_highlight(code, language, config) {
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
    }

    for raw_line in LinesWithEndings::from(code) {
        let trimmed = raw_line.trim_end_matches('\n');
        let mut line = MarkdownLine::default();
        line.prepend_segments(&augmented_prefix);
        if !trimmed.is_empty() {
            line.push_segment(code_block_style(theme_styles, base_style), trimmed);
        }
        lines.push(line);
    }

    if code.ends_with('\n') {
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
    let theme = load_theme(&config.theme, config.cache_themes);
    let mut highlighter = HighlightLines::new(syntax, &theme);
    let mut rendered = Vec::new();

    let mut ends_with_newline = false;
    for line in LinesWithEndings::from(code) {
        ends_with_newline = line.ends_with('\n');
        let trimmed = line.trim_end_matches('\n');
        let ranges = highlighter.highlight_line(trimmed, &SYNTAX_SET).ok()?;
        let mut segments = Vec::new();
        for (style, part) in ranges {
            if part.is_empty() {
                continue;
            }
            segments.push((to_anstyle(style), part.to_string()));
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
        .and_then(|lang| SYNTAX_SET.find_syntax_by_token(lang))
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text())
}

fn load_theme(theme_name: &str, cache: bool) -> Theme {
    if let Some(theme) = THEME_CACHE.read().get(theme_name).cloned() {
        return theme;
    }

    let defaults = ThemeSet::load_defaults();
    if let Some(theme) = defaults.themes.get(theme_name).cloned() {
        if cache {
            let mut guard = THEME_CACHE.write();
            if guard.len() >= MAX_THEME_CACHE_SIZE {
                if let Some(first_key) = guard.keys().next().cloned() {
                    guard.remove(&first_key);
                }
            }
            guard.insert(theme_name.to_string(), theme.clone());
        }
        theme
    } else {
        warn!(
            "theme" = theme_name,
            "Falling back to default syntax highlighting theme"
        );
        defaults
            .themes
            .into_iter()
            .next()
            .map(|(_, theme)| theme)
            .unwrap_or_default()
    }
}
