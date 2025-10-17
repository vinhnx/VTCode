use serde_json::Value;

#[derive(Default, Clone)]
pub(crate) struct ReasoningBuffer {
    text: String,
    last_chunk: Option<String>,
}

impl ReasoningBuffer {
    pub(crate) fn push(&mut self, chunk: &str) -> Option<String> {
        if chunk.trim().is_empty() {
            return None;
        }

        let normalized = Self::normalize_chunk(chunk);

        if normalized.is_empty() {
            return None;
        }

        if self.last_chunk.as_deref() == Some(&normalized) {
            return None;
        }

        let last_has_spacing = self.text.ends_with(' ') || self.text.ends_with('\n');
        let chunk_starts_with_space = chunk
            .chars()
            .next()
            .map(|value| value.is_whitespace())
            .unwrap_or(false);
        let leading_punctuation = Self::is_leading_punctuation(chunk);
        let trailing_connector = Self::ends_with_connector(&self.text);

        let mut delta = String::new();

        if !self.text.is_empty()
            && !last_has_spacing
            && !chunk_starts_with_space
            && !leading_punctuation
            && !trailing_connector
        {
            delta.push(' ');
        }

        delta.push_str(&normalized);
        self.text.push_str(&delta);
        self.last_chunk = Some(normalized);

        Some(delta)
    }

    pub(crate) fn finalize(self) -> Option<String> {
        let trimmed = self.text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn normalize_chunk(chunk: &str) -> String {
        let mut normalized = String::new();
        for part in chunk.split_whitespace() {
            if !normalized.is_empty() {
                normalized.push(' ');
            }
            normalized.push_str(part);
        }
        normalized
    }

    fn is_leading_punctuation(chunk: &str) -> bool {
        chunk
            .chars()
            .find(|ch| !ch.is_whitespace())
            .map(|ch| matches!(ch, ',' | '.' | '!' | '?' | ':' | ';' | ')' | ']' | '}'))
            .unwrap_or(false)
    }

    fn ends_with_connector(text: &str) -> bool {
        text.chars()
            .rev()
            .find(|ch| !ch.is_whitespace())
            .map(|ch| matches!(ch, '(' | '[' | '{' | '/' | '-'))
            .unwrap_or(false)
    }
}

const PRIMARY_TEXT_KEYS: &[&str] = &[
    "text",
    "content",
    "reasoning",
    "thought",
    "thinking",
    "value",
];
const SECONDARY_COLLECTION_KEYS: &[&str] = &[
    "messages", "parts", "items", "entries", "steps", "segments", "records", "output", "outputs",
    "logs",
];

const REASONING_TAGS: &[&str] = &["think", "thinking", "reasoning", "analysis", "thought"];
const ANSWER_TAGS: &[&str] = &["answer", "final"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum TagCategory {
    Reasoning,
    Answer,
}

struct ParsedTag<'a> {
    name: &'a str,
    end_index: usize,
    category: TagCategory,
}

pub(crate) fn extract_reasoning_trace(value: &Value) -> Option<String> {
    let mut segments = Vec::new();
    collect_reasoning_segments(value, &mut segments);
    let combined = segments.join("\n");
    let trimmed = combined.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn collect_reasoning_segments(value: &Value, segments: &mut Vec<String>) {
    match value {
        Value::Null => {}
        Value::Bool(_) | Value::Number(_) => {}
        Value::String(text) => {
            let (mut tagged_segments, cleaned) = split_reasoning_from_text(text);

            if !tagged_segments.is_empty() {
                for segment in tagged_segments.drain(..) {
                    push_unique_segment(segments, &segment);
                }
                if let Some(cleaned_text) = cleaned {
                    let trimmed = cleaned_text.trim();
                    if !trimmed.is_empty() {
                        push_unique_segment(segments, trimmed);
                    }
                }
                return;
            }

            let trimmed = text.trim();
            if trimmed.is_empty() {
                return;
            }

            push_unique_segment(segments, trimmed);
        }
        Value::Array(items) => {
            for item in items {
                collect_reasoning_segments(item, segments);
            }
        }
        Value::Object(map) => {
            let mut matched_key = false;
            for key in PRIMARY_TEXT_KEYS {
                if let Some(nested) = map.get(*key) {
                    collect_reasoning_segments(nested, segments);
                    matched_key = true;
                }
            }

            if !matched_key {
                for key in SECONDARY_COLLECTION_KEYS {
                    if let Some(nested) = map.get(*key) {
                        collect_reasoning_segments(nested, segments);
                        matched_key = true;
                    }
                }
            }

            if !matched_key {
                for nested in map.values() {
                    if matches!(nested, Value::Array(_) | Value::Object(_)) {
                        collect_reasoning_segments(nested, segments);
                    }
                }
            }
        }
    }
}

fn push_unique_segment(segments: &mut Vec<String>, segment: &str) {
    if segment.trim().is_empty() {
        return;
    }

    if segments
        .last()
        .map(|last| last.as_str() == segment)
        .unwrap_or(false)
    {
        return;
    }

    segments.push(segment.to_string());
}

fn parse_start_tag<'a>(lower: &'a str, start: usize) -> Option<ParsedTag<'a>> {
    let bytes = lower.as_bytes();
    let mut index = start + 1;

    if index >= lower.len() {
        return None;
    }

    match bytes[index] {
        b'/' | b'!' | b'?' => return None,
        _ => {}
    }

    while index < lower.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }

    if index >= lower.len() {
        return None;
    }

    let name_start = index;
    while index < lower.len() {
        let ch = bytes[index];
        if ch == b'>' || ch.is_ascii_whitespace() {
            break;
        }
        index += 1;
    }

    if index == name_start {
        return None;
    }

    let mut end_index = index;
    while end_index < lower.len() && bytes[end_index] != b'>' {
        end_index += 1;
    }

    if end_index >= lower.len() {
        return None;
    }

    let name = &lower[name_start..index];
    let category = if REASONING_TAGS.iter().any(|candidate| *candidate == name) {
        TagCategory::Reasoning
    } else if ANSWER_TAGS.iter().any(|candidate| *candidate == name) {
        TagCategory::Answer
    } else {
        return None;
    };

    Some(ParsedTag {
        name,
        end_index,
        category,
    })
}

pub(crate) fn split_reasoning_from_text(text: &str) -> (Vec<String>, Option<String>) {
    if text.trim().is_empty() {
        return (Vec::new(), None);
    }

    let lower = text.to_ascii_lowercase();
    let mut segments: Vec<String> = Vec::new();
    let mut cleaned = String::new();
    let mut modified = false;
    let mut index = 0usize;

    while index < text.len() {
        let Some(relative) = lower[index..].find('<') else {
            cleaned.push_str(&text[index..]);
            break;
        };

        let open_index = index + relative;
        cleaned.push_str(&text[index..open_index]);

        if let Some(tag) = parse_start_tag(&lower, open_index) {
            let content_start = tag.end_index + 1;
            let close_sequence = format!("</{}>", tag.name);

            if let Some(relative_close) = lower[content_start..].find(&close_sequence) {
                let content_end = content_start + relative_close;
                let inner = &text[content_start..content_end];

                match tag.category {
                    TagCategory::Reasoning => {
                        modified = true;
                        let (nested_segments, nested_cleaned) = split_reasoning_from_text(inner);

                        if nested_segments.is_empty() {
                            let trimmed = inner.trim();
                            if !trimmed.is_empty() {
                                push_unique_segment(&mut segments, trimmed);
                            }
                        } else {
                            for segment in nested_segments {
                                push_unique_segment(&mut segments, &segment);
                            }
                            if let Some(cleaned_inner) = nested_cleaned {
                                let trimmed = cleaned_inner.trim();
                                if !trimmed.is_empty() {
                                    push_unique_segment(&mut segments, trimmed);
                                }
                            }
                        }
                    }
                    TagCategory::Answer => {
                        modified = true;
                        let (nested_segments, nested_cleaned) = split_reasoning_from_text(inner);
                        for segment in nested_segments {
                            push_unique_segment(&mut segments, &segment);
                        }
                        if let Some(cleaned_inner) = nested_cleaned {
                            cleaned.push_str(&cleaned_inner);
                        }
                    }
                }

                index = content_end + close_sequence.len();
                continue;
            }
        }

        cleaned.push('<');
        index = open_index + 1;
    }

    if !modified {
        return (segments, None);
    }

    let output = if cleaned.trim().is_empty() {
        None
    } else {
        Some(cleaned)
    };

    (segments, output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_text_from_string() {
        let value = Value::String("  sample reasoning  ".to_string());
        let extracted = extract_reasoning_trace(&value);
        assert_eq!(extracted, Some("sample reasoning".to_string()));
    }

    #[test]
    fn extracts_text_from_nested_array() {
        let value = Value::Array(vec![
            Value::Object(
                serde_json::json!({
                    "type": "thinking",
                    "text": "step one"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
            Value::Object(
                serde_json::json!({
                    "type": "thinking",
                    "text": "step two"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        ]);
        let extracted = extract_reasoning_trace(&value);
        assert_eq!(extracted, Some("step one\nstep two".to_string()));
    }

    #[test]
    fn deduplicates_adjacent_segments() {
        let value = Value::Array(vec![
            Value::String("repeat".to_string()),
            Value::String("repeat".to_string()),
            Value::String("unique".to_string()),
        ]);
        let extracted = extract_reasoning_trace(&value);
        assert_eq!(extracted, Some("repeat\nunique".to_string()));
    }

    #[test]
    fn extracts_reasoning_from_think_markup() {
        let source = "<think>first step</think>\n<answer>final output</answer>";
        let (segments, cleaned) = split_reasoning_from_text(source);
        assert_eq!(segments, vec!["first step".to_string()]);
        assert_eq!(cleaned, Some("\nfinal output".to_string()));
    }

    #[test]
    fn handles_nested_reasoning_markup() {
        let source = "<think><analysis>deep dive</analysis> summary</think>";
        let (segments, cleaned) = split_reasoning_from_text(source);
        assert_eq!(
            segments,
            vec!["deep dive".to_string(), "summary".to_string()]
        );
        assert!(cleaned.is_none());
    }
}
