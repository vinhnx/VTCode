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
enum DiffLineKind {
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

/// Unified diff rendering with line numbers and colors
fn render_unified_diff(
    renderer: &mut AnsiRenderer,
    diff_content: &str,
    git_styles: &GitStyles,
    path: Option<&str>,
) -> Result<()> {
    use std::fmt::Write;
    
    if diff_content.is_empty() {
        return Ok(());
    }

    if let Some(p) = path {
        renderer.line(MessageStyle::Info, &format!("● Path: {}", p))?;
        renderer.line(MessageStyle::Info, "")?;
    }

    let mut line_num = 0;
    let mut in_hunk = false;
    let mut buf = String::with_capacity(128);
    let default_style = AnsiStyle::new();

    for line in diff_content.lines() {
        let trimmed = line.trim_start();

        if trimmed.starts_with("@@") {
            in_hunk = true;
            if let Some(start) = parse_hunk_line_number(trimmed) {
                line_num = start;
            }
            renderer.line_with_style(git_styles.header.unwrap_or(default_style), line)?;
            continue;
        }

        if !in_hunk {
            renderer.line(MessageStyle::Info, line)?;
            continue;
        }

        match trimmed.chars().next() {
            Some('+') if !trimmed.starts_with("+++") => {
                buf.clear();
                let _ = write!(&mut buf, "+ {:>4}: {}", line_num, &trimmed[1..]);
                renderer.line_with_style(git_styles.add.unwrap_or(default_style), &buf)?;
                line_num += 1;
            }
            Some('-') if !trimmed.starts_with("---") => {
                buf.clear();
                let _ = write!(&mut buf, "- {:>4}: {}", line_num, &trimmed[1..]);
                renderer.line_with_style(git_styles.remove.unwrap_or(default_style), &buf)?;
            }
            _ if !trimmed.starts_with("+++") && !trimmed.starts_with("---") => {
                buf.clear();
                let _ = write!(&mut buf, "  {:>4}: {}", line_num, trimmed);
                renderer.line(MessageStyle::Response, &buf)?;
                line_num += 1;
            }
            _ => {
                renderer.line(MessageStyle::Info, line)?;
            }
        }
    }

    Ok(())
}

fn parse_hunk_line_number(hunk_header: &str) -> Option<usize> {
    hunk_header
        .split_whitespace()
        .nth(2)?
        .trim_start_matches('+')
        .split(',')
        .next()?
        .parse()
        .ok()
}

#[cfg_attr(
    feature = "profiling",
    tracing::instrument(
        skip(renderer, payload, git_styles, ls_styles, config),
        level = "debug"
    )
)]
fn render_git_diff(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    mode: ToolOutputMode,
    tail_limit: usize,
    git_styles: &GitStyles,
    _ls_styles: &LsStyles,
    _allow_ansi: bool,
    _config: Option<&VTCodeConfig>,
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
                render_unified_diff(renderer, &preview, git_styles, Some(last.path.as_ref()))?;
            }
        }
        return Ok(());
    }

    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            renderer.line(MessageStyle::Info, "")?;
        }
        if section.formatted.trim().is_empty() {
            render_structured_diff_section(renderer, section, git_styles)?;
        } else {
            render_unified_diff(
                renderer,
                section.formatted.as_ref(),
                git_styles,
                Some(section.path.as_ref()),
            )?;
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

fn render_structured_diff_section(
    renderer: &mut AnsiRenderer,
    section: &GitDiffSection<'_>,
    git_styles: &GitStyles,
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

use anstyle::{AnsiColor, Style as AnsiStyle};
use anyhow::{Context, Result};
use serde_json::Value;
use shell_words::split as shell_split;
use smallvec::SmallVec;
use std::borrow::Cow;
use std::cmp::min;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::{defaults, tools};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::mcp::McpRendererProfile;
use vtcode_core::tools::{PlanCompletionState, StepStatus, TaskPlan};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::transcript;

const INLINE_STREAM_MAX_LINES: usize = 30;

use crate::agent::runloop::text_tools::CodeFenceBlock;

struct PanelContentLine {
    rendered: String,
    style: MessageStyle,
    override_style: Option<AnsiStyle>,
}

impl PanelContentLine {
    fn new(text: impl Into<String>, style: MessageStyle) -> Self {
        Self {
            rendered: text.into(),
            style,
            override_style: None,
        }
    }

    fn with_rendered(rendered: impl Into<String>, style: MessageStyle) -> Self {
        Self {
            rendered: rendered.into(),
            style,
            override_style: None,
        }
    }

    fn with_override(
        rendered: impl Into<String>,
        style: MessageStyle,
        override_style: AnsiStyle,
    ) -> Self {
        Self {
            rendered: rendered.into(),
            style,
            override_style: Some(override_style),
        }
    }
}
fn render_panel(
    renderer: &mut AnsiRenderer,
    title: Option<String>,
    lines: Vec<PanelContentLine>,
    header_style: MessageStyle,
) -> Result<()> {
    if let Some(title_text) = title {
        renderer.line(header_style, title_text.trim_end())?;
    }

    for line in lines {
        let text = line.rendered.trim_end();
        if let Some(override_style) = line.override_style {
            renderer.line_with_override_style(line.style, override_style, text)?;
        } else {
            renderer.line(line.style, text)?;
        }
    }

    Ok(())
}

fn render_left_border_panel(
    renderer: &mut AnsiRenderer,
    lines: Vec<PanelContentLine>,
) -> Result<()> {
    for line in lines {
        if let Some(override_style) = line.override_style {
            renderer.line_with_override_style(
                line.style,
                override_style,
                line.rendered.as_str(),
            )?;
        } else {
            renderer.line(line.style, line.rendered.as_str())?;
        }
    }

    Ok(())
}

fn clamp_panel_text(text: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut truncated = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index + 1 >= limit {
            truncated.push('…');
            break;
        }
        truncated.push(ch);
    }
    truncated
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        let word_len = word.chars().count();

        // If adding this word would exceed the line width
        if !current_line.is_empty() && current_line.chars().count() + 1 + word_len > width {
            if !current_line.is_empty() {
                lines.push(current_line);
                current_line = String::new();
            }
        }

        // If the word itself is longer than the width, we need to break it
        if word_len > width {
            if !current_line.is_empty() {
                lines.push(current_line);
                current_line = String::new();
            }
            // Break long word into chunks
            let mut remaining = word;
            while !remaining.is_empty() {
                let mut byte_len = remaining.len();
                let mut chars_taken = 0;

                for (idx, ch) in remaining.char_indices() {
                    if chars_taken == width {
                        byte_len = idx;
                        break;
                    }
                    chars_taken += 1;
                    byte_len = idx + ch.len_utf8();
                }

                let chunk = &remaining[..byte_len];
                lines.push(chunk.to_string());
                remaining = &remaining[byte_len..];
            }
        } else {
            // Add word to current line
            if !current_line.is_empty() {
                current_line.push(' ');
            }
            current_line.push_str(word);
        }
    }

    // Add the last line if it's not empty
    if !current_line.is_empty() {
        lines.push(current_line);
    }

    // If no lines were created but we have text, add it as one line
    if lines.is_empty() && !text.is_empty() {
        lines.push(text.to_string());
    }

    lines
}

pub(crate) fn render_tool_output(
    renderer: &mut AnsiRenderer,
    tool_name: Option<&str>,
    val: &Value,
    vt_config: Option<&VTCodeConfig>,
) -> Result<()> {
    let allow_tool_ansi = vt_config.map(|cfg| cfg.ui.allow_tool_ansi).unwrap_or(false);

    // Handle special tools first (they have their own enhanced display)
    match tool_name {
        Some(tools::UPDATE_PLAN) => return render_plan_update(renderer, val),
        Some(tools::WRITE_FILE)
        | Some(tools::CREATE_FILE)
        | Some(tools::APPLY_PATCH)
        | Some(tools::EDIT_FILE) => {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            return render_write_file_preview(renderer, val, &git_styles, &ls_styles);
        }
        Some(tools::GIT_DIFF) => {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            let output_mode = vt_config
                .map(|cfg| cfg.ui.tool_output_mode)
                .unwrap_or(ToolOutputMode::Compact);
            let tail_limit = resolve_stdout_tail_limit(vt_config);
            return render_git_diff(
                renderer,
                val,
                output_mode,
                tail_limit,
                &git_styles,
                &ls_styles,
                allow_tool_ansi,
                vt_config,
            );
        }
        Some(tools::RUN_COMMAND) => {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            return render_terminal_command_panel(
                renderer,
                val,
                &git_styles,
                &ls_styles,
                vt_config,
                allow_tool_ansi,
            );
        }
        Some(tools::CURL) => {
            let output_mode = vt_config
                .map(|cfg| cfg.ui.tool_output_mode)
                .unwrap_or(ToolOutputMode::Compact);
            let tail_limit = resolve_stdout_tail_limit(vt_config);
            return render_curl_result(
                renderer,
                val,
                output_mode,
                tail_limit,
                allow_tool_ansi,
                vt_config,
            );
        }
        Some(tools::LIST_FILES) => {
            return render_list_dir_output(renderer, val);
        }
        Some(tools::READ_FILE) => {
            return render_read_file_output(renderer, val);
        }
        _ => {}
    }

    // For other tools, render a simple status header
    render_simple_tool_status(renderer, tool_name, val)?;

    // Render security notice if present
    if let Some(notice) = val.get("security_notice").and_then(Value::as_str) {
        renderer.line(MessageStyle::Info, notice)?;
    }

    // Handle MCP tools
    if let Some(tool) = tool_name
        && tool.starts_with("mcp_")
    {
        if let Some(profile) = resolve_mcp_renderer_profile(tool, vt_config) {
            match profile {
                McpRendererProfile::Context7 => render_mcp_context7_output(renderer, val)?,
                McpRendererProfile::SequentialThinking => {
                    render_mcp_sequential_output(renderer, val)?
                }
            }
        } else {
            // Generic MCP tool - render content field
            render_generic_mcp_output(renderer, val)?;
        }
    }

    // Render stdout/stderr
    let output_mode = vt_config
        .map(|cfg| cfg.ui.tool_output_mode)
        .unwrap_or(ToolOutputMode::Compact);
    let tail_limit = resolve_stdout_tail_limit(vt_config);
    let git_styles = GitStyles::new();
    let ls_styles = LsStyles::from_env();

    if let Some(stdout) = val.get("stdout").and_then(Value::as_str) {
        render_stream_section(
            renderer,
            "stdout",
            stdout,
            output_mode,
            tail_limit,
            tool_name,
            &git_styles,
            &ls_styles,
            MessageStyle::Response,
            allow_tool_ansi,
            vt_config,
            Some(5),
        )?;
    }
    if let Some(stderr) = val.get("stderr").and_then(Value::as_str) {
        render_stream_section(
            renderer,
            "stderr",
            stderr,
            output_mode,
            tail_limit,
            tool_name,
            &git_styles,
            &ls_styles,
            MessageStyle::Error,
            allow_tool_ansi,
            vt_config,
            Some(5),
        )?;
    }
    Ok(())
}

fn render_simple_tool_status(
    renderer: &mut AnsiRenderer,
    _tool_name: Option<&str>,
    val: &Value,
) -> Result<()> {
    // Status is now rendered in the tool summary line
    // Only render error details if present
    let has_error = val.get("error").is_some() || val.get("error_type").is_some();

    if has_error {
        render_error_details(renderer, val)?;
    }

    Ok(())
}

fn render_error_details(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    // Render error message with better formatting
    if let Some(error_msg) = val.get("message").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Error, &format!("  Error: {}", error_msg))?;
    }

    // Render error type
    if let Some(error_type) = val.get("error_type").and_then(|v| v.as_str()) {
        let type_description = match error_type {
            "InvalidParameters" => "Invalid parameters provided",
            "ToolNotFound" => "Tool not found",
            "ResourceNotFound" => "Resource not found",
            "PermissionDenied" => "Permission denied",
            "ExecutionError" => "Execution error",
            "PolicyViolation" => "Policy violation",
            "Timeout" => "Operation timed out",
            "NetworkError" => "Network error",
            "EncodingError" => "Encoding error",
            "FileSystemError" => "File system error",
            _ => error_type,
        };
        renderer.line(MessageStyle::Info, &format!("  Type: {}", type_description))?;
    }

    // Render original error details if available
    if let Some(original) = val.get("original_error").and_then(|v| v.as_str()) {
        if !original.trim().is_empty() {
            // Truncate very long error messages
            let display_error = if original.len() > 200 {
                format!("{}...", &original[..197])
            } else {
                original.to_string()
            };
            renderer.line(MessageStyle::Info, &format!("  Details: {}", display_error))?;
        }
    }

    // Render file path if error is file-related
    if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Info, &format!("  Path: {}", path))?;
    }

    // Render line/column info if available
    if let Some(line) = val.get("line").and_then(|v| v.as_u64()) {
        if let Some(col) = val.get("column").and_then(|v| v.as_u64()) {
            renderer.line(
                MessageStyle::Info,
                &format!("  Location: line {}, column {}", line, col),
            )?;
        } else {
            renderer.line(MessageStyle::Info, &format!("  Location: line {}", line))?;
        }
    }

    // Render recovery suggestions if available
    if let Some(suggestions) = val.get("recovery_suggestions").and_then(|v| v.as_array()) {
        if !suggestions.is_empty() {
            renderer.line(MessageStyle::Info, "")?;
            renderer.line(MessageStyle::Info, "  Suggestions:")?;
            for (idx, suggestion) in suggestions.iter().take(5).enumerate() {
                if let Some(text) = suggestion.as_str() {
                    renderer.line(MessageStyle::Info, &format!("    {}. {}", idx + 1, text))?;
                }
            }
            if suggestions.len() > 5 {
                renderer.line(
                    MessageStyle::Info,
                    &format!("    ... and {} more", suggestions.len() - 5),
                )?;
            }
        }
    }

    Ok(())
}

fn render_plan_update(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    if let Some(error) = val.get("error") {
        renderer.line(MessageStyle::Info, "[plan] Update failed")?;
        render_plan_error(renderer, error)?;
        return Ok(());
    }

    let plan_value = match val.get("plan").cloned() {
        Some(value) => value,
        None => {
            renderer.line(MessageStyle::Error, "[plan] No plan data returned")?;
            return Ok(());
        }
    };

    let plan: TaskPlan =
        serde_json::from_value(plan_value).context("Plan tool returned malformed plan payload")?;

    let heading = val
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("Plan updated");

    renderer.line(MessageStyle::Info, &format!("[plan] {}", heading))?;

    if matches!(plan.summary.status, PlanCompletionState::Empty) {
        renderer.line(MessageStyle::Info, "  No tasks defined")?;
        return Ok(());
    }

    renderer.set_plan(&plan);

    render_plan_panel(renderer, &plan)?;
    Ok(())
}

fn render_mcp_context7_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    // Status is now rendered in the tool summary line, so we skip it here

    // Show query if present
    if let Some(meta) = val.get("meta").and_then(|value| value.as_object()) {
        if let Some(query) = meta.get("query").and_then(|value| value.as_str()) {
            renderer.line(MessageStyle::Info, &format!("  {}", shorten(query, 120)))?;
        }
    }

    // Show snippet count
    if let Some(messages) = val.get("messages").and_then(|value| value.as_array())
        && !messages.is_empty()
    {
        renderer.line(
            MessageStyle::Response,
            &format!("  {} snippets retrieved", messages.len()),
        )?;
    }

    // Show errors if any
    if let Some(errors) = val.get("errors").and_then(|value| value.as_array())
        && !errors.is_empty()
    {
        for err in errors.iter().take(1) {
            if let Some(msg) = err.get("message").and_then(|value| value.as_str()) {
                renderer.line(MessageStyle::Error, &format!("  {}", shorten(msg, 120)))?;
            }
        }
        if errors.len() > 1 {
            renderer.line(
                MessageStyle::Error,
                &format!("  … {} more errors", errors.len() - 1),
            )?;
        }
    }

    Ok(())
}

fn render_mcp_sequential_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    let summary = val
        .get("summary")
        .and_then(|value| value.as_str())
        .unwrap_or("Sequential reasoning summary unavailable");

    // Status is now rendered in the tool summary line, so we skip it here

    renderer.line(MessageStyle::Info, &format!("  {}", shorten(summary, 120)))?;

    // Show event count if present
    if let Some(events) = val.get("events").and_then(|value| value.as_array())
        && !events.is_empty()
    {
        renderer.line(
            MessageStyle::Response,
            &format!("  {} reasoning steps", events.len()),
        )?;
    }

    // Show errors if any
    if let Some(errors) = val.get("errors").and_then(|value| value.as_array())
        && !errors.is_empty()
    {
        for err in errors.iter().take(1) {
            if let Some(msg) = err.get("message").and_then(|value| value.as_str()) {
                renderer.line(MessageStyle::Error, &format!("  {}", shorten(msg, 120)))?;
            }
        }
        if errors.len() > 1 {
            renderer.line(
                MessageStyle::Error,
                &format!("  … {} more errors", errors.len() - 1),
            )?;
        }
    }

    Ok(())
}

fn render_generic_mcp_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    let mut block_lines: Vec<PanelContentLine> = Vec::new();

    if let Some(content) = val.get("content").and_then(|v| v.as_array()) {
        for (idx, item) in content.iter().enumerate() {
            let mut render_text_content = |text: &str| -> Result<()> {
                if text.trim().is_empty() {
                    return Ok(());
                }
                if let Ok(json_val) = serde_json::from_str::<Value>(text) {
                    if content.len() > 1 {
                        block_lines.push(PanelContentLine::new(
                            format!("  [content {}]", idx + 1),
                            MessageStyle::Info,
                        ));
                    }
                    collect_formatted_json_lines(&mut block_lines, &json_val)?;
                } else if text.contains("```") {
                    collect_text_with_code_blocks(&mut block_lines, text);
                } else {
                    for line in text.lines() {
                        block_lines.push(PanelContentLine::new(line, MessageStyle::Response));
                    }
                }
                Ok(())
            };

            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                render_text_content(text)?;
            } else if let Some(text) = item.get("type").and_then(|t| {
                if t.as_str() == Some("text") {
                    item.get("text").and_then(|v| v.as_str())
                } else {
                    None
                }
            }) {
                render_text_content(text)?;
            } else if item.get("type").and_then(|t| t.as_str()) == Some("image") {
                block_lines.push(PanelContentLine::new(
                    "  [image content]",
                    MessageStyle::Info,
                ));
                if let Some(mime) = item.get("mimeType").and_then(|v| v.as_str()) {
                    block_lines.push(PanelContentLine::new(
                        format!("    type: {}", mime),
                        MessageStyle::Info,
                    ));
                }
            } else if item.get("type").and_then(|t| t.as_str()) == Some("resource") {
                if let Some(uri) = item.get("uri").and_then(|v| v.as_str()) {
                    block_lines.push(PanelContentLine::new(
                        format!("  [resource: {}]", uri),
                        MessageStyle::Info,
                    ));
                }
            }
        }
    }

    if let Some(meta) = val.get("meta").and_then(|v| v.as_object()) {
        if !meta.is_empty() {
            if !block_lines.is_empty() {
                block_lines.push(PanelContentLine::new(String::new(), MessageStyle::Info));
            }
            for (key, value) in meta {
                if let Some(text) = value.as_str() {
                    block_lines.push(PanelContentLine::new(
                        format!("  {}: {}", key, shorten(text, 100)),
                        MessageStyle::Info,
                    ));
                } else if let Some(num) = value.as_u64() {
                    block_lines.push(PanelContentLine::new(
                        format!("  {}: {}", key, num),
                        MessageStyle::Info,
                    ));
                }
            }
        }
    }

    if block_lines.is_empty() {
        return Ok(());
    }

    render_left_border_panel(renderer, block_lines)
}

fn collect_text_with_code_blocks(lines: &mut Vec<PanelContentLine>, text: &str) {
    let mut in_code_block = false;

    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if in_code_block {
                in_code_block = false;
            } else {
                in_code_block = true;
                let lang = line.trim_start().trim_start_matches("```").trim();
                if !lang.is_empty() {
                    lines.push(PanelContentLine::new(
                        format!("  [{}]", lang),
                        MessageStyle::Info,
                    ));
                }
            }
        } else if in_code_block {
            lines.push(PanelContentLine::new(
                format!("  {}", line),
                MessageStyle::Response,
            ));
        } else {
            lines.push(PanelContentLine::new(line, MessageStyle::Response));
        }
    }
}

fn collect_formatted_json_lines(lines: &mut Vec<PanelContentLine>, json: &Value) -> Result<()> {
    const SKIP_FIELDS: &[&str] = &["model", "_meta", "isError"];

    match json {
        Value::Object(map) => {
            for (key, value) in map {
                if SKIP_FIELDS.contains(&key.as_str()) {
                    continue;
                }

                let entry = match value {
                    Value::String(s) => format!("  {}: {}", key, s),
                    Value::Number(n) => format!("  {}: {}", key, n),
                    Value::Bool(b) => format!("  {}: {}", key, b),
                    Value::Null => format!("  {}: null", key),
                    Value::Array(arr) => format!("  {}: [] ({} items)", key, arr.len()),
                    Value::Object(_) => format!("  {}: {{...}}", key),
                };
                lines.push(PanelContentLine::new(entry, MessageStyle::Response));
            }
        }
        Value::Array(arr) => {
            for (idx, item) in arr.iter().enumerate() {
                lines.push(PanelContentLine::new(
                    format!("  [{}]: {}", idx, serde_json::to_string(item)?),
                    MessageStyle::Response,
                ));
            }
        }
        Value::String(s) => {
            lines.push(PanelContentLine::new(s, MessageStyle::Response));
        }
        _ => {
            lines.push(PanelContentLine::new(
                json.to_string(),
                MessageStyle::Response,
            ));
        }
    }
    Ok(())
}

fn shorten(text: &str, max_len: usize) -> String {
    const ELLIPSIS: &str = "…";
    if text.chars().count() <= max_len {
        return text.to_string();
    }

    let mut result = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx + ELLIPSIS.len() >= max_len {
            result.push_str(ELLIPSIS);
            break;
        }
        result.push(ch);
    }
    result
}

fn resolve_mcp_renderer_profile(
    tool_name: &str,
    vt_config: Option<&VTCodeConfig>,
) -> Option<McpRendererProfile> {
    let config = vt_config?;
    config.mcp.ui.renderer_for_tool(tool_name)
}

fn render_plan_panel(renderer: &mut AnsiRenderer, plan: &TaskPlan) -> Result<()> {
    const PANEL_WIDTH: u16 = 100;
    let content_width = PANEL_WIDTH.saturating_sub(4) as usize;

    let mut lines = Vec::new();
    let progress = format!(
        "  Progress: {}/{} completed",
        plan.summary.completed_steps, plan.summary.total_steps
    );
    lines.push(PanelContentLine::new(
        clamp_panel_text(&progress, content_width),
        MessageStyle::Info,
    ));

    let explanation_line = plan
        .explanation
        .as_ref()
        .and_then(|text| text.lines().next())
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| clamp_panel_text(line, content_width));

    if explanation_line.is_some() || !plan.steps.is_empty() {
        lines.push(PanelContentLine::new(String::new(), MessageStyle::Info));
    }

    if let Some(line) = explanation_line {
        lines.push(PanelContentLine::new(line, MessageStyle::Info));
        if !plan.steps.is_empty() {
            lines.push(PanelContentLine::new(String::new(), MessageStyle::Info));
        }
    }

    for (index, step) in plan.steps.iter().enumerate() {
        let (checkbox, _style) = match step.status {
            StepStatus::Pending => ("[ ]", MessageStyle::Info),
            StepStatus::InProgress => ("[>]", MessageStyle::Tool),
            StepStatus::Completed => ("[x]", MessageStyle::Response),
        };
        let step_text = step.step.trim();
        let step_number = index + 1;

        // Calculate prefix length (e.g., "1. [x] ")
        let prefix = format!("{step_number}. {checkbox} ");
        let prefix_len = prefix.chars().count();

        if prefix_len >= content_width {
            // If prefix is too long, just truncate the whole thing
            let content = format!("{step_number}. {checkbox} {step_text}");
            let truncated = clamp_panel_text(&content, content_width);
            lines.push(PanelContentLine::new(truncated, MessageStyle::Info));
        } else {
            // Wrap the step text to multiple lines
            let available_width = content_width - prefix_len;
            let wrapped_lines = wrap_text(step_text, available_width);

            for (line_idx, line) in wrapped_lines.into_iter().enumerate() {
                let content = if line_idx == 0 {
                    format!("{prefix}{line}")
                } else {
                    format!("{}{}", " ".repeat(prefix_len), line)
                };
                lines.push(PanelContentLine::new(content, MessageStyle::Info));
            }
        }
    }

    render_panel(renderer, None, lines, MessageStyle::Info)
}

fn render_plan_error(renderer: &mut AnsiRenderer, error: &Value) -> Result<()> {
    let error_message = error
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("Plan update failed due to an unknown error.");
    let error_type = error
        .get("error_type")
        .and_then(|value| value.as_str())
        .unwrap_or("Unknown");

    renderer.line(
        MessageStyle::Error,
        &format!("  {} ({})", error_message, error_type),
    )?;

    if let Some(original_error) = error
        .get("original_error")
        .and_then(|value| value.as_str())
        .filter(|message| !message.is_empty())
    {
        renderer.line(
            MessageStyle::Info,
            &format!("  Details: {}", original_error),
        )?;
    }

    if let Some(suggestions) = error
        .get("recovery_suggestions")
        .and_then(|value| value.as_array())
    {
        let tips: Vec<_> = suggestions
            .iter()
            .filter_map(|suggestion| suggestion.as_str())
            .collect();
        if !tips.is_empty() {
            renderer.line(MessageStyle::Info, "  Recovery suggestions:")?;
            for tip in tips {
                renderer.line(MessageStyle::Info, &format!("    - {}", tip))?;
            }
        }
    }

    Ok(())
}

/// Resolves the tail limit for tool output from config.
/// Prefers ui.tool_output_max_lines, falls back to pty.stdout_tail_lines for backward compatibility.
fn resolve_stdout_tail_limit(config: Option<&VTCodeConfig>) -> usize {
    config
        .map(|cfg| {
            // Prefer the new unified tool_output_max_lines setting
            if cfg.ui.tool_output_max_lines > 0 {
                cfg.ui.tool_output_max_lines
            } else {
                // Fall back to PTY-specific setting for backward compatibility
                cfg.pty.stdout_tail_lines
            }
        })
        .filter(|&lines| lines > 0)
        .unwrap_or(defaults::DEFAULT_PTY_STDOUT_TAIL_LINES)
}

/// Spools oversized tool output to disk and returns the log path.
/// Returns None if spooling is disabled or the content is below the threshold.
fn spool_output_if_needed(
    content: &str,
    tool_name: &str,
    config: Option<&VTCodeConfig>,
) -> Result<Option<PathBuf>> {
    let threshold = config
        .map(|cfg| cfg.ui.tool_output_spool_bytes)
        .unwrap_or(200_000);

    if content.len() < threshold {
        return Ok(None);
    }

    // Determine spool directory
    let spool_dir = config
        .and_then(|cfg| cfg.ui.tool_output_spool_dir.as_deref())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".vtcode/tool-output"));

    // Create directory if it doesn't exist
    fs::create_dir_all(&spool_dir)
        .with_context(|| format!("Failed to create spool directory: {}", spool_dir.display()))?;

    // Generate unique filename with timestamp
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let filename = format!("{}-{}.log", tool_name.replace('/', "-"), timestamp);
    let log_path = spool_dir.join(filename);

    // Write content to file
    let mut file = fs::File::create(&log_path)
        .with_context(|| format!("Failed to create spool file: {}", log_path.display()))?;
    file.write_all(content.as_bytes())
        .with_context(|| format!("Failed to write to spool file: {}", log_path.display()))?;

    Ok(Some(log_path))
}

/// Streaming tail iterator that extracts the last N lines without buffering all lines.
/// Uses SmallVec for stack allocation when tail is small (≤32 lines).
/// Optimized to use a single pass with modulo indexing instead of VecDeque.
#[cfg_attr(
    feature = "profiling",
    tracing::instrument(skip(text), level = "trace")
)]
fn tail_lines_streaming<'a>(text: &'a str, limit: usize) -> (SmallVec<[&'a str; 32]>, usize) {
    if text.is_empty() {
        return (SmallVec::new(), 0);
    }
    if limit == 0 {
        return (SmallVec::new(), text.lines().count());
    }

    // Use a fixed-size buffer with modulo indexing to avoid VecDeque overhead
    let mut buffer: SmallVec<[&'a str; 32]> = SmallVec::with_capacity(limit);
    let mut total = 0usize;
    let mut write_idx = 0usize;

    for line in text.lines() {
        if buffer.len() < limit {
            buffer.push(line);
        } else {
            // Circular buffer: overwrite oldest entry
            buffer[write_idx] = line;
            write_idx = (write_idx + 1) % limit;
        }
        total += 1;
    }

    // If we wrapped around, rotate to get correct order
    if total > limit {
        buffer.rotate_left(write_idx);
    }

    (buffer, total)
}

/// Streaming line selection that avoids buffering all lines when possible.
/// Returns SmallVec for efficient stack allocation on small outputs.
/// In Full mode, still uses tail_limit as a safety cap to prevent unbounded memory growth.
#[cfg_attr(
    feature = "profiling",
    tracing::instrument(skip(content), level = "trace")
)]
fn select_stream_lines_streaming<'a>(
    content: &'a str,
    mode: ToolOutputMode,
    tail_limit: usize,
    prefer_full: bool,
) -> (SmallVec<[&'a str; 32]>, usize, bool) {
    if content.is_empty() {
        return (SmallVec::new(), 0, false);
    }

    // Even in Full mode, use tail_limit as a safety cap to prevent unbounded memory
    // The caller (render_stream_section) will further cap at INLINE_STREAM_MAX_LINES if needed
    let effective_limit = if prefer_full || matches!(mode, ToolOutputMode::Full) {
        tail_limit.max(1000) // Use at least 1000 lines in full mode, but respect higher limits
    } else {
        tail_limit
    };

    let (tail, total) = tail_lines_streaming(content, effective_limit);
    let truncated = total > tail.len();
    (tail, total, truncated)
}

/// Legacy wrapper for backward compatibility (used in tests).
#[inline]
#[cfg(test)]
fn select_stream_lines(
    content: &str,
    mode: ToolOutputMode,
    tail_limit: usize,
    prefer_full: bool,
) -> (Vec<&str>, usize, bool) {
    let (lines, total, truncated) =
        select_stream_lines_streaming(content, mode, tail_limit, prefer_full);
    (lines.into_vec(), total, truncated)
}

fn render_write_file_preview(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    git_styles: &GitStyles,
    _ls_styles: &LsStyles,
) -> Result<()> {
    // Show path with bullet point
    let path = payload.get("path").and_then(|v| v.as_str());
    if let Some(p) = path {
        renderer.line(MessageStyle::Info, &format!("● Path: {}", p))?;
    }

    // Show encoding if specified
    if let Some(encoding) = payload.get("encoding").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Info, &format!("  encoding: {}", encoding))?;
    }

    // Show file creation status
    if payload
        .get("created")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        renderer.line(MessageStyle::Response, "  ✓ File created")?;
    }

    let diff_value = match payload.get("diff_preview") {
        Some(value) => value,
        None => return Ok(()),
    };

    if diff_value
        .get("skipped")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        let reason = diff_value
            .get("reason")
            .and_then(|value| value.as_str())
            .unwrap_or("diff preview skipped");
        renderer.line(
            MessageStyle::Info,
            &format!("  diff preview skipped: {reason}"),
        )?;

        if let Some(detail) = diff_value.get("detail").and_then(|value| value.as_str()) {
            renderer.line(MessageStyle::Info, &format!("  detail: {detail}"))?;
        }

        if let Some(max_bytes) = diff_value.get("max_bytes").and_then(|value| value.as_u64()) {
            renderer.line(
                MessageStyle::Info,
                &format!("  preview limit: {max_bytes} bytes"),
            )?;
        }
        return Ok(());
    }

    let diff_content = diff_value
        .get("content")
        .and_then(|value| value.as_str())
        .unwrap_or("");

    if diff_content.is_empty()
        && diff_value
            .get("is_empty")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    {
        renderer.line(MessageStyle::Info, "  No changes")?;
        return Ok(());
    }

    if !diff_content.is_empty() {
        renderer.line(MessageStyle::Info, "")?;
        render_unified_diff(renderer, diff_content, git_styles, None)?;
    }

    if diff_value
        .get("truncated")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        if let Some(omitted) = diff_value
            .get("omitted_line_count")
            .and_then(|value| value.as_u64())
        {
            renderer.line(
                MessageStyle::Info,
                &format!("  ... ({} lines omitted)", omitted),
            )?;
        } else {
            renderer.line(MessageStyle::Info, "  ...")?;
        }
    }

    Ok(())
}

#[cfg_attr(
    feature = "profiling",
    tracing::instrument(
        skip(renderer, content, git_styles, ls_styles, config),
        level = "debug"
    )
)]
fn render_stream_section(
    renderer: &mut AnsiRenderer,
    title: &str,
    content: &str,
    mode: ToolOutputMode,
    tail_limit: usize,
    tool_name: Option<&str>,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
    fallback_style: MessageStyle,
    allow_ansi: bool,
    config: Option<&VTCodeConfig>,
    transcript_tail: Option<usize>,
) -> Result<()> {
    let is_mcp_tool = tool_name.map_or(false, |name| name.starts_with("mcp_"));
    let is_git_diff = matches!(tool_name, Some(tools::GIT_DIFF));
    let is_run_command = matches!(tool_name, Some(tools::RUN_COMMAND));
    let force_tail_mode = is_run_command;
    let normalized_content = if allow_ansi {
        Cow::Borrowed(content)
    } else {
        strip_ansi_codes(content)
    };
    let prefix: &'static str = if is_mcp_tool || is_git_diff { "" } else { "  " };

    struct TranscriptTailSnapshot {
        limit: usize,
        total: usize,
        omitted: usize,
        lines: Vec<String>,
        prefix: &'static str,
    }

    let tail_snapshot = transcript_tail.map(|limit| {
        let (tail, total) = tail_lines_streaming(normalized_content.as_ref(), limit);
        let tail_len = tail.len();
        let lines = tail
            .into_iter()
            .map(|line| {
                if line.is_empty() {
                    String::new()
                } else if prefix.is_empty() {
                    line.to_string()
                } else {
                    let mut prefixed = String::with_capacity(prefix.len() + line.len());
                    prefixed.push_str(prefix);
                    prefixed.push_str(line);
                    prefixed
                }
            })
            .collect::<Vec<_>>();
        TranscriptTailSnapshot {
            limit,
            total,
            omitted: total.saturating_sub(tail_len),
            lines,
            prefix,
        }
    });

    let mut render_body = |spool_note: &mut Option<String>| -> Result<()> {
        if let Some(tool) = tool_name {
            if let Ok(Some(log_path)) =
                spool_output_if_needed(normalized_content.as_ref(), tool, config)
            {
                use std::fmt::Write as _;
                let (tail, total) = tail_lines_streaming(normalized_content.as_ref(), 20);

                *spool_note = Some(log_path.display().to_string());

                let mut msg_buffer = String::with_capacity(256);
                if !is_run_command {
                    let uppercase_title = if title.is_empty() {
                        Cow::Borrowed("OUTPUT")
                    } else {
                        Cow::Owned(title.to_ascii_uppercase())
                    };
                    let _ = write!(
                        &mut msg_buffer,
                        "[{}] Output too large ({} bytes, {} lines), spooled to: {}",
                        uppercase_title.as_ref(),
                        content.len(),
                        total,
                        log_path.display()
                    );
                } else {
                    let _ = write!(
                        &mut msg_buffer,
                        "Command output too large ({} bytes, {} lines), spooled to: {}",
                        content.len(),
                        total,
                        log_path.display()
                    );
                }
                renderer.line(MessageStyle::Info, &msg_buffer)?;
                renderer.line(MessageStyle::Info, "Last 20 lines:")?;

                msg_buffer.clear();
                msg_buffer.reserve(128);

                let hidden = total.saturating_sub(tail.len());
                if hidden > 0 {
                    msg_buffer.clear();
                    msg_buffer.push_str(prefix);
                    msg_buffer.push_str("[... ");
                    msg_buffer.push_str(&hidden.to_string());
                    msg_buffer.push_str(" line");
                    if hidden != 1 {
                        msg_buffer.push('s');
                    }
                    msg_buffer.push_str(" truncated ...]");
                    renderer.line(MessageStyle::Info, &msg_buffer)?;
                }

                for line in &tail {
                    if line.is_empty() {
                        msg_buffer.clear();
                    } else {
                        msg_buffer.clear();
                        msg_buffer.push_str(prefix);
                        msg_buffer.push_str(line);
                    }
                    if !is_git_diff {
                        if let Some(style) =
                            select_line_style(tool_name, line, git_styles, ls_styles)
                        {
                            renderer.line_with_style(style, &msg_buffer)?;
                            continue;
                        }
                    }
                    renderer.line(fallback_style, &msg_buffer)?;
                }
                return Ok(());
            }
        }

        let (lines_vec, total, truncated) = if force_tail_mode {
            let (tail, total) = tail_lines_streaming(normalized_content.as_ref(), tail_limit);
            let truncated = total > tail.len();
            (tail, total, truncated)
        } else {
            let prefer_full = renderer.prefers_untruncated_output();
            let (mut lines, total, mut truncated) = select_stream_lines_streaming(
                normalized_content.as_ref(),
                mode,
                tail_limit,
                prefer_full,
            );
            if prefer_full && lines.len() > INLINE_STREAM_MAX_LINES {
                let drop = lines.len() - INLINE_STREAM_MAX_LINES;
                lines.drain(..drop);
                truncated = true;
            }
            (lines, total, truncated)
        };

        if lines_vec.is_empty() {
            return Ok(());
        }

        let mut format_buffer = String::with_capacity(64);

        let hidden = if truncated {
            total.saturating_sub(lines_vec.len())
        } else {
            0
        };
        if hidden > 0 {
            format_buffer.clear();
            format_buffer.push_str(prefix);
            format_buffer.push_str("[... ");
            format_buffer.push_str(&hidden.to_string());
            format_buffer.push_str(" line");
            if hidden != 1 {
                format_buffer.push('s');
            }
            format_buffer.push_str(" truncated ...]");
            renderer.line(MessageStyle::Info, &format_buffer)?;
        }

        if !is_mcp_tool && !is_git_diff && !is_run_command && !title.is_empty() {
            format_buffer.clear();
            format_buffer.push('[');
            for ch in title.chars() {
                format_buffer.push(ch.to_ascii_uppercase());
            }
            format_buffer.push(']');
            renderer.line(MessageStyle::Info, &format_buffer)?;
        }

        let mut display_buffer = String::with_capacity(128);

        for line in &lines_vec {
            if line.is_empty() {
                display_buffer.clear();
            } else {
                display_buffer.clear();
                display_buffer.push_str(prefix);
                display_buffer.push_str(line);
            }

            if !is_git_diff {
                if let Some(style) = select_line_style(tool_name, line, git_styles, ls_styles) {
                    renderer.line_with_style(style, &display_buffer)?;
                    continue;
                }
            }
            renderer.line(fallback_style, &display_buffer)?;
        }

        Ok(())
    };

    let mut spool_note: Option<String> = None;
    let render_result = if transcript_tail.is_some() {
        transcript::with_suppressed(|| render_body(&mut spool_note))
    } else {
        render_body(&mut spool_note)
    };
    render_result?;

    if let Some(snapshot) = tail_snapshot {
        if snapshot.total > 0 || !snapshot.lines.is_empty() || spool_note.is_some() {
            let mut header = String::new();
            if !snapshot.prefix.is_empty() {
                header.push_str(snapshot.prefix);
            }
            header.push_str(&format!("[stdout tail -{}]", snapshot.limit));
            if let Some(path) = &spool_note {
                header.push_str(&format!(" spooled to {}", path));
            }
            if snapshot.total > snapshot.lines.len() {
                header.push_str(&format!(
                    " showing last {} of {} lines ({} omitted); full output preserved for agent",
                    snapshot.lines.len(),
                    snapshot.total,
                    snapshot.omitted
                ));
            } else if snapshot.total > 0 {
                header.push_str(&format!(
                    " showing {} line{}; full output preserved for agent",
                    snapshot.lines.len(),
                    if snapshot.lines.len() == 1 { "" } else { "s" }
                ));
            } else {
                header.push_str(" showing no stdout lines; full output preserved for agent");
            }
            transcript::append(&header);
            for line in snapshot.lines {
                transcript::append(&line);
            }
        }
    }

    Ok(())
}

fn describe_code_fence_header(language: Option<&str>) -> String {
    let Some(lang) = language
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return "Code block".to_string();
    };

    let lower = lang.to_ascii_lowercase();
    match lower.as_str() {
        "sh" | "bash" | "zsh" | "shell" | "pwsh" | "powershell" | "cmd" | "batch" | "bat" => {
            format!("Shell ({lower})")
        }
        _ => {
            let mut chars = lower.chars();
            let Some(first) = chars.next() else {
                return "Code block".to_string();
            };
            let mut label = first.to_uppercase().collect::<String>();
            label.extend(chars);
            format!("{label} block")
        }
    }
}

pub(crate) fn render_code_fence_blocks(
    renderer: &mut AnsiRenderer,
    blocks: &[CodeFenceBlock],
) -> Result<()> {
    const MAX_CONTENT_WIDTH: usize = 96;
    let content_limit = MAX_CONTENT_WIDTH.saturating_sub(4);
    for (index, block) in blocks.iter().enumerate() {
        let header = describe_code_fence_header(block.language.as_deref());

        let mut lines = Vec::new();

        if block.lines.is_empty() {
            lines.push(PanelContentLine::new(
                clamp_panel_text("    (no content)", content_limit),
                MessageStyle::Info,
            ));
        } else {
            lines.push(PanelContentLine::new(String::new(), MessageStyle::Response));
            for line in &block.lines {
                let display = format!("    {}", line);
                lines.push(PanelContentLine::new(
                    clamp_panel_text(&display, content_limit),
                    MessageStyle::Response,
                ));
            }
        }

        render_panel(
            renderer,
            Some(clamp_panel_text(&header, content_limit)),
            lines,
            MessageStyle::Response,
        )?;

        if index + 1 < blocks.len() {
            renderer.line(MessageStyle::Response, "")?;
        }
    }

    Ok(())
}

fn render_terminal_command_panel(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
    vt_config: Option<&VTCodeConfig>,
    allow_ansi: bool,
) -> Result<()> {
    // Status is now rendered in the tool summary line, so we skip it here
    // Command and exit code are already displayed in the tool call summary,
    // so we skip rendering them again to avoid duplication

    let stdout_raw = payload.get("stdout").and_then(Value::as_str).unwrap_or("");
    let stderr_raw = payload.get("stderr").and_then(Value::as_str).unwrap_or("");
    let command_tokens = parse_command_tokens(payload);
    let stdout = preprocess_terminal_stdout(command_tokens.as_deref(), stdout_raw);
    let stderr = preprocess_terminal_stdout(command_tokens.as_deref(), stderr_raw);

    let output_mode = vt_config
        .map(|cfg| cfg.ui.tool_output_mode)
        .unwrap_or(ToolOutputMode::Compact);
    let tail_limit = resolve_stdout_tail_limit(vt_config);

    if stdout.trim().is_empty() && stderr.trim().is_empty() {
        renderer.line(MessageStyle::Info, "(no output)")?;
        return Ok(());
    }

    if !stdout.trim().is_empty() {
        render_stream_section(
            renderer,
            "",
            &stdout,
            output_mode,
            tail_limit,
            Some(tools::RUN_COMMAND),
            git_styles,
            ls_styles,
            MessageStyle::Response,
            allow_ansi,
            vt_config,
            Some(5),
        )?;
    }
    if !stderr.trim().is_empty() {
        render_stream_section(
            renderer,
            "stderr",
            &stderr,
            output_mode,
            tail_limit,
            Some(tools::RUN_COMMAND),
            git_styles,
            ls_styles,
            MessageStyle::Error,
            allow_ansi,
            vt_config,
            Some(5),
        )?;
    }

    Ok(())
}

fn parse_command_tokens(payload: &Value) -> Option<Vec<String>> {
    if let Some(array) = payload.get("command").and_then(Value::as_array) {
        let mut tokens = Vec::new();
        for value in array {
            if let Some(segment) = value.as_str() {
                if !segment.is_empty() {
                    tokens.push(segment.to_string());
                }
            }
        }
        if !tokens.is_empty() {
            return Some(tokens);
        }
    }

    if let Some(command_str) = payload.get("command").and_then(Value::as_str) {
        if command_str.trim().is_empty() {
            return None;
        }
        if let Ok(segments) = shell_split(command_str) {
            if !segments.is_empty() {
                return Some(segments);
            }
        }
    }
    None
}

fn normalized_command_name(tokens: &[String]) -> Option<String> {
    tokens
        .first()
        .and_then(|cmd| Path::new(cmd).file_name())
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase())
}

fn command_is_multicol_listing(tokens: &[String]) -> bool {
    normalized_command_name(tokens)
        .map(|name| {
            matches!(
                name.as_str(),
                "ls" | "dir" | "vdir" | "gls" | "colorls" | "exa" | "eza"
            )
        })
        .unwrap_or(false)
}

fn listing_has_single_column_flag(tokens: &[String]) -> bool {
    tokens.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "-1" | "--format=single-column"
                | "--long"
                | "-l"
                | "--tree"
                | "--grid=never"
                | "--no-grid"
        )
    })
}

fn preprocess_terminal_stdout(tokens: Option<&[String]>, stdout: &str) -> String {
    if stdout.trim().is_empty() {
        return String::new();
    }

    // Always strip ANSI codes to remove terminal control sequences (progress bars, cursor movements)
    let with_normalized_cr = normalize_carriage_returns(stdout);
    let normalized = strip_ansi_codes(with_normalized_cr.as_ref()).into_owned();

    let should_strip_numbers = tokens
        .map(command_can_emit_rust_diagnostics)
        .unwrap_or(false)
        && looks_like_rust_diagnostic(&normalized);

    if should_strip_numbers {
        return strip_rust_diagnostic_columns(Cow::Borrowed(&normalized)).into_owned();
    }

    if let Some(parts) = tokens {
        if command_is_multicol_listing(parts) && !listing_has_single_column_flag(parts) {
            let mut rows = String::with_capacity(normalized.len());
            for entry in normalized.split_whitespace() {
                if !entry.is_empty() {
                    rows.push_str(entry);
                    rows.push('\n');
                }
            }
            return rows;
        }
    }

    normalized
}

fn command_can_emit_rust_diagnostics(tokens: &[String]) -> bool {
    tokens
        .first()
        .map(|cmd| matches!(cmd.as_str(), "cargo" | "rustc"))
        .unwrap_or(false)
}

fn looks_like_rust_diagnostic(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }

    let mut snippet_lines = 0usize;
    let mut pointer_lines = 0usize;
    let mut has_location_marker = false;

    for line in text.lines().take(200) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("--> ") {
            has_location_marker = true;
        }
        if trimmed.starts_with('|') {
            pointer_lines += 1;
        }
        if let Some((prefix, _)) = trimmed.split_once('|') {
            let prefix_trimmed = prefix.trim();
            if !prefix_trimmed.is_empty() && prefix_trimmed.chars().all(|ch| ch.is_ascii_digit()) {
                snippet_lines += 1;
            }
        }
        if snippet_lines >= 1 && pointer_lines >= 1 {
            return true;
        }
        if snippet_lines >= 2 && has_location_marker {
            return true;
        }
    }

    false
}

fn strip_rust_diagnostic_columns<'a>(content: Cow<'a, str>) -> Cow<'a, str> {
    match content {
        Cow::Borrowed(text) => strip_rust_diagnostic_columns_from_str(text)
            .map(Cow::Owned)
            .unwrap_or_else(|| Cow::Borrowed(text)),
        Cow::Owned(text) => {
            if let Some(stripped) = strip_rust_diagnostic_columns_from_str(&text) {
                Cow::Owned(stripped)
            } else {
                Cow::Owned(text)
            }
        }
    }
}

fn strip_rust_diagnostic_columns_from_str(input: &str) -> Option<String> {
    if input.is_empty() {
        return None;
    }

    let mut output = String::with_capacity(input.len());
    let mut changed = false;

    for chunk in input.split_inclusive('\n') {
        let (line, had_newline) = chunk
            .strip_suffix('\n')
            .map(|line| (line, true))
            .unwrap_or((chunk, false));

        if let Some(prefix_end) = rust_diagnostic_prefix_end(line) {
            changed = true;
            output.push_str(&line[prefix_end..]);
        } else {
            output.push_str(line);
        }

        if had_newline {
            output.push('\n');
        }
    }

    if changed { Some(output) } else { None }
}

fn rust_diagnostic_prefix_end(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let len = bytes.len();

    let mut idx = 0usize;
    while idx < len && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    if idx >= len {
        return None;
    }

    if bytes[idx].is_ascii_digit() {
        let mut cursor = idx;
        while cursor < len && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor == idx {
            return None;
        }
        while cursor < len && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor < len && bytes[cursor] == b'|' {
            cursor += 1;
            if cursor < len && bytes[cursor] == b' ' {
                cursor += 1;
            }
            return Some(cursor);
        }
        return None;
    }

    if bytes[idx] == b'|' {
        let mut cursor = idx + 1;
        if cursor < len && bytes[cursor] == b' ' {
            cursor += 1;
        }
        return Some(cursor);
    }

    None
}

fn normalize_carriage_returns(input: &str) -> Cow<'_, str> {
    if !input.contains('\r') {
        return Cow::Borrowed(input);
    }

    let mut output = String::with_capacity(input.len());
    let mut current_line = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if matches!(chars.peek(), Some('\n')) {
                    chars.next();
                    output.push_str(&current_line);
                    output.push('\n');
                    current_line.clear();
                } else {
                    current_line.clear();
                }
            }
            '\n' => {
                output.push_str(&current_line);
                output.push('\n');
                current_line.clear();
            }
            _ => current_line.push(ch),
        }
    }

    if !current_line.is_empty() {
        output.push_str(&current_line);
    }

    Cow::Owned(output)
}

fn strip_ansi_codes(input: &str) -> Cow<'_, str> {
    if !input.contains('\x1b') {
        return Cow::Borrowed(input);
    }

    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                while let Some(next) = chars.next() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        output.push(ch);
    }
    Cow::Owned(output)
}

fn strip_diff_fences(input: &str) -> Cow<'_, str> {
    // Fast path: check first line without collecting
    let mut lines_iter = input.lines();
    let first_line = match lines_iter.next() {
        Some(line) if line.trim().starts_with("```") => line,
        _ => return Cow::Borrowed(input),
    };

    // Count lines efficiently
    let line_count = 1 + lines_iter.clone().count();
    if line_count < 2 {
        return Cow::Borrowed(input);
    }

    // Check last line
    let last_line = lines_iter.clone().last();
    let has_closing_fence = last_line.is_some_and(|line| line.trim() == "```");

    if !has_closing_fence {
        // Only strip first line
        let first_len = first_line.len();
        let remainder_start = if input.as_bytes().get(first_len) == Some(&b'\n') {
            first_len + 1
        } else {
            first_len
        };
        return Cow::Borrowed(&input[remainder_start..]);
    }

    // Strip both first and last - need to rebuild
    let mut result = String::with_capacity(input.len());
    let mut lines = input.lines();
    lines.next(); // Skip first

    let middle_lines: SmallVec<[&str; 32]> = lines.collect();
    if middle_lines.is_empty() {
        return Cow::Owned(String::new());
    }

    // Join all but last
    for (i, line) in middle_lines.iter().enumerate() {
        if i == middle_lines.len() - 1 {
            break; // Skip last line (closing fence)
        }
        if i > 0 {
            result.push('\n');
        }
        result.push_str(line);
    }

    Cow::Owned(result)
}

fn render_curl_result(
    renderer: &mut AnsiRenderer,
    val: &Value,
    mode: ToolOutputMode,
    tail_limit: usize,
    allow_ansi: bool,
    config: Option<&VTCodeConfig>,
) -> Result<()> {
    // Status is now rendered in the tool summary line, so we skip it here

    // Reuse buffer for formatting
    use std::fmt::Write as _;
    let mut msg_buffer = String::with_capacity(128);

    // Show HTTP status if available
    if let Some(status) = val.get("status").and_then(|v| v.as_u64()) {
        let status_style = if status >= 200 && status < 300 {
            MessageStyle::Response
        } else if status >= 400 {
            MessageStyle::Error
        } else {
            MessageStyle::Info
        };
        msg_buffer.clear();
        let _ = write!(&mut msg_buffer, "  HTTP {}", status);
        renderer.line(status_style, &msg_buffer)?;
    }

    // Show content type if available
    if let Some(content_type) = val.get("content_type").and_then(|v| v.as_str()) {
        msg_buffer.clear();
        let _ = write!(&mut msg_buffer, "  Content-Type: {}", content_type);
        renderer.line(MessageStyle::Info, &msg_buffer)?;
    }

    // Body output
    if let Some(body) = val.get("body").and_then(Value::as_str)
        && !body.trim().is_empty()
    {
        let normalized_body = if allow_ansi {
            Cow::Borrowed(body)
        } else {
            strip_ansi_codes(body)
        };

        // Check if we should spool to disk
        if let Ok(Some(log_path)) =
            spool_output_if_needed(normalized_body.as_ref(), tools::CURL, config)
        {
            let (tail, total) = tail_lines_streaming(normalized_body.as_ref(), 20);
            msg_buffer.clear();
            let _ = write!(
                &mut msg_buffer,
                "Response body too large ({} bytes, {} lines), spooled to: {}",
                body.len(),
                total,
                log_path.display()
            );
            renderer.line(MessageStyle::Info, &msg_buffer)?;
            renderer.line(MessageStyle::Info, "Last 20 lines:")?;
            for line in &tail {
                renderer.line(MessageStyle::Response, line.trim_end())?;
            }
            return Ok(());
        }

        let prefer_full = renderer.prefers_untruncated_output();
        let (lines, _total, _truncated) =
            select_stream_lines_streaming(normalized_body.as_ref(), mode, tail_limit, prefer_full);

        for line in &lines {
            renderer.line(MessageStyle::Response, line.trim_end())?;
        }
    } else {
        renderer.line(MessageStyle::Info, "(no response body)")?;
    }

    Ok(())
}

fn render_list_dir_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    // Show path being listed
    if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Info, &format!("  {}", path))?;
    }

    // Show pagination info
    if let Some(page) = val.get("page").and_then(|v| v.as_u64()) {
        if let Some(total) = val.get("total_items").and_then(|v| v.as_u64()) {
            renderer.line(
                MessageStyle::Info,
                &format!("  Page {} ({} items total)", page, total),
            )?;
        }
    }

    // Render items
    if let Some(items) = val.get("items").and_then(|v| v.as_array()) {
        if items.is_empty() {
            renderer.line(MessageStyle::Info, "  (empty directory)")?;
        } else {
            for item in items {
                if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("file");

                    let display_name = if item_type == "directory" {
                        format!("{}/", name)
                    } else {
                        name.to_string()
                    };

                    let display = format!("  {}", display_name);
                    renderer.line(MessageStyle::Response, &display)?;
                }
            }
        }
    }

    // Show "has more" indicator
    if val
        .get("has_more")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        renderer.line(MessageStyle::Info, "  ... more items available")?;
    }

    Ok(())
}

fn render_read_file_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    // Show encoding if specified
    if let Some(encoding) = val.get("encoding").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Info, &format!("  encoding: {}", encoding))?;
    }

    // Show file size
    if let Some(size) = val.get("size").and_then(|v| v.as_u64()) {
        renderer.line(
            MessageStyle::Info,
            &format!("  size: {}", format_size(size)),
        )?;
    }

    // Show line range if partial read
    if let Some(start) = val.get("start_line").and_then(|v| v.as_u64()) {
        if let Some(end) = val.get("end_line").and_then(|v| v.as_u64()) {
            renderer.line(MessageStyle::Info, &format!("  lines: {}-{}", start, end))?;
        }
    }

    // Content is typically shown via stdout, so we don't duplicate it here
    Ok(())
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

struct GitStyles {
    add: Option<AnsiStyle>,
    remove: Option<AnsiStyle>,
    header: Option<AnsiStyle>,
}

impl GitStyles {
    fn new() -> Self {
        Self {
            add: Some(AnsiStyle::new().fg_color(Some(AnsiColor::Green.into()))),
            remove: Some(AnsiStyle::new().fg_color(Some(AnsiColor::Red.into()))),
            header: Some(
                AnsiStyle::new()
                    .bold()
                    .fg_color(Some(AnsiColor::Yellow.into())),
            ),
        }
    }

    fn style_for_line(&self, kind: &DiffLineKind) -> Option<AnsiStyle> {
        match kind {
            DiffLineKind::Addition => self.add.clone(),
            DiffLineKind::Deletion => self.remove.clone(),
            DiffLineKind::Context => None,
        }
    }
}

struct LsStyles {
    classes: HashMap<String, AnsiStyle>,
    suffixes: Vec<(String, AnsiStyle)>,
}

impl LsStyles {
    fn from_env() -> Self {
        let mut classes: HashMap<String, AnsiStyle> = HashMap::new();
        let suffixes: Vec<(String, AnsiStyle)> = Vec::new();

        // For now, skip parsing LS_COLORS and just use defaults
        // TODO: Implement ANSI parsing if needed

        // Default styles
        classes.insert(
            "di".to_string(),
            AnsiStyle::new()
                .bold()
                .fg_color(Some(AnsiColor::Blue.into())),
        );
        classes.insert(
            "ln".to_string(),
            AnsiStyle::new()
                .bold()
                .fg_color(Some(AnsiColor::Cyan.into())),
        );
        classes.insert(
            "ex".to_string(),
            AnsiStyle::new()
                .bold()
                .fg_color(Some(AnsiColor::Green.into())),
        );
        classes.insert(
            "pi".to_string(),
            AnsiStyle::new().fg_color(Some(AnsiColor::Yellow.into())),
        );
        classes.insert(
            "so".to_string(),
            AnsiStyle::new()
                .bold()
                .fg_color(Some(AnsiColor::Magenta.into())),
        );
        classes.insert(
            "bd".to_string(),
            AnsiStyle::new()
                .bold()
                .fg_color(Some(AnsiColor::Yellow.into())),
        );
        classes.insert(
            "cd".to_string(),
            AnsiStyle::new()
                .bold()
                .fg_color(Some(AnsiColor::Yellow.into())),
        );

        LsStyles { classes, suffixes }
    }

    fn style_for_line(&self, line: &str) -> Option<AnsiStyle> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        let token = trimmed
            .split_whitespace()
            .last()
            .unwrap_or(trimmed)
            .trim_matches('"');

        let mut name = token;
        let mut class_hint: Option<&str> = None;

        if let Some(stripped) = name.strip_suffix('/') {
            name = stripped;
            class_hint = Some("di");
        } else if let Some(stripped) = name.strip_suffix('@') {
            name = stripped;
            class_hint = Some("ln");
        } else if let Some(stripped) = name.strip_suffix('*') {
            name = stripped;
            class_hint = Some("ex");
        } else if let Some(stripped) = name.strip_suffix('|') {
            name = stripped;
            class_hint = Some("pi");
        } else if let Some(stripped) = name.strip_suffix('=') {
            name = stripped;
            class_hint = Some("so");
        }

        if class_hint.is_none() {
            match trimmed.chars().next() {
                Some('d') => class_hint = Some("di"),
                Some('l') => class_hint = Some("ln"),
                Some('p') => class_hint = Some("pi"),
                Some('s') => class_hint = Some("so"),
                Some('b') => class_hint = Some("bd"),
                Some('c') => class_hint = Some("cd"),
                _ => {}
            }
        }

        if let Some(code) = class_hint {
            if let Some(style) = self.classes.get(code) {
                return Some(*style);
            }
        }

        let lower = name
            .trim_matches(|c| matches!(c, '"' | ',' | ' ' | '\u{0009}'))
            .to_ascii_lowercase();
        for (suffix, style) in &self.suffixes {
            if lower.ends_with(suffix) {
                return Some(*style);
            }
        }

        if lower.ends_with('*') {
            if let Some(style) = self.classes.get("ex") {
                return Some(*style);
            }
        }

        None
    }

    #[cfg(test)]
    fn from_components(
        classes: HashMap<String, AnsiStyle>,
        suffixes: Vec<(String, AnsiStyle)>,
    ) -> Self {
        Self { classes, suffixes }
    }
}

fn select_line_style(
    tool_name: Option<&str>,
    line: &str,
    git: &GitStyles,
    ls: &LsStyles,
) -> Option<AnsiStyle> {
    match tool_name {
        Some(name)
            if matches!(
                name,
                tools::RUN_COMMAND | tools::WRITE_FILE | tools::EDIT_FILE | tools::APPLY_PATCH
            ) =>
        {
            let trimmed = line.trim_start();
            if trimmed.starts_with("diff --")
                || trimmed.starts_with("index ")
                || trimmed.starts_with("@@")
            {
                return git.header;
            }

            if trimmed.starts_with('+') {
                return git.add;
            }
            if trimmed.starts_with('-') {
                return git.remove;
            }
            if name != tools::RUN_COMMAND {
                if let Some(style) = ls.style_for_line(trimmed) {
                    return Some(style);
                }
            }
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn test_wrap_text() {
        // Test basic wrapping
        let result = wrap_text("Hello world this is a long text", 10);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], "Hello");
        assert_eq!(result[1], "world this");
        assert_eq!(result[2], "is a long");
        assert_eq!(result[3], "text");

        // Test single word longer than width
        let result = wrap_text("supercalifragilisticexpialidocious", 10);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], "supercalif");
        assert_eq!(result[1], "ragilistic");
        assert_eq!(result[2], "expialidoc");

        // Test empty text
        let result = wrap_text("", 10);
        assert_eq!(result.len(), 0);

        // Test text shorter than width
        let result = wrap_text("Hello", 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "Hello");
    }

    #[test]
    fn preprocess_strips_rust_line_numbers_for_cargo_output() {
        let tokens = vec!["cargo".to_string(), "check".to_string()];
        let input = "\
warning: this is a warning
  --> src/main.rs:12:5
   |
12 |     let x = 5;
   |     ----- value defined here
   |
   = note: additional context
";
        let processed = preprocess_terminal_stdout(Some(&tokens), input);
        let output = processed.to_string();
        assert!(!output.contains("12 |"));
        assert!(output.contains("let x = 5;"));
        assert!(output.contains("----- value defined here"));
    }

    #[test]
    fn detects_rust_diagnostic_shape() {
        let sample = "\
warning: something
  --> src/lib.rs:7:9
   |
 7 |     println!(\"hi\");
   |     ^^^^^^^^^^^^^^^
";
        assert!(
            looks_like_rust_diagnostic(sample),
            "should detect diagnostic structure"
        );
    }

    #[test]
    fn rust_prefix_end_handles_pointer_lines() {
        let line = "   |         ^ expected struct `Foo`, found enum `Bar`";
        let idx = rust_diagnostic_prefix_end(line).expect("prefix");
        assert_eq!(
            &line[idx..],
            "        ^ expected struct `Foo`, found enum `Bar`"
        );
    }

    #[test]
    fn strip_rust_columns_returns_none_when_unmodified() {
        let sample = "no diagnostics here";
        assert!(strip_rust_diagnostic_columns_from_str(sample).is_none());
    }

    fn build_terminal_followup_message(
        command: &str,
        absorbed: bool,
        exit_code: Option<i32>,
    ) -> String {
        let command = command.replace('\n', " ");
        if absorbed {
            format!("Absorbed terminal output for `{}`.", command)
        } else {
            match exit_code {
                Some(code) => format!("Captured `{}` output (exit code {}).", command, code),
                None => format!("Captured `{}` output.", command),
            }
        }
    }

    #[test]
    fn describes_shell_code_fence_as_shell_header() {
        let header = describe_code_fence_header(Some("bash"));
        assert_eq!(header, "Shell (bash)");

        let rust_header = describe_code_fence_header(Some("rust"));
        assert_eq!(rust_header, "Rust block");

        let empty_header = describe_code_fence_header(None);
        assert_eq!(empty_header, "Code block");
    }

    #[test]
    fn detects_git_diff_styling() {
        let git = GitStyles::new();
        let ls = LsStyles::from_components(HashMap::new(), Vec::new());
        let added = select_line_style(Some("run_terminal_cmd"), "+added line", &git, &ls);
        assert_eq!(added, git.add);
        let removed = select_line_style(Some("run_terminal_cmd"), "-removed line", &git, &ls);
        assert_eq!(removed, git.remove);
        let header = select_line_style(
            Some("run_terminal_cmd"),
            "diff --git a/file b/file",
            &git,
            &ls,
        );
        assert_eq!(header, git.header);
    }

    #[test]
    fn detects_ls_styles_for_directories_and_executables() {
        use anstyle::AnsiColor;

        let git = GitStyles::new();
        let dir_style = AnsiStyle::new().bold();
        let exec_style =
            AnsiStyle::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green.into())));
        let mut classes = HashMap::new();
        classes.insert("di".to_string(), dir_style);
        classes.insert("ex".to_string(), exec_style);
        let ls = LsStyles::from_components(classes, Vec::new());
        let directory = select_line_style(Some("run_terminal_cmd"), "folder/", &git, &ls);
        assert_eq!(directory, Some(dir_style));
        let executable = select_line_style(Some("run_terminal_cmd"), "script*", &git, &ls);
        assert_eq!(executable, Some(exec_style));
    }

    #[test]
    fn non_terminal_tools_do_not_apply_special_styles() {
        let git = GitStyles::new();
        let ls = LsStyles::from_components(HashMap::new(), Vec::new());
        let styled = select_line_style(Some("context7"), "+added", &git, &ls);
        assert!(styled.is_none());
    }

    #[test]
    fn applies_extension_based_styles() {
        let git = GitStyles::new();
        let mut suffixes = Vec::new();
        suffixes.push((
            ".rs".to_string(),
            AnsiStyle::new().fg_color(Some(anstyle::AnsiColor::Red.into())),
        ));
        let ls = LsStyles::from_components(HashMap::new(), suffixes);
        let styled = select_line_style(Some("run_terminal_cmd"), "main.rs", &git, &ls);
        assert!(styled.is_some());
    }

    #[test]
    fn extension_matching_requires_dot_boundary() {
        let git = GitStyles::new();
        let mut suffixes = Vec::new();
        suffixes.push((
            ".rs".to_string(),
            AnsiStyle::new().fg_color(Some(anstyle::AnsiColor::Green.into())),
        ));
        let ls = LsStyles::from_components(HashMap::new(), suffixes);

        let without_extension = select_line_style(Some("run_terminal_cmd"), "helpers", &git, &ls);
        assert!(without_extension.is_none());

        let with_extension = select_line_style(Some("run_terminal_cmd"), "helpers.rs", &git, &ls);
        assert!(with_extension.is_some());
    }

    #[test]
    fn followup_message_references_command() {
        let message = build_terminal_followup_message("ls -a", true, None);
        assert_eq!(message, "Absorbed terminal output for `ls -a`.");
    }

    #[test]
    fn followup_message_includes_exit_code() {
        let message = build_terminal_followup_message("npm test", false, Some(1));
        assert_eq!(message, "Captured `npm test` output (exit code 1).");
    }

    #[test]
    fn followup_message_collapses_whitespace() {
        let message = build_terminal_followup_message("echo foo\nbar", true, None);
        assert!(message.contains("echo foo bar"));
        assert!(!message.contains('\n'));
    }

    #[test]
    fn compact_mode_truncates_when_not_inline() {
        let content = (1..=50)
            .map(|index| format!("line-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (lines, total, truncated) =
            select_stream_lines(&content, ToolOutputMode::Compact, 10, false);
        assert_eq!(total, 50);
        assert_eq!(lines.len(), 10);
        assert!(truncated);
        assert_eq!(lines.first().copied(), Some("line-41"));
    }

    #[test]
    fn inline_rendering_preserves_full_scrollback() {
        let content = (1..=30)
            .map(|index| format!("row-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (lines, total, truncated) =
            select_stream_lines(&content, ToolOutputMode::Compact, 5, true);
        assert_eq!(total, 30);
        assert_eq!(lines.len(), 30);
        assert!(!truncated);
        assert_eq!(lines.first().copied(), Some("row-1"));
        assert_eq!(lines.last().copied(), Some("row-30"));
    }

    // Tests removed - render_tool_status_header function no longer exists
}
