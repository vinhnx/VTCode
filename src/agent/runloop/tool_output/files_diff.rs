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
