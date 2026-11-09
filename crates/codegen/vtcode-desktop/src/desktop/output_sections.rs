//! Split tool-output tails into prose and structured-JSON sections.
//!
//! Tool aggregations append a `Structured output:\n{...}` metadata block for
//! the LLM; user-visible surfaces must never show that JSON envelope.
//! [`human_output_text`] is the single projection from captured output to
//! display text: prose stays literal (tool output is untrusted data and must
//! never be interpreted as Markdown) and only the user-relevant `spool_note`
//! survives from the metadata block.

use gpui_kit::SharedString;
use serde::de::IgnoredAny;
use serde_json::Deserializer;

/// Marker line introducing the structured metadata block in aggregated tool
/// output. Contract with `structured_stream_metadata` in
/// `crates/codegen/vtcode-core/src/core/agent/events/lifecycle/summary.rs`,
/// which emits `{marker}\n{pretty_json}` as one part; later parts (error or
/// hint text) may follow, so only a *leading* JSON value is claimed.
pub(super) const STRUCTURED_OUTPUT_MARKER: &str = "Structured output:";

/// A tool-output tail split for stripping.
///
/// The split feeds the human projection: labels and copies render the
/// stripped text, so a missed split degrades to plain rendering of the full
/// tail instead of losing text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OutputSections {
    /// Text before the marker line, or the whole tail when `structured` is
    /// `None` (no marker, or the candidate after it is not valid JSON).
    pub leading: SharedString,
    /// The validated structured section, if the tail carries one.
    pub structured: Option<StructuredSection>,
}

/// The validated structured section of a tool-output tail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StructuredSection {
    /// The JSON value text exactly as emitted (surrounding blank lines
    /// trimmed). Always parses as JSON; see [`split_output_sections`].
    pub json: SharedString,
    /// Text after the JSON value, when core appended later parts. Empty in
    /// the common case where the metadata block ends the output.
    pub trailing: SharedString,
}

/// Maximum strip passes. Realistic captures carry at most two blocks (one the
/// tool echoed, one appended by core); the bound only constrains adversarial
/// input, where a `spool_note` could itself smuggle a marker.
const MAX_STRIP_PASSES: usize = 8;

/// Human-readable projection of captured tool output for every display surface.
///
/// Removes every `Structured output:` JSON metadata block, last first. Only
/// the user-relevant `spool_note` of each block survives, spliced in where
/// its block stood so document order never changes. Stripping all blocks
/// (rather than just core's trailing one) is what keeps the projection
/// idempotent — streaming re-stores, page joins, and copy joins all re-strip,
/// and any fixed point they converge on must be stable.
///
/// Postconditions: the result holds no valid block the splitter can still
/// find (up to [`MAX_STRIP_PASSES`]); marker-less input passes through
/// byte-identical; anything unparseable (truncated JSON, an empty candidate,
/// a mid-line mention) degrades to the input unchanged. Trade-off: a tool
/// echoing an envelope-shaped block (e.g. catting a spooled capture) loses
/// that echo too — in practice nested blocks are core metadata echoes, and
/// uniformity across transcript, Output tab, and copies wins over preserving
/// them.
#[must_use]
pub(super) fn human_output_text(raw: &str) -> String {
    let mut text = raw.to_string();
    for _ in 0..MAX_STRIP_PASSES {
        let sections = split_output_sections(&text);
        let Some(structured) = sections.structured else {
            return text;
        };
        let mut parts = Vec::new();
        if !sections.leading.is_empty() {
            parts.push(sections.leading.to_string());
        }
        if let Some(note) = spool_note(&structured.json) {
            parts.push(note);
        }
        if !structured.trailing.is_empty() {
            parts.push(structured.trailing.to_string());
        }
        text = parts.join("\n");
    }
    text
}

/// [`human_output_text`] that collapses blank captures to `None`.
///
/// Every `TranscriptItem.output` store routes through here so `has_output`
/// never offers an Output tab or deep link that lands on nothing.
pub(super) fn human_output_option(raw: &str) -> Option<String> {
    let human = human_output_text(raw);
    (!human.trim().is_empty()).then_some(human)
}

/// The user-relevant pointer inside a structured metadata block, if any.
///
/// Only `spool_note` survives: it tells the user the full capture was spooled
/// to a file and how to inspect it. Plumbing (`backend`, `pid`,
/// `lifecycle_state`, byte counts) and bulk echoes (`preview`) stay out.
/// Blank or mistyped notes vanish rather than rendering an empty line.
fn spool_note(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    value
        .get("spool_note")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|note| !note.is_empty())
        .map(str::to_string)
}

/// Split a bounded output tail into literal prose and structured JSON.
///
/// Markers are tried from last to first against the full tail; the first
/// one (at a line start, followed by a newline and one valid leading JSON
/// value) wins. Anything else — no marker, a mid-line mention, truncated
/// JSON, an empty candidate — falls back to the whole tail as
/// [`OutputSections::leading`], which renders exactly like today.
///
/// Postcondition: `structured` is `Some` only with JSON that parses, so
/// callers can consume it without revalidating. Splitting never drops
/// text: `leading` + marker + `json` + `trailing` cover the tail modulo
/// boundary whitespace.
/// A marker embedded inside the JSON string itself would split there instead
/// of at the real marker; core metadata never contains that shape, and every
/// byte stays visible either way.
pub(super) fn split_output_sections(tail: &str) -> OutputSections {
    let text: &str = tail;
    let mut end = text.len();
    // The shrinking prefix only scopes the marker search; every candidate
    // splits the full tail so a rejected later marker stays visible.
    while let Some(prefix) = text.get(..end) {
        let Some(marker_start) = last_marker_offset(prefix) else {
            break;
        };
        debug_assert!(marker_start < end, "a found marker sits inside the searched prefix");
        let content = text.get(marker_start + STRUCTURED_OUTPUT_MARKER.len()..);
        if let Some((json, trailing)) = content.and_then(leading_json_section) {
            let leading = text.get(..marker_start).unwrap_or_default().trim_end();
            return OutputSections {
                leading: leading.to_string().into(),
                structured: Some(StructuredSection { json, trailing }),
            };
        }
        end = marker_start;
    }
    OutputSections { leading: tail.to_string().into(), structured: None }
}

/// Offset of the last marker that starts a line: position 0, or right after
/// a newline. A mid-line mention (command echo, prose) is not a section.
fn last_marker_offset(haystack: &str) -> Option<usize> {
    haystack
        .rmatch_indices(STRUCTURED_OUTPUT_MARKER)
        .map(|(offset, _)| offset)
        .find(|&offset| offset == 0 || haystack.as_bytes().get(offset - 1) == Some(&b'\n'))
}

/// Claim one leading JSON value after a marker: the marker must be followed
/// by a newline (core's exact emission shape), then one value must parse.
/// Returns the value text plus whatever text follows it.
fn leading_json_section(after_marker: &str) -> Option<(SharedString, SharedString)> {
    let candidate = after_marker.strip_prefix("\r\n").or_else(|| after_marker.strip_prefix('\n'))?;
    // Validate without building a DOM: only acceptance and the value's end
    // offset matter, and this runs per render over a bounded tail.
    let mut values = Deserializer::from_str(candidate).into_iter::<IgnoredAny>();
    if !matches!(values.next(), Some(Ok(_))) {
        return None;
    }
    // The split sits right after the value's last byte, which is ASCII for
    // every JSON scalar and closing bracket; `.get` keeps it total anyway.
    let end = values.byte_offset();
    let json = candidate.get(..end)?.trim();
    let trailing = candidate.get(end..).unwrap_or_default().trim();
    Some((json.to_string().into(), trailing.to_string().into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(fixture: &str) -> OutputSections {
        split_output_sections(&SharedString::from(fixture.to_string()))
    }

    #[test]
    fn split_without_marker_returns_whole_tail_as_leading() {
        let sections = split("command output\nsecond line");
        assert_eq!(sections.leading, "command output\nsecond line");
        assert_eq!(sections.structured, None);
    }

    #[test]
    fn split_empty_tail_stays_empty() {
        let sections = split("");
        assert_eq!(sections.leading, "");
        assert_eq!(sections.structured, None);
    }

    #[test]
    fn split_accepts_marker_at_line_start() {
        let sections = split("log line\nStructured output:\n{\"b\": 2}\n");
        assert_eq!(sections.leading, "log line");
        let structured = sections.structured.expect("valid JSON section");
        assert_eq!(structured.json, "{\"b\": 2}");
        assert_eq!(structured.trailing, "");
    }

    #[test]
    fn split_accepts_marker_at_text_start() {
        let sections = split("Structured output:\n[1, 2]");
        assert_eq!(sections.leading, "");
        let structured = sections.structured.expect("valid JSON section");
        assert_eq!(structured.json, "[1, 2]");
        assert_eq!(structured.trailing, "");
    }

    #[test]
    fn split_rejects_marker_mid_line() {
        let sections = split("echo Structured output: {\"a\": 1}");
        assert_eq!(sections.leading, "echo Structured output: {\"a\": 1}");
        assert_eq!(sections.structured, None);
    }

    #[test]
    fn split_rejects_marker_without_a_following_newline() {
        // Asymmetric with the text-start acceptance: same position, but the
        // marker introduces prose (`42` would parse as JSON) rather than a
        // newline-led block, so it must not split.
        let sections = split("Structured output: 42");
        assert_eq!(sections.leading, "Structured output: 42");
        assert_eq!(sections.structured, None);
    }

    #[test]
    fn split_rejects_truncated_json_with_byte_identical_fallback() {
        let fixture = "out\nStructured output:\n{\"a\": 1,";
        let sections = split(fixture);
        assert_eq!(sections.leading, fixture);
        assert_eq!(sections.structured, None);
    }

    #[test]
    fn split_rejects_marker_with_empty_candidate() {
        let fixture = "out\nStructured output:\n";
        let sections = split(fixture);
        assert_eq!(sections.leading, fixture);
        assert_eq!(sections.structured, None);
    }

    #[test]
    fn split_prefers_the_last_valid_marker() {
        let sections = split("Structured output:\n[bogus]\nmore\nStructured output:\n{\"ok\": true}");
        assert_eq!(sections.leading, "Structured output:\n[bogus]\nmore");
        let structured = sections.structured.expect("valid JSON section");
        assert_eq!(structured.json, "{\"ok\": true}");
        assert_eq!(structured.trailing, "");
    }

    #[test]
    fn split_falls_back_to_an_earlier_marker_when_the_last_is_invalid() {
        // Asymmetric with the last-valid preference: the bogus marker comes
        // last, so the earlier valid one wins and the bogus tail stays
        // visible as trailing prose.
        let sections = split("Structured output:\n{\"ok\": true}\nmore\nStructured output:\n[bogus]");
        assert_eq!(sections.leading, "");
        let structured = sections.structured.expect("earlier valid section");
        assert_eq!(structured.json, "{\"ok\": true}");
        assert_eq!(structured.trailing, "more\nStructured output:\n[bogus]");
    }

    #[test]
    fn split_keeps_text_after_json_as_trailing() {
        let sections = split("out\nStructured output:\n{\"a\": 1}\nerror: boom");
        assert_eq!(sections.leading, "out");
        let structured = sections.structured.expect("valid JSON section");
        assert_eq!(structured.json, "{\"a\": 1}");
        assert_eq!(structured.trailing, "error: boom");
    }

    #[test]
    fn split_handles_multibyte_prose_before_the_marker() {
        let sections = split("界 界\nStructured output:\n{\"a\": 1}");
        assert_eq!(sections.leading, "界 界");
        let structured = sections.structured.expect("valid JSON section");
        assert_eq!(structured.json, "{\"a\": 1}");
        assert_eq!(structured.trailing, "");
    }

    #[test]
    fn human_output_strips_metadata_but_keeps_spool_note_in_order() {
        let raw = "log line\nStructured output:\n{\"backend\": \"pipe\", \"spool_note\": \"Spooled to out.txt\", \"pid\": 1}\nerror: boom";
        assert_eq!(human_output_text(raw), "log line\nSpooled to out.txt\nerror: boom");
        // Asymmetric: no note, no trailing — only the prose survives.
        let raw = "log line\nStructured output:\n{\"backend\": \"pipe\", \"pid\": 1}";
        assert_eq!(human_output_text(raw), "log line");
    }

    #[test]
    fn human_output_without_marker_passes_through_byte_identical() {
        for fixture in ["", "plain output\nsecond line", "echo Structured output: {\"a\": 1}"] {
            assert_eq!(human_output_text(fixture), fixture, "{fixture:?} passes through");
        }
    }

    #[test]
    fn human_output_degrades_to_input_on_unparseable_blocks() {
        // Truncated JSON and empty candidates keep every byte: noise, not loss.
        for fixture in ["out\nStructured output:\n{\"a\": 1,", "out\nStructured output:\n"] {
            assert_eq!(human_output_text(fixture), fixture, "{fixture:?} degrades intact");
        }
    }

    #[test]
    fn human_output_drops_blank_and_mistyped_spool_notes() {
        for json in [
            "{\"spool_note\": \"  \"}",
            "{\"spool_note\": 42}",
            "{\"spool_note\": null}",
        ] {
            let raw = format!("out\nStructured output:\n{json}");
            assert_eq!(human_output_text(&raw), "out", "{json} leaves prose only");
        }
        // A note-only block with no prose yields just the note.
        let raw = "Structured output:\n{\"spool_note\": \"  Spooled to out.txt  \"}";
        assert_eq!(human_output_text(raw), "Spooled to out.txt");
    }

    #[test]
    fn human_output_is_idempotent() {
        let raw = "log\nStructured output:\n{\"spool_note\": \"Spooled\"}\ntail";
        let once = human_output_text(raw);
        assert_eq!(human_output_text(&once), once);
    }

    #[test]
    fn human_output_strips_every_block_last_first_and_keeps_note_order() {
        let raw = concat!(
            "prose-a\n",
            "Structured output:\n{\"backend\": \"pipe\", \"spool_note\": \"Note A\"}\n",
            "prose-mid\n",
            "Structured output:\n{\"backend\": \"pipe\", \"spool_note\": \"Note B\"}\n",
            "prose-end",
        );
        assert_eq!(human_output_text(raw), "prose-a\nNote A\nprose-mid\nNote B\nprose-end");
        // The fixed point is stable: re-stripping changes nothing.
        let once = human_output_text(raw);
        assert_eq!(human_output_text(&once), once);
        assert!(!once.contains("Structured output:"));
    }
}
