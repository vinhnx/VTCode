/// Parse Ollama pull event JSON responses into structured events.
/// Adapted from OpenAI Codex's codex-ollama/src/parser.rs
use serde_json::Value as JsonValue;

use super::pull::OllamaPullEvent;

/// Convert a single JSON object representing a pull update into one or more events.
pub fn pull_events_from_value(value: &JsonValue) -> Vec<OllamaPullEvent> {
    let mut events = Vec::new();

    if let Some(status) = value.get("status").and_then(|s| s.as_str()) {
        events.push(OllamaPullEvent::Status(status.to_string()));
        if status == "success" {
            events.push(OllamaPullEvent::Success);
        }
    }

    let digest = value
        .get("digest")
        .and_then(|d| d.as_str())
        .unwrap_or("")
        .to_string();
    let total = value.get("total").and_then(JsonValue::as_u64);
    let completed = value.get("completed").and_then(JsonValue::as_u64);

    if total.is_some() || completed.is_some() {
        events.push(OllamaPullEvent::ChunkProgress {
            digest,
            total,
            completed,
        });
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pull_events_decoder_status_and_success() {
        let v: JsonValue = serde_json::json!({"status":"verifying"});
        let events = pull_events_from_value(&v);
        assert_eq!(events.len(), 1);
        match &events[0] {
            OllamaPullEvent::Status(s) => assert_eq!(s, "verifying"),
            _ => panic!("Expected Status event"),
        }

        let v2: JsonValue = serde_json::json!({"status":"success"});
        let events2 = pull_events_from_value(&v2);
        assert_eq!(events2.len(), 2);
        match &events2[0] {
            OllamaPullEvent::Status(s) => assert_eq!(s, "success"),
            _ => panic!("Expected Status event"),
        }
        match &events2[1] {
            OllamaPullEvent::Success => {},
            _ => panic!("Expected Success event"),
        }
    }

    #[test]
    fn test_pull_events_decoder_progress() {
        let v: JsonValue = serde_json::json!({"digest":"sha256:abc","total":100});
        let events = pull_events_from_value(&v);
        assert_eq!(events.len(), 1);
        match &events[0] {
            OllamaPullEvent::ChunkProgress {
                digest,
                total,
                completed,
            } => {
                assert_eq!(digest, "sha256:abc");
                assert_eq!(*total, Some(100));
                assert_eq!(*completed, None);
            }
            _ => panic!("Expected ChunkProgress event"),
        }

        let v2: JsonValue = serde_json::json!({"digest":"sha256:def","completed":42});
        let events2 = pull_events_from_value(&v2);
        assert_eq!(events2.len(), 1);
        match &events2[0] {
            OllamaPullEvent::ChunkProgress {
                digest,
                total,
                completed,
            } => {
                assert_eq!(digest, "sha256:def");
                assert_eq!(*total, None);
                assert_eq!(*completed, Some(42));
            }
            _ => panic!("Expected ChunkProgress event"),
        }
    }
}
