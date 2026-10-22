use super::parsing::{flush_current_line, push_blank_line};
use super::{CODE_LINE_NUMBER_MIN_WIDTH, CodeBlockWeight, MarkdownLine, MarkdownSegment, RenderMarkdownOptions};
use crate::tui::ui::syntax_highlight;
use crate::tui::ui::theme::ThemeStyles;
use crate::tui::utils::diff_styles::{
    DiffLineType, current_diff_render_style_context, diff_add_fg, diff_del_fg, style_content_ansi,
    style_file_header_new_ansi, style_file_header_old_ansi, style_hunk_header_ansi, style_sign_ansi,
};
use anstyle::{Color as AnstyleColor, Effects, Style};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::fmt::Write;
use syntect::util::LinesWithEndings;
use vtcode_commons::diff_paths::{
    is_diff_addition_line, is_diff_deletion_line, is_diff_header_line, is_diff_new_file_marker_line,
    is_prose_language_hint, language_hint_from_path, looks_like_diff_content, parse_diff_git_path,
    parse_diff_marker_path,
};
use vtcode_commons::diff_preview::{DiffDisplayKind, DiffDisplayLine, annotate_word_level_diffs};
use vtcode_config::loader::SyntaxHighlightingConfig;

const DIFF_SUMMARY_PREFIX: &str = "• Diff ";

/// Share of the theme background mixed into a muted code foreground.
///
/// Muted code keeps its hue, so syntax-highlighted tokens stay
/// distinguishable while only their intensity drops. The blend is lighter
/// than the line-number gutter blend, because the code body is content and the
/// gutter is chrome. Both stop at the theme's contrast floor.
const MUTED_CODE_BLEND_RATIO: f32 = 0.25;

/// Share of the theme background mixed into a muted line-number gutter.
///
/// The same share the primary gutter blends by; unlike the primary gutter it
/// stops at the theme's contrast floor, so the result can sit slightly
/// brighter than a primary block's gutter when the blend would cross it.
const MUTED_GUTTER_BLEND_RATIO: f32 = 0.35;

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

pub(crate) fn finalize_unclosed_code_block(code_block: &mut Option<CodeBlockState>, env: &mut CodeBlockRenderEnv<'_>) {
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

    let prefix = build_prefix_segments(env.blockquote_depth, env.list_continuation_prefix, env.base_style);
    highlight_code_block(
        &state.buffer,
        state.language.as_deref(),
        env.highlight_config,
        env.theme_styles,
        env.base_style,
        &prefix,
        env.render_options.preserve_code_indentation,
        env.render_options.code_block_weight,
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
    super::render_markdown_to_lines_with_options(source, base_style, theme_styles, highlight_config, nested_options)
}

fn build_prefix_segments(
    _blockquote_depth: usize,
    list_continuation_prefix: &str,
    base_style: Style,
) -> Vec<MarkdownSegment> {
    let mut segments = Vec::with_capacity(usize::from(!list_continuation_prefix.is_empty()));
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
    code_block_weight: CodeBlockWeight,
) -> Vec<MarkdownLine> {
    let mut lines = Vec::new();

    let normalized_code = normalize_code_indentation(code, language, preserve_code_indentation);
    let code_to_display = &normalized_code;
    if is_diff_language(language) || (language.is_none() && looks_like_diff_content(code_to_display)) {
        return render_diff_code_block(code_to_display, theme_styles, base_style, prefix_segments);
    }
    let use_line_numbers = language.is_some_and(|lang| !lang.trim().is_empty()) && !is_diff_language(language);

    if let Some(config) = highlight_config.filter(|cfg| cfg.enabled)
        && let Some(highlighted) = try_highlight(code_to_display, language, config)
    {
        let source_lines: Vec<&str> = code_to_display.lines().collect();
        let line_count = source_line_count(code_to_display);
        let number_width = line_number_width(line_count);
        let gutter_style = code_gutter_style(theme_styles, base_style, code_block_weight);
        let code_style = code_block_style(theme_styles, base_style, code_block_weight);
        let mut line_number = 1usize;
        for (index, src) in source_lines.iter().enumerate() {
            let Some(segments) = highlighted.get(index) else {
                break;
            };
            let is_omitted = parse_omitted_line_count(src).is_some();
            let (gutter_text, omitted) = if use_line_numbers {
                let (text, om) = format_gutter_text(line_number, number_width, src);
                (Some(text), om)
            } else {
                (None, 1)
            };
            let mut line = code_line_with_prefix(prefix_segments, gutter_text.as_deref(), gutter_style);
            if is_omitted {
                line.push_segment(gutter_style, src);
            } else {
                let mut in_leading_whitespace = true;
                for (style, text) in segments {
                    let is_leading_whitespace = in_leading_whitespace && text.trim().is_empty();
                    if is_leading_whitespace {
                        line.push_segment(code_style, text);
                    } else {
                        in_leading_whitespace = false;
                        line.push_segment(apply_code_weight(*style, theme_styles, code_block_weight), text);
                    }
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
    let gutter_style = code_gutter_style(theme_styles, base_style, code_block_weight);
    let code_style = code_block_style(theme_styles, base_style, code_block_weight);

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
                line.push_segment(code_style, trimmed);
            }
        }
        lines.push(line);
        line_number = line_number.saturating_add(omitted);
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
        output.push(format_diff_summary(block.path.as_str(), block.additions, block.deletions));
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
    render_diff_code_block_with_context(
        code,
        theme_styles,
        base_style,
        prefix_segments,
        current_diff_render_style_context(),
    )
}

pub(crate) fn render_diff_code_block_with_context(
    code: &str,
    theme_styles: &ThemeStyles,
    base_style: Style,
    prefix_segments: &[MarkdownSegment],
    style_context: crate::tui::utils::diff_styles::DiffRenderStyleContext,
) -> Vec<MarkdownLine> {
    let mut lines = Vec::new();
    // Diffs carry their own semantic palette (add/delete tints and chips), so
    // they keep the primary code weight even on muted surfaces.
    let context_style = code_block_style(theme_styles, base_style, CodeBlockWeight::Primary);
    let metadata_style = Style::new()
        .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlack)))
        .effects(Effects::DIMMED);
    let hunk_style = style_hunk_header_ansi(style_context);
    let added_style = style_content_ansi(DiffLineType::Insert, style_context);
    let removed_style = style_content_ansi(DiffLineType::Delete, style_context);
    let added_marker_style = style_sign_ansi(DiffLineType::Insert, style_context);
    let removed_marker_style = style_sign_ansi(DiffLineType::Delete, style_context);
    let file_old_style = style_file_header_old_ansi(style_context);
    let file_new_style = style_file_header_new_ansi(style_context);
    let added_background = added_style.get_bg_color();
    let removed_background = removed_style.get_bg_color();
    let added_word = style_context.add_word_bg();
    let removed_word = style_context.del_word_bg();
    let mut current_language: Option<String> = None;
    let normalized = normalize_diff_lines(code);
    let word_ranges_by_line = word_level_ranges_for_normalized(&normalized);

    for (line_index, line) in normalized.iter().enumerate() {
        let trimmed = line.trim_end_matches('\n');
        let trimmed_start = trimmed.trim_start();
        if let Some(path) = diff_path_for_language(trimmed_start) {
            current_language = language_hint_from_path(&path);
        }
        if let Some((path, additions, deletions)) = parse_diff_summary_line(trimmed_start) {
            let leading_len = trimmed.len().saturating_sub(trimmed_start.len());
            let leading = &trimmed[..leading_len];
            let mut line = prefixed_line(prefix_segments);
            if !leading.is_empty() {
                line.push_segment(context_style, leading);
            }
            // Summary counts keep accessible red/green foregrounds without
            // the body tint.
            let count_style = |kind: DiffLineType| {
                let fg = match kind {
                    DiffLineType::Insert => Some(diff_add_fg(style_context.theme(), style_context.level())),
                    DiffLineType::Delete => Some(diff_del_fg(style_context.theme(), style_context.level())),
                    DiffLineType::Context => None,
                };
                Style::new().fg_color(fg)
            };
            line.push_segment(context_style, &format!("{DIFF_SUMMARY_PREFIX}{path} ("));
            line.push_segment(count_style(DiffLineType::Insert), &format!("+{additions}"));
            line.push_segment(context_style, " ");
            line.push_segment(count_style(DiffLineType::Delete), &format!("-{deletions}"));
            line.push_segment(context_style, ")");
            lines.push(line);
            continue;
        }

        let mut line = prefixed_line(prefix_segments);
        if !trimmed.is_empty() {
            if trimmed_start.starts_with("diff --git ") || trimmed_start.starts_with("index ") {
                line.push_segment(metadata_style, trimmed);
            } else if trimmed_start.starts_with("--- ") || trimmed_start.starts_with("*** Delete File:") {
                line.push_segment(file_old_style, trimmed);
            } else if trimmed_start.starts_with("+++ ") || trimmed_start.starts_with("*** Add File:") {
                line.push_segment(file_new_style, trimmed);
            } else if is_diff_header_line(trimmed_start) {
                line.push_segment(hunk_style, trimmed);
            } else if is_diff_addition_line(trimmed_start) {
                line.push_segment(added_marker_style, "+");
                let body = &trimmed_start[1..];
                if body.is_empty() {
                    line.push_segment(added_style, " ");
                } else {
                    let word_ranges = word_ranges_by_line.get(line_index).map(Vec::as_slice).unwrap_or(&[]);
                    push_highlighted_diff_body(
                        &mut line,
                        body,
                        current_language.as_deref(),
                        added_style,
                        added_background,
                        word_ranges,
                        added_word,
                    );
                }
                paint_line_background(&mut line, added_background);
                line.set_line_background(added_background);
            } else if trimmed_start.starts_with("*** Update File:") {
                line.push_segment(added_marker_style, "+");
                line.push_segment(added_style, trimmed_start);
                paint_line_background(&mut line, added_background);
                line.set_line_background(added_background);
            } else if is_diff_deletion_line(trimmed_start) {
                line.push_segment(removed_marker_style, "-");
                let body = &trimmed_start[1..];
                if body.is_empty() {
                    line.push_segment(removed_style, " ");
                } else {
                    let word_ranges = word_ranges_by_line.get(line_index).map(Vec::as_slice).unwrap_or(&[]);
                    push_highlighted_diff_body(
                        &mut line,
                        body,
                        current_language.as_deref(),
                        removed_style,
                        removed_background,
                        word_ranges,
                        removed_word,
                    );
                }
                paint_line_background(&mut line, removed_background);
                line.set_line_background(removed_background);
            } else if let Some(context_body) = trimmed.strip_prefix(' ') {
                line.push_segment(context_style, " ");
                line.push_segment(context_style, context_body);
            } else {
                line.push_segment(context_style, trimmed);
            }
        }
        lines.push(line);
    }

    lines
}

/// Fill unpainted holes on a tinted diff row, including any blockquote/list
/// prefix that was added before the diff marker.
fn paint_line_background(line: &mut MarkdownLine, background: Option<AnstyleColor>) {
    let Some(background) = background else {
        return;
    };
    for segment in &mut line.segments {
        if segment.style.get_bg_color().is_none() {
            segment.style = segment.style.bg_color(Some(background));
        }
    }
}

/// Push a `+`/`-` diff body while preserving syntax foregrounds and applying
/// the stronger background only to changed intraline ranges.
fn push_highlighted_diff_body(
    line: &mut MarkdownLine,
    body: &str,
    language: Option<&str>,
    fallback: Style,
    row_background: Option<AnstyleColor>,
    word_ranges: &[(usize, usize)],
    word_background: Option<AnstyleColor>,
) {
    let segments = render_diff_content_segments(body, language, fallback);
    if segments.is_empty() {
        line.push_segment(fallback.bg_color(row_background), " ");
        return;
    }

    let mut cursor = 0usize;
    for segment in segments {
        let segment_len = segment.text.len();
        let segment_end = cursor.saturating_add(segment_len);
        let mut boundaries = vec![0, segment_len];
        for &(start, end) in word_ranges {
            if start < segment_end && end > cursor {
                boundaries.push(start.max(cursor).saturating_sub(cursor).min(segment_len));
                boundaries.push(end.min(segment_end).saturating_sub(cursor).min(segment_len));
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();

        for pair in boundaries.windows(2) {
            let start = pair[0];
            let end = pair[1];
            if start >= end {
                continue;
            }
            let global_start = cursor.saturating_add(start);
            let global_end = cursor.saturating_add(end);
            let changed = word_background.is_some()
                && word_ranges
                    .iter()
                    .any(|&(range_start, range_end)| global_start < range_end && global_end > range_start);
            let background = if changed { word_background } else { row_background };
            let mut style = segment.style.bg_color(background);
            if style.get_fg_color().is_none() {
                style = style.fg_color(fallback.get_fg_color());
            }
            line.push_segment(style, &segment.text[start..end]);
        }
        cursor = segment_end;
    }
}

/// Map each normalized body line to byte-safe word-level ranges against its
/// consecutive opposite-side line.
fn word_level_ranges_for_normalized(normalized: &[String]) -> Vec<Vec<(usize, usize)>> {
    let mut display = Vec::with_capacity(normalized.len());
    for line in normalized {
        let trimmed = line.trim_end_matches('\n');
        let trimmed_start = trimmed.trim_start();
        let kind = if is_diff_addition_line(trimmed_start) {
            DiffDisplayKind::Addition
        } else if is_diff_deletion_line(trimmed_start) {
            DiffDisplayKind::Deletion
        } else {
            DiffDisplayKind::Context
        };
        let body = match kind {
            DiffDisplayKind::Addition | DiffDisplayKind::Deletion => {
                trimmed_start.strip_prefix(['+', '-']).unwrap_or("").to_owned()
            }
            DiffDisplayKind::Context | DiffDisplayKind::Metadata | DiffDisplayKind::HunkHeader => trimmed.to_owned(),
        };
        display.push(DiffDisplayLine::body(kind, None, None, body));
    }
    annotate_word_level_diffs(&mut display);
    display.into_iter().map(|line| line.changed).collect()
}

fn diff_path_for_language(line: &str) -> Option<String> {
    parse_diff_git_path(line).or_else(|| parse_diff_marker_path(line)).or_else(|| {
        ["*** Update File:", "*** Add File:", "*** Delete File:"]
            .iter()
            .find_map(|prefix| line.strip_prefix(prefix).map(str::trim).filter(|path| !path.is_empty()))
            .map(ToOwned::to_owned)
    })
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
    let source = if base_style == theme_styles.tool_output {
        theme_styles.tool_detail
    } else {
        base_style
    };
    // Blend the foreground with background at 35% for a visible but subdued gutter color.
    // No DIMMED effect — keep line numbers readable without squinting.
    let blended_fg = source
        .get_fg_color()
        .and_then(|fg| vtcode_commons::colors::blend_colors(&fg, &theme_styles.background, 0.35));
    let mut style = Style::new();
    if let Some(color) = blended_fg {
        style = style.fg_color(Some(color));
    } else if let Some(fg) = source.get_fg_color() {
        style = style.fg_color(Some(fg));
    } else {
        // Last resort: use the theme foreground color
        style = style.fg_color(Some(theme_styles.foreground));
    }
    style
}

/// Gutter style for a code block at the given visual weight.
fn code_gutter_style(theme_styles: &ThemeStyles, base_style: Style, weight: CodeBlockWeight) -> Style {
    match weight {
        CodeBlockWeight::Primary => line_number_style(theme_styles, base_style),
        CodeBlockWeight::Muted => muted_line_number_style(theme_styles),
    }
}

/// Gutter style for a muted code surface.
///
/// The gutter is chrome, so it stays neutral: a muted block introduces the
/// code with the theme foreground rather than the severity color of the
/// surrounding incident, which would read as part of the code. It recedes by
/// the same share as the primary gutter and stops at the theme's contrast
/// floor, so the numbers stay readable on light and dark backgrounds alike.
fn muted_line_number_style(theme_styles: &ThemeStyles) -> Style {
    let receded = crate::theme::recede_toward_background(
        theme_styles.foreground,
        theme_styles.background,
        MUTED_GUTTER_BLEND_RATIO,
        crate::theme::get_minimum_contrast(),
    );
    Style::new().fg_color(Some(receded))
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
    code.lines().map(|line| parse_omitted_line_count(line).unwrap_or(1)).sum()
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
    let Some(lang) = language else {
        return false;
    };
    let lang_lower = lang.to_ascii_lowercase();
    if !matches!(lang_lower.as_str(), "markdown" | "md") {
        return false;
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
    // Preserve the authored hunk header (including range counts). Rewriting to
    // start-only form made partial hunks such as `@@ -65,19 +65,0 @@` read as a
    // one-line change, contradicting the enclosing `• Diff … (+0 -19)` summary.
    line.to_string()
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
    language.is_some_and(|lang| matches!(lang.to_ascii_lowercase().as_str(), "diff" | "patch" | "udiff" | "git"))
}

fn code_block_style(theme_styles: &ThemeStyles, base_style: Style, weight: CodeBlockWeight) -> Style {
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
    } else {
        // Ensure code text always has a visible foreground color
        style = style.fg_color(Some(theme_styles.foreground));
    }
    apply_code_weight(style, theme_styles, weight)
}

/// Recede a code foreground toward the theme background.
///
/// Muted code keeps its hue so syntax-highlighted tokens stay
/// distinguishable; only the intensity drops. The recede never crosses the
/// theme's contrast floor, so evidence stays readable on light and dark
/// backgrounds alike. The result is an explicit color rather than a faint/dim
/// attribute, so it is identical on every terminal.
fn apply_code_weight(style: Style, theme_styles: &ThemeStyles, weight: CodeBlockWeight) -> Style {
    if weight != CodeBlockWeight::Muted {
        return style;
    }
    let Some(foreground) = style.get_fg_color() else {
        return style;
    };
    let receded = crate::theme::recede_toward_background(
        foreground,
        theme_styles.background,
        MUTED_CODE_BLEND_RATIO,
        crate::theme::get_minimum_contrast(),
    );
    style.fg_color(Some(receded))
}

pub(crate) fn normalize_code_indentation(code: &str, language: Option<&str>, preserve_indentation: bool) -> String {
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
                        anstyle::Color::Rgb(anstyle::RgbColor(brighten(rgb.0), brighten(rgb.1), brighten(rgb.2)))
                    }
                    anstyle::Color::Ansi(ansi) => match ansi {
                        anstyle::AnsiColor::Black => anstyle::Color::Ansi(anstyle::AnsiColor::BrightWhite),
                        anstyle::AnsiColor::Red => anstyle::Color::Ansi(anstyle::AnsiColor::BrightRed),
                        anstyle::AnsiColor::Green => anstyle::Color::Ansi(anstyle::AnsiColor::BrightGreen),
                        anstyle::AnsiColor::Yellow => anstyle::Color::Ansi(anstyle::AnsiColor::BrightYellow),
                        anstyle::AnsiColor::Blue => anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlue),
                        anstyle::AnsiColor::Magenta => anstyle::Color::Ansi(anstyle::AnsiColor::BrightMagenta),
                        anstyle::AnsiColor::Cyan => anstyle::Color::Ansi(anstyle::AnsiColor::BrightCyan),
                        anstyle::AnsiColor::White => anstyle::Color::Ansi(anstyle::AnsiColor::BrightWhite),
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
        return vec![MarkdownSegment::new(fallback_style, " ")];
    }

    // Prose diffs render as solid fallback: syntax colors on tinted diff
    // backgrounds hurt contrast and obscure add/del semantics.
    if is_prose_language_hint(language) {
        return vec![MarkdownSegment::new(fallback_style, text)];
    }

    // Unknown/missing languages must stay solid so the diff add/del color
    // (green/red) is preserved. Syntect falls back to "Plain Text" with a
    // single default-style span, which would drop the diff foreground.
    let hint = language.map(str::trim).filter(|hint| !hint.is_empty());
    let Some(hint) = hint else {
        return vec![MarkdownSegment::new(fallback_style, text)];
    };
    if std::ptr::eq(syntax_highlight::find_syntax_by_token(hint), syntax_highlight::find_syntax_plain_text()) {
        return vec![MarkdownSegment::new(fallback_style, text)];
    }

    if let Some(segments) = highlight_line_for_diff(text, Some(hint))
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
        let direct_match = config.enabled_languages.iter().any(|entry| entry.eq_ignore_ascii_case(lang));
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

    let rendered = syntax_highlight::highlight_code_to_anstyle_line_segments(code, language, &config.theme, true);

    Some(rendered)
}

#[derive(Clone, Debug)]
pub struct HighlightedSegment {
    style: Style,
    text: String,
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
                let _ = write!(ansi_line, "{rendered}{}{}", seg.text, anstyle::Reset);
            }
            ansi_line
        })
        .collect()
}
