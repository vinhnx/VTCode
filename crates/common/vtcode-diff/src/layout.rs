//! Renderer-neutral semantic row layout with bounded wrapping.

use std::collections::VecDeque;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::display::{display_lines_from_hunks_with_timeout, for_each_side_by_side_pair};
use super::format::diff_display_line_number_width;
use super::types::{
    DiffCell, DiffDisplayKind, DiffDisplayLine, DiffDocument, DiffLayout, DiffRow, DiffRowKind, DiffSegment,
    LayoutOptions,
};

pub(crate) fn layout_document(document: &DiffDocument, options: LayoutOptions) -> Vec<DiffRow> {
    let display = display_lines_from_hunks_with_timeout(&document.hunks, document.inline_timeout);
    layout_display_lines(&display, options)
}

/// Lays out precomputed display lines without repeating intraline analysis.
///
/// Interactive applications can cache semantic lines when an overlay opens
/// and call this inexpensive step again after a resize.
#[must_use]
pub fn layout_display_lines(display: &[DiffDisplayLine], options: LayoutOptions) -> Vec<DiffRow> {
    let use_side_by_side = options.layout == DiffLayout::SideBySide && options.width >= options.min_side_by_side_width;
    let mut rows = RowCollector::new(options.max_rows);
    if use_side_by_side {
        layout_side_by_side(display, options, &mut rows);
    } else {
        layout_unified(display, options, &mut rows);
    }
    rows.finish()
}

/// Returns a bounded head/tail excerpt of semantic display lines.
///
/// The returned vector contains at most `max_rows` entries. When rows are
/// omitted, one metadata entry (`... N lines omitted ...`) is inserted between
/// the retained head and tail. Source line numbers and intraline ranges on
/// retained entries are preserved, so callers can render the excerpt without
/// reparsing or inventing positions.
#[must_use]
pub fn bounded_display_lines(lines: &[DiffDisplayLine], max_rows: usize) -> Vec<DiffDisplayLine> {
    if max_rows == 0 {
        return Vec::new();
    }
    if lines.len() <= max_rows {
        return lines.to_vec();
    }

    let retained = max_rows.saturating_sub(1);
    let head_count = retained.saturating_add(1) / 2;
    let tail_count = retained / 2;
    let omitted = lines.len().saturating_sub(head_count + tail_count);

    let mut bounded = Vec::with_capacity(max_rows);
    bounded.extend_from_slice(&lines[..head_count]);
    bounded.push(DiffDisplayLine::body(
        DiffDisplayKind::Metadata,
        None,
        None,
        format!("... {omitted} lines omitted ..."),
    ));
    if tail_count > 0 {
        bounded.extend_from_slice(&lines[lines.len() - tail_count..]);
    }
    bounded
}

fn layout_unified(lines: &[DiffDisplayLine], options: LayoutOptions, rows: &mut RowCollector) {
    let gutter_width = diff_display_line_number_width(lines).saturating_add(4);
    let content_width = options.width.saturating_sub(gutter_width).max(1);
    let mut hunk_index = None;
    for line in lines {
        if line.kind == DiffDisplayKind::Metadata {
            rows.push(metadata_row(&line.text, hunk_index));
            continue;
        }
        if line.kind == DiffDisplayKind::HunkHeader {
            hunk_index = Some(hunk_index.map_or(0, |index| index + 1));
            rows.push(header_row(&line.text, hunk_index));
            continue;
        }
        let marker = marker_for_kind(line.kind);
        for (continuation, segments) in wrap_segments(&line.text, &line.changed, content_width, options.wrap)
            .into_iter()
            .enumerate()
        {
            rows.push(DiffRow {
                kind: row_kind(line.kind),
                marker,
                hunk_index,
                continuation: continuation > 0,
                left: Some(DiffCell {
                    old_line: (continuation == 0).then_some(line.old_line).flatten(),
                    new_line: (continuation == 0).then_some(line.new_line).flatten(),
                    marker,
                    segments,
                }),
                right: None,
            });
        }
    }
}

fn layout_side_by_side(lines: &[DiffDisplayLine], options: LayoutOptions, rows: &mut RowCollector) {
    let pane_width = options.width.saturating_sub(1) / 2;
    let content_width = pane_width.saturating_sub(6).max(1);
    let mut hunk_index = None;
    for_each_side_by_side_pair(lines, |left, right| {
        let is_full_width = right.is_none()
            && left.is_some_and(|line| matches!(line.kind, DiffDisplayKind::HunkHeader | DiffDisplayKind::Metadata));
        if is_full_width {
            let text = left.map_or("", |line| line.text.as_str());
            if left.is_some_and(|line| line.kind == DiffDisplayKind::HunkHeader) {
                hunk_index = Some(hunk_index.map_or(0, |index| index + 1));
                rows.push(header_row(text, hunk_index));
            } else {
                rows.push(metadata_row(text, hunk_index));
            }
            return;
        }
        let left_parts = left.map_or_else(
            || vec![Vec::new()],
            |line| wrap_segments(&line.text, &line.changed, content_width, options.wrap),
        );
        let right_parts = right.map_or_else(
            || vec![Vec::new()],
            |line| wrap_segments(&line.text, &line.changed, content_width, options.wrap),
        );
        let count = left_parts.len().max(right_parts.len());
        for offset in 0..count {
            let left_cell = left.and_then(|line| {
                left_parts.get(offset).map(|segments| DiffCell {
                    old_line: (offset == 0).then_some(line.old_line).flatten(),
                    new_line: (offset == 0).then_some(line.new_line).flatten(),
                    marker: marker_for_kind(line.kind),
                    segments: segments.clone(),
                })
            });
            let right_cell = right.and_then(|line| {
                right_parts.get(offset).map(|segments| DiffCell {
                    old_line: (offset == 0).then_some(line.old_line).flatten(),
                    new_line: (offset == 0).then_some(line.new_line).flatten(),
                    marker: marker_for_kind(line.kind),
                    segments: segments.clone(),
                })
            });
            let kind = right.or(left).map_or(DiffRowKind::Context, |line| row_kind(line.kind));
            rows.push(DiffRow {
                kind,
                marker: right_cell.as_ref().or(left_cell.as_ref()).map_or(' ', |cell| cell.marker),
                hunk_index,
                continuation: offset > 0,
                left: left_cell,
                right: right_cell,
            });
        }
    });
}

fn header_row(text: &str, hunk_index: Option<usize>) -> DiffRow {
    DiffRow {
        kind: DiffRowKind::HunkHeader,
        marker: '@',
        hunk_index,
        continuation: false,
        left: Some(DiffCell {
            old_line: None,
            new_line: None,
            marker: '@',
            segments: vec![DiffSegment { text: text.to_owned(), emphasized: false }],
        }),
        right: None,
    }
}

fn metadata_row(text: &str, hunk_index: Option<usize>) -> DiffRow {
    DiffRow {
        kind: DiffRowKind::Metadata,
        marker: ' ',
        hunk_index,
        continuation: false,
        left: Some(DiffCell {
            old_line: None,
            new_line: None,
            marker: ' ',
            segments: vec![DiffSegment { text: text.to_owned(), emphasized: false }],
        }),
        right: None,
    }
}

struct RowCollector {
    head: Vec<DiffRow>,
    tail: VecDeque<DiffRow>,
    max_rows: usize,
    total_rows: usize,
    overflowed: bool,
}

impl RowCollector {
    fn new(max_rows: usize) -> Self {
        Self {
            head: Vec::new(),
            tail: VecDeque::new(),
            max_rows,
            total_rows: 0,
            overflowed: false,
        }
    }

    fn push(&mut self, row: DiffRow) {
        self.total_rows = self.total_rows.saturating_add(1);
        if self.max_rows == 0 {
            return;
        }
        if !self.overflowed {
            self.head.push(row);
            if self.head.len() <= self.max_rows {
                return;
            }

            let retained = self.max_rows.saturating_sub(1);
            let head_count = retained.saturating_add(1) / 2;
            let tail_count = retained / 2;
            if tail_count > 0 {
                let tail_start = self.head.len() - tail_count;
                self.tail = self.head.split_off(tail_start).into();
            }
            self.head.truncate(head_count);
            self.overflowed = true;
            return;
        }

        let tail_count = self.max_rows.saturating_sub(1) / 2;
        if tail_count > 0 {
            if self.tail.len() == tail_count {
                let _ = self.tail.pop_front();
            }
            self.tail.push_back(row);
        }
    }

    fn finish(mut self) -> Vec<DiffRow> {
        if !self.overflowed {
            return self.head;
        }
        let omitted = self.total_rows.saturating_sub(self.head.len()).saturating_sub(self.tail.len());
        self.head.push(omission_row(omitted));
        self.head.extend(self.tail);
        self.head
    }
}

fn omission_row(omitted: usize) -> DiffRow {
    DiffRow {
        kind: DiffRowKind::Omission,
        marker: '…',
        hunk_index: None,
        continuation: false,
        left: Some(DiffCell {
            old_line: None,
            new_line: None,
            marker: '…',
            segments: vec![DiffSegment {
                text: format!("{omitted} rows omitted"),
                emphasized: false,
            }],
        }),
        right: None,
    }
}

fn wrap_segments(text: &str, changed: &[(usize, usize)], width: usize, wrap: bool) -> Vec<Vec<DiffSegment>> {
    if !wrap || UnicodeWidthStr::width(text) <= width {
        return vec![segment_slice(text, changed, 0, text.len())];
    }
    let mut rows = Vec::new();
    let mut byte_start = 0usize;
    let mut display_width = 0usize;
    for (byte, character) in text.char_indices() {
        let char_width = UnicodeWidthChar::width(character).unwrap_or_default();
        if display_width > 0 && display_width.saturating_add(char_width) > width {
            rows.push(segment_slice(text, changed, byte_start, byte));
            byte_start = byte;
            display_width = 0;
        }
        display_width = display_width.saturating_add(char_width);
    }
    rows.push(segment_slice(text, changed, byte_start, text.len()));
    rows
}

fn segment_slice(text: &str, changed: &[(usize, usize)], start: usize, end: usize) -> Vec<DiffSegment> {
    if start == end {
        return vec![DiffSegment { text: String::new(), emphasized: false }];
    }
    let mut boundaries = vec![start, end];
    for &(range_start, range_end) in changed {
        if range_start < end && range_end > start {
            let bounded_start = range_start.max(start).min(end);
            let bounded_end = range_end.max(start).min(end);
            if text.is_char_boundary(bounded_start) && text.is_char_boundary(bounded_end) {
                boundaries.push(bounded_start);
                boundaries.push(bounded_end);
            }
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries
        .windows(2)
        .filter_map(|pair| {
            let segment_start = pair[0];
            let segment_end = pair[1];
            (segment_start < segment_end).then(|| DiffSegment {
                text: text[segment_start..segment_end].to_owned(),
                emphasized: changed
                    .iter()
                    .any(|&(range_start, range_end)| segment_start >= range_start && segment_end <= range_end),
            })
        })
        .collect()
}

fn marker_for_kind(kind: DiffDisplayKind) -> char {
    match kind {
        DiffDisplayKind::Addition => '+',
        DiffDisplayKind::Deletion => '-',
        DiffDisplayKind::HunkHeader => '@',
        DiffDisplayKind::Metadata | DiffDisplayKind::Context => ' ',
    }
}

fn row_kind(kind: DiffDisplayKind) -> DiffRowKind {
    match kind {
        DiffDisplayKind::Addition => DiffRowKind::Addition,
        DiffDisplayKind::Deletion => DiffRowKind::Deletion,
        DiffDisplayKind::HunkHeader => DiffRowKind::HunkHeader,
        DiffDisplayKind::Metadata => DiffRowKind::Metadata,
        DiffDisplayKind::Context => DiffRowKind::Context,
    }
}
