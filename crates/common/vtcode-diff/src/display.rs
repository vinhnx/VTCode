//! Semantic display-line extraction, side-by-side pairing, and line counts.

use std::time::Duration;

use super::format::format_hunk_header;
use super::intraline::{annotate_word_level_diffs, annotate_word_level_diffs_with_timeout};
use super::parse::{parse_hunk_range, parse_hunk_starts, parse_omitted_line_count, trim_line_ending};
use super::types::{
    DiffChangeCounts, DiffDisplayKind, DiffDisplayLine, DiffHunk, DiffLineKind, SideBySideRow, default_inline_timeout,
};

/// Counts additions and deletions in structured hunks.
#[must_use]
pub fn count_diff_changes(hunks: &[DiffHunk]) -> DiffChangeCounts {
    let mut counts = DiffChangeCounts::default();
    for line in hunks.iter().flat_map(|hunk| &hunk.lines) {
        match line.kind {
            DiffLineKind::Addition => counts.additions += 1,
            DiffLineKind::Deletion => counts.deletions += 1,
            DiffLineKind::Context => {}
        }
    }
    counts
}
/// Converts hunks to semantic display lines and bounded intraline ranges.
#[must_use]
pub fn display_lines_from_hunks(hunks: &[DiffHunk]) -> Vec<DiffDisplayLine> {
    display_lines_from_hunks_with_timeout(hunks, default_inline_timeout())
}

pub(crate) fn display_lines_from_hunks_with_timeout(
    hunks: &[DiffHunk],
    inline_timeout: Duration,
) -> Vec<DiffDisplayLine> {
    let mut output = Vec::new();
    for hunk in hunks {
        output.push(DiffDisplayLine::body(
            DiffDisplayKind::HunkHeader,
            None,
            None,
            format_hunk_header(hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines),
        ));
        output.extend(hunk.lines.iter().map(|line| {
            let kind = match line.kind {
                DiffLineKind::Context => DiffDisplayKind::Context,
                DiffLineKind::Addition => DiffDisplayKind::Addition,
                DiffLineKind::Deletion => DiffDisplayKind::Deletion,
            };
            DiffDisplayLine::body(kind, line.old_line, line.new_line, trim_line_ending(&line.text).to_owned())
        }));
    }
    annotate_word_level_diffs_with_timeout(&mut output, inline_timeout);
    output
}

/// Parses unified text into display lines, retaining metadata lines.
#[must_use]
pub fn display_lines_from_unified_diff(input: &str) -> Vec<DiffDisplayLine> {
    let mut output = Vec::new();
    let mut old_line = 0u32;
    let mut new_line = 0u32;
    let mut in_hunk = false;
    let mut remaining_old = 0usize;
    let mut remaining_new = 0usize;
    let mut omission_in_hunk = false;
    let mut hunk_old_start = 0u32;
    let mut hunk_new_start = 0u32;
    let mut hunk_old_count = 0usize;
    let mut hunk_new_count = 0usize;
    let mut omitted_tail = Vec::new();
    for raw in input.lines() {
        if let Some((old_start, new_start)) = parse_hunk_starts(raw) {
            assign_omitted_tail_line_numbers(
                &mut output,
                &omitted_tail,
                hunk_old_start,
                hunk_old_count,
                hunk_new_start,
                hunk_new_count,
            );
            omitted_tail.clear();
            old_line = old_start;
            new_line = new_start;
            if let Some((_, old_count, _, new_count)) = parse_hunk_range(raw) {
                remaining_old = old_count;
                remaining_new = new_count;
                hunk_old_count = old_count;
                hunk_new_count = new_count;
            }
            hunk_old_start = old_start;
            hunk_new_start = new_start;
            in_hunk = true;
            omission_in_hunk = false;
            // Preserve the authored header verbatim so range counts survive.
            // Collapsing to `@@ -65 +65 @@` made a pure deletion read as a
            // one-line change.
            output.push(DiffDisplayLine::body(DiffDisplayKind::HunkHeader, None, None, raw.to_owned()));
        } else if in_hunk && (omission_in_hunk || remaining_new > 0) && raw.starts_with('+') {
            let output_index = output.len();
            output.push(DiffDisplayLine::body(
                DiffDisplayKind::Addition,
                None,
                (!omission_in_hunk).then_some(new_line),
                raw[1..].to_owned(),
            ));
            if omission_in_hunk {
                omitted_tail.push(output_index);
            }
            new_line = new_line.saturating_add(1);
            remaining_new = remaining_new.saturating_sub(1);
        } else if in_hunk && (omission_in_hunk || remaining_old > 0) && raw.starts_with('-') {
            let output_index = output.len();
            output.push(DiffDisplayLine::body(
                DiffDisplayKind::Deletion,
                (!omission_in_hunk).then_some(old_line),
                None,
                raw[1..].to_owned(),
            ));
            if omission_in_hunk {
                omitted_tail.push(output_index);
            }
            old_line = old_line.saturating_add(1);
            remaining_old = remaining_old.saturating_sub(1);
        } else if in_hunk && (omission_in_hunk || (remaining_old > 0 && remaining_new > 0)) && raw.starts_with(' ') {
            let output_index = output.len();
            output.push(DiffDisplayLine::body(
                DiffDisplayKind::Context,
                (!omission_in_hunk).then_some(old_line),
                (!omission_in_hunk).then_some(new_line),
                raw[1..].to_owned(),
            ));
            if omission_in_hunk {
                omitted_tail.push(output_index);
            }
            old_line = old_line.saturating_add(1);
            new_line = new_line.saturating_add(1);
            remaining_old = remaining_old.saturating_sub(1);
            remaining_new = remaining_new.saturating_sub(1);
        } else if in_hunk && let Some(omitted) = parse_omitted_line_count(raw) {
            output.push(DiffDisplayLine::body(DiffDisplayKind::Metadata, None, None, raw.to_owned()));
            omission_in_hunk = true;
            let omitted = omitted.min(remaining_old).min(remaining_new);
            old_line = old_line.saturating_add(u32::try_from(omitted).unwrap_or(u32::MAX));
            new_line = new_line.saturating_add(u32::try_from(omitted).unwrap_or(u32::MAX));
            remaining_old = remaining_old.saturating_sub(omitted);
            remaining_new = remaining_new.saturating_sub(omitted);
        } else {
            output.push(DiffDisplayLine::body(DiffDisplayKind::Metadata, None, None, raw.to_owned()));
        }
    }
    assign_omitted_tail_line_numbers(
        &mut output,
        &omitted_tail,
        hunk_old_start,
        hunk_old_count,
        hunk_new_start,
        hunk_new_count,
    );
    annotate_word_level_diffs(&mut output);
    output
}

fn assign_omitted_tail_line_numbers(
    lines: &mut [DiffDisplayLine],
    tail: &[usize],
    old_start: u32,
    old_count: usize,
    new_start: u32,
    new_count: usize,
) {
    if tail.is_empty() {
        return;
    }
    let old_tail_count = tail
        .iter()
        .filter(|&&index| lines[index].kind != DiffDisplayKind::Addition)
        .count();
    let new_tail_count = tail
        .iter()
        .filter(|&&index| lines[index].kind != DiffDisplayKind::Deletion)
        .count();
    let mut old_line = old_start
        .saturating_add(u32::try_from(old_count).unwrap_or(u32::MAX))
        .saturating_sub(u32::try_from(old_tail_count).unwrap_or(u32::MAX));
    let mut new_line = new_start
        .saturating_add(u32::try_from(new_count).unwrap_or(u32::MAX))
        .saturating_sub(u32::try_from(new_tail_count).unwrap_or(u32::MAX));
    for &index in tail {
        match lines[index].kind {
            DiffDisplayKind::Addition => {
                lines[index].new_line = Some(new_line);
                new_line = new_line.saturating_add(1);
            }
            DiffDisplayKind::Deletion => {
                lines[index].old_line = Some(old_line);
                old_line = old_line.saturating_add(1);
            }
            DiffDisplayKind::Context => {
                lines[index].old_line = Some(old_line);
                lines[index].new_line = Some(new_line);
                old_line = old_line.saturating_add(1);
                new_line = new_line.saturating_add(1);
            }
            DiffDisplayKind::Metadata | DiffDisplayKind::HunkHeader => {}
        }
    }
}
/// Pairs deletions and additions for side-by-side presentation.
#[must_use]
pub fn side_by_side_rows(lines: &[DiffDisplayLine]) -> Vec<SideBySideRow> {
    let mut rows = Vec::new();
    for_each_side_by_side_pair(lines, |left, right| {
        rows.push(SideBySideRow { left: left.cloned(), right: right.cloned() });
    });
    rows
}

pub(crate) fn for_each_side_by_side_pair<'a, F>(lines: &'a [DiffDisplayLine], mut visit: F)
where
    F: FnMut(Option<&'a DiffDisplayLine>, Option<&'a DiffDisplayLine>),
{
    let mut index = 0usize;
    while index < lines.len() {
        match lines[index].kind {
            DiffDisplayKind::HunkHeader | DiffDisplayKind::Metadata => {
                visit(Some(&lines[index]), None);
                index += 1;
            }
            DiffDisplayKind::Context => {
                let line = &lines[index];
                visit(Some(line), Some(line));
                index += 1;
            }
            DiffDisplayKind::Deletion => {
                let delete_start = index;
                while index < lines.len() && lines[index].kind == DiffDisplayKind::Deletion {
                    index += 1;
                }
                let insert_start = index;
                while index < lines.len() && lines[index].kind == DiffDisplayKind::Addition {
                    index += 1;
                }
                let delete_count = insert_start - delete_start;
                let insert_count = index - insert_start;
                for offset in 0..delete_count.max(insert_count) {
                    visit(
                        (offset < delete_count).then(|| &lines[delete_start + offset]),
                        (offset < insert_count).then(|| &lines[insert_start + offset]),
                    );
                }
            }
            DiffDisplayKind::Addition => {
                visit(None, Some(&lines[index]));
                index += 1;
            }
        }
    }
}
