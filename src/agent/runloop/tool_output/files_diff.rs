const SMALL_CONTEXT_RADIUS: usize = 1;
const MAX_EDIT_PREVIEW_LINES: usize = 28;
const EDIT_PREVIEW_HEAD_LINES: usize = 18;
const EDIT_PREVIEW_TAIL_LINES: usize = 8;

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

pub(crate) fn format_diff_content_lines(diff_content: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    for line in diff_content.lines() {
        let rewritten = format_start_only_hunk_header(line).unwrap_or_else(|| line.to_string());
        lines.push(rewritten);
    }

    lines
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreviewLineKind {
    Context,
    Addition,
    Removal,
}

#[derive(Clone, Debug)]
struct PreviewLine {
    kind: PreviewLineKind,
    line_number: usize,
    text: String,
}

#[derive(Clone, Debug, Default)]
struct ParsedFileDiff {
    path: String,
    additions: usize,
    deletions: usize,
    hunks: Vec<Vec<PreviewLine>>,
}

fn parse_hunk_starts(line: &str) -> Option<(usize, usize)> {
    let trimmed = line.trim_end();
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
    Some((old_start, new_start))
}

fn parse_git_diff_path(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    if parts.next()? != "diff" || parts.next()? != "--git" {
        return None;
    }

    let _old_path = parts.next()?;
    let new_path = parts.next()?;
    Some(new_path.trim_start_matches("b/").to_string())
}

fn parse_marker_path(line: &str) -> Option<String> {
    let marker = line.trim_start();
    if !marker.starts_with("+++ ") {
        return None;
    }
    let path = marker
        .strip_prefix("+++ ")?
        .trim()
        .trim_start_matches("b/")
        .trim_start_matches("a/");
    if path == "/dev/null" || path.is_empty() {
        return None;
    }
    Some(path.to_string())
}

fn format_preview_line(line: &PreviewLine) -> String {
    match line.kind {
        PreviewLineKind::Context => format!("{:>5}  {}", line.line_number, line.text),
        PreviewLineKind::Addition => format!("+{:>4}  {}", line.line_number, line.text),
        PreviewLineKind::Removal => format!("-{:>4}  {}", line.line_number, line.text),
    }
}

fn condense_hunk_lines(lines: &[PreviewLine], context_radius: usize) -> Vec<String> {
    if lines.is_empty() {
        return Vec::new();
    }

    let mut keep = vec![false; lines.len()];
    for (idx, line) in lines.iter().enumerate() {
        if matches!(
            line.kind,
            PreviewLineKind::Addition | PreviewLineKind::Removal
        ) {
            let start = idx.saturating_sub(context_radius);
            let end = (idx + context_radius).min(lines.len().saturating_sub(1));
            for flag in keep.iter_mut().take(end + 1).skip(start) {
                *flag = true;
            }
        }
    }

    if !keep.iter().any(|k| *k) {
        return Vec::new();
    }

    let mut condensed = Vec::new();
    let mut last_kept: Option<usize> = None;
    for (idx, line) in lines.iter().enumerate() {
        if !keep[idx] {
            continue;
        }

        if let Some(previous) = last_kept
            && idx > previous + 1
        {
            condensed.push("⋮".to_string());
        }

        condensed.push(format_preview_line(line));
        last_kept = Some(idx);
    }

    condensed
}

fn truncate_edit_preview_lines(lines: Vec<String>) -> Vec<String> {
    if lines.len() <= MAX_EDIT_PREVIEW_LINES {
        return lines;
    }

    let head_count = EDIT_PREVIEW_HEAD_LINES.min(lines.len());
    let tail_count = EDIT_PREVIEW_TAIL_LINES.min(lines.len().saturating_sub(head_count));
    let omitted = lines.len().saturating_sub(head_count + tail_count);

    let mut output = Vec::with_capacity(head_count + tail_count + 1);
    output.extend(lines[..head_count].iter().cloned());
    if omitted > 0 {
        output.push(format!("⋮ +{} more lines", omitted));
    }
    if tail_count > 0 {
        output.extend(lines[lines.len() - tail_count..].iter().cloned());
    }
    output
}

fn parse_unified_diff_for_preview(diff_content: &str) -> Vec<ParsedFileDiff> {
    let mut files: Vec<ParsedFileDiff> = Vec::new();
    let mut current: Option<ParsedFileDiff> = None;
    let mut current_hunk: Vec<PreviewLine> = Vec::new();
    let mut old_line_no = 0usize;
    let mut new_line_no = 0usize;
    let mut in_hunk = false;

    let flush_hunk = |current_hunk: &mut Vec<PreviewLine>, current: &mut Option<ParsedFileDiff>| {
        if current_hunk.is_empty() {
            return;
        }
        if let Some(file) = current.as_mut() {
            file.hunks.push(std::mem::take(current_hunk));
        }
    };

    let flush_file = |files: &mut Vec<ParsedFileDiff>, current: &mut Option<ParsedFileDiff>| {
        if let Some(file) = current.take()
            && (!file.hunks.is_empty() || file.additions > 0 || file.deletions > 0)
        {
            files.push(file);
        }
    };

    for line in diff_content.lines() {
        if let Some(path) = parse_git_diff_path(line) {
            flush_hunk(&mut current_hunk, &mut current);
            flush_file(&mut files, &mut current);
            current = Some(ParsedFileDiff {
                path,
                ..ParsedFileDiff::default()
            });
            in_hunk = false;
            continue;
        }

        if let Some(path) = parse_marker_path(line) {
            if current.is_none() {
                current = Some(ParsedFileDiff {
                    path,
                    ..ParsedFileDiff::default()
                });
            } else if let Some(file) = current.as_mut()
                && file.path.is_empty()
            {
                file.path = path;
            }
        }

        if let Some((old_start, new_start)) = parse_hunk_starts(line) {
            if current.is_none() {
                current = Some(ParsedFileDiff {
                    path: "file".to_string(),
                    ..ParsedFileDiff::default()
                });
            }
            flush_hunk(&mut current_hunk, &mut current);
            in_hunk = true;
            old_line_no = old_start;
            new_line_no = new_start;
            continue;
        }

        if !in_hunk {
            continue;
        }

        if line.starts_with('+') && !line.starts_with("+++") {
            if let Some(file) = current.as_mut() {
                file.additions += 1;
            }
            current_hunk.push(PreviewLine {
                kind: PreviewLineKind::Addition,
                line_number: new_line_no,
                text: line[1..].to_string(),
            });
            new_line_no = new_line_no.saturating_add(1);
            continue;
        }

        if line.starts_with('-') && !line.starts_with("---") {
            if let Some(file) = current.as_mut() {
                file.deletions += 1;
            }
            current_hunk.push(PreviewLine {
                kind: PreviewLineKind::Removal,
                line_number: old_line_no,
                text: line[1..].to_string(),
            });
            old_line_no = old_line_no.saturating_add(1);
            continue;
        }

        if let Some(context_line) = line.strip_prefix(' ') {
            let line_number = new_line_no;
            current_hunk.push(PreviewLine {
                kind: PreviewLineKind::Context,
                line_number,
                text: context_line.to_string(),
            });
            old_line_no = old_line_no.saturating_add(1);
            new_line_no = new_line_no.saturating_add(1);
            continue;
        }
    }

    flush_hunk(&mut current_hunk, &mut current);
    flush_file(&mut files, &mut current);
    files
}

pub(crate) fn format_condensed_edit_diff_lines(diff_content: &str) -> Vec<String> {
    let parsed_files = parse_unified_diff_for_preview(diff_content);
    if parsed_files.is_empty() {
        return format_diff_content_lines(diff_content);
    }

    let mut output = Vec::new();
    for (file_index, file) in parsed_files.iter().enumerate() {
        if file_index > 0 {
            output.push(String::new());
        }

        let display_path = if file.path.is_empty() {
            "file"
        } else {
            file.path.as_str()
        };
        output.push(format!(
            "• Edited {} (+{} -{})",
            display_path, file.additions, file.deletions
        ));

        let mut file_lines = Vec::new();
        for (hunk_index, hunk) in file.hunks.iter().enumerate() {
            let condensed = condense_hunk_lines(hunk, SMALL_CONTEXT_RADIUS);
            if condensed.is_empty() {
                continue;
            }

            if hunk_index > 0 && !file_lines.is_empty() {
                file_lines.push("⋮".to_string());
            }
            file_lines.extend(condensed);
        }

        if file_lines.is_empty() {
            continue;
        }

        for line in truncate_edit_preview_lines(file_lines) {
            output.push(format!("    {}", line));
        }
    }

    output
}
