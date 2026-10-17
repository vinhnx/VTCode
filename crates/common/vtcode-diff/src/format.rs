//! Unified-diff text formatting and numbered rendering.

use super::display::display_lines_from_unified_diff;
use super::parse::trim_line_ending;
use super::types::{DiffDisplayLine, DiffDocument, DiffHunk, DiffLineKind, DiffOptions};

/// Formats a structured diff as plain unified text.
///
/// Labels are emitted when both [`DiffOptions::old_label`] and
/// [`DiffOptions::new_label`] are present. Source line terminators are
/// normalized to `\n` in the formatted output while missing-final-newline
/// hints remain controlled by [`DiffOptions::missing_newline_hint`].
#[must_use]
pub fn format_unified_diff(old: &str, new: &str, options: DiffOptions<'_>) -> String {
    let document = DiffDocument::between(old, new, options.clone());
    format_unified_hunks(&document.hunks, &options)
}

/// Formats precomputed hunks as plain unified text.
#[must_use]
pub fn format_unified_hunks(hunks: &[DiffHunk], options: &DiffOptions<'_>) -> String {
    if hunks.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    if let (Some(old_label), Some(new_label)) = (options.old_label, options.new_label) {
        output.push_str("--- ");
        output.push_str(old_label);
        output.push('\n');
        output.push_str("+++ ");
        output.push_str(new_label);
        output.push('\n');
    }

    for hunk in hunks {
        output.push_str("@@ -");
        output.push_str(&format_unified_range(hunk.old_start, hunk.old_lines));
        output.push_str(" +");
        output.push_str(&format_unified_range(hunk.new_start, hunk.new_lines));
        output.push_str(" @@\n");
        for line in &hunk.lines {
            let prefix = match line.kind {
                DiffLineKind::Context => ' ',
                DiffLineKind::Addition => '+',
                DiffLineKind::Deletion => '-',
            };
            output.push(prefix);
            output.push_str(trim_line_ending(&line.text));
            output.push('\n');

            let has_line_terminator = line.text.ends_with('\n') || line.text.ends_with('\r');
            if options.missing_newline_hint && !has_line_terminator {
                output.push_str(r"\ No newline at end of file");
                output.push('\n');
            }
        }
    }
    output
}

fn format_unified_range(start: usize, count: usize) -> String {
    if count == 0 {
        return format!("{},0", start.saturating_sub(1));
    }
    if count == 1 {
        return start.to_string();
    }
    format!("{start},{count}")
}

/// Formats a git-compatible `@@ -old +new @@` hunk header.
///
/// Range counts are preserved rather than collapsed to start-only form: a
/// pure deletion such as `@@ -65,19 +65,0 @@` must not read as a one-line
/// change. Counts of one are elided (`@@ -1 +1 @@`) to match `git diff`.
#[must_use]
pub fn format_hunk_header(old_start: usize, old_count: usize, new_start: usize, new_count: usize) -> String {
    format!(
        "@@ -{} +{} @@",
        format_unified_range(old_start, old_count),
        format_unified_range(new_start, new_count)
    )
}
/// Formats a numbered unified diff without ANSI color.
#[must_use]
pub fn format_numbered_unified_diff(input: &str) -> Vec<String> {
    let lines = display_lines_from_unified_diff(input);
    let width = diff_display_line_number_width(&lines);
    lines.iter().map(|line| line.numbered_text(width)).collect()
}

/// Computes the clamped line-number gutter width.
#[must_use]
pub fn diff_display_line_number_width(lines: &[DiffDisplayLine]) -> usize {
    let maximum = lines
        .iter()
        .flat_map(|line| [line.old_line, line.new_line])
        .flatten()
        .max()
        .unwrap_or_default();
    decimal_digits(maximum).clamp(5, 6)
}
fn decimal_digits(mut number: u32) -> usize {
    let mut digits = 1usize;
    while number >= 10 {
        number /= 10;
        digits += 1;
    }
    digits
}
