use vtcode_commons::diff_paths::{
    format_start_only_hunk_header, is_diff_addition_line, is_diff_deletion_line, parse_hunk_starts,
};

pub(crate) fn format_diff_content_lines_with_numbers(diff_content: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut old_line_no = 0usize;
    let mut new_line_no = 0usize;
    let mut in_hunk = false;

    for line in diff_content.lines() {
        if let Some((old_start, new_start)) = parse_hunk_starts(line) {
            old_line_no = old_start;
            new_line_no = new_start;
            in_hunk = true;
            lines.push(
                format_start_only_hunk_header(line)
                    .unwrap_or_else(|| format!("@@ -{} +{} @@", old_start, new_start)),
            );
            continue;
        }

        if !in_hunk {
            lines.push(line.to_string());
            continue;
        }

        if is_diff_addition_line(line) {
            lines.push(format!("+{:>5} {}", new_line_no, &line[1..]));
            new_line_no = new_line_no.saturating_add(1);
            continue;
        }

        if is_diff_deletion_line(line) {
            lines.push(format!("-{:>5} {}", old_line_no, &line[1..]));
            old_line_no = old_line_no.saturating_add(1);
            continue;
        }

        if let Some(context_line) = line.strip_prefix(' ') {
            lines.push(format!(" {:>5} {}", new_line_no, context_line));
            old_line_no = old_line_no.saturating_add(1);
            new_line_no = new_line_no.saturating_add(1);
            continue;
        }

        // End of hunk or metadata line
        lines.push(line.to_string());
    }

    lines
}
