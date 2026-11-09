//! Bounded, read-only projections of captured tool output.

use gpui_kit::SharedString;

pub(super) const MAX_OUTPUT_LINES: usize = 1_000;
pub(super) const MAX_OUTPUT_BYTES: usize = 128 * 1024;
const COMPACT_OUTPUT_LINES: usize = 8;
const COMPACT_OUTPUT_BYTES: usize = 4 * 1024;

fn bounded_tail(text: &SharedString, max_bytes: usize, max_lines: usize) -> SharedString {
    let mut start = text.ceil_char_boundary(text.len().saturating_sub(max_bytes));
    let mut tail = text.get(start..).unwrap_or_default();
    let mut newlines_to_keep = max_lines + usize::from(tail.ends_with('\n'));
    for (index, byte) in tail.bytes().enumerate().rev() {
        if byte == b'\n' {
            newlines_to_keep -= 1;
            if newlines_to_keep == 0 {
                start += index + 1;
                tail = text.get(start..).unwrap_or_default();
                break;
            }
        }
    }
    if start == 0 {
        return text.clone();
    }
    if let Some(without_terminator) = tail.strip_suffix('\n') {
        tail = without_terminator;
    }
    let notice = rust_i18n::t!("app.inspector.truncation_notice").to_string();
    let mut rendered = String::with_capacity(notice.len().saturating_add(tail.len()));
    rendered.push_str(&notice);
    rendered.push_str(tail);
    rendered.into()
}

/// Retain raw bytes in the bounded capture used by restored-page bookkeeping.
pub(super) fn output_tail(text: &SharedString) -> SharedString {
    bounded_tail(text, MAX_OUTPUT_BYTES, MAX_OUTPUT_LINES)
}

fn clean_tail(text: &SharedString, max_bytes: usize, max_lines: usize) -> SharedString {
    let tail = bounded_tail(text, max_bytes, max_lines);
    let clean = vtcode_commons::ansi::strip_ansi(&tail);
    if clean == tail.as_ref() { tail } else { clean.into() }
}

/// Inspector output uses the CLI's shared terminal-control sanitizer.
pub(super) fn display_output_tail(text: &SharedString) -> SharedString {
    clean_tail(text, MAX_OUTPUT_BYTES, MAX_OUTPUT_LINES)
}

/// Keep transcript rows short; the Output inspector offers the fuller capture.
pub(super) fn compact_output_tail(text: &SharedString) -> SharedString {
    clean_tail(text, COMPACT_OUTPUT_BYTES, COMPACT_OUTPUT_LINES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_tail_keeps_recent_lines_and_bounds_unicode() {
        let lines: SharedString = (0..12)
            .map(|index| format!("line-{index}"))
            .collect::<Vec<_>>()
            .join("\n")
            .into();
        let tail = compact_output_tail(&lines);
        assert!(tail.contains("line-11"));
        assert!(!tail.contains("line-3\n"));
        assert!(tail.contains("line-4"));
        let long: SharedString = format!("prefix{}", "界".repeat(2_000)).into();
        let tail = compact_output_tail(&long);
        assert!(!tail.contains("prefix"));
        assert!(tail.ends_with("界"));
        assert!(tail.len() < COMPACT_OUTPUT_BYTES + 100);
        assert_eq!(compact_output_tail(&"".into()), "");
    }
}
