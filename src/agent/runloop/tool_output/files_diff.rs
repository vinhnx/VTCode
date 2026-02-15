use anstyle::{AnsiColor, Color, Reset, Style as AnsiStyle};

const DIFF_SUMMARY_PREFIX: &str = "• Diff ";

fn format_diff_summary_line(path: &str, additions: usize, deletions: usize) -> String {
    format!(
        "{DIFF_SUMMARY_PREFIX}{} (+{} -{})",
        path, additions, deletions
    )
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

pub(crate) fn colorize_diff_summary_line(line: &str, use_color: bool) -> Option<String> {
    let (path, additions, deletions) = parse_diff_summary_line(line)?;
    if !use_color {
        return Some(line.to_string());
    }
    let green = AnsiStyle::new().fg_color(Some(Color::Ansi(AnsiColor::Green)));
    let red = AnsiStyle::new().fg_color(Some(Color::Ansi(AnsiColor::Red)));
    let reset = format!("{}", Reset.render());
    Some(format!(
        "{DIFF_SUMMARY_PREFIX}{path} ({}{:+}{reset} {}-{deletions}{reset})",
        green.render(),
        additions,
        red.render(),
    ))
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

fn is_addition_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('+') && !trimmed.starts_with("+++")
}

fn is_deletion_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('-') && !trimmed.starts_with("---")
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

fn parse_apply_patch_path(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("*** ")?;
    let (kind, path) = rest.split_once(':')?;
    let kind = kind.trim();
    if !matches!(kind, "Update File" | "Add File" | "Delete File") {
        return None;
    }
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    Some(path.to_string())
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

pub(crate) fn format_diff_content_lines(diff_content: &str) -> Vec<String> {
    #[derive(Default)]
    struct DiffBlock {
        header: String,
        path: String,
        lines: Vec<String>,
        additions: usize,
        deletions: usize,
    }

    let mut preface: Vec<String> = Vec::new();
    let mut blocks: Vec<DiffBlock> = Vec::new();
    let mut current: Option<DiffBlock> = None;

    for line in diff_content.lines() {
        if let Some(path) = parse_diff_git_path(line).or_else(|| parse_apply_patch_path(line)) {
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

        for line in diff_content.lines() {
            if fallback_path.is_none() {
                fallback_path =
                    parse_diff_marker_path(line).or_else(|| parse_apply_patch_path(line));
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
        let summary = format_diff_summary_line(&path, additions, deletions);

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
        output.push(format_diff_summary_line(
            &block.path,
            block.additions,
            block.deletions,
        ));
        output.extend(block.lines);
    }
    output
}
