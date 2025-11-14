//! Diff utilities for generating structured and formatted diffs.

use anstyle::{AnsiColor, Color, Reset, Style};
use anstyle_git;
use dissimilar::{Chunk, diff};
use serde::Serialize;
use std::cmp::min;
use std::collections::HashMap;

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
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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
    let old_lines_owned = split_lines_with_terminator(old);
    let new_lines_owned = split_lines_with_terminator(new);

    let old_refs: Vec<&str> = old_lines_owned.iter().map(String::as_str).collect();
    let new_refs: Vec<&str> = new_lines_owned.iter().map(String::as_str).collect();

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
        format_colored_diff(&hunks, &options)
    };

    DiffBundle {
        hunks,
        formatted,
        is_empty: !has_changes,
    }
}

fn split_lines_with_terminator(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut lines: Vec<String> = text
        .split_inclusive('\n')
        .map(|line| line.to_string())
        .collect();

    if lines.is_empty() {
        // The input had no newline characters; capture as a single line.
        lines.push(text.to_string());
    }

    lines
}

fn collect_line_records<'a>(
    old_lines: &'a [&'a str],
    new_lines: &'a [&'a str],
) -> Vec<LineRecord<'a>> {
    let (old_encoded, new_encoded) = encode_line_sequences(old_lines, new_lines);
    let mut records = Vec::new();
    let mut old_index = 0usize;
    let mut new_index = 0usize;

    for chunk in diff(old_encoded.as_str(), new_encoded.as_str()) {
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

/// Format diff hunks with simple ANSI colors for terminal display.
///
/// This function generates a unified diff format with built-in ANSI color codes
/// instead of relying on external syntax highlighting. This ensures consistent
/// and correct diff coloring in the terminal:
/// - Cyan for file headers and hunk headers
/// - Green for additions (+)
/// - Red for deletions (-)
/// - White for context lines
fn format_colored_diff(hunks: &[DiffHunk], options: &DiffOptions<'_>) -> String {
    if hunks.is_empty() {
        return String::new();
    }

    // Use git-standard colors from anstyle-git to match Git's default color scheme
    let header_style = anstyle_git::parse("section")
        .unwrap_or_else(|_| Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan))));
    let hunk_header_style = anstyle_git::parse("meta")
        .unwrap_or_else(|_| Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan))));
    let addition_style = anstyle_git::parse("new")
        .unwrap_or_else(|_| Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green))));
    let deletion_style = anstyle_git::parse("old")
        .unwrap_or_else(|_| Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red))));
    let context_style = anstyle_git::parse("context")
        .unwrap_or_else(|_| Style::new().fg_color(Some(Color::Ansi(AnsiColor::White))));

    let mut output = String::new();

    if let (Some(old_label), Some(new_label)) = (options.old_label, options.new_label) {
        let formatted = format!("--- {old_label}\n");
        output.push_str(&format!(
            "{}{}{}",
            header_style.render(),
            formatted,
            Reset.render()
        ));

        let formatted = format!("+++ {new_label}\n");
        output.push_str(&format!(
            "{}{}{}",
            header_style.render(),
            formatted,
            Reset.render()
        ));
    }

    for hunk in hunks {
        let header = format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
        );
        output.push_str(&format!(
            "{}{}{}",
            hunk_header_style.render(),
            header,
            Reset.render()
        ));

        for line in &hunk.lines {
            let (style, prefix) = match line.kind {
                DiffLineKind::Addition => (&addition_style, '+'),
                DiffLineKind::Deletion => (&deletion_style, '-'),
                DiffLineKind::Context => (&context_style, ' '),
            };

            let mut display = String::with_capacity(line.text.len() + 2);
            display.push(prefix);
            display.push_str(&line.text);
            if !line.text.ends_with('\n') {
                display.push('\n');
            }

            output.push_str(&format!("{}{}{}", style.render(), display, Reset.render()));

            if options.missing_newline_hint && !line.text.ends_with('\n') {
                let eof_hint = "\\ No newline at end of file\n";
                output.push_str(&format!(
                    "{}{}{}",
                    context_style.render(),
                    eof_hint,
                    Reset.render()
                ));
            }
        }
    }

    output
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
    let mut ranges = Vec::new();
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
        } else if let Some(start) = current_start {
            if idx > current_end {
                ranges.push((start, current_end));
                current_start = None;
            }
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

    #[test]
    fn trims_context_lines_to_requested_window() {
        let before: String = (0..200).map(|idx| format!("line {idx}\n")).collect();
        let mut after_lines: Vec<String> = (0..200).map(|idx| format!("line {idx}")).collect();
        after_lines[100] = "line 100 changed".to_string();
        let after = after_lines.join("\n");

        let bundle = compute_diff(
            &before,
            &after,
            DiffOptions {
                context_lines: 2,
                ..Default::default()
            },
        );

        assert_eq!(bundle.hunks.len(), 1);
        let hunk = &bundle.hunks[0];

        let total_context = hunk
            .lines
            .iter()
            .filter(|line| matches!(line.kind, DiffLineKind::Context))
            .count();

        assert!(
            total_context <= 4,
            "expected limited context, got {total_context}"
        );

        let formatted_context = bundle
            .formatted
            .lines()
            .filter(|line| line.starts_with(' '))
            .count();

        assert!(
            formatted_context <= 4,
            "formatted output should only include limited context but had {formatted_context} lines"
        );
    }
}
