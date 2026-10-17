//! Exec read-target extraction shared by loop guards and execution-history replay.
//!
//! A bounded shell slice read (`sed -n '<range>p' <file>`, `awk 'NR...' <file>`)
//! is a positioned read of a workspace file, structured the same way as a
//! `read_file` call with an offset/limit identity. The turn-local read guard and
//! the cross-turn history replay both need the same (path, slice) identity, so
//! the parsers live here instead of in one consumer and being re-derived
//! (divergently) in the other.

use serde_json::Value;

/// A parsed bounded shell read: the target path plus the requested slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecReadTarget {
    pub path: String,
    pub start_line: usize,
    /// Inclusive end line of the requested slice.
    pub end_line: usize,
    /// Whether the command output is the file's own lines. Only a bare
    /// `sed -n '<range>p' <file>` (no pipe) is verbatim: piped stages and awk
    /// programs transform the output, so it must never be served as positioned
    /// file evidence for a verbatim query (and vice versa in replay mode).
    /// Loop guards use path/slice regardless of this flag.
    pub verbatim: bool,
}

/// Lexically normalize a shell-measurement target path so spelling variants
/// (`./README.md`, `docs/../README.md`) share one identity. Reuses the shared
/// code_search normalizer: leading `..` is preserved (fail-closed, never
/// collides with in-workspace entries), so workspace escape via `../` cannot
/// masquerade as a local read.
fn normalized_shell_read_path(path: &str) -> String {
    crate::tools::normalised_code_search_path(path)
}

/// Parse a bounded shell slice read from command args.
///
/// Measurement loops hide a simple read behind a pipe
/// (`sed -n '65,71p' README.md | awk ...`, `awk 'NR...' README.md | cat`):
/// only the leading pipeline stage determines what is read, so parse that and
/// ignore trailing pipe stages. Only `|` is split: `;`/`&&`/`||` compounds
/// keep their untracked behavior (the full word list fails the simple-shape
/// parse below), which under-counts rather than over-blocks multi-read
/// one-liners.
pub fn parse_exec_read_target(args: &Value) -> Option<ExecReadTarget> {
    let parts = crate::tools::command_args::command_words(args).ok()??;
    let leading = leading_shell_segment(parts.as_slice());
    let piped = leading.len() != parts.len();
    parse_sed_read_target(leading)
        .or_else(|| parse_awk_read_target(leading))
        .map(|mut target| {
            // A stripped pipe stage transforms the output downstream, so the
            // lines that reach the model are not the file's own lines.
            target.verbatim = target.verbatim && !piped;
            target
        })
}

/// Leading pipeline stage before the first `|` token. `command_words` splits
/// via `shell_words`, so a `|` inside a quoted awk program stays embedded in
/// the program word and never splits; only a standalone pipe operator cuts.
fn leading_shell_segment(parts: &[String]) -> &[String] {
    let end = parts.iter().position(|part| part.as_str() == "|").unwrap_or(parts.len());
    &parts[..end]
}

/// Parse `awk '<program>' <single-file>` ranged measurement reads.
///
/// KISS scope: exactly `awk`, one program word, one file word — no options
/// (`-F`, `-v` fail closed to `None`), no multi-file scans, and the program
/// must reference an `NR` line range (see [`awk_nr_line_range`]). Whole-file
/// scans stay untracked: distinct whole-file queries are diverse research, not
/// pagination (mirrors the `code_search` per-path exclusion), so counting them
/// would false-trip the path cap. Callers gate on read-only classification, so
/// mutating programs (`>`, `|`, `system()`, `@`) never reach here; no need to
/// re-scan the program for writes. The file word must not look like a flag.
/// Awk output is always computed, never the file's own lines, so `verbatim` is
/// always false.
fn parse_awk_read_target(parts: &[String]) -> Option<ExecReadTarget> {
    let path_word = parts.get(2)?.as_str();
    let start = parts.first().map(String::as_str)?;
    if parts.len() != 3 || start != "awk" {
        return None;
    }
    let program = parts.get(1)?.as_str();
    if program.trim().is_empty() {
        return None;
    }
    if path_word.starts_with('-') || path_word.is_empty() {
        return None;
    }

    let (start, end) = awk_nr_line_range(program)?;
    Some(ExecReadTarget {
        path: normalized_shell_read_path(path_word),
        start_line: start,
        end_line: end,
        verbatim: false,
    })
}

/// Extract the line range referenced by `NR` comparisons in an awk program.
///
/// Scans for `NR` then an optional comparison operator then a decimal number.
/// Only numbers in that position count, so thresholds like `length > 120` (no
/// `NR`) do not masquerade as line ranges. Requires at least two numbers: a
/// single `NR` reference (`NR>1` header-skip, one `NR==67` probe, one
/// `/NR==65/` regex hit) cannot delimit a range, and grouping all such
/// open-ended scans by path alone would trip the family cap on diverse column
/// queries over the same file. Returns `(min, max)` over all matches.
fn awk_nr_line_range(program: &str) -> Option<(usize, usize)> {
    let bytes = program.as_bytes();
    let mut numbers: Vec<usize> = Vec::new();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'N' && bytes[index + 1] == b'R' {
            let mut cursor = index + 2;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            while cursor < bytes.len() && matches!(bytes[cursor], b'>' | b'<' | b'=' | b'!') {
                cursor += 1;
            }
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            let start = cursor;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
            }
            if start < cursor
                && let Ok(number) = program[start..cursor].parse::<usize>()
            {
                numbers.push(number);
            }
            index = cursor.max(index + 1);
        } else {
            index += 1;
        }
    }
    let min = *numbers.iter().min()?;
    let max = *numbers.iter().max()?;
    // A lone `NR` reference cannot delimit a range (see doc comment); without
    // this, every `NR>1` header-skip would collapse to `off=1::lim=1` and trip
    // the family cap on diverse same-file queries.
    if numbers.len() < 2 {
        return None;
    }
    // Sanity: line numbers, not byte counts. Ranges spanning millions of
    // lines are not real file slices; leave untracked.
    if max == 0 || max.saturating_sub(min) > 1_000_000 {
        return None;
    }
    Some((min, max))
}

/// Parse `sed -n '<range>p' <file>` (single file, no options). `-` (stdin) is
/// not a file read.
fn parse_sed_read_target(parts: &[String]) -> Option<ExecReadTarget> {
    if parts.first().map(String::as_str) != Some("sed") {
        return None;
    }

    let mut cursor = 1usize;
    if parts.get(cursor).map(String::as_str) != Some("-n") {
        return None;
    }
    cursor += 1;

    let script = parts.get(cursor)?.as_str();
    cursor += 1;

    let path = parts.get(cursor)?.as_str();
    if cursor + 1 != parts.len() {
        return None;
    }
    // Fail closed on flag-like paths (`sed -n '1p' --help`). `-` (stdin) is
    // not a file read either.
    if path.starts_with('-') {
        return None;
    }

    let (start, end) = parse_sed_print_range(script)?;
    Some(ExecReadTarget {
        path: normalized_shell_read_path(path),
        start_line: start,
        end_line: end,
        // Bare `sed -n '<range>p' <file>` prints the file's own lines. The
        // caller clears this when a pipe stage was stripped.
        verbatim: true,
    })
}

fn parse_sed_print_range(script: &str) -> Option<(usize, usize)> {
    let range = script.strip_suffix('p')?;
    let (start, end) = match range.split_once(',') {
        Some((start, end)) => (start, end),
        None => (range, range),
    };

    let start = start.parse::<usize>().ok()?;
    let end = end.parse::<usize>().ok()?;
    (start <= end).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn target(command: &str) -> Option<ExecReadTarget> {
        parse_exec_read_target(&json!({"cmd": command}))
    }

    #[test]
    fn sed_slice_read_parses_its_path_and_range() {
        let read = target("sed -n '270,300p' README.md").expect("simple sed read parses");
        assert_eq!(read.path, "README.md");
        assert_eq!(read.start_line, 270);
        assert_eq!(read.end_line, 300);
        assert!(read.verbatim);
    }

    #[test]
    fn piped_sed_read_loses_verbatim_but_keeps_identity() {
        let read = target("sed -n '65,71p' README.md | awk '{print length}'").expect("piped sed parses");
        assert_eq!(read.path, "README.md");
        assert!(!read.verbatim);
    }

    #[test]
    fn awk_measurement_read_is_never_verbatim() {
        let read = target("awk 'NR>=65 && NR<=71' README.md").expect("awk read parses");
        assert_eq!(read.path, "README.md");
        assert!(!read.verbatim);
    }

    #[test]
    fn mutating_and_dynamic_shapes_fail_closed() {
        // `sed -i` mutates (different flag) — never parsed as a read target.
        assert!(target("sed -i 's/a/b/' README.md").is_none());
        // Path normalization preserves workspace escape spelled as leading `..`.
        let escape = target("sed -n '1,2p' ../README.md").expect("escape path parses");
        assert!(escape.path.starts_with(".."), "{}", escape.path);
        // Wrong arity (`sed` with extra operands) is not a simple read.
        assert!(target("sed -n '1,2p' README.md LICENSE.md").is_none());
    }
}
