//! Diff utilities for generating structured diffs.

use anstyle::Reset;
use serde::Serialize;
use std::cmp::min;
use std::collections::HashMap;
use std::fmt::Write;

use crate::ui::theme;

/// Represents a chunk of text in a diff (Equal, Delete, or Insert).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chunk<'a> {
    Equal(&'a str),
    Delete(&'a str),
    Insert(&'a str),
}

/// Compute an optimal diff between two strings using Myers algorithm.
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

    // Strip common prefix and suffix first (optimization)
    let mut prefix_len = 0;
    for (o, n) in old.chars().zip(new.chars()) {
        if o == n {
            prefix_len += o.len_utf8();
        } else {
            break;
        }
    }

    let mut suffix_len = 0;
    let old_rest = &old[prefix_len..];
    let new_rest = &new[prefix_len..];

    let old_chars: Vec<char> = old_rest.chars().collect();
    let new_chars: Vec<char> = new_rest.chars().collect();

    for (o, n) in old_chars.iter().rev().zip(new_chars.iter().rev()) {
        if o == n {
            suffix_len += o.len_utf8();
        } else {
            break;
        }
    }

    let old_middle_end = old_rest.len() - suffix_len;
    let new_middle_end = new_rest.len() - suffix_len;

    let old_middle = &old_rest[..old_middle_end];
    let new_middle = &new_rest[..new_middle_end];

    let mut result = Vec::with_capacity(old_middle.len() + new_middle.len());

    // Add common prefix
    if prefix_len > 0 {
        result.push(Chunk::Equal(&old[..prefix_len]));
    }

    // Compute optimal diff for the middle section
    if !old_middle.is_empty() || !new_middle.is_empty() {
        let old_chars: Vec<char> = old_middle.chars().collect();
        let new_chars: Vec<char> = new_middle.chars().collect();
        let edits = myers_diff(&old_chars, &new_chars);

        let mut old_pos = 0;
        let mut new_pos = 0;

        for edit in edits {
            match edit {
                Edit::Equal => {
                    old_pos += 1;
                    new_pos += 1;
                }
                Edit::Delete => {
                    let ch = old_chars[old_pos];
                    let byte_start = old_middle.char_indices().nth(old_pos).unwrap().0;
                    let byte_end = byte_start + ch.len_utf8();
                    result.push(Chunk::Delete(&old_middle[byte_start..byte_end]));
                    old_pos += 1;
                }
                Edit::Insert => {
                    let ch = new_chars[new_pos];
                    let byte_start = new_middle.char_indices().nth(new_pos).unwrap().0;
                    let byte_end = byte_start + ch.len_utf8();
                    result.push(Chunk::Insert(&new_middle[byte_start..byte_end]));
                    new_pos += 1;
                }
            }
        }
    }

    // Add common suffix
    if suffix_len > 0 {
        result.push(Chunk::Equal(&old[old.len() - suffix_len..]));
    }

    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    Equal,
    Delete,
    Insert,
}

fn myers_diff(old: &[char], new: &[char]) -> Vec<Edit> {
    let n = old.len();
    let m = new.len();

    if n == 0 {
        return vec![Edit::Insert; m];
    }
    if m == 0 {
        return vec![Edit::Delete; n];
    }

    let max_d = n + m;
    let mut v = vec![0; 2 * max_d + 1];
    let mut v_index = vec![0usize; (max_d + 1) * (2 * max_d + 1)];
    let row_len = 2 * max_d + 1;

    v[max_d] = 0;

    for d in 0..=max_d {
        for k in (-(d as i32)..=(d as i32)).step_by(2) {
            let k_idx = (k + max_d as i32) as usize;

            let x = if k == -(d as i32) || (k != d as i32 && v[k_idx - 1] < v[k_idx + 1]) {
                v[k_idx + 1]
            } else {
                v[k_idx - 1] + 1
            };

            let mut x = x;
            let mut y = (x as i32 - k) as usize;

            while x < n && y < m && old[x] == new[y] {
                x += 1;
                y += 1;
            }

            v[k_idx] = x;
            v_index[d * row_len + k_idx] = x;

            if x >= n && y >= m {
                return backtrack_myers(old, new, &v_index, d, k, max_d);
            }
        }
    }

    vec![]
}

fn backtrack_myers(
    old: &[char],
    new: &[char],
    v_index: &[usize],
    d: usize,
    mut k: i32,
    max_d: usize,
) -> Vec<Edit> {
    let mut edits = Vec::with_capacity(old.len() + new.len());
    let mut x = old.len();
    let mut y = new.len();
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

        let k_idx = (k + max_d as i32) as usize;

        let prev_k = if k == -(cur_d as i32)
            || (k != cur_d as i32
                && v_index[(cur_d - 1) * row_len + k_idx - 1]
                    < v_index[(cur_d - 1) * row_len + k_idx + 1])
        {
            k + 1
        } else {
            k - 1
        };

        let prev_k_idx = (prev_k + max_d as i32) as usize;
        let prev_x_val = v_index[(cur_d - 1) * row_len + prev_k_idx];
        let prev_y = (prev_x_val as i32 - prev_k) as usize;

        let (move_x, move_y) = if prev_k == k + 1 {
            (prev_x_val, prev_y + 1)
        } else {
            (prev_x_val + 1, prev_y)
        };

        while x > move_x && y > move_y {
            edits.push(Edit::Equal);
            x -= 1;
            y -= 1;
        }

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
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
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

    DiffBundle {
        hunks,
        formatted,
        is_empty: !has_changes,
    }
}

/// Format a unified diff without ANSI color codes.
pub fn format_unified_diff(old: &str, new: &str, options: DiffOptions<'_>) -> String {
    let mut options = options;
    options.missing_newline_hint = false;
    let bundle = compute_diff(old, new, options, format_colored_diff);
    crate::utils::ansi_parser::strip_ansi(&bundle.formatted)
}

/// Compute a structured diff bundle using the default theme-aware formatter.
pub fn compute_diff_with_theme(old: &str, new: &str, options: DiffOptions<'_>) -> DiffBundle {
    compute_diff(old, new, options, format_colored_diff)
}

/// Format diff hunks with theme colors for terminal display.
pub fn format_colored_diff(hunks: &[DiffHunk], options: &DiffOptions<'_>) -> String {
    if hunks.is_empty() {
        return String::new();
    }

    let active_styles = theme::active_styles();
    let header_style = active_styles.status;
    let hunk_header_style = active_styles.status;
    let addition_style = active_styles.secondary;
    let deletion_style = active_styles.error;
    let context_style = active_styles.output;

    let mut output = String::new();

    if let (Some(old_label), Some(new_label)) = (options.old_label, options.new_label) {
        let _ = write!(
            output,
            "{}--- {old_label}\n{}",
            header_style.render(),
            Reset.render()
        );

        let _ = write!(
            output,
            "{}+++ {new_label}\n{}",
            header_style.render(),
            Reset.render()
        );
    }

    for hunk in hunks {
        let _ = write!(
            output,
            "{}@@ -{},{} +{},{} @@\n{}",
            hunk_header_style.render(),
            hunk.old_start,
            hunk.old_lines,
            hunk.new_start,
            hunk.new_lines,
            Reset.render()
        );

        for line in &hunk.lines {
            let (style, prefix) = match line.kind {
                DiffLineKind::Addition => (&addition_style, '+'),
                DiffLineKind::Deletion => (&deletion_style, '-'),
                DiffLineKind::Context => (&context_style, ' '),
            };

            let mut display = String::with_capacity(line.text.len() + 2);
            display.push(prefix);
            display.push_str(&line.text);

            let has_newline = display.ends_with('\n');
            let display_content = if has_newline {
                &display[..display.len() - 1]
            } else {
                &display
            };

            let _ = write!(
                output,
                "{}{} {}",
                style.render(),
                display_content,
                Reset.render()
            );
            output.push('\n');

            if options.missing_newline_hint && !line.text.ends_with('\n') {
                let eof_hint = r"\ No newline at end of file";
                let _ = write!(
                    output,
                    "{}{} {}",
                    context_style.render(),
                    eof_hint,
                    Reset.render()
                );
                output.push('\n');
            }
        }
    }

    output
}

fn split_lines_with_terminator(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::with_capacity(0);
    }

    let mut lines: Vec<String> = text
        .split_inclusive('\n')
        .map(|line| line.to_string())
        .collect();

    if lines.is_empty() {
        lines.push(text.to_string());
    }

    lines
}

fn collect_line_records<'a>(
    old_lines: &'a [&'a str],
    new_lines: &'a [&'a str],
) -> Vec<LineRecord<'a>> {
    let (old_encoded, new_encoded) = encode_line_sequences(old_lines, new_lines);
    let mut records = Vec::with_capacity(old_lines.len() + new_lines.len());
    let mut old_index = 0usize;
    let mut new_index = 0usize;

    for chunk in compute_diff_chunks(old_encoded.as_str(), new_encoded.as_str()) {
        match chunk {
            Chunk::Equal(text) => {
                for _ in text.chars() {
                    let old_line = old_index + 1;
                    let new_line = new_index + 1;
                    let line = old_lines[old_index];
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
                    let line = old_lines[old_index];
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
                    let line = new_lines[new_index];
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

fn encode_line_sequences<'a>(
    old_lines: &'a [&'a str],
    new_lines: &'a [&'a str],
) -> (String, String) {
    let mut token_map: HashMap<&'a str, char> = HashMap::new();
    let mut next_codepoint: u32 = 0;

    let old_encoded = encode_line_list(old_lines, &mut token_map, &mut next_codepoint);
    let new_encoded = encode_line_list(new_lines, &mut token_map, &mut next_codepoint);

    (old_encoded, new_encoded)
}

fn encode_line_list<'a>(
    lines: &'a [&'a str],
    map: &mut HashMap<&'a str, char>,
    next_codepoint: &mut u32,
) -> String {
    let mut encoded = String::with_capacity(lines.len());
    for &line in lines {
        let token = if let Some(&value) = map.get(line) {
            value
        } else {
            let ch = next_token_char(next_codepoint).expect("exceeded diff token capacity");
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
    old_line: Option<usize>,
    new_line: Option<usize>,
    text: &'a str,
    anchor_old: usize,
    anchor_new: usize,
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
            .unwrap_or(1);

        let new_start = slice
            .iter()
            .filter_map(|r| r.new_line)
            .min()
            .or_else(|| slice.iter().map(|r| r.anchor_new).min())
            .unwrap_or(1);

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
                kind: record.kind.clone(),
                old_line: record.old_line,
                new_line: record.new_line,
                text: record.text.to_string(),
            })
            .collect();

        hunks.push(DiffHunk {
            old_start,
            old_lines,
            new_start,
            new_lines,
            lines,
        });
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
                if start < existing_start {
                    current_start = Some(start);
                }
                if end > current_end {
                    current_end = end;
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
