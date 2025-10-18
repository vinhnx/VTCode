//! Diff utilities for generating structured and formatted diffs.

use serde::Serialize;
use similar::{ChangeTag, TextDiff};

/// Options for diff generation.
#[derive(Debug, Clone)]
pub struct DiffOptions<'a> {
    /// Number of context lines around changes.
    pub context_lines: usize,
    /// Optional label for the old/left side of the diff.
    pub old_label: Option<&'a str>,
    /// Optional label for the new/right side of the diff.
    pub new_label: Option<&'a str>,
    /// Whether to emit the ``\ No newline at end of file`` hint.
    pub missing_newline_hint: bool,
}

impl Default for DiffOptions<'_> {
    fn default() -> Self {
        Self {
            context_lines: 3,
            old_label: None,
            new_label: None,
            missing_newline_hint: true,
        }
    }
}

/// A diff rendered with both structured hunks and formatted text.
#[derive(Debug, Clone, Serialize)]
pub struct DiffBundle {
    /// Structured hunks capturing change metadata.
    pub hunks: Vec<DiffHunk>,
    /// Unified diff formatted as plain text.
    pub formatted: String,
    /// Indicates whether the diff has no changes.
    pub is_empty: bool,
}

/// A diff hunk with metadata for old/new ranges.
#[derive(Debug, Clone, Serialize)]
pub struct DiffHunk {
    /// Starting line (1-based) in the original content.
    pub old_start: usize,
    /// Number of lines in the original content spanned by the hunk.
    pub old_lines: usize,
    /// Starting line (1-based) in the new content.
    pub new_start: usize,
    /// Number of lines in the new content spanned by the hunk.
    pub new_lines: usize,
    /// Individual line changes inside the hunk.
    pub lines: Vec<DiffLine>,
}

/// A single diff line annotated with metadata and type.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    /// Context line unchanged between versions.
    Context,
    /// Line added in the new version.
    Addition,
    /// Line removed from the old version.
    Deletion,
}

/// Metadata for a single line inside a diff hunk.
#[derive(Debug, Clone, Serialize)]
pub struct DiffLine {
    /// Classification of the diff line.
    pub kind: DiffLineKind,
    /// Optional line number (1-based) in the original content.
    pub old_line: Option<usize>,
    /// Optional line number (1-based) in the new content.
    pub new_line: Option<usize>,
    /// The line text (includes trailing newline if present).
    pub text: String,
}

/// Compute a structured diff bundle.
pub fn compute_diff(old: &str, new: &str, options: DiffOptions<'_>) -> DiffBundle {
    let diff = TextDiff::from_lines(old, new);
    let grouped_ops = diff.grouped_ops(options.context_lines);

    let mut hunks = Vec::new();
    for ops in grouped_ops {
        if ops.is_empty() {
            continue;
        }

        let mut lines = Vec::new();
        let mut old_min: Option<usize> = None;
        let mut old_max: Option<usize> = None;
        let mut new_min: Option<usize> = None;
        let mut new_max: Option<usize> = None;

        for op in &ops {
            for change in diff.iter_changes(op) {
                let old_idx = change.old_index().map(|idx| idx + 1);
                let new_idx = change.new_index().map(|idx| idx + 1);

                if let Some(idx) = old_idx {
                    old_min = Some(old_min.map_or(idx, |current| current.min(idx)));
                    old_max = Some(old_max.map_or(idx, |current| current.max(idx)));
                }
                if let Some(idx) = new_idx {
                    new_min = Some(new_min.map_or(idx, |current| current.min(idx)));
                    new_max = Some(new_max.map_or(idx, |current| current.max(idx)));
                }

                let kind = match change.tag() {
                    ChangeTag::Delete => DiffLineKind::Deletion,
                    ChangeTag::Insert => DiffLineKind::Addition,
                    ChangeTag::Equal => DiffLineKind::Context,
                };

                lines.push(DiffLine {
                    kind,
                    old_line: old_idx,
                    new_line: new_idx,
                    text: change.value().to_string(),
                });
            }
        }

        let old_start = old_min
            .or_else(|| ops.first().map(|op| op.old_range().start + 1))
            .unwrap_or(1);
        let new_start = new_min
            .or_else(|| ops.first().map(|op| op.new_range().start + 1))
            .unwrap_or(1);

        let old_lines = match (old_min, old_max) {
            (Some(min), Some(max)) => max.saturating_sub(min) + 1,
            _ => 0,
        };
        let new_lines = match (new_min, new_max) {
            (Some(min), Some(max)) => max.saturating_sub(min) + 1,
            _ => 0,
        };

        hunks.push(DiffHunk {
            old_start,
            old_lines,
            new_start,
            new_lines,
            lines,
        });
    }

    let mut unified = diff.unified_diff();
    unified.context_radius(options.context_lines);
    unified.missing_newline_hint(options.missing_newline_hint);

    if let (Some(old_label), Some(new_label)) = (options.old_label, options.new_label) {
        unified.header(old_label, new_label);
    } else if let Some(old_label) = options.old_label {
        unified.header(old_label, old_label);
    } else if let Some(new_label) = options.new_label {
        unified.header(new_label, new_label);
    }

    let formatted = unified.to_string();
    let is_empty = hunks.is_empty();

    DiffBundle {
        hunks,
        formatted,
        is_empty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_structured_diff() {
        let before = "a\nb\nc\n";
        let after = "a\nc\nd\n";
        let bundle = compute_diff(
            before,
            after,
            DiffOptions {
                context_lines: 2,
                old_label: Some("old"),
                new_label: Some("new"),
                ..Default::default()
            },
        );

        assert!(!bundle.is_empty);
        assert_eq!(bundle.hunks.len(), 1);
        let hunk = &bundle.hunks[0];
        assert_eq!(hunk.old_start, 1);
        assert_eq!(hunk.new_start, 1);
        assert!(bundle.formatted.contains("@@"));
        assert!(bundle.formatted.contains("-b"));
        assert!(bundle.formatted.contains("+d"));
    }
}
