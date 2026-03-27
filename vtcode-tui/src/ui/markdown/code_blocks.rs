use super::parsing::{flush_current_line, push_blank_line};
use super::{CODE_LINE_NUMBER_MIN_WIDTH, MarkdownLine, MarkdownSegment, RenderMarkdownOptions};
use crate::config::loader::SyntaxHighlightingConfig;
use crate::ui::syntax_highlight;
use crate::ui::theme::ThemeStyles;
use crate::utils::diff_styles::DiffColorPalette;
use anstyle::{Effects, Style};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use syntect::util::LinesWithEndings;
use vtcode_commons::diff_paths::{
    format_start_only_hunk_header, is_diff_addition_line, is_diff_deletion_line,
    is_diff_header_line, is_diff_new_file_marker_line, language_hint_from_path,
    looks_like_diff_content, parse_diff_git_path, parse_diff_marker_path,
};

const DIFF_SUMMARY_PREFIX: &str = "• Diff ";

#[derive(Clone, Debug)]
pub(crate) struct CodeBlockState {
    pub(crate) language: Option<String>,
    pub(crate) buffer: String,
}

pub(crate) struct CodeBlockRenderEnv<'a> {
    pub(crate) lines: &'a mut Vec<MarkdownLine>,
    pub(crate) current_line: &'a mut MarkdownLine,
    pub(crate) blockquote_depth: usize,
    pub(crate) list_continuation_prefix: &'a str,
    pub(crate) pending_list_prefix: &'a mut Option<String>,
    pub(crate) base_style: Style,
    pub(crate) theme_styles: &'a ThemeStyles,
    pub(crate) highlight_config: Option<&'a SyntaxHighlightingConfig>,
    pub(crate) render_options: RenderMarkdownOptions,
}

pub(crate) fn handle_code_block_event(
    event: &Event<'_>,
    code_block: &mut Option<CodeBlockState>,
    env: &mut CodeBlockRenderEnv<'_>,
) -> bool {
    if code_block.is_none() {
        return false;
    }

    match event {
        Event::Text(text) => {
            if let Some(state) = code_block.as_mut() {
                state.buffer.push_str(text);
            }
            true
        }
        Event::End(TagEnd::CodeBlock) => {
            finalize_code_block(env, code_block, true, true);
            true
        }
        _ => false,
    }
}

pub(crate) fn finalize_unclosed_code_block(
    code_block: &mut Option<CodeBlockState>,
    env: &mut CodeBlockRenderEnv<'_>,
) {
    finalize_code_block(env, code_block, false, false);
}

fn finalize_code_block(
    env: &mut CodeBlockRenderEnv<'_>,
    code_block: &mut Option<CodeBlockState>,
    allow_table_reparse: bool,
    append_trailing_blank_line: bool,
) {
    flush_current_line(
        env.lines,
        env.current_line,
        env.blockquote_depth,
        env.list_continuation_prefix,
        env.pending_list_prefix,
        env.base_style,
    );
    if let Some(state) = code_block.take() {
        let rendered = render_code_block_state(&state, env, allow_table_reparse);
        env.lines.extend(rendered);
        if append_trailing_blank_line {
            push_blank_line(env.lines);
        }
    }
}

fn render_code_block_state(
    state: &CodeBlockState,
    env: &CodeBlockRenderEnv<'_>,
    allow_table_reparse: bool,
) -> Vec<MarkdownLine> {
    if allow_table_reparse
        && !env.render_options.disable_code_block_table_reparse
        && code_block_contains_table(&state.buffer, state.language.as_deref())
    {
        return render_markdown_code_block_table(
            &state.buffer,
            env.base_style,
            env.theme_styles,
            env.highlight_config,
            env.render_options,
        );
    }

    let prefix = build_prefix_segments(
        env.blockquote_depth,
        env.list_continuation_prefix,
        env.base_style,
    );
    highlight_code_block(
        &state.buffer,
        state.language.as_deref(),
        env.highlight_config,
        env.theme_styles,
        env.base_style,
        &prefix,
        env.render_options.preserve_code_indentation,
    )
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
    super::render_markdown_to_lines_with_options(
        source,
        base_style,
        theme_styles,
        highlight_config,
        nested_options,
    )
}

fn build_prefix_segments(
    blockquote_depth: usize,
    list_continuation_prefix: &str,
    base_style: Style,
) -> Vec<MarkdownSegment> {
    let mut segments =
        Vec::with_capacity(blockquote_depth + usize::from(!list_continuation_prefix.is_empty()));
    for _ in 0..blockquote_depth {
        segments.push(MarkdownSegment::new(base_style.dimmed().italic(), "│ "));
    }
    if !list_continuation_prefix.is_empty() {
        segments.push(MarkdownSegment::new(base_style, list_continuation_prefix));
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
        let source_lines: Vec<&str> = code_to_display.lines().collect();
        let line_count = source_line_count(code_to_display);
        let number_width = line_number_width(line_count);
        let gutter_style = line_number_style(theme_styles, base_style);
        let mut line_number = 1usize;
        for (index, segments) in highlighted.into_iter().enumerate() {
            let src = source_lines.get(index).copied().unwrap_or("");
            let is_omitted = parse_omitted_line_count(src).is_some();
            let (gutter_text, omitted) = if use_line_numbers {
                let (text, om) = format_gutter_text(line_number, number_width, src);
                (Some(text), om)
            } else {
                (None, 1)
            };
            let mut line =
                code_line_with_prefix(prefix_segments, gutter_text.as_deref(), gutter_style);
            if is_omitted {
                line.push_segment(gutter_style, src);
            } else {
                for (style, text) in segments {
                    line.push_segment(style, &text);
                }
            }
            line_number = line_number.saturating_add(omitted);
            lines.push(line);
        }
        return lines;
    }

    let mut line_number = 1usize;
    let line_count = source_line_count(code_to_display);
    let number_width = line_number_width(line_count);
    let gutter_style = line_number_style(theme_styles, base_style);

    for raw_line in LinesWithEndings::from(code_to_display) {
        let trimmed = raw_line.trim_end_matches('\n');
        let is_omitted = parse_omitted_line_count(trimmed).is_some();
        let (gutter_text, omitted) = if use_line_numbers {
            let (text, om) = format_gutter_text(line_number, number_width, trimmed);
            (Some(text), om)
        } else {
            (None, 1)
        };
        let mut line = code_line_with_prefix(prefix_segments, gutter_text.as_deref(), gutter_style);
        if !trimmed.is_empty() {
            if is_omitted {
                line.push_segment(gutter_style, trimmed);
            } else {
                line.push_segment(code_block_style(theme_styles, base_style), trimmed);
            }
        }
        lines.push(line);
        line_number = line_number.saturating_add(omitted);
    }

    if code_to_display.ends_with('\n') {
        let (gutter_text, _) = format_gutter_text(line_number, number_width, "");
        let line = code_line_with_prefix(
            prefix_segments,
            use_line_numbers.then_some(gutter_text.as_str()),
            gutter_style,
        );
        lines.push(line);
    }

    lines
}

pub(crate) fn normalize_diff_lines(code: &str) -> Vec<String> {
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
    let mut fallback_additions = 0usize;
    let mut fallback_deletions = 0usize;
    let mut fallback_path: Option<String> = None;
    let mut summary_insert_index: Option<usize> = None;

    for line in code.lines() {
        if fallback_path.is_none() {
            fallback_path = parse_diff_marker_path(line);
        }
        if summary_insert_index.is_none() && is_diff_new_file_marker_line(line.trim_start()) {
            summary_insert_index = Some(preface.len());
        }
        bump_diff_counters(line, &mut fallback_additions, &mut fallback_deletions);

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

        let rewritten = rewrite_diff_line(line);
        if let Some(block) = current.as_mut() {
            bump_diff_counters(line, &mut block.additions, &mut block.deletions);
            block.lines.push(rewritten);
        } else {
            preface.push(rewritten);
        }
    }

    if let Some(block) = current {
        blocks.push(block);
    }

    if blocks.is_empty() {
        let path = fallback_path.unwrap_or_else(|| "file".to_string());
        let summary = format_diff_summary(path.as_str(), fallback_additions, fallback_deletions);

        let mut output = Vec::with_capacity(preface.len() + 1);
        if let Some(idx) = summary_insert_index {
            output.extend(preface[..=idx].iter().cloned());
            output.push(summary);
            output.extend(preface[idx + 1..].iter().cloned());
        } else {
            output.push(summary);
            output.extend(preface);
        }
        return output;
    }

    let mut output = Vec::new();
    output.extend(preface);
    for block in blocks {
        output.push(block.header);
        output.push(format_diff_summary(
            block.path.as_str(),
            block.additions,
            block.deletions,
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
    let mut current_language_hint: Option<String> = None;

    for line in normalize_diff_lines(code) {
        let trimmed = line.trim_end_matches('\n');
        let trimmed_start = trimmed.trim_start();
        if let Some(path) =
            parse_diff_git_path(trimmed_start).or_else(|| parse_diff_marker_path(trimmed_start))
        {
            current_language_hint = language_hint_from_path(&path);
        }
        if let Some((path, additions, deletions)) = parse_diff_summary_line(trimmed_start) {
            let leading_len = trimmed.len().saturating_sub(trimmed_start.len());
            let leading = &trimmed[..leading_len];
            let mut line = prefixed_line(prefix_segments);
            if !leading.is_empty() {
                line.push_segment(context_style, leading);
            }
            line.push_segment(context_style, &format!("{DIFF_SUMMARY_PREFIX}{path} ("));
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
        } else if is_diff_addition_line(trimmed_start) {
            added_style
        } else if is_diff_deletion_line(trimmed_start) {
            removed_style
        } else {
            context_style
        };

        let mut line = prefixed_line(prefix_segments);
        if !trimmed.is_empty() {
            if is_diff_header_line(trimmed_start) {
                line.push_segment(style, trimmed);
            } else {
                let marker_len = trimmed
                    .chars()
                    .next()
                    .map(|ch| ch.len_utf8())
                    .unwrap_or_default();
                let (marker, content) = trimmed.split_at(marker_len);
                line.push_segment(style, marker);
                for segment in
                    render_diff_content_segments(content, current_language_hint.as_deref(), style)
                {
                    line.push_segment(segment.style, &segment.text);
                }
            }
        }
        lines.push(line);
    }

    if code.ends_with('\n') {
        lines.push(prefixed_line(prefix_segments));
    }

    lines
}

fn parse_diff_summary_line(line: &str) -> Option<(&str, usize, usize)> {
    let summary = line.strip_prefix(DIFF_SUMMARY_PREFIX)?;
    let (path, counts) = summary.rsplit_once(" (")?;
    let counts = counts.strip_suffix(')')?;
    let mut parts = counts.split_whitespace();
    let additions = parts.next()?.strip_prefix('+')?.parse().ok()?;
    let deletions = parts.next()?.strip_prefix('-')?.parse().ok()?;
    Some((path, additions, deletions))
}

fn format_diff_summary(path: &str, additions: usize, deletions: usize) -> String {
    format!("{DIFF_SUMMARY_PREFIX}{path} (+{additions} -{deletions})")
}

fn append_prefix_segments(line: &mut MarkdownLine, prefix_segments: &[MarkdownSegment]) {
    for segment in prefix_segments {
        line.push_segment(segment.style, &segment.text);
    }
}

fn prefixed_line(prefix_segments: &[MarkdownSegment]) -> MarkdownLine {
    let mut line = MarkdownLine::default();
    append_prefix_segments(&mut line, prefix_segments);
    line
}

fn line_number_style(theme_styles: &ThemeStyles, base_style: Style) -> Style {
    if base_style == theme_styles.tool_output {
        theme_styles.tool_detail.dimmed()
    } else {
        base_style.dimmed()
    }
}

/// Format the gutter text for a line. Returns `(gutter_text, source_line_advance)`.
fn format_gutter_text(line_num: usize, width: usize, line_text: &str) -> (String, usize) {
    if let Some(omitted) = parse_omitted_line_count(line_text) {
        let range_end = line_num.saturating_add(omitted.saturating_sub(1));
        let range = format!("{line_num}-{range_end}");
        (format!("{range:>width$}  "), omitted)
    } else {
        (format!("{line_num:>width$}  "), 1)
    }
}

fn code_line_with_prefix(
    prefix_segments: &[MarkdownSegment],
    gutter_text: Option<&str>,
    gutter_style: Style,
) -> MarkdownLine {
    let mut line = MarkdownLine::default();
    append_prefix_segments(&mut line, prefix_segments);
    if let Some(text) = gutter_text {
        line.push_segment(gutter_style, text);
    }
    line
}

fn line_number_width(line_count: usize) -> usize {
    let digits = line_count.max(1).to_string().len();
    digits.max(CODE_LINE_NUMBER_MIN_WIDTH)
}

fn source_line_count(code: &str) -> usize {
    let mut count = 0usize;
    for raw_line in LinesWithEndings::from(code) {
        let trimmed = raw_line.trim_end_matches('\n');
        count = count.saturating_add(parse_omitted_line_count(trimmed).unwrap_or(1));
    }
    if code.ends_with('\n') {
        count = count.saturating_add(1);
    }
    count
}

/// Parse the number of omitted lines from a condensed line like
/// `"… [+220 lines omitted; ...]"`.
fn parse_omitted_line_count(text: &str) -> Option<usize> {
    let trimmed = text.trim();
    let after = trimmed.strip_prefix("… [+")?;
    let end = after.find(' ')?;
    let count_str = &after[..end];
    count_str.parse::<usize>().ok()
}

fn code_block_contains_table(content: &str, language: Option<&str>) -> bool {
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

fn rewrite_diff_line(line: &str) -> String {
    format_start_only_hunk_header(line).unwrap_or_else(|| line.to_string())
}

fn bump_diff_counters(line: &str, additions: &mut usize, deletions: &mut usize) {
    let trimmed = line.trim_start();
    if is_diff_addition_line(trimmed) {
        *additions += 1;
    } else if is_diff_deletion_line(trimmed) {
        *deletions += 1;
    }
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

pub(crate) fn normalize_code_indentation(
    code: &str,
    language: Option<&str>,
    preserve_indentation: bool,
) -> String {
    if preserve_indentation {
        return code.to_string();
    }
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

    if !has_language_hint && language.is_some() {
        return code.to_string();
    }

    let lines: Vec<&str> = code.lines().collect();
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

    let normalized = lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                line
            } else if line.len() >= min_indent {
                &line[min_indent..]
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    if code.ends_with('\n') {
        format!("{normalized}\n")
    } else {
        normalized
    }
}

pub fn highlight_line_for_diff(line: &str, language: Option<&str>) -> Option<Vec<(Style, String)>> {
    syntax_highlight::highlight_line_to_anstyle_segments(
        line,
        language,
        syntax_highlight::get_active_syntax_theme(),
        true,
    )
    .map(|segments| {
        segments
            .into_iter()
            .map(|(style, text)| {
                let fg = style.get_fg_color().map(|c| match c {
                    anstyle::Color::Rgb(rgb) => {
                        let brighten = |v: u8| (v as u16 * 120 / 100).min(255) as u8;
                        anstyle::Color::Rgb(anstyle::RgbColor(
                            brighten(rgb.0),
                            brighten(rgb.1),
                            brighten(rgb.2),
                        ))
                    }
                    anstyle::Color::Ansi(ansi) => match ansi {
                        anstyle::AnsiColor::Black => {
                            anstyle::Color::Ansi(anstyle::AnsiColor::BrightWhite)
                        }
                        anstyle::AnsiColor::Red => {
                            anstyle::Color::Ansi(anstyle::AnsiColor::BrightRed)
                        }
                        anstyle::AnsiColor::Green => {
                            anstyle::Color::Ansi(anstyle::AnsiColor::BrightGreen)
                        }
                        anstyle::AnsiColor::Yellow => {
                            anstyle::Color::Ansi(anstyle::AnsiColor::BrightYellow)
                        }
                        anstyle::AnsiColor::Blue => {
                            anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlue)
                        }
                        anstyle::AnsiColor::Magenta => {
                            anstyle::Color::Ansi(anstyle::AnsiColor::BrightMagenta)
                        }
                        anstyle::AnsiColor::Cyan => {
                            anstyle::Color::Ansi(anstyle::AnsiColor::BrightCyan)
                        }
                        anstyle::AnsiColor::White => {
                            anstyle::Color::Ansi(anstyle::AnsiColor::BrightWhite)
                        }
                        other => anstyle::Color::Ansi(other),
                    },
                    other => other,
                });
                let bg = style.get_bg_color();
                let new_style = style.fg_color(fg).bg_color(bg);
                (new_style, text)
            })
            .collect()
    })
}

pub(crate) fn render_diff_content_segments(
    content: &str,
    language: Option<&str>,
    fallback_style: Style,
) -> Vec<MarkdownSegment> {
    let text = content.trim_end_matches('\n');
    if text.is_empty() {
        return Vec::new();
    }

    if let Some(segments) = highlight_line_for_diff(text, language)
        && !segments.is_empty()
    {
        return segments
            .into_iter()
            .map(|(style, text)| MarkdownSegment::new(style, text))
            .collect();
    }

    vec![MarkdownSegment::new(fallback_style, text)]
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

    if let Some(lang) = language
        && !config.enabled_languages.is_empty()
    {
        let direct_match = config
            .enabled_languages
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(lang));
        if !direct_match {
            let syntax_ref = syntax_highlight::find_syntax_by_token(lang);
            let resolved_match = config
                .enabled_languages
                .iter()
                .any(|entry| entry.eq_ignore_ascii_case(&syntax_ref.name));
            if !resolved_match {
                return None;
            }
        }
    }

    let rendered = syntax_highlight::highlight_code_to_anstyle_line_segments(
        code,
        language,
        &config.theme,
        true,
    );

    Some(rendered)
}

#[derive(Clone, Debug)]
pub struct HighlightedSegment {
    pub style: Style,
    pub text: String,
}

pub fn highlight_code_to_segments(
    code: &str,
    language: Option<&str>,
    theme_name: &str,
) -> Vec<Vec<HighlightedSegment>> {
    syntax_highlight::highlight_code_to_anstyle_line_segments(code, language, theme_name, true)
        .into_iter()
        .map(|segments| {
            segments
                .into_iter()
                .map(|(style, text)| HighlightedSegment { style, text })
                .collect()
        })
        .collect()
}

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
