//! Structured diff computation and `DiffDocument` construction.

use similar::{ChangeTag, TextDiff};

use super::layout::layout_document;
use super::parse::{is_unified_metadata_line, parse_hunk_range, parse_omitted_line_count, trim_line_ending};
use super::types::{
    DiffBundle, DiffDocument, DiffHunk, DiffLine, DiffLineKind, DiffOptions, DiffRow, DiffStats, LayoutOptions,
    ParseDiffError, default_inline_timeout,
};

impl DiffDocument {
    /// Computes a bounded diff from before/after text.
    #[must_use]
    pub fn between(old: &str, new: &str, options: DiffOptions<'_>) -> Self {
        let old_lines = split_lines_with_terminator(old);
        let new_lines = split_lines_with_terminator(new);
        let old_line_refs: Vec<&str> = old_lines.iter().map(String::as_str).collect();
        let new_line_refs: Vec<&str> = new_lines.iter().map(String::as_str).collect();
        let mut config = TextDiff::configure();
        let _ = config.algorithm(options.algorithm.similar()).timeout(options.timeout);
        let diff = config.diff_slices(&old_line_refs, &new_line_refs);
        let mut hunks = Vec::new();

        for group in diff.grouped_ops(options.context_lines) {
            let Some(_first) = group.first() else {
                continue;
            };
            let mut lines = Vec::new();
            for operation in &group {
                for change in diff.iter_changes(operation) {
                    let (kind, old_line, new_line) = match change.tag() {
                        ChangeTag::Equal => (
                            DiffLineKind::Context,
                            change.old_index().and_then(one_based_u32),
                            change.new_index().and_then(one_based_u32),
                        ),
                        ChangeTag::Delete => (DiffLineKind::Deletion, change.old_index().and_then(one_based_u32), None),
                        ChangeTag::Insert => (DiffLineKind::Addition, None, change.new_index().and_then(one_based_u32)),
                    };
                    lines.push(DiffLine {
                        kind,
                        old_line,
                        new_line,
                        text: change.value().to_owned(),
                    });
                }
            }
            let old_lines = lines.iter().filter(|line| line.kind != DiffLineKind::Addition).count();
            let new_lines = lines.iter().filter(|line| line.kind != DiffLineKind::Deletion).count();
            let old_start = hunk_start_from_lines(&lines, true);
            let new_start = hunk_start_from_lines(&lines, false);
            hunks.push(DiffHunk { old_start, old_lines, new_start, new_lines, lines });
        }
        let mut document = Self::from_hunks(hunks);
        document.inline_timeout = options.inline_timeout;
        document
    }

    /// Constructs a document from precomputed hunks.
    #[must_use]
    pub fn from_hunks(hunks: Vec<DiffHunk>) -> Self {
        let mut stats = DiffStats { hunks: hunks.len(), ..DiffStats::default() };
        for line in hunks.iter().flat_map(|hunk| &hunk.lines) {
            match line.kind {
                DiffLineKind::Context => stats.context += 1,
                DiffLineKind::Addition => stats.additions += 1,
                DiffLineKind::Deletion => stats.deletions += 1,
            }
        }
        Self {
            hunks,
            stats,
            inline_timeout: default_inline_timeout(),
        }
    }

    /// Parses unified-diff text into a structured document.
    ///
    /// File metadata is accepted and ignored. A body line before the first
    /// valid hunk header is rejected instead of receiving invented numbers.
    pub fn from_unified(input: &str) -> Result<Self, ParseDiffError> {
        let mut hunks = Vec::new();
        let mut current: Option<DiffHunk> = None;
        let mut old_line = 0u32;
        let mut new_line = 0u32;
        let mut expected_old_lines = 0usize;
        let mut expected_new_lines = 0usize;
        let mut omitted_in_current_hunk = false;
        let mut omitted_rows = 0usize;
        let mut omitted_tail = Vec::new();

        for raw_with_ending in split_line_slices(input) {
            let raw = trim_line_ending(raw_with_ending);
            if raw.starts_with("@@") {
                if let Some(mut hunk) = current.take() {
                    assign_omitted_hunk_tail_line_numbers(
                        &mut hunk,
                        &omitted_tail,
                        expected_old_lines,
                        expected_new_lines,
                    );
                    if !omitted_in_current_hunk {
                        validate_hunk_counts(&hunk, expected_old_lines, expected_new_lines)?;
                    }
                    hunks.push(hunk);
                }
                omitted_tail.clear();
                let (old_start, old_count, new_start, new_count) =
                    parse_hunk_range(raw).ok_or_else(|| ParseDiffError::new("invalid hunk header"))?;
                old_line = old_start;
                new_line = new_start;
                expected_old_lines = old_count;
                expected_new_lines = new_count;
                omitted_in_current_hunk = false;
                current = Some(DiffHunk {
                    old_start: old_start as usize,
                    old_lines: 0,
                    new_start: new_start as usize,
                    new_lines: 0,
                    lines: Vec::new(),
                });
                continue;
            }
            if raw.starts_with("diff ")
                || raw.starts_with("index ")
                || raw == r"\ No newline at end of file"
                || raw.is_empty()
            {
                continue;
            }
            // Git's file/mode headers precede the first hunk in ordinary
            // unified output. Keep them out of the body parser while still
            // rejecting an actual `-...`/`+...` body before any hunk.
            let hunk_complete = current
                .as_ref()
                .is_some_and(|hunk| hunk.old_lines >= expected_old_lines && hunk.new_lines >= expected_new_lines);
            if (current.is_none() || hunk_complete) && is_unified_metadata_line(raw) {
                continue;
            }
            if hunk_complete && (raw.starts_with("--- ") || raw.starts_with("+++ ")) {
                continue;
            }
            let Some(hunk) = current.as_mut() else {
                return Err(ParseDiffError::new("diff body appears before a hunk header"));
            };
            let is_body_line = matches!(raw.as_bytes().first().copied(), Some(b'-' | b'+' | b' '));
            if !is_body_line && let Some(omitted) = parse_omitted_line_count(raw) {
                omitted_in_current_hunk = true;
                let marker_rows = omitted;
                let advance = marker_rows
                    .min(expected_old_lines.saturating_sub(hunk.old_lines))
                    .min(expected_new_lines.saturating_sub(hunk.new_lines));
                old_line = old_line.saturating_add(u32::try_from(advance).unwrap_or(u32::MAX));
                new_line = new_line.saturating_add(u32::try_from(advance).unwrap_or(u32::MAX));
                hunk.old_lines = hunk.old_lines.saturating_add(advance);
                hunk.new_lines = hunk.new_lines.saturating_add(advance);
                omitted_rows = omitted_rows.saturating_add(marker_rows);
                continue;
            }
            let (kind, old_number, new_number) = match raw.as_bytes().first().copied() {
                Some(b'-') => {
                    if hunk.old_lines >= expected_old_lines {
                        return Err(ParseDiffError::new("too many old-side lines for hunk header"));
                    }
                    let number = old_line;
                    old_line = old_line.saturating_add(1);
                    hunk.old_lines += 1;
                    (DiffLineKind::Deletion, Some(number), None)
                }
                Some(b'+') => {
                    if hunk.new_lines >= expected_new_lines {
                        return Err(ParseDiffError::new("too many new-side lines for hunk header"));
                    }
                    let number = new_line;
                    new_line = new_line.saturating_add(1);
                    hunk.new_lines += 1;
                    (DiffLineKind::Addition, None, Some(number))
                }
                Some(b' ') => {
                    if hunk.old_lines >= expected_old_lines || hunk.new_lines >= expected_new_lines {
                        return Err(ParseDiffError::new("too many context lines for hunk header"));
                    }
                    let old_number = old_line;
                    let new_number = new_line;
                    old_line = old_line.saturating_add(1);
                    new_line = new_line.saturating_add(1);
                    hunk.old_lines += 1;
                    hunk.new_lines += 1;
                    (DiffLineKind::Context, Some(old_number), Some(new_number))
                }
                _ => return Err(ParseDiffError::new("invalid unified diff body line")),
            };
            hunk.lines.push(DiffLine {
                kind,
                old_line: old_number,
                new_line: new_number,
                text: raw_with_ending[1..].to_owned(),
            });
            if omitted_in_current_hunk {
                omitted_tail.push(hunk.lines.len() - 1);
            }
        }
        if let Some(mut hunk) = current {
            assign_omitted_hunk_tail_line_numbers(&mut hunk, &omitted_tail, expected_old_lines, expected_new_lines);
            if !omitted_in_current_hunk {
                validate_hunk_counts(&hunk, expected_old_lines, expected_new_lines)?;
            }
            hunks.push(hunk);
        }
        let mut document = Self::from_hunks(hunks);
        document.stats.omitted_rows = omitted_rows;
        Ok(document)
    }

    /// Lays out this document with terminal display-width wrapping.
    #[must_use]
    pub fn layout(&self, options: LayoutOptions) -> Vec<DiffRow> {
        layout_document(self, options)
    }
}
fn one_based_u32(index: usize) -> Option<u32> {
    u32::try_from(index).ok()?.checked_add(1)
}

fn hunk_start_from_lines(lines: &[DiffLine], old_side: bool) -> usize {
    let line_number = lines
        .iter()
        .find_map(|line| if old_side { line.old_line } else { line.new_line });
    if let Some(line_number) = line_number {
        return usize::try_from(line_number).unwrap_or(usize::MAX).max(1);
    }

    let counterpart = lines
        .iter()
        .find_map(|line| if old_side { line.new_line } else { line.old_line });
    usize::try_from(counterpart.unwrap_or(1)).unwrap_or(usize::MAX).max(1)
}

fn validate_hunk_counts(
    hunk: &DiffHunk,
    expected_old_lines: usize,
    expected_new_lines: usize,
) -> Result<(), ParseDiffError> {
    if hunk.old_lines == expected_old_lines && hunk.new_lines == expected_new_lines {
        Ok(())
    } else {
        Err(ParseDiffError::new("hunk line counts do not match header"))
    }
}

fn assign_omitted_hunk_tail_line_numbers(
    hunk: &mut DiffHunk,
    tail: &[usize],
    expected_old_lines: usize,
    expected_new_lines: usize,
) {
    if tail.is_empty() {
        return;
    }
    let old_tail_count = tail
        .iter()
        .filter(|&&index| hunk.lines[index].kind != DiffLineKind::Addition)
        .count();
    let new_tail_count = tail
        .iter()
        .filter(|&&index| hunk.lines[index].kind != DiffLineKind::Deletion)
        .count();
    let mut old_line = u32::try_from(hunk.old_start)
        .unwrap_or(u32::MAX)
        .saturating_add(u32::try_from(expected_old_lines).unwrap_or(u32::MAX))
        .saturating_sub(u32::try_from(old_tail_count).unwrap_or(u32::MAX));
    let mut new_line = u32::try_from(hunk.new_start)
        .unwrap_or(u32::MAX)
        .saturating_add(u32::try_from(expected_new_lines).unwrap_or(u32::MAX))
        .saturating_sub(u32::try_from(new_tail_count).unwrap_or(u32::MAX));
    for &index in tail {
        match hunk.lines[index].kind {
            DiffLineKind::Addition => {
                hunk.lines[index].new_line = Some(new_line);
                new_line = new_line.saturating_add(1);
            }
            DiffLineKind::Deletion => {
                hunk.lines[index].old_line = Some(old_line);
                old_line = old_line.saturating_add(1);
            }
            DiffLineKind::Context => {
                hunk.lines[index].old_line = Some(old_line);
                hunk.lines[index].new_line = Some(new_line);
                old_line = old_line.saturating_add(1);
                new_line = new_line.saturating_add(1);
            }
        }
    }
}
/// Computes a structured diff and passes non-empty hunks to a formatter.
pub fn compute_diff<F>(old: &str, new: &str, options: DiffOptions<'_>, formatter: F) -> DiffBundle
where
    F: FnOnce(&[DiffHunk], &DiffOptions<'_>) -> String,
{
    if old == new {
        return DiffBundle {
            hunks: Vec::new(),
            formatted: String::new(),
            is_empty: true,
        };
    }
    let document = DiffDocument::between(old, new, options.clone());
    let formatted = formatter(&document.hunks, &options);
    DiffBundle { hunks: document.hunks, formatted, is_empty: false }
}
pub(crate) fn split_lines_with_terminator(text: &str) -> Vec<String> {
    split_line_slices(text).into_iter().map(str::to_owned).collect()
}

pub(crate) fn split_line_slices(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'\n' && bytes[index] != b'\r' {
            index += 1;
            continue;
        }
        let end = if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            index + 2
        } else {
            index + 1
        };
        lines.push(&text[start..end]);
        start = end;
        index = end;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}
