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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffDisplayKind {
    Metadata,
    HunkHeader,
    Context,
    Addition,
    Deletion,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffDisplayLine {
    pub kind: DiffDisplayKind,
    pub line_number: Option<usize>,
    pub text: String,
}

impl DiffDisplayLine {
    pub fn numbered_text(&self, line_number_width: usize) -> String {
        match self.kind {
            DiffDisplayKind::Metadata | DiffDisplayKind::HunkHeader => self.text.clone(),
            DiffDisplayKind::Addition => format!(
                "+{:>line_number_width$} {}",
                self.line_number.unwrap_or_default(),
                self.text
            ),
            DiffDisplayKind::Deletion => format!(
                "-{:>line_number_width$} {}",
                self.line_number.unwrap_or_default(),
                self.text
            ),
            DiffDisplayKind::Context => format!(
                " {:>line_number_width$} {}",
                self.line_number.unwrap_or_default(),
                self.text
            ),
        }
    }
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

pub fn display_lines_from_hunks(hunks: &[DiffHunk]) -> Vec<DiffDisplayLine> {
    let mut lines = Vec::new();

    for hunk in hunks {
        lines.push(DiffDisplayLine {
            kind: DiffDisplayKind::HunkHeader,
            line_number: None,
            text: format!("@@ -{} +{} @@", hunk.old_start, hunk.new_start),
        });

        for line in &hunk.lines {
            lines.push(display_line_from_diff_line(line));
        }
    }

    lines
}

pub fn display_lines_from_unified_diff(diff_content: &str) -> Vec<DiffDisplayLine> {
    let mut lines = Vec::new();
    let mut old_line_no = 0usize;
    let mut new_line_no = 0usize;
    let mut in_hunk = false;

    for line in diff_content.lines() {
        if let Some((old_start, new_start)) = parse_hunk_starts(line) {
            old_line_no = old_start;
            new_line_no = new_start;
            in_hunk = true;
            lines.push(DiffDisplayLine {
                kind: DiffDisplayKind::HunkHeader,
                line_number: None,
                text: format_start_only_hunk_header(line)
                    .unwrap_or_else(|| format!("@@ -{old_start} +{new_start} @@")),
            });
            continue;
        }

        if !in_hunk {
            lines.push(DiffDisplayLine {
                kind: DiffDisplayKind::Metadata,
                line_number: None,
                text: line.to_string(),
            });
            continue;
        }

        if is_diff_addition_line(line) {
            lines.push(DiffDisplayLine {
                kind: DiffDisplayKind::Addition,
                line_number: Some(new_line_no),
                text: line[1..].to_string(),
            });
            new_line_no = new_line_no.saturating_add(1);
            continue;
        }

        if is_diff_deletion_line(line) {
            lines.push(DiffDisplayLine {
                kind: DiffDisplayKind::Deletion,
                line_number: Some(old_line_no),
                text: line[1..].to_string(),
            });
            old_line_no = old_line_no.saturating_add(1);
            continue;
        }

        if let Some(context_line) = line.strip_prefix(' ') {
            lines.push(DiffDisplayLine {
                kind: DiffDisplayKind::Context,
                line_number: Some(new_line_no),
                text: context_line.to_string(),
            });
            old_line_no = old_line_no.saturating_add(1);
            new_line_no = new_line_no.saturating_add(1);
            continue;
        }

        lines.push(DiffDisplayLine {
            kind: DiffDisplayKind::Metadata,
            line_number: None,
            text: line.to_string(),
        });
    }

    lines
}

pub fn diff_display_line_number_width(lines: &[DiffDisplayLine]) -> usize {
    let max_digits = lines
        .iter()
        .filter_map(|line| line.line_number.map(|line_no| line_no.to_string().len()))
        .max()
        .unwrap_or(4);
    max_digits.clamp(5, 6)
}

pub fn format_numbered_unified_diff(diff_content: &str) -> Vec<String> {
    let display_lines = display_lines_from_unified_diff(diff_content);
    let width = diff_display_line_number_width(&display_lines);
    display_lines
        .into_iter()
        .map(|line| line.numbered_text(width))
        .collect()
}

fn display_line_from_diff_line(line: &crate::diff::DiffLine) -> DiffDisplayLine {
    let text = line.text.trim_end_matches('\n').to_string();
    match line.kind {
        DiffLineKind::Context => DiffDisplayLine {
            kind: DiffDisplayKind::Context,
            line_number: line.new_line,
            text,
        },
        DiffLineKind::Addition => DiffDisplayLine {
            kind: DiffDisplayKind::Addition,
            line_number: line.new_line,
            text,
        },
        DiffLineKind::Deletion => DiffDisplayLine {
            kind: DiffDisplayKind::Deletion,
            line_number: line.old_line,
            text,
        },
    }
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
    fn display_lines_from_hunks_preserves_semantics() {
        let hunks = vec![DiffHunk {
            old_start: 10,
            old_lines: 2,
            new_start: 10,
            new_lines: 2,
            lines: vec![
                DiffLine {
                    kind: DiffLineKind::Deletion,
                    old_line: Some(10),
                    new_line: None,
                    text: "old\n".to_string(),
                },
                DiffLine {
                    kind: DiffLineKind::Addition,
                    old_line: None,
                    new_line: Some(10),
                    text: "new\n".to_string(),
                },
                DiffLine {
                    kind: DiffLineKind::Context,
                    old_line: Some(11),
                    new_line: Some(11),
                    text: "same\n".to_string(),
                },
            ],
        }];

        let lines = display_lines_from_hunks(&hunks);
        assert_eq!(lines[0].kind, DiffDisplayKind::HunkHeader);
        assert_eq!(lines[0].text, "@@ -10 +10 @@");
        assert_eq!(lines[1].kind, DiffDisplayKind::Deletion);
        assert_eq!(lines[1].line_number, Some(10));
        assert_eq!(lines[1].text, "old");
        assert_eq!(lines[2].kind, DiffDisplayKind::Addition);
        assert_eq!(lines[2].line_number, Some(10));
        assert_eq!(lines[3].kind, DiffDisplayKind::Context);
        assert_eq!(lines[3].line_number, Some(11));
    }

    #[test]
    fn diff_display_line_number_width_tracks_max_digits() {
        let lines = vec![
            DiffDisplayLine {
                kind: DiffDisplayKind::Addition,
                line_number: Some(99),
                text: "let a = 1;".to_string(),
            },
            DiffDisplayLine {
                kind: DiffDisplayKind::Context,
                line_number: Some(10_420),
                text: "let b = 2;".to_string(),
            },
        ];

        assert_eq!(diff_display_line_number_width(&lines), 5);
    }

    #[test]
    fn preserves_plain_text_when_not_diff() {
        let lines = format_numbered_unified_diff("plain text output");
        assert_eq!(lines, vec!["plain text output".to_string()]);
    }
}
