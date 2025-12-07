//! Loop detection for agent operations
//!
//! Detects when the agent is stuck in repetitive patterns and suggests intervention.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

const MAX_SAME_TOOL_CALLS: usize = 5;
const DETECTION_WINDOW: usize = 10;
const HARD_LIMIT_MULTIPLIER: usize = 2; // Hard stop at 2x soft limit

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

        let normalized_args = normalize_args_for_detection(tool_name, args);
        let tool_name_owned = tool_name.to_owned();

        let mut hasher = DefaultHasher::new();
        normalized_args.to_string().hash(&mut hasher);
        let args_hash = hasher.finish();

        // Check for immediate repetition (same tool + args within last 2 calls)
        if self.recent_calls.len() >= 2 {
            let last_two: Vec<_> = self.recent_calls.iter().rev().take(2).collect();
            if last_two.iter().all(|r| r.tool_name == tool_name_owned && r.args_hash == args_hash) {
                return Some(format!(
                    "HARD STOP: Identical tool call repeated 3+ times: {} with same arguments. This indicates a loop.",
                    tool_name
                ));
            }
        }

        let record = ToolCallRecord {
            tool_name: tool_name_owned.clone(),
            args_hash,
            timestamp: Instant::now(),
        };

        if self.recent_calls.len() >= DETECTION_WINDOW
            && let Some(old) = self.recent_calls.pop_front()
            && let Some(count) = self.tool_counts.get_mut(&old.tool_name)
        {
            *count = count.saturating_sub(1);
        }

        self.recent_calls.push_back(record);
        *self.tool_counts.entry(tool_name_owned).or_insert(0) += 1;

        self.check_for_loops(tool_name)
    }

    fn check_for_loops(&mut self, tool_name: &str) -> Option<String> {
        let count = self.tool_counts.get(tool_name).copied().unwrap_or(0);
        
        // Hard limit check - immediate halt
        if count >= MAX_SAME_TOOL_CALLS * HARD_LIMIT_MULTIPLIER {
            return Some(format!(
                "🛑 CRITICAL: Tool '{}' called {} times (hard limit: {}). Execution halted to prevent infinite loop.\n\
                 Agent must reformulate task or request user guidance.",
                tool_name, count, MAX_SAME_TOOL_CALLS * HARD_LIMIT_MULTIPLIER
            ));
        }

        // Soft limit - warning with cooldown
        if count >= MAX_SAME_TOOL_CALLS {
            let now = Instant::now();
            let should_warn = self.last_warning
                .map(|last| now.duration_since(last).as_secs() > 30)
                .unwrap_or(true);

            if should_warn {
                self.last_warning = Some(now);
                return Some(format!(
                    "⚠️  Loop detected: '{}' called {} times in last {} operations. Consider:\n\
                     • Narrowing scope (specify exact files/paths)\n\
                     • Using different tool (grep_file instead of list_files)\n\
                     • Stopping and reformulating the task\n\
                     Hard limit at {} calls.",
                    tool_name, count, DETECTION_WINDOW, MAX_SAME_TOOL_CALLS * HARD_LIMIT_MULTIPLIER
                ));
            }
        }

        None
    }

    /// Check if hard limit is exceeded (should halt execution)
    pub fn is_hard_limit_exceeded(&self, tool_name: &str) -> bool {
        let count = self.tool_counts.get(tool_name).copied().unwrap_or(0);
        count >= MAX_SAME_TOOL_CALLS * HARD_LIMIT_MULTIPLIER
    }

    /// Get current call count for a tool
    pub fn get_call_count(&self, tool_name: &str) -> usize {
        self.tool_counts.get(tool_name).copied().unwrap_or(0)
    }

    /// Reset tracking for specific tool (use after successful progress)
    pub fn reset_tool(&mut self, tool_name: &str) {
        self.tool_counts.remove(tool_name);
        self.recent_calls.retain(|r| r.tool_name != tool_name);
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
    fn test_immediate_repetition_detection() {
        let mut detector = LoopDetector::new();
        let args = json!({"path": "src/"});

        // First two calls - no warning
        assert!(detector.record_call("grep_file", &args).is_none());
        assert!(detector.record_call("grep_file", &args).is_none());
        
        // Third identical call - hard stop
        let warning = detector.record_call("grep_file", &args);
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("HARD STOP"));
    }

    #[test]
    fn test_root_path_normalization() {
        let mut detector = LoopDetector::new();
        
        // All these should be treated as identical
        let paths = vec![
            json!({"path": "."}),
            json!({"path": ""}),
            json!({"path": "./"}),
            json!({}),
        ];

        for path in &paths[..2] {
            assert!(detector.record_call("list_files", path).is_none());
        }
        
        // Third call with any root variation should trigger
        let warning = detector.record_call("list_files", &paths[2]);
        assert!(warning.is_some());
    }

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
    fn test_hard_limit_enforcement() {
        let mut detector = LoopDetector::new();
        let args = json!({"pattern": "test"});

        // Fill up to hard limit
        for i in 0..MAX_SAME_TOOL_CALLS * HARD_LIMIT_MULTIPLIER {
            let result = detector.record_call("grep_file", &args);
            if i >= MAX_SAME_TOOL_CALLS * HARD_LIMIT_MULTIPLIER - 1 {
                assert!(result.is_some());
                assert!(result.unwrap().contains("CRITICAL"));
            }
        }

        assert!(detector.is_hard_limit_exceeded("grep_file"));
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
