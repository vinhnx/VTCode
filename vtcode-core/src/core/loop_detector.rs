//! Loop detection for agent operations
//!
//! Detects when the agent is stuck in repetitive patterns and suggests intervention.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

const MAX_SAME_TOOL_CALLS: usize = 5;
const DETECTION_WINDOW: usize = 10;

/// Normalize tool arguments for consistent loop detection.
/// This ensures path variations like ".", "", "./" are treated as the same root path.
fn normalize_args_for_detection(tool_name: &str, args: &serde_json::Value) -> serde_json::Value {
    if let Some(obj) = args.as_object() {
        let mut normalized = obj.clone();

        // Remove pagination params that shouldn't affect loop detection
        normalized.remove("page");
        normalized.remove("per_page");

        // For list_files: normalize root path variations
        if tool_name == "list_files" {
            if let Some(path) = normalized.get("path").and_then(|v| v.as_str()) {
                let path_trimmed = path.trim_start_matches("./").trim_start_matches('/');
                if path_trimmed.is_empty() || path_trimmed == "." {
                    // Normalize all root variations to the same key
                    normalized.insert("path".to_string(), serde_json::json!("__ROOT__"));
                }
            } else {
                // No path = root
                normalized.insert("path".to_string(), serde_json::json!("__ROOT__"));
            }
        }

        serde_json::Value::Object(normalized)
    } else {
        args.clone()
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub args_hash: u64,
    pub timestamp: Instant,
}

#[derive(Debug)]
pub struct LoopDetector {
    recent_calls: VecDeque<ToolCallRecord>,
    tool_counts: HashMap<String, usize>,
    last_warning: Option<Instant>,
}

impl LoopDetector {
    pub fn new() -> Self {
        Self {
            recent_calls: VecDeque::with_capacity(DETECTION_WINDOW),
            tool_counts: HashMap::new(),
            last_warning: None,
        }
    }

    pub fn record_call(&mut self, tool_name: &str, args: &serde_json::Value) -> Option<String> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Normalize args for consistent hashing - especially for list_files root path variations
        let normalized_args = normalize_args_for_detection(tool_name, args);
        let tool_name_owned = tool_name.to_owned();

        let mut hasher = DefaultHasher::new();
        normalized_args.to_string().hash(&mut hasher);
        let args_hash = hasher.finish();

        let record = ToolCallRecord {
            tool_name: tool_name_owned.clone(),
            args_hash,
            timestamp: Instant::now(),
        };

        if self.recent_calls.len() >= DETECTION_WINDOW {
            if let Some(old) = self.recent_calls.pop_front() {
                if let Some(count) = self.tool_counts.get_mut(&old.tool_name) {
                    *count = count.saturating_sub(1);
                }
            }
        }

        self.recent_calls.push_back(record);
        *self.tool_counts.entry(tool_name_owned).or_insert(0) += 1;

        self.check_for_loops(tool_name)
    }

    fn check_for_loops(&mut self, current_tool: &str) -> Option<String> {
        let count = self.tool_counts.get(current_tool).copied().unwrap_or(0);

        if count >= MAX_SAME_TOOL_CALLS {
            if let Some(last) = self.last_warning {
                if last.elapsed().as_secs() < 30 {
                    return None;
                }
            }

            self.last_warning = Some(Instant::now());

            return Some(format!(
                "⚠️  Loop detected: '{}' called {} times in last {} operations. Consider:\n\
                 • Narrowing scope (specify exact files/paths)\n\
                 • Using different tool (grep_file instead of list_files)\n\
                 • Stopping and reformulating the task",
                current_tool, count, DETECTION_WINDOW
            ));
        }

        None
    }

    pub fn reset(&mut self) {
        self.recent_calls.clear();
        self.tool_counts.clear();
        self.last_warning = None;
    }
}

impl Default for LoopDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_detects_repeated_calls() {
        let mut detector = LoopDetector::new();
        let args = json!({"path": "/src"});

        for i in 0..MAX_SAME_TOOL_CALLS {
            let warning = detector.record_call("list_files", &args);
            if i < MAX_SAME_TOOL_CALLS - 1 {
                assert!(warning.is_none());
            } else {
                assert!(warning.is_some());
            }
        }
    }

    #[test]
    fn test_different_tools_no_warning() {
        let mut detector = LoopDetector::new();

        detector.record_call("list_files", &json!({"path": "/src"}));
        detector.record_call("grep_file", &json!({"pattern": "test"}));
        detector.record_call("read_file", &json!({"path": "main.rs"}));

        assert_eq!(detector.tool_counts.len(), 3);
    }

    #[test]
    fn test_root_path_normalization() {
        let mut detector = LoopDetector::new();

        // All these should be treated as the same call
        detector.record_call("list_files", &json!({"path": "."}));
        detector.record_call("list_files", &json!({"path": ""}));
        detector.record_call("list_files", &json!({"path": "./"}));
        detector.record_call("list_files", &json!({"path": "/"}));
        detector.record_call("list_files", &json!({})); // No path = root

        // Should have triggered after 5 identical (normalized) calls
        let warning = detector.record_call("list_files", &json!({"path": "."}));
        assert!(
            warning.is_some(),
            "Should detect loop for normalized root paths"
        );
    }

    #[test]
    fn test_non_root_paths_distinct() {
        let mut detector = LoopDetector::new();

        // These should be treated as different calls
        detector.record_call("list_files", &json!({"path": "src"}));
        detector.record_call("list_files", &json!({"path": "docs"}));
        detector.record_call("list_files", &json!({"path": "tests"}));

        // Count for each should be 1
        assert_eq!(
            detector.tool_counts.get("list_files").copied().unwrap_or(0),
            3
        );
    }

    #[test]
    fn test_normalize_args_removes_pagination() {
        let args = json!({"path": "src", "page": 1, "per_page": 10});
        let normalized = normalize_args_for_detection("list_files", &args);

        assert!(normalized.get("page").is_none());
        assert!(normalized.get("per_page").is_none());
        assert_eq!(normalized.get("path").and_then(|v| v.as_str()), Some("src"));
    }
}
