use crate::tools::handlers::planning_workflow::parse_bracket_list;

/// Markers that may trail a task description as inline `Action -> files: [...]
/// -> verify: [...]` metadata. `outcome` is accepted so a polluted suffix is
/// still stripped from the visible row even though the compact view never
/// renders it.
const INLINE_TASK_METADATA_ARROWS: &[&str] = &[" -> ", " → "];
const INLINE_TASK_METADATA_LABELS: &[&str] = &["files:", "verify:", "verification:", "outcome:"];

/// Split an inline `Action -> files: [...] -> verify: [...]` suffix out of a
/// task description.
///
/// Returns `(clean_description, files, verify)`. Both arrow spellings are
/// accepted outside inline code and quoted metadata values.
/// Metadata is sliced from the original step to preserve the command text.
pub fn split_task_description_metadata(source: &str) -> (String, Vec<String>, Vec<String>) {
    let Some(first) = find_inline_metadata_marker(source, 0, None) else {
        return (source.trim().to_string(), Vec::new(), Vec::new());
    };
    let head = source[..first.start].trim();
    if head.is_empty() {
        return (source.trim().to_string(), Vec::new(), Vec::new());
    }

    let mut files = Vec::new();
    let mut verify = Vec::new();
    let mut marker = Some(first);
    while let Some(current) = marker {
        marker = find_inline_metadata_marker(source, current.value_start, Some(current.label));
        let end = marker.as_ref().map_or(source.len(), |next| next.start);
        let value = source[current.value_start..end].trim();
        if current.label == "files:" {
            files.extend(parse_bracket_list(value));
        } else if is_verification_label(current.label) {
            verify.extend(parse_bracket_list(value));
        }
    }
    (head.to_string(), files, verify)
}

fn is_verification_label(label: &str) -> bool {
    matches!(label, "verify:" | "verification:")
}

struct InlineMetadataMarker {
    start: usize,
    value_start: usize,
    label: &'static str,
}

fn find_inline_metadata_marker(source: &str, from: usize, value_label: Option<&str>) -> Option<InlineMetadataMarker> {
    let mut inline_ticks: Option<usize> = None;
    let mut quote = None;
    let mut marker_in_quote = None;
    let mut in_shell_comment = false;
    let mut previous = None;
    let mut previous_non_whitespace = None;
    let mut chars = source[from..].char_indices().peekable();
    while let Some((offset, character)) = chars.next() {
        let cursor = from + offset;
        let remainder = &source[cursor..];
        let preceding = previous.replace(character);
        if value_label.is_some() && inline_ticks.is_none() && !in_shell_comment {
            let at_entry_start = previous_non_whitespace.is_none_or(|ch| matches!(ch, '[' | ','));
            if !character.is_whitespace() {
                previous_non_whitespace = Some(character);
            }
            if character == '\\' && quote != Some('\'') {
                chars.next_if(|(_, next)| matches!(next, '\'' | '"' | '`' | '#' | '\\'));
                continue;
            }
            // Match plan validation for apostrophes after word characters.
            let word_apostrophe = character == '\'' && preceding.is_some_and(char::is_alphanumeric);
            if let Some(delimiter) = quote {
                if character == delimiter {
                    quote = None;
                    marker_in_quote = None;
                }
            } else if matches!(character, '\'' | '"')
                && !word_apostrophe
                && (value_label.is_some_and(is_verification_label) || at_entry_start)
            {
                quote = Some(character);
            } else if character == '#'
                && value_label.is_some_and(is_verification_label)
                && preceding.is_none_or(|ch| ch.is_whitespace() || matches!(ch, ';' | '&' | '|' | '(' | ')'))
            {
                in_shell_comment = true;
            }
        }
        if quote.is_none() && !in_shell_comment && character == '`' {
            let mut run = 1;
            while chars.next_if(|(_, next)| *next == '`').is_some() {
                run += 1;
            }
            if inline_ticks.is_some_and(|ticks| ticks == run) {
                inline_ticks = None;
            } else if inline_ticks.is_none() {
                inline_ticks = Some(run);
            }
            continue;
        }
        if inline_ticks.is_none()
            && character == ' '
            && let Some(arrow) = INLINE_TASK_METADATA_ARROWS.iter().find(|arrow| remainder.starts_with(**arrow))
            && let Some(&label) = INLINE_TASK_METADATA_LABELS.iter().find(|label| {
                remainder[arrow.len()..]
                    .get(..label.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(label))
            })
        {
            let marker = InlineMetadataMarker {
                start: cursor,
                value_start: cursor + arrow.len() + label.len(),
                label,
            };
            if quote.is_none() {
                return Some(marker);
            }
            // Unmatched punctuation in comments or manual check text must
            // not swallow subsequent metadata.
            marker_in_quote.get_or_insert(marker);
        }
    }
    marker_in_quote
}
