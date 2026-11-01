//! Split tool-output tails into prose and structured-JSON sections.
//!
//! Tool aggregations append a `Structured output:\n{...}` metadata block;
//! the Output tab and expanded transcript rows render the JSON part through
//! the highlighted code-block path while prose stays literal text (tool
//! output is untrusted data and must never be interpreted as Markdown).

use gpui_kit::SharedString;
use serde::de::IgnoredAny;
use serde_json::Deserializer;

/// Marker line introducing the structured metadata block in aggregated tool
/// output. Contract with `structured_stream_metadata` in
/// `crates/codegen/vtcode-core/src/core/agent/events/lifecycle/summary.rs`,
/// which emits `{marker}\n{pretty_json}` as one part; later parts (error or
/// hint text) may follow, so only a *leading* JSON value is claimed.
pub(super) const STRUCTURED_OUTPUT_MARKER: &str = "Structured output:";

/// A tool-output tail split for rendering.
///
/// The split is display-only: accessibility labels and clipboard copies keep
/// the unsplit tail, so a missed split degrades to today's plain rendering
/// instead of losing text.
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

/// Split a bounded output tail into literal prose and structured JSON.
///
/// Markers are tried from last to first against the full tail; the first
/// one (at a line start, followed by a newline and one valid leading JSON
/// value) wins. Anything else — no marker, a mid-line mention, truncated
/// JSON, an empty candidate — falls back to the whole tail as
/// [`OutputSections::leading`], which renders exactly like today.
///
/// Postcondition: `structured` is `Some` only with JSON that parses, so
/// renderers can highlight it without revalidating. Splitting never drops
/// text: `leading` + marker + `json` + `trailing` cover the tail modulo
/// boundary whitespace (which survives in accessibility labels and copies).
/// A marker embedded inside the JSON string itself would split there instead
/// of at the real marker; core metadata never contains that shape, and every
/// byte stays visible either way.
pub(super) fn split_output_sections(tail: &SharedString) -> OutputSections {
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
    OutputSections { leading: tail.clone(), structured: None }
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
}
