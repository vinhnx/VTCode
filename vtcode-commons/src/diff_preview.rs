//! Shared helpers for rendering diff previews.

use crate::diff::{DiffHunk, DiffLineKind};
use crate::diff_paths::{
    format_start_only_hunk_header, is_diff_addition_line, is_diff_deletion_line, parse_hunk_starts,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiffChangeCounts {
    pub additions: usize,
    pub deletions: usize,
}

impl DiffChangeCounts {
    pub fn total(self) -> usize {
        self.additions + self.deletions
    }
}

pub fn count_diff_changes(hunks: &[DiffHunk]) -> DiffChangeCounts {
    let mut counts = DiffChangeCounts::default();

    for hunk in hunks {
        for line in &hunk.lines {
            match line.kind {
                DiffLineKind::Addition => counts.additions += 1,
                DiffLineKind::Deletion => counts.deletions += 1,
                DiffLineKind::Context => {}
            }
        }
    }

    counts
}

pub fn format_numbered_unified_diff(diff_content: &str) -> Vec<String> {
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
                    .unwrap_or_else(|| format!("@@ -{old_start} +{new_start} @@")),
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

        lines.push(line.to_string());
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{DiffLine, DiffLineKind};

    #[test]
    fn counts_diff_changes_from_hunks() {
        let hunks = vec![DiffHunk {
            old_start: 1,
            old_lines: 2,
            new_start: 1,
            new_lines: 2,
            lines: vec![
                DiffLine {
                    kind: DiffLineKind::Context,
                    old_line: Some(1),
                    new_line: Some(1),
                    text: "same\n".to_string(),
                },
                DiffLine {
                    kind: DiffLineKind::Deletion,
                    old_line: Some(2),
                    new_line: None,
                    text: "old\n".to_string(),
                },
                DiffLine {
                    kind: DiffLineKind::Addition,
                    old_line: None,
                    new_line: Some(2),
                    text: "new\n".to_string(),
                },
            ],
        }];

        let counts = count_diff_changes(&hunks);
        assert_eq!(counts.additions, 1);
        assert_eq!(counts.deletions, 1);
        assert_eq!(counts.total(), 2);
    }

    #[test]
    fn formats_numbered_unified_diff_with_start_only_headers() {
        let diff = "\
diff --git a/file.txt b/file.txt
@@ -10,2 +10,2 @@
-old
+new
 context
";

        let lines = format_numbered_unified_diff(diff);
        assert_eq!(lines[0], "diff --git a/file.txt b/file.txt");
        assert!(lines.iter().any(|line| line == "@@ -10 +10 @@"));
        assert!(lines.iter().any(|line| line.starts_with("-   10 old")));
        assert!(lines.iter().any(|line| line.starts_with("+   10 new")));
        assert!(lines.iter().any(|line| line.starts_with("    11 context")));
    }

    #[test]
    fn preserves_plain_text_when_not_diff() {
        let lines = format_numbered_unified_diff("plain text output");
        assert_eq!(lines, vec!["plain text output".to_string()]);
    }
}
