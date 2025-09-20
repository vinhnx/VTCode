use serde_json::Value;

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
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return;
            }
            if segments
                .last()
                .map(|last| last.as_str() == trimmed)
                .unwrap_or(false)
            {
                return;
            }
            segments.push(trimmed.to_string());
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
}
