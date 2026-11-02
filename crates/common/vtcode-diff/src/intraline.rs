//! Bounded character- and word-level intraline refinement.

use std::time::{Duration, Instant};

use similar::{ChangeTag, TextDiff};

use super::types::{Chunk, DiffDisplayKind, DiffDisplayLine, WordChangedRanges, default_inline_timeout};

/// Computes a character-level diff with adjacent chunks coalesced.
///
/// # Contract
///
/// - Requires: `old` and `new` are valid UTF-8 `&str` (enforced by the type).
/// - Ensures: every returned [`Chunk`] borrows a char-boundary slice of its
///   side, and concatenating Equal+Delete reconstructs `old` while
///   Equal+Insert reconstructs `new`.
/// - Termination: iterates `similar` changes once (variant: remaining changes).
#[must_use]
pub fn compute_diff_chunks<'a>(old: &'a str, new: &'a str) -> Vec<Chunk<'a>> {
    if old == new {
        return (!old.is_empty()).then_some(Chunk::Equal(old)).into_iter().collect();
    }
    let diff = TextDiff::configure()
        .algorithm(similar::Algorithm::Myers)
        .timeout(Duration::from_millis(200))
        .diff_chars(old, new);
    let mut chunks = Vec::new();
    let mut old_offset = 0usize;
    let mut new_offset = 0usize;
    let mut run: Option<(ChangeTag, usize, usize)> = None;

    for change in diff.iter_all_changes() {
        let value = change.value();
        let byte_len = value.len();
        let (start, end) = match change.tag() {
            ChangeTag::Equal | ChangeTag::Delete => (old_offset, old_offset.saturating_add(byte_len)),
            ChangeTag::Insert => (new_offset, new_offset.saturating_add(byte_len)),
        };
        if let Some((tag, run_start, run_end)) = run {
            if tag == change.tag() && run_end == start {
                run = Some((tag, run_start, end));
            } else {
                push_chunk(&mut chunks, tag, run_start, run_end, old, new);
                run = Some((change.tag(), start, end));
            }
        } else {
            run = Some((change.tag(), start, end));
        }
        match change.tag() {
            ChangeTag::Equal => {
                old_offset = old_offset.saturating_add(byte_len);
                new_offset = new_offset.saturating_add(byte_len);
            }
            ChangeTag::Delete => old_offset = old_offset.saturating_add(byte_len),
            ChangeTag::Insert => new_offset = new_offset.saturating_add(byte_len),
        }
    }
    if let Some((tag, start, end)) = run {
        push_chunk(&mut chunks, tag, start, end, old, new);
    }
    chunks
}

fn push_chunk<'a>(chunks: &mut Vec<Chunk<'a>>, tag: ChangeTag, start: usize, end: usize, old: &'a str, new: &'a str) {
    // Invariant (Creusot requires): `start..end` must be a char-boundary
    // slice of the selected side. Offsets are built by summing `value.len()`
    // partitions of `old`/`new`, so contiguity holds; debug-assert it so a
    // violated invariant crashes loud in release (debug-assertions stay on).
    debug_assert!(start <= end, "chunk range must not be inverted");
    let chunk = match tag {
        ChangeTag::Equal => {
            debug_assert!(old.is_char_boundary(start) && old.is_char_boundary(end));
            Chunk::Equal(&old[start..end])
        }
        ChangeTag::Delete => {
            debug_assert!(old.is_char_boundary(start) && old.is_char_boundary(end));
            Chunk::Delete(&old[start..end])
        }
        ChangeTag::Insert => {
            debug_assert!(new.is_char_boundary(start) && new.is_char_boundary(end));
            Chunk::Insert(&new[start..end])
        }
    };
    chunks.push(chunk);
}
/// Adds byte-safe intraline ranges to consecutive deletion/addition groups.
pub fn annotate_word_level_diffs(lines: &mut [DiffDisplayLine]) {
    annotate_word_level_diffs_with_timeout(lines, default_inline_timeout());
}

pub(crate) fn annotate_word_level_diffs_with_timeout(lines: &mut [DiffDisplayLine], timeout: Duration) {
    // Binary content (NUL bytes) makes word-level refinement meaningless and
    // expensive; skip it entirely so intraline work stays within budget.
    if lines.iter().any(|line| line.text.as_bytes().contains(&0)) {
        return;
    }
    let deadline = Instant::now().checked_add(timeout);
    let mut index = 0usize;
    while index < lines.len() {
        if lines[index].kind != DiffDisplayKind::Deletion {
            index += 1;
            continue;
        }
        let delete_start = index;
        while index < lines.len() && lines[index].kind == DiffDisplayKind::Deletion {
            index += 1;
        }
        let insert_start = index;
        while index < lines.len() && lines[index].kind == DiffDisplayKind::Addition {
            index += 1;
        }
        let pair_count = (insert_start - delete_start).min(index - insert_start);
        for offset in 0..pair_count {
            if deadline.is_some_and(|limit| Instant::now() >= limit) {
                return;
            }
            let (old_ranges, new_ranges) =
                word_level_changed_ranges(&lines[delete_start + offset].text, &lines[insert_start + offset].text);
            lines[delete_start + offset].changed = old_ranges;
            lines[insert_start + offset].changed = new_ranges;
        }
    }
}

/// Computes byte-safe word-level changed ranges for a line pair.
///
/// # Contract
///
/// - Requires: nothing beyond valid `&str` inputs.
/// - Ensures: every returned range is a char-boundary range within its side,
///   and an empty input or an over-budget pair (`> 16_384` bytes) yields no
///   ranges. Overflow-free: offsets advance with `saturating_add`.
/// - Termination: single pass over `similar` changes.
#[must_use]
pub fn word_level_changed_ranges(old: &str, new: &str) -> (WordChangedRanges, WordChangedRanges) {
    if old.is_empty() || new.is_empty() || old.len().saturating_add(new.len()) > 16_384 {
        return (Vec::new(), Vec::new());
    }
    let diff = TextDiff::configure()
        .algorithm(similar::Algorithm::Myers)
        .timeout(Duration::from_millis(10))
        .diff_unicode_words(old, new);
    if diff.ratio() < 0.35 {
        return (Vec::new(), Vec::new());
    }
    let mut old_offset = 0usize;
    let mut new_offset = 0usize;
    let mut old_ranges = Vec::new();
    let mut new_ranges = Vec::new();
    for change in diff.iter_all_changes() {
        let length = change.value().len();
        match change.tag() {
            ChangeTag::Equal => {
                old_offset = old_offset.saturating_add(length);
                new_offset = new_offset.saturating_add(length);
            }
            ChangeTag::Delete => {
                push_range(&mut old_ranges, old_offset, old_offset.saturating_add(length));
                old_offset = old_offset.saturating_add(length);
            }
            ChangeTag::Insert => {
                push_range(&mut new_ranges, new_offset, new_offset.saturating_add(length));
                new_offset = new_offset.saturating_add(length);
            }
        }
    }
    (old_ranges, new_ranges)
}

fn push_range(ranges: &mut WordChangedRanges, start: usize, end: usize) {
    // Invariant: ranges stay ordered and coalesced (`prior_end == start`
    // extends instead of pushing). Callers pass saturating offsets.
    debug_assert!(start <= end, "intraline range must not be inverted");
    match ranges.last_mut() {
        Some((_, prior_end)) if *prior_end == start => *prior_end = end,
        _ => ranges.push((start, end)),
    }
}
