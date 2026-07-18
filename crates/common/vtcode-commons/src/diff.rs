//! Diff utilities for generating structured diffs.

use hashbrown::HashMap;
use serde::Serialize;
use std::cmp::min;

/// Represents a chunk of text in a diff (Equal, Delete, or Insert).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chunk<'a> {
    Equal(&'a str),
    Delete(&'a str),
    Insert(&'a str),
}

/// Compute an optimal diff between two strings using Myers algorithm.
#[inline]
pub fn compute_diff_chunks<'a>(old: &'a str, new: &'a str) -> Vec<Chunk<'a>> {
    if old.is_empty() && new.is_empty() {
        return Vec::with_capacity(0);
    }
    if old.is_empty() {
        return vec![Chunk::Insert(new)];
    }
    if new.is_empty() {
        return vec![Chunk::Delete(old)];
    }

    // Strip common prefix first (optimisation).
    let prefix_byte_len: usize = old
        .chars()
        .zip(new.chars())
        .take_while(|(o, n)| o == n)
        .map(|(c, _)| c.len_utf8())
        .sum();

    // Strip common suffix on the remaining text.
    let old_rest = &old[prefix_byte_len..];
    let new_rest = &new[prefix_byte_len..];

    let suffix_byte_len: usize = old_rest
        .chars()
        .rev()
        .zip(new_rest.chars().rev())
        .take_while(|(o, n)| o == n)
        .map(|(c, _)| c.len_utf8())
        .sum();

    let old_middle_end = old_rest.len() - suffix_byte_len;
    let new_middle_end = new_rest.len() - suffix_byte_len;

    let old_middle = &old_rest[..old_middle_end];
    let new_middle = &new_rest[..new_middle_end];

    let mut result = Vec::with_capacity(old_middle.len() + new_middle.len());

    // Add common prefix
    if prefix_byte_len > 0 {
        result.push(Chunk::Equal(&old[..prefix_byte_len]));
    }

    // Compute optimal diff for the middle section
    if !old_middle.is_empty() || !new_middle.is_empty() {
        let old_chars: Vec<char> = old_middle.chars().collect();
        let new_chars: Vec<char> = new_middle.chars().collect();
        let old_byte_starts: Vec<usize> = old_middle.char_indices().map(|(idx, _)| idx).collect();
        let new_byte_starts: Vec<usize> = new_middle.char_indices().map(|(idx, _)| idx).collect();
        let edits = myers_diff(&old_chars, &new_chars);

        let mut old_pos = 0;
        let mut new_pos = 0;
        // Track the start of a consecutive Equal run so we can emit a single
        // Chunk::Equal for the whole run (instead of one per character).
        let mut equal_run_start: Option<usize> = None;

        for edit in edits {
            match edit {
                Edit::Equal => {
                    if equal_run_start.is_none() {
                        equal_run_start = Some(old_pos);
                    }
                    old_pos += 1;
                    new_pos += 1;
                }
                Edit::Delete => {
                    // Flush any accumulated equal run before emitting a Delete
                    if let Some(start) = equal_run_start.take() {
                        let byte_start = old_byte_starts[start];
                        let byte_end = old_byte_starts[old_pos];
                        if byte_start < byte_end {
                            result.push(Chunk::Equal(&old_middle[byte_start..byte_end]));
                        }
                    }
                    let Some(ch) = old_chars.get(old_pos).copied() else {
                        break;
                    };
                    let Some(byte_start) = old_byte_starts.get(old_pos).copied() else {
                        break;
                    };
                    let byte_end = byte_start + ch.len_utf8();
                    result.push(Chunk::Delete(&old_middle[byte_start..byte_end]));
                    old_pos += 1;
                }
                Edit::Insert => {
                    // Flush any accumulated equal run before emitting an Insert.
                    // old_pos may equal old_byte_starts.len() when the equal run
                    // reaches the end of old_middle, so use old_middle.len() as fallback.
                    if let Some(start) = equal_run_start.take() {
                        let byte_start = old_byte_starts[start];
                        let byte_end = if old_pos < old_byte_starts.len() {
                            old_byte_starts[old_pos]
                        } else {
                            old_middle.len()
                        };
                        if byte_start < byte_end {
                            result.push(Chunk::Equal(&old_middle[byte_start..byte_end]));
                        }
                    }
                    let Some(ch) = new_chars.get(new_pos).copied() else {
                        break;
                    };
                    let Some(byte_start) = new_byte_starts.get(new_pos).copied() else {
                        break;
                    };
                    let byte_end = byte_start + ch.len_utf8();
                    result.push(Chunk::Insert(&new_middle[byte_start..byte_end]));
                    new_pos += 1;
                }
            }
        }
        // Flush any trailing equal run
        if let Some(start) = equal_run_start.take() {
            let byte_start = old_byte_starts[start];
            let byte_end = old_middle.len();
            if byte_start < byte_end {
                result.push(Chunk::Equal(&old_middle[byte_start..byte_end]));
            }
        }
    }

    // Add common suffix
    if suffix_byte_len > 0 {
        result.push(Chunk::Equal(&old[old.len() - suffix_byte_len..]));
    }

    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    Equal,
    Delete,
    Insert,
}

/// Advance along matching characters. Extracted from `myers_diff` so the
/// compiler sees a tight leaf loop with no surrounding state, enabling better
/// register allocation and (in some cases) auto-vectorization heuristics.
#[inline]
fn advance_matching(old: &[char], new: &[char], mut x: usize, mut y: usize) -> (usize, usize) {
    while x < old.len() && y < new.len() && old[x] == new[y] {
        x += 1;
        y += 1;
    }
    (x, y)
}

/// Erase the trailing equal run during backtracking. Same rationale as
/// `advance_matching` — a focused leaf function that the compiler can
/// optimise in isolation.
/// Returns the final `(x, y)` position after removing equal edits.
#[inline]
fn backtrack_equal_run(
    mut x: usize,
    mut y: usize,
    move_x: usize,
    move_y: usize,
    edits: &mut Vec<Edit>,
) -> (usize, usize) {
    while x > move_x && y > move_y {
        edits.push(Edit::Equal);
        x -= 1;
        y -= 1;
    }
    (x, y)
}

#[allow(clippy::cast_sign_loss)]
fn myers_diff(old: &[char], new: &[char]) -> Vec<Edit> {
    let n = old.len();
    let m = new.len();

    if n == 0 {
        return vec![Edit::Insert; m];
    }
    if m == 0 {
        return vec![Edit::Delete; n];
    }

    let max_d = n.saturating_add(m).min(i32::MAX as usize);
    let max_d_i32 = max_d as i32;
    let mut v = vec![0; 2 * max_d + 1];
    let mut v_index = vec![0usize; (max_d + 1) * (2 * max_d + 1)];
    let row_len = 2 * max_d + 1;

    v[max_d] = 0;

    for d in 0..=max_d {
        let d_i32 = d as i32;
        let row_start = d * row_len;
        for k in (-d_i32..=d_i32).step_by(2) {
            let k_idx = (k + max_d_i32) as usize;

            let x = if k == -d_i32 || (k != d_i32 && v[k_idx - 1] < v[k_idx + 1]) {
                v[k_idx + 1]
            } else {
                v[k_idx - 1] + 1
            };

            let mut x = x;
            let mut y = (x as i32 - k) as usize;

            (x, y) = advance_matching(old, new, x, y);

            v[k_idx] = x;
            v_index[row_start + k_idx] = x;

            if x >= n && y >= m {
                return backtrack_myers(old, new, &v_index, d, k, max_d);
            }
        }
    }

    vec![]
}

#[allow(clippy::cast_sign_loss)]
fn backtrack_myers(old: &[char], new: &[char], v_index: &[usize], d: usize, mut k: i32, max_d: usize) -> Vec<Edit> {
    let mut edits = Vec::with_capacity(old.len() + new.len());
    let mut x = old.len();
    let mut y = new.len();
    let max_d_i32 = max_d as i32;
    let row_len = 2 * max_d + 1;

    for cur_d in (0..=d).rev() {
        if cur_d == 0 {
            while x > 0 && y > 0 {
                edits.push(Edit::Equal);
                x -= 1;
                y -= 1;
            }
            break;
        }

        let k_idx = (k + max_d_i32) as usize;
        let prev_row_start = (cur_d - 1) * row_len;

        let cur_d_i32 = cur_d as i32;
        let prev_k = if k == cur_d_i32.wrapping_neg()
            || (k != cur_d_i32 && v_index[prev_row_start + k_idx - 1] < v_index[prev_row_start + k_idx + 1])
        {
            k + 1
        } else {
            k - 1
        };

        let prev_k_idx = (prev_k + max_d_i32) as usize;
        let prev_x_val = v_index[prev_row_start + prev_k_idx];
        let prev_y = (prev_x_val as i32 - prev_k) as usize;

        let (move_x, move_y) = if prev_k == k + 1 {
            (prev_x_val, prev_y + 1)
        } else {
            (prev_x_val + 1, prev_y)
        };

        (x, y) = backtrack_equal_run(x, y, move_x, move_y, &mut edits);

        if prev_k == k + 1 {
            edits.push(Edit::Insert);
            y -= 1;
        } else {
            edits.push(Edit::Delete);
            x -= 1;
        }

        k = prev_k;
    }

    edits.reverse();
    edits
}

/// Options for diff generation.
#[derive(Debug, Clone)]
pub struct DiffOptions<'a> {
    pub context_lines: usize,
    pub old_label: Option<&'a str>,
    pub new_label: Option<&'a str>,
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
    pub hunks: Vec<DiffHunk>,
    pub formatted: String,
    pub is_empty: bool,
}

/// A diff hunk with metadata for old/new ranges.
#[derive(Debug, Clone, Serialize)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub lines: Vec<DiffLine>,
}

/// A single diff line annotated with metadata and type.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
}

/// Metadata for a single line inside a diff hunk.
#[derive(Debug, Clone, Serialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub text: String,
}

/// Compute a structured diff bundle.
pub fn compute_diff<F>(old: &str, new: &str, options: DiffOptions<'_>, formatter: F) -> DiffBundle
where
    F: FnOnce(&[DiffHunk], &DiffOptions<'_>) -> String,
{
    let old_lines_owned = split_lines_with_terminator(old);
    let new_lines_owned = split_lines_with_terminator(new);

    let old_refs: Vec<&str> = old_lines_owned.iter().map(|s| s.as_str()).collect();
    let new_refs: Vec<&str> = new_lines_owned.iter().map(|s| s.as_str()).collect();

    let records = collect_line_records(&old_refs, &new_refs);
    let has_changes = records
        .iter()
        .any(|record| matches!(record.kind, DiffLineKind::Addition | DiffLineKind::Deletion));

    let hunks = if has_changes {
        build_hunks(&records, options.context_lines)
    } else {
        Vec::new()
    };

    let formatted = if hunks.is_empty() {
        String::new()
    } else {
        formatter(&hunks, &options)
    };

    DiffBundle { hunks, formatted, is_empty: !has_changes }
}

fn split_lines_with_terminator(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::with_capacity(0);
    }

    let mut lines: Vec<String> = text.split_inclusive('\n').map(|line| line.to_string()).collect();

    if lines.is_empty() {
        lines.push(text.to_string());
    }

    lines
}

#[inline]
fn collect_line_records<'a>(old_lines: &'a [&'a str], new_lines: &'a [&'a str]) -> Vec<LineRecord<'a>> {
    let (old_encoded, new_encoded) = encode_line_sequences(old_lines, new_lines);
    let mut records = Vec::with_capacity(old_lines.len() + new_lines.len());
    let mut old_index = 0u32;
    let mut new_index = 0u32;

    for chunk in compute_diff_chunks(old_encoded.as_str(), new_encoded.as_str()) {
        match chunk {
            Chunk::Equal(text) => {
                for _ in text.chars() {
                    let old_line = old_index + 1;
                    let new_line = new_index + 1;
                    let line = old_lines[old_index as usize];
                    records.push(LineRecord {
                        kind: DiffLineKind::Context,
                        old_line: Some(old_line),
                        new_line: Some(new_line),
                        text: line,
                        anchor_old: old_line,
                        anchor_new: new_line,
                    });
                    old_index += 1;
                    new_index += 1;
                }
            }
            Chunk::Delete(text) => {
                for _ in text.chars() {
                    let old_line = old_index + 1;
                    let anchor_new = new_index + 1;
                    let line = old_lines[old_index as usize];
                    records.push(LineRecord {
                        kind: DiffLineKind::Deletion,
                        old_line: Some(old_line),
                        new_line: None,
                        text: line,
                        anchor_old: old_line,
                        anchor_new,
                    });
                    old_index += 1;
                }
            }
            Chunk::Insert(text) => {
                for _ in text.chars() {
                    let new_line = new_index + 1;
                    let anchor_old = old_index + 1;
                    let line = new_lines[new_index as usize];
                    records.push(LineRecord {
                        kind: DiffLineKind::Addition,
                        old_line: None,
                        new_line: Some(new_line),
                        text: line,
                        anchor_old,
                        anchor_new: new_line,
                    });
                    new_index += 1;
                }
            }
        }
    }

    records
}

fn encode_line_sequences<'a>(old_lines: &'a [&'a str], new_lines: &'a [&'a str]) -> (String, String) {
    let mut token_map: HashMap<&'a str, char> = HashMap::new();
    let mut next_codepoint: u32 = 0;

    let old_encoded = encode_line_list(old_lines, &mut token_map, &mut next_codepoint);
    let new_encoded = encode_line_list(new_lines, &mut token_map, &mut next_codepoint);

    (old_encoded, new_encoded)
}

fn encode_line_list<'a>(lines: &'a [&'a str], map: &mut HashMap<&'a str, char>, next_codepoint: &mut u32) -> String {
    let mut encoded = String::with_capacity(lines.len());
    for &line in lines {
        let token = if let Some(&value) = map.get(line) {
            value
        } else {
            let Some(ch) = next_token_char(next_codepoint) else {
                break;
            };
            map.insert(line, ch);
            ch
        };
        encoded.push(token);
    }
    encoded
}

fn next_token_char(counter: &mut u32) -> Option<char> {
    while *counter <= 0x10FFFF {
        let candidate = *counter;
        *counter += 1;
        if (0xD800..=0xDFFF).contains(&candidate) {
            continue;
        }
        if let Some(ch) = char::from_u32(candidate) {
            return Some(ch);
        }
    }
    None
}

#[derive(Debug)]
struct LineRecord<'a> {
    kind: DiffLineKind,
    old_line: Option<u32>,
    new_line: Option<u32>,
    text: &'a str,
    anchor_old: u32,
    anchor_new: u32,
}

fn build_hunks(records: &[LineRecord<'_>], context: usize) -> Vec<DiffHunk> {
    if records.is_empty() {
        return Vec::new();
    }

    let ranges = compute_hunk_ranges(records, context);
    let mut hunks = Vec::with_capacity(ranges.len());

    for (start, end) in ranges {
        let slice = &records[start..=end];

        let old_start = slice
            .iter()
            .filter_map(|r| r.old_line)
            .min()
            .or_else(|| slice.iter().map(|r| r.anchor_old).min())
            .unwrap_or(1) as usize;

        let new_start = slice
            .iter()
            .filter_map(|r| r.new_line)
            .min()
            .or_else(|| slice.iter().map(|r| r.anchor_new).min())
            .unwrap_or(1) as usize;

        let old_lines = slice
            .iter()
            .filter(|r| matches!(r.kind, DiffLineKind::Context | DiffLineKind::Deletion))
            .count();
        let new_lines = slice
            .iter()
            .filter(|r| matches!(r.kind, DiffLineKind::Context | DiffLineKind::Addition))
            .count();

        let lines = slice
            .iter()
            .map(|record| DiffLine {
                kind: record.kind,
                old_line: record.old_line,
                new_line: record.new_line,
                text: record.text.to_string(),
            })
            .collect();

        hunks.push(DiffHunk { old_start, old_lines, new_start, new_lines, lines });
    }

    hunks
}

fn compute_hunk_ranges(records: &[LineRecord<'_>], context: usize) -> Vec<(usize, usize)> {
    let mut ranges = Vec::with_capacity(4);
    let mut current_start: Option<usize> = None;
    let mut current_end: usize = 0;

    for (idx, record) in records.iter().enumerate() {
        if record.kind != DiffLineKind::Context {
            let start = idx.saturating_sub(context);
            let end = min(idx + context, records.len().saturating_sub(1));

            if let Some(existing_start) = current_start {
                // Close the previous range if this change is beyond its context window
                if idx > current_end {
                    ranges.push((existing_start, current_end));
                    current_start = Some(start);
                    current_end = end;
                } else {
                    if start < existing_start {
                        current_start = Some(start);
                    }
                    if end > current_end {
                        current_end = end;
                    }
                }
            } else {
                current_start = Some(start);
                current_end = end;
            }
        } else if let Some(start) = current_start
            && idx > current_end
        {
            ranges.push((start, current_end));
            current_start = None;
        }
    }

    if let Some(start) = current_start {
        ranges.push((start, current_end));
    }

    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── compute_diff_chunks ──────────────────────────────────────────

    #[test]
    fn chunks_both_empty() {
        let chunks = compute_diff_chunks("", "");
        assert!(chunks.is_empty());
    }

    #[test]
    fn chunks_old_empty() {
        let chunks = compute_diff_chunks("", "hello");
        assert_eq!(chunks, vec![Chunk::Insert("hello")]);
    }

    #[test]
    fn chunks_new_empty() {
        let chunks = compute_diff_chunks("hello", "");
        assert_eq!(chunks, vec![Chunk::Delete("hello")]);
    }

    #[test]
    fn chunks_identical() {
        let chunks = compute_diff_chunks("abc", "abc");
        assert_eq!(chunks.len(), 1);
        assert!(matches!(chunks[0], Chunk::Equal("abc")));
    }

    #[test]
    fn chunks_single_insertion() {
        let chunks = compute_diff_chunks("ac", "abc");
        // Common prefix "a", insert "b", common suffix "c"
        assert_eq!(chunks.len(), 3);
        assert!(matches!(chunks[0], Chunk::Equal("a")));
        assert!(matches!(chunks[1], Chunk::Insert("b")));
        assert!(matches!(chunks[2], Chunk::Equal("c")));
    }

    #[test]
    fn chunks_single_deletion() {
        let chunks = compute_diff_chunks("abc", "ac");
        assert_eq!(chunks.len(), 3);
        assert!(matches!(chunks[0], Chunk::Equal("a")));
        assert!(matches!(chunks[1], Chunk::Delete("b")));
        assert!(matches!(chunks[2], Chunk::Equal("c")));
    }

    #[test]
    fn chunks_replacement() {
        let chunks = compute_diff_chunks("abc", "axc");
        // Equal("a"), Delete("b"), Insert("x"), Equal("c")
        assert_eq!(chunks.len(), 4);
        assert!(matches!(chunks[0], Chunk::Equal("a")));
        assert!(matches!(chunks[1], Chunk::Delete("b")));
        assert!(matches!(chunks[2], Chunk::Insert("x")));
        assert!(matches!(chunks[3], Chunk::Equal("c")));
    }

    #[test]
    fn chunks_completely_different() {
        let chunks = compute_diff_chunks("aaa", "bbb");
        // No common prefix or suffix
        assert!(!chunks.is_empty());
        // All old chars deleted, all new chars inserted
        let deletes: usize = chunks.iter().filter(|c| matches!(c, Chunk::Delete(_))).count();
        let inserts: usize = chunks.iter().filter(|c| matches!(c, Chunk::Insert(_))).count();
        assert!(deletes > 0 || inserts > 0);
    }

    #[test]
    fn chunks_multiline() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nline modified\nline3\n";
        let chunks = compute_diff_chunks(old, new);

        // Should have at least some Equal chunks for the unchanged lines
        let has_equal = chunks.iter().any(|c| matches!(c, Chunk::Equal(_)));
        assert!(has_equal);

        // Should have a delete and insert for the changed line
        let has_delete = chunks.iter().any(|c| matches!(c, Chunk::Delete(_)));
        let has_insert = chunks.iter().any(|c| matches!(c, Chunk::Insert(_)));
        assert!(has_delete || has_insert);
    }

    #[test]
    fn chunks_unicode() {
        let old = "hello \u{00e9}l\u{00e8}ve";
        let new = "hello \u{00e9}l\u{00e8}ve you";
        let chunks = compute_diff_chunks(old, new);

        // Common prefix should include unicode chars
        let prefix = match &chunks[0] {
            Chunk::Equal(s) => s,
            _ => panic!("expected Equal prefix"),
        };
        assert!(prefix.starts_with("hello "));
    }

    #[test]
    fn chunks_append_only() {
        let old = "a\nb\n";
        let new = "a\nb\nc\nd\n";
        let chunks = compute_diff_chunks(old, new);
        let has_insert = chunks.iter().any(|c| matches!(c, Chunk::Insert(_)));
        assert!(has_insert);
    }

    #[test]
    fn chunks_remove_only() {
        let old = "a\nb\nc\n";
        let new = "a\n";
        let chunks = compute_diff_chunks(old, new);
        let has_delete = chunks.iter().any(|c| matches!(c, Chunk::Delete(_)));
        assert!(has_delete);
    }

    // ── compute_diff ─────────────────────────────────────────────────

    fn identity_formatter(hunks: &[DiffHunk], _opts: &DiffOptions<'_>) -> String {
        hunks
            .iter()
            .flat_map(|h| h.lines.iter().map(|l| l.text.clone()))
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn diff_identical_content() {
        let result = compute_diff("hello\n", "hello\n", DiffOptions::default(), identity_formatter);
        assert!(result.is_empty);
        assert!(result.hunks.is_empty());
        assert!(result.formatted.is_empty());
    }

    #[test]
    fn diff_empty_both() {
        let result = compute_diff("", "", DiffOptions::default(), identity_formatter);
        assert!(result.is_empty);
        assert!(result.hunks.is_empty());
    }

    #[test]
    fn diff_old_empty() {
        let result = compute_diff("", "line1\nline2\n", DiffOptions::default(), identity_formatter);
        assert!(!result.is_empty);
        assert!(!result.hunks.is_empty());
        // All lines should be additions
        for hunk in &result.hunks {
            for line in &hunk.lines {
                assert_eq!(line.kind, DiffLineKind::Addition);
            }
        }
    }

    #[test]
    fn diff_new_empty() {
        let result = compute_diff("line1\nline2\n", "", DiffOptions::default(), identity_formatter);
        assert!(!result.is_empty);
        assert!(!result.hunks.is_empty());
        for hunk in &result.hunks {
            for line in &hunk.lines {
                assert_eq!(line.kind, DiffLineKind::Deletion);
            }
        }
    }

    #[test]
    fn diff_single_line_change() {
        let old = "aaa\nbbb\nccc\n";
        let new = "aaa\nxxx\nccc\n";
        let result = compute_diff(old, new, DiffOptions::default(), identity_formatter);

        assert!(!result.is_empty);
        assert_eq!(result.hunks.len(), 1);

        let hunk = &result.hunks[0];
        // Should have context lines for aaa and ccc, plus the change
        let kinds: Vec<DiffLineKind> = hunk.lines.iter().map(|l| l.kind).collect();
        assert!(kinds.contains(&DiffLineKind::Context));
        assert!(kinds.contains(&DiffLineKind::Deletion));
        assert!(kinds.contains(&DiffLineKind::Addition));
    }

    #[test]
    fn diff_line_numbers() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nline2 modified\nline3\n";
        let result = compute_diff(old, new, DiffOptions::default(), identity_formatter);

        let hunk = &result.hunks[0];
        // Context lines should have both old_line and new_line
        for line in &hunk.lines {
            if line.kind == DiffLineKind::Context {
                assert!(line.old_line.is_some());
                assert!(line.new_line.is_some());
            }
        }
        // Deletion should have old_line but no new_line
        for line in &hunk.lines {
            if line.kind == DiffLineKind::Deletion {
                assert!(line.old_line.is_some());
                assert!(line.new_line.is_none());
            }
        }
        // Addition should have new_line but no old_line
        for line in &hunk.lines {
            if line.kind == DiffLineKind::Addition {
                assert!(line.old_line.is_none());
                assert!(line.new_line.is_some());
            }
        }
    }

    #[test]
    fn diff_context_lines_zero() {
        let old = "a\nb\nc\nd\ne\n";
        let new = "a\nb\nX\nd\ne\n";
        let opts = DiffOptions { context_lines: 0, ..DiffOptions::default() };
        let result = compute_diff(old, new, opts, identity_formatter);

        assert!(!result.is_empty);
        // With 0 context, only the changed line and its neighbors should appear
        let hunk = &result.hunks[0];
        // Should be minimal: just the deletion and addition
        let context_count = hunk.lines.iter().filter(|l| l.kind == DiffLineKind::Context).count();
        assert!(context_count <= 2); // At most one context line on each side
    }

    #[test]
    fn diff_context_lines_large() {
        let old = "a\nb\nc\nd\ne\n";
        let new = "a\nb\nX\nd\ne\n";
        let opts = DiffOptions { context_lines: 10, ..DiffOptions::default() };
        let result = compute_diff(old, new, opts, identity_formatter);

        assert!(!result.is_empty);
        // With 10 context lines and only 6 total lines (trailing newline creates 6th), all lines appear
        let hunk = &result.hunks[0];
        assert_eq!(hunk.lines.len(), 6);
    }

    #[test]
    fn diff_hunk_metadata() {
        let old = "aaa\nbbb\nccc\n";
        let new = "aaa\nxxx\nccc\n";
        let result = compute_diff(old, new, DiffOptions::default(), identity_formatter);

        let hunk = &result.hunks[0];
        assert!(hunk.old_start >= 1);
        assert!(hunk.new_start >= 1);
        assert!(hunk.old_lines > 0);
        assert!(hunk.new_lines > 0);
        assert!(!hunk.lines.is_empty());
    }

    #[test]
    fn diff_multiple_hunks() {
        // Insert in first half and insert in second half with small context => two hunks
        let old = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let new = "a\nINSERTED1\nb\nc\nd\ne\nf\ng\nINSERTED2\nh\n";
        let opts = DiffOptions { context_lines: 1, ..DiffOptions::default() };
        let result = compute_diff(old, new, opts, identity_formatter);

        assert!(!result.is_empty);
        assert!(result.hunks.len() >= 2, "expected at least 2 hunks, got {}", result.hunks.len());
    }

    #[test]
    fn diff_formatter_called() {
        let old = "aaa\n";
        let new = "bbb\n";
        let mut called = false;
        let formatter = |hunks: &[DiffHunk], _opts: &DiffOptions<'_>| -> String {
            called = true;
            hunks
                .iter()
                .flat_map(|h| h.lines.iter().map(|l| l.text.clone()))
                .collect::<Vec<_>>()
                .join("")
        };

        let result = compute_diff(old, new, DiffOptions::default(), formatter);
        assert!(called);
        assert!(!result.formatted.is_empty());
    }

    #[test]
    fn diff_formatter_not_called_when_empty() {
        let mut called = false;
        let formatter = |_hunks: &[DiffHunk], _opts: &DiffOptions<'_>| -> String {
            called = true;
            String::new()
        };

        let result = compute_diff("same\n", "same\n", DiffOptions::default(), formatter);
        assert!(!called);
        assert!(result.formatted.is_empty());
    }

    #[test]
    fn diff_options_labels() {
        let old = "aaa\n";
        let new = "bbb\n";
        let opts = DiffOptions {
            old_label: Some("old.txt"),
            new_label: Some("new.txt"),
            ..DiffOptions::default()
        };
        let result = compute_diff(old, new, opts, identity_formatter);
        assert!(!result.is_empty);
        // Labels are passed to formatter but don't affect hunks
        assert_eq!(result.hunks.len(), 1);
    }

    #[test]
    fn diff_insertion_only() {
        let old = "line1\nline3\n";
        let new = "line1\nline2\nline3\n";
        let result = compute_diff(old, new, DiffOptions::default(), identity_formatter);

        assert!(!result.is_empty);
        let additions: Vec<&DiffLine> = result
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.kind == DiffLineKind::Addition)
            .collect();
        assert_eq!(additions.len(), 1);
        assert_eq!(additions[0].text, "line2\n");
    }

    #[test]
    fn diff_deletion_only() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nline3\n";
        let result = compute_diff(old, new, DiffOptions::default(), identity_formatter);

        assert!(!result.is_empty);
        let deletions: Vec<&DiffLine> = result
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.kind == DiffLineKind::Deletion)
            .collect();
        assert_eq!(deletions.len(), 1);
        assert_eq!(deletions[0].text, "line2\n");
    }

    // ── DiffBundle serialization ─────────────────────────────────────

    #[test]
    fn diff_bundle_serializes() {
        let old = "aaa\nbbb\n";
        let new = "aaa\nxxx\n";
        let result = compute_diff(old, new, DiffOptions::default(), identity_formatter);

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("hunks"));
        assert!(json.contains("formatted"));
        assert!(json.contains("is_empty"));
    }

    #[test]
    fn diff_hunk_serializes() {
        let hunk = DiffHunk {
            old_start: 1,
            old_lines: 2,
            new_start: 1,
            new_lines: 2,
            lines: vec![DiffLine {
                kind: DiffLineKind::Context,
                old_line: Some(1),
                new_line: Some(1),
                text: "hello\n".to_string(),
            }],
        };
        let json = serde_json::to_string(&hunk).unwrap();
        assert!(json.contains("old_start"));
        assert!(json.contains("context"));
    }

    #[test]
    fn diff_line_kind_serializes() {
        assert_eq!(serde_json::to_string(&DiffLineKind::Context).unwrap(), "\"context\"");
        assert_eq!(serde_json::to_string(&DiffLineKind::Addition).unwrap(), "\"addition\"");
        assert_eq!(serde_json::to_string(&DiffLineKind::Deletion).unwrap(), "\"deletion\"");
    }

    // ── Edge cases ───────────────────────────────────────────────────

    #[test]
    fn chunks_very_long_identical() {
        let text = "x".repeat(10_000);
        let chunks = compute_diff_chunks(&text, &text);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(chunks[0], Chunk::Equal(_)));
    }

    #[test]
    fn chunks_single_char_diff() {
        let chunks = compute_diff_chunks("a", "b");
        assert!(!chunks.is_empty());
        let has_delete = chunks.iter().any(|c| matches!(c, Chunk::Delete(_)));
        let has_insert = chunks.iter().any(|c| matches!(c, Chunk::Insert(_)));
        assert!(has_delete && has_insert);
    }

    #[test]
    fn diff_no_trailing_newline() {
        let old = "line1\nline2";
        let new = "line1\nline2\n";
        let result = compute_diff(old, new, DiffOptions::default(), identity_formatter);
        assert!(!result.is_empty);
    }

    #[test]
    fn diff_only_newlines_differ() {
        let old = "a\nb\n";
        let new = "a\nb";
        let result = compute_diff(old, new, DiffOptions::default(), identity_formatter);
        assert!(!result.is_empty);
    }

    #[test]
    fn chunks_prefix_suffix_optimization() {
        // Verify that common prefix and suffix are preserved as Equal chunks.
        // Myers works character-by-character, so the middle diff is char-level.
        let old = "AAAA BBBB CCCC";
        let new = "AAAA DDDD CCCC";
        let chunks = compute_diff_chunks(old, new);

        // First chunk should be Equal prefix "AAAA "
        assert!(matches!(&chunks[0], Chunk::Equal(s) if *s == "AAAA "));
        // Last chunk should be Equal suffix " CCCC"
        assert!(matches!(chunks.last().unwrap(), Chunk::Equal(s) if *s == " CCCC"));
        // Middle should contain deletes and inserts (character-level)
        let has_delete = chunks.iter().any(|c| matches!(c, Chunk::Delete(_)));
        let has_insert = chunks.iter().any(|c| matches!(c, Chunk::Insert(_)));
        assert!(has_delete, "expected Delete chunks in middle");
        assert!(has_insert, "expected Insert chunks in middle");
    }
}
