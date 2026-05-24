//! Diff utilities for generating structured diffs.

use anstyle::{AnsiColor, Color, Reset, Style};
use std::fmt::Write;
pub use vtcode_commons::diff::*;

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

/// Format diff hunks with standard ANSI colors for terminal display.
pub fn format_colored_diff(hunks: &[DiffHunk], options: &DiffOptions<'_>) -> String {
    if hunks.is_empty() {
        return String::new();
    }

    let header_style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan)));
    let hunk_header_style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan)));
    let addition_style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)));
    let deletion_style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red)));
    let context_style = Style::new();

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
            format_colored_diff,
        );

        assert!(!bundle.is_empty);
        assert_eq!(bundle.hunks.len(), 1);
        let hunk = &bundle.hunks[0];
        assert_eq!(hunk.old_start, 1);
        assert_eq!(hunk.new_start, 1);
        assert!(bundle.formatted.contains("@@"));
        assert!(
            hunk.lines
                .iter()
                .any(|line| matches!(line.kind, DiffLineKind::Deletion))
        );
        assert!(
            hunk.lines
                .iter()
                .any(|line| matches!(line.kind, DiffLineKind::Addition))
        );
    }
}
