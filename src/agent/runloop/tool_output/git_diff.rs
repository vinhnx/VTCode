use std::borrow::Cow;
use std::cmp::min;

use anstyle::Style as AnsiStyle;
use anyhow::Result;
use serde_json::Value;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::panels::{PanelContentLine, render_panel};
use super::streams::{render_stream_section, strip_ansi_codes};
use super::styles::{GitStyles, LsStyles};

struct GitDiffSection<'a> {
    path: Cow<'a, str>,
    additions: usize,
    deletions: usize,
    formatted: Cow<'a, str>,
    hunks: Vec<GitDiffHunk<'a>>,
}

struct GitDiffHunk<'a> {
    old_start: usize,
    old_lines: usize,
    new_start: usize,
    new_lines: usize,
    lines: Vec<GitDiffLine<'a>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DiffLineKind {
    Addition,
    Deletion,
    Context,
}

struct GitDiffLine<'a> {
    kind: DiffLineKind,
    old_line: Option<usize>,
    new_line: Option<usize>,
    text: &'a str,
}

#[cfg_attr(
    feature = "profiling",
    tracing::instrument(
        skip(renderer, payload, git_styles, ls_styles, config),
        level = "debug"
    )
)]
pub(super) fn render_git_diff(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    mode: ToolOutputMode,
    tail_limit: usize,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
    allow_ansi: bool,
    config: Option<&VTCodeConfig>,
) -> Result<()> {
    let sections = parse_git_diff_sections(payload);
    if sections.is_empty() {
        return Ok(());
    }

    let should_virtualize = matches!(mode, ToolOutputMode::Compact) && sections.len() > 3;

    if should_virtualize {
        for section in &sections {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "  {} +{} -{} ({} lines)",
                    section.path,
                    section.additions,
                    section.deletions,
                    section
                        .hunks
                        .iter()
                        .map(|hunk| hunk.lines.len())
                        .sum::<usize>()
                ),
            )?;
        }

        if let Some(last) = sections.last() {
            let preview_limit = tail_limit.min(20);
            if let Some(preview) = build_git_diff_preview(last, preview_limit.max(1)) {
                renderer.line(MessageStyle::Info, "")?;
                renderer.line(MessageStyle::Info, &format!("Preview of {}:", last.path))?;
                render_stream_section(
                    renderer,
                    "",
                    &preview,
                    mode,
                    preview_limit,
                    Some(tools::GIT_DIFF),
                    git_styles,
                    ls_styles,
                    MessageStyle::Info,
                    allow_ansi,
                    config,
                )?;
            }
        }
        return Ok(());
    }

    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            renderer.line(MessageStyle::Info, "")?;
        }
        if section.formatted.trim().is_empty() {
            render_structured_diff_section(renderer, section, git_styles, allow_ansi)?;
        } else {
            render_formatted_diff_section(renderer, section, allow_ansi)?;
        }
    }

    Ok(())
}

fn build_git_diff_preview(section: &GitDiffSection<'_>, max_lines: usize) -> Option<String> {
    if max_lines == 0 {
        return None;
    }

    let mut output = String::new();
    let mut lines_written = 0usize;
    let mut truncated = false;

    for hunk in &section.hunks {
        if !hunk
            .lines
            .iter()
            .any(|line| line.kind != DiffLineKind::Context)
        {
            continue;
        }

        let header = format!(
            "@@ -{},{} +{},{} @@",
            hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
        );
        if !push_preview_line(&mut output, &header, &mut lines_written, max_lines) {
            truncated = true;
            break;
        }

        for line in &hunk.lines {
            let marker = match line.kind {
                DiffLineKind::Addition => '+',
                DiffLineKind::Deletion => '-',
                DiffLineKind::Context => continue,
            };

            let raw = line.text.strip_suffix('\n').unwrap_or(line.text);
            let sanitized = sanitize_diff_text(raw);
            let sanitized_ref = sanitized.as_ref();
            let mut line_buf = String::with_capacity(1 + sanitized_ref.len() + 1);
            line_buf.push(marker);
            if !sanitized_ref.is_empty() {
                line_buf.push(' ');
                line_buf.push_str(sanitized_ref);
            }

            if !push_preview_line(&mut output, &line_buf, &mut lines_written, max_lines) {
                truncated = true;
                break;
            }
        }

        if truncated {
            break;
        }
    }

    if output.is_empty() {
        return None;
    }

    if truncated {
        output.push('\n');
        output.push_str("...");
    }

    Some(output)
}

fn push_preview_line(
    buffer: &mut String,
    line: &str,
    lines_written: &mut usize,
    max_lines: usize,
) -> bool {
    if *lines_written >= max_lines {
        return false;
    }

    if !buffer.is_empty() {
        buffer.push('\n');
    }

    buffer.push_str(line);
    *lines_written += 1;

    *lines_written < max_lines
}

fn parse_git_diff_sections<'a>(payload: &'a Value) -> Vec<GitDiffSection<'a>> {
    let mut sections = Vec::new();
    let Some(files) = payload.get("files").and_then(Value::as_array) else {
        return sections;
    };

    for file in files {
        let path = file.get("path").and_then(Value::as_str).unwrap_or("diff");

        let summary = file.get("summary").and_then(Value::as_object);
        let additions = summary
            .and_then(|value| value.get("additions"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let deletions = summary
            .and_then(|value| value.get("deletions"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;

        let formatted_raw = file.get("formatted").and_then(Value::as_str).unwrap_or("");
        let formatted = strip_diff_fences(formatted_raw);

        let hunks = parse_diff_hunks(file.get("hunks"));

        let is_empty = file
            .get("is_empty")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if is_empty && hunks.is_empty() && formatted.trim().is_empty() {
            continue;
        }

        sections.push(GitDiffSection {
            path: Cow::Borrowed(path),
            additions,
            deletions,
            formatted,
            hunks,
        });
    }

    sections
}

fn parse_diff_hunks<'a>(value: Option<&'a Value>) -> Vec<GitDiffHunk<'a>> {
    let mut hunks = Vec::new();
    let Some(array) = value.and_then(Value::as_array) else {
        return hunks;
    };

    hunks.reserve(array.len());

    for item in array {
        let old_start = item.get("old_start").and_then(Value::as_u64).unwrap_or(0) as usize;
        let old_lines = item.get("old_lines").and_then(Value::as_u64).unwrap_or(0) as usize;
        let new_start = item.get("new_start").and_then(Value::as_u64).unwrap_or(0) as usize;
        let new_lines = item.get("new_lines").and_then(Value::as_u64).unwrap_or(0) as usize;
        let lines = parse_diff_lines(item.get("lines"));

        hunks.push(GitDiffHunk {
            old_start,
            old_lines,
            new_start,
            new_lines,
            lines,
        });
    }

    hunks
}

fn parse_diff_lines<'a>(value: Option<&'a Value>) -> Vec<GitDiffLine<'a>> {
    let mut lines = Vec::new();
    let Some(array) = value.and_then(Value::as_array) else {
        return lines;
    };

    lines.reserve(array.len());

    for entry in array {
        let Some(kind_value) = entry.get("kind") else {
            continue;
        };
        let Some(kind) = parse_diff_line_kind(kind_value) else {
            continue;
        };
        let text = entry.get("text").and_then(Value::as_str).unwrap_or("");
        let old_line = entry
            .get("old_line")
            .and_then(Value::as_u64)
            .map(|value| value as usize);
        let new_line = entry
            .get("new_line")
            .and_then(Value::as_u64)
            .map(|value| value as usize);

        lines.push(GitDiffLine {
            kind,
            old_line,
            new_line,
            text,
        });
    }

    lines
}

fn parse_diff_line_kind(value: &Value) -> Option<DiffLineKind> {
    let kind_str = value.as_str()?;
    match kind_str {
        "addition" => Some(DiffLineKind::Addition),
        "deletion" => Some(DiffLineKind::Deletion),
        "context" => Some(DiffLineKind::Context),
        _ => None,
    }
}

fn render_formatted_diff_section(
    renderer: &mut AnsiRenderer,
    section: &GitDiffSection<'_>,
    allow_ansi: bool,
) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        &format!(
            "{}  (+{}  -{})",
            section.path, section.additions, section.deletions
        ),
    )?;

    let trimmed = section.formatted.trim_matches(['\n', '\r']);
    if trimmed.is_empty() {
        renderer.line(MessageStyle::Info, "  (no diff content available)")?;
        return Ok(());
    }

    let diff_body = if allow_ansi {
        Cow::Borrowed(trimmed)
    } else {
        strip_ansi_codes(trimmed)
    };

    const MAX_DIFF_LINES: usize = 500;
    const MAX_LINE_LENGTH: usize = 200;
    let mut line_count = 0;
    for line in diff_body.lines() {
        if line_count >= MAX_DIFF_LINES {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "  ... ({} more lines truncated)",
                    diff_body.lines().count() - MAX_DIFF_LINES
                ),
            )?;
            break;
        }

        let display_line = if line.len() > MAX_LINE_LENGTH {
            &line[..line
                .char_indices()
                .nth(MAX_LINE_LENGTH)
                .map(|(i, _)| i)
                .unwrap_or(MAX_LINE_LENGTH)]
        } else {
            line
        };

        if allow_ansi {
            renderer.line_with_override_style(
                MessageStyle::Info,
                AnsiStyle::new(),
                display_line,
            )?;
        } else {
            renderer.line(MessageStyle::Info, display_line)?;
        }
        line_count += 1;
    }

    Ok(())
}

fn render_structured_diff_section(
    renderer: &mut AnsiRenderer,
    section: &GitDiffSection<'_>,
    git_styles: &GitStyles,
    _allow_ansi: bool,
) -> Result<()> {
    const CONTEXT_LINE_WINDOW: usize = 2;

    let mut lines = Vec::new();

    if section.hunks.is_empty() {
        if section.formatted.trim().is_empty() {
            lines.push(PanelContentLine::new(
                "     (no diff content available)",
                MessageStyle::Info,
            ));
        } else {
            for line in section.formatted.lines() {
                lines.push(PanelContentLine::new(line.to_string(), MessageStyle::Info));
            }
        }

        return render_panel(
            renderer,
            Some(format!(
                "{}  (+{}  -{})",
                section.path, section.additions, section.deletions
            )),
            lines,
            MessageStyle::Info,
        );
    }

    for (hunk_index, hunk) in section.hunks.iter().enumerate() {
        if hunk_index > 0 {
            lines.push(PanelContentLine::new(String::new(), MessageStyle::Info));
        }

        lines.push(format_hunk_header_line(hunk));

        let line_count = hunk.lines.len();
        let mut include = vec![false; line_count];

        for (idx, diff_line) in hunk.lines.iter().enumerate() {
            if diff_line.kind != DiffLineKind::Context {
                include[idx] = true;
                let start = idx.saturating_sub(CONTEXT_LINE_WINDOW);
                for offset in start..idx {
                    include[offset] = true;
                }
                if line_count > 0 {
                    let end = min(idx + CONTEXT_LINE_WINDOW, line_count - 1);
                    for offset in (idx + 1)..=end {
                        include[offset] = true;
                    }
                }
            }
        }

        let first_included = include.iter().position(|included| *included);
        let last_included_idx = include.iter().rposition(|included| *included);
        let mut emitted_change = false;

        if let (Some(first), Some(last)) = (first_included, last_included_idx) {
            if first > 0 {
                push_diff_gap_line(&mut lines, &hunk.lines[..first]);
            }

            let mut previous = None;
            for (idx, diff_line) in hunk.lines.iter().enumerate().take(last + 1) {
                if !include[idx] {
                    continue;
                }

                if let Some(prev_idx) = previous {
                    if idx > prev_idx + 1 {
                        push_diff_gap_line(&mut lines, &hunk.lines[prev_idx + 1..idx]);
                    }
                }

                lines.push(format_diff_line_row(diff_line, git_styles));
                if diff_line.kind != DiffLineKind::Context {
                    emitted_change = true;
                }
                previous = Some(idx);
            }

            if last < line_count.saturating_sub(1) {
                push_diff_gap_line(&mut lines, &hunk.lines[last + 1..]);
            }
        }

        if !emitted_change {
            lines.push(PanelContentLine::new(
                "     (no visible changes)".to_string(),
                MessageStyle::Info,
            ));
        }
    }

    render_panel(
        renderer,
        Some(format!(
            "{}  (+{}  -{})",
            section.path, section.additions, section.deletions
        )),
        lines,
        MessageStyle::Info,
    )
}

fn push_diff_gap_line(lines: &mut Vec<PanelContentLine>, gap: &[GitDiffLine<'_>]) {
    if gap.is_empty() {
        return;
    }

    let mut render_buffer = String::with_capacity(48);
    render_buffer.push_str("     ");

    let count = gap.len();
    render_buffer.push_str("...");

    render_buffer.push(' ');
    render_buffer.push('(');
    render_buffer.push_str(&count.to_string());
    render_buffer.push_str(" line");
    if count != 1 {
        render_buffer.push('s');
    }

    if let Some((old_start, old_end)) = line_range(gap.iter().filter_map(|line| line.old_line)) {
        render_buffer.push_str(", old ");
        format_line_interval(&mut render_buffer, old_start, old_end);
    }

    if let Some((new_start, new_end)) = line_range(gap.iter().filter_map(|line| line.new_line)) {
        render_buffer.push_str(", new ");
        format_line_interval(&mut render_buffer, new_start, new_end);
    }

    render_buffer.push(')');

    lines.push(PanelContentLine::new(render_buffer, MessageStyle::Info));
}

fn line_range<I>(mut iter: I) -> Option<(usize, usize)>
where
    I: Iterator<Item = usize>,
{
    let start = iter.next()?;
    let mut end = start;
    for value in iter {
        end = value;
    }
    Some((start, end))
}

fn format_line_interval(buffer: &mut String, start: usize, end: usize) {
    if start == end {
        buffer.push_str(&start.to_string());
    } else {
        buffer.push_str(&start.to_string());
        buffer.push('-');
        buffer.push_str(&end.to_string());
    }
}

fn format_hunk_header_line(hunk: &GitDiffHunk<'_>) -> PanelContentLine {
    let plain = format!(
        "     @@ -{},{} +{},{} @@",
        hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
    );
    PanelContentLine::with_rendered(plain, MessageStyle::Info)
}

fn format_diff_line_row(line: &GitDiffLine<'_>, git_styles: &GitStyles) -> PanelContentLine {
    use std::fmt::Write as FmtWrite;

    let kind = &line.kind;
    let style = match kind {
        DiffLineKind::Addition => MessageStyle::Response,
        DiffLineKind::Deletion => MessageStyle::Error,
        DiffLineKind::Context => MessageStyle::Info,
    };
    let marker = match kind {
        DiffLineKind::Addition => "+",
        DiffLineKind::Deletion => "-",
        DiffLineKind::Context => " ",
    };

    let raw_text = line.text.strip_suffix('\n').unwrap_or(line.text);
    let sanitized = sanitize_diff_text(raw_text);
    let sanitized_ref = sanitized.as_ref();
    let estimated_len = 4 + 1 + 4 + 2 + 1 + sanitized_ref.len();
    let mut rendered = String::with_capacity(estimated_len);

    if let Some(value) = line.old_line {
        let _ = FmtWrite::write_fmt(&mut rendered, format_args!("{:>4}", value));
    } else {
        rendered.push_str("    ");
    }

    rendered.push(' ');

    if let Some(value) = line.new_line {
        let _ = FmtWrite::write_fmt(&mut rendered, format_args!("{:>4}", value));
    } else {
        rendered.push_str("    ");
    }

    rendered.push_str("  ");
    rendered.push_str(marker);

    if !sanitized_ref.is_empty() {
        rendered.push(' ');
        rendered.push_str(sanitized_ref);
    }

    if let Some(override_style) = git_styles.style_for_line(kind) {
        PanelContentLine::with_override(rendered, style, override_style)
    } else {
        PanelContentLine::with_rendered(rendered, style)
    }
}

fn sanitize_diff_text(text: &str) -> Cow<'_, str> {
    if text.contains('\t') {
        Cow::Owned(text.replace('\t', "    "))
    } else {
        Cow::Borrowed(text)
    }
}

fn strip_diff_fences(input: &str) -> Cow<'_, str> {
    let trimmed = input.trim();
    if !trimmed.starts_with("```diff") {
        return Cow::Borrowed(trimmed);
    }

    let body = trimmed.trim_start_matches("```diff");
    let body = body.trim_start_matches(|ch| ch == '\r' || ch == '\n');
    let body = body.trim_end_matches(|ch| ch == '\r' || ch == '\n');
    let body = body.trim_end_matches("```");

    Cow::Owned(body.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_diff_fences_handles_diff_code_blocks() {
        let input = "```diff\n+ addition\n- deletion\n```";
        let result = strip_diff_fences(input);
        assert_eq!(result.as_ref(), "+ addition\n- deletion");
    }

    #[test]
    fn test_strip_diff_fences_returns_trimmed_when_not_diff() {
        let input = "some text";
        let result = strip_diff_fences(input);
        assert_eq!(result.as_ref(), "some text");
    }

    #[test]
    fn push_preview_line_respects_limit() {
        let mut buffer = String::new();
        let mut count = 0;
        assert!(push_preview_line(&mut buffer, "line1", &mut count, 2));
        assert!(push_preview_line(&mut buffer, "line2", &mut count, 2));
        assert!(!push_preview_line(&mut buffer, "line3", &mut count, 2));
        assert_eq!(buffer, "line1\nline2");
    }

    #[test]
    fn sanitize_diff_text_replaces_tabs() {
        let sanitized = sanitize_diff_text("\tfoo");
        assert_eq!(sanitized.as_ref(), "    foo");
    }

    #[test]
    fn line_range_extracts_bounds() {
        let range = line_range([1, 2, 3].into_iter());
        assert_eq!(range, Some((1, 3)));
    }
}
