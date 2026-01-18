//! Loop detection for agent operations
//!
//! Detects when the agent is stuck in repetitive patterns and suggests intervention.

use crate::config::constants::{defaults, tools};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

// Separate limits for different operation types to reduce false positives
const MAX_READONLY_TOOL_CALLS: usize = 10; // read_file, grep_file, list_files
const MAX_WRITE_TOOL_CALLS: usize = 3; // write_file, edit_file, apply_patch
const MAX_COMMAND_TOOL_CALLS: usize = 5; // shell, run_pty_cmd
const MAX_OTHER_TOOL_CALLS: usize = 3; // Other tools (default)
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
        if tool_name == tools::LIST_FILES {
            if let Some(path) = normalized.get("path").and_then(|v| v.as_str()) {
                let trimmed = path.trim();
                let only_root_markers = trimmed.trim_matches(|c| c == '.' || c == '/').is_empty();
                if trimmed.is_empty() || only_root_markers {
                    // Normalize any root-only path markers (., /, ././, //, etc.) to the same key
                    normalized.insert("path".into(), serde_json::json!("__ROOT__"));
                }
            } else {
                // No path = root
                normalized.insert("path".into(), serde_json::json!("__ROOT__"));
            }
        }

        serde_json::Value::Object(normalized)
    } else {
        args.clone()
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub tool_name: &'static str, // Use &'static str for known tool names to avoid allocations
    pub args_hash: u64,
    pub timestamp: Instant,
}

#[derive(Debug)]
pub struct LoopDetector {
    recent_calls: VecDeque<ToolCallRecord>,
    tool_counts: HashMap<String, usize>,
    last_warning: Option<Instant>,
    max_identical_call_limit: Option<usize>,
    custom_limits: HashMap<String, usize>,
}

impl LoopDetector {
    pub fn new() -> Self {
        Self::with_max_repeated_calls(defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS)
    }

    pub fn with_max_repeated_calls(limit: usize) -> Self {
        let normalized_limit = if limit > 1 { Some(limit) } else { None };
        Self {
            recent_calls: VecDeque::with_capacity(DETECTION_WINDOW),
            tool_counts: HashMap::new(),
            last_warning: None,
            max_identical_call_limit: normalized_limit,
            custom_limits: HashMap::new(),
        }
    }
    
    /// Set a custom limit for a specific tool.
    /// This overrides the default category-based limits.
    pub fn set_tool_limit(&mut self, tool_name: &str, limit: usize) {
        self.custom_limits.insert(tool_name.to_string(), limit);
    }

    pub fn record_call(&mut self, tool_name: &str, args: &serde_json::Value) -> Option<String> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let normalized_args = normalize_args_for_detection(tool_name, args);

        let mut hasher = DefaultHasher::new();
        // OPTIMIZATION: Hash the value directly instead of converting to string first
        if let Ok(bytes) = serde_json::to_vec(&normalized_args) {
            bytes.hash(&mut hasher);
        } else {
            normalized_args.to_string().hash(&mut hasher);
        }
        let args_hash = hasher.finish();

        if let Some(limit) = self.max_identical_call_limit {
            let required_history = limit.saturating_sub(1);
            if required_history > 0 && self.recent_calls.len() >= required_history {
                let identical = self
                    .recent_calls
                    .iter()
                    .rev()
                    .take(required_history)
                    .all(|record| record.tool_name == tool_name && record.args_hash == args_hash);

                if identical {
                    // Escalate to hard limit so callers halt immediately.
                    let hard_limit = self.get_limit_for_tool(tool_name) * HARD_LIMIT_MULTIPLIER;
                    self.tool_counts.insert(tool_name.to_string(), hard_limit);

                    return Some(format!(
                        "HARD STOP: Identical tool call repeated {} times: {} with same arguments. This indicates a loop.",
                        limit, tool_name
                    ));
                }
            }
        }

        let record = ToolCallRecord {
            tool_name: Box::leak(tool_name.to_string().into_boxed_str()), // Leak for &'static str
            args_hash,
            timestamp: Instant::now(),
        };

        if self.recent_calls.len() >= DETECTION_WINDOW
            && let Some(old) = self.recent_calls.pop_front()
            && let Some(count) = self.tool_counts.get_mut(old.tool_name)
        {
            *count = count.saturating_sub(1);
        }

        self.recent_calls.push_back(record);
        *self.tool_counts.entry(tool_name.to_string()).or_insert(0) += 1;

        if let Some(pattern_warning) = self.detect_patterns() {
            return Some(pattern_warning);
        }

        self.check_for_loops(tool_name)
    }

    fn check_for_loops(&mut self, tool_name: &str) -> Option<String> {
        let count = self.tool_counts.get(tool_name).copied().unwrap_or(0);

        // Determine tool-specific limits
        let max_calls = self.get_limit_for_tool(tool_name);

        // Hard limit check - immediate halt
        let hard_limit = max_calls * HARD_LIMIT_MULTIPLIER;
        if count >= hard_limit {
            return Some(format!(
                "CRITICAL: Tool '{}' called {} times (hard limit: {}). Execution halted to prevent infinite loop.\n\
                 Agent must reformulate task or request user guidance.",
                tool_name, count, hard_limit
            ));
        }

        // Soft limit - warning with cooldown and alternative suggestions
        if count >= max_calls {
            let now = Instant::now();
            let should_warn = self
                .last_warning
                .map(|last| now.duration_since(last).as_secs() > 30)
                .unwrap_or(true);

            if should_warn {
                self.last_warning = Some(now);
                let alternatives = Self::suggest_alternative_for_tool(tool_name);

                return Some(format!(
                    "Loop detected: '{}' called {} times in last {} operations.\n\n\
                     {}\n\n\
                     Hard limit at {} calls.",
                    tool_name, count, DETECTION_WINDOW, alternatives, hard_limit
                ));
            }
        }

        None
    }

    /// Check if hard limit is exceeded (should halt execution)
    pub fn is_hard_limit_exceeded(&self, tool_name: &str) -> bool {
        let count = self.tool_counts.get(tool_name).copied().unwrap_or(0);
        let max_calls = self.get_limit_for_tool(tool_name);
        count >= max_calls * HARD_LIMIT_MULTIPLIER
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

    /// Suggest alternative approaches for common loop patterns
    pub fn suggest_alternative(&self, tool_name: &str) -> Option<String> {
        match tool_name {
            tools::LIST_FILES => Some(
                "Instead of listing files repeatedly:\n\
                 • Use grep_file to search for specific patterns\n\
                 • Target specific subdirectories (e.g., 'src/', 'tests/')\n\
                 • Use read_file if you know the exact file path"
                    .to_string(),
            ),
            tools::GREP_FILE => Some(
                "Instead of grepping repeatedly:\n\
                 • Refine your search pattern to be more specific\n\
                 • Use read_file to examine specific files\n\
                 • Consider using execute_code for complex filtering"
                    .to_string(),
            ),
            tools::READ_FILE => Some(
                "Instead of reading files repeatedly:\n\
                 • Use grep_file to find specific content first\n\
                 • Read specific line ranges with read_range parameter\n\
                 • Consider if you already have the information needed"
                    .to_string(),
            ),
            tools::SEARCH_TOOLS => Some(
                "Instead of searching tools repeatedly:\n\
                 • Review the tools you've already discovered\n\
                 • Use execute_code to call tools directly\n\
                 • Check if you need a different approach to the task"
                    .to_string(),
            ),
            _ => Some(
                "Consider:\n\
                 • Using a different tool for this task\n\
                 • Breaking down the problem into smaller steps\n\
                 • Asking for user guidance if stuck"
                    .to_string(),
            ),
        }
    }

    /// Get the number of tools currently being tracked
    pub fn get_tracked_tool_count(&self) -> usize {
        self.tool_counts.len()
    }

    pub fn reset(&mut self) {
        self.recent_calls.clear();
        self.tool_counts.clear();
        self.last_warning = None;
    }

    /// Get limit for a specific tool.
    /// Checks custom limits first, then falls back to category defaults.
    #[inline]
    fn get_limit_for_tool(&self, tool_name: &str) -> usize {
        if let Some(&limit) = self.custom_limits.get(tool_name) {
            return limit;
        }

        match tool_name {
            tools::READ_FILE | tools::GREP_FILE | tools::LIST_FILES => MAX_READONLY_TOOL_CALLS,
            tools::WRITE_FILE | tools::EDIT_FILE | "apply_patch" => MAX_WRITE_TOOL_CALLS,
            tools::RUN_PTY_CMD | "shell" => MAX_COMMAND_TOOL_CALLS,
            _ => MAX_OTHER_TOOL_CALLS,
        }
    }

    /// Suggest alternatives for stuck tools (extracted to static method for efficiency)
    #[inline]
    fn suggest_alternative_for_tool(tool_name: &str) -> String {
        match tool_name {
            tools::LIST_FILES => "Instead of listing repeatedly:\n\
                 • Use grep_file to search for specific patterns\n\
                 • Target specific subdirectories (e.g., 'src/', 'tests/')\n\
                 • Use read_file if you know the exact file path"
                .to_string(),
            tools::GREP_FILE => "Instead of grepping repeatedly:\n\
                 • Refine your search pattern to be more specific\n\
                 • Use read_file to examine specific files\n\
                 • Consider using execute_code for complex filtering"
                .to_string(),
            tools::READ_FILE => "Instead of reading files repeatedly:\n\
                 • Use grep_file to find specific content first\n\
                 • Read specific line ranges with read_range parameter\n\
                 • Consider if you already have the information needed"
                .to_string(),
            tools::SEARCH_TOOLS => "Instead of searching tools repeatedly:\n\
                 • Review the tools you've already discovered\n\
                 • Use execute_code to call tools directly\n\
                 • Check if you need a different approach to the task"
                .to_string(),
            _ => "Consider:\n\
                 • Using a different tool for this task\n\
                 • Breaking down the problem into smaller steps\n\
                 • Asking for user guidance if stuck"
                .to_string(),
        }
    }

    /// Detect complex repetitive patterns (e.g. A -> B -> A -> B)
    fn detect_patterns(&self) -> Option<String> {
        let history: Vec<(&str, u64)> = self
            .recent_calls
            .iter()
            .map(|r| (r.tool_name, r.args_hash))
            .collect();

        let len = history.len();
        if len < 4 {
            return None;
        }

        // Check for patterns of length K where 2*K <= len
        // We look for imminent repetition: [.. A, B, A, B]
        for k in 2..=(len / 2) {
            let suffix = &history[len - k..];
            let prev = &history[len - 2 * k..len - k];

            if suffix == prev {
                let pattern_desc: Vec<&str> = suffix.iter().map(|(name, _)| *name).collect();
                let pattern_str = pattern_desc.join(" -> ");

                return Some(format!(
                    "Repetitive pattern detected: [{}]\n\
                     The agent appears to be cycling through the same actions. \
                     Please pause and reassess the strategy.",
                     pattern_str
                ));
            }
            
            // Fuzzy detection: if tool names match but hashes differ, check semantic similarity?
            // For now, simpler fuzzy check: ignore edit_file content arguments? 
            // Better: Detecting "oscillating" behavior A->B->A->B even if args slightly differ.
            // If tool names match exactly for a sequence of length >= 3
            let suffix_names: Vec<&str> = suffix.iter().map(|(n, _)| *n).collect();
            let prev_names: Vec<&str> = prev.iter().map(|(n, _)| *n).collect();
            
            if suffix_names == prev_names && k >= 2 {
                 return Some(format!(
                    "Oscillating tool pattern detected: [{}]\n\
                     The agent is repeating the same sequence of tools. \
                     Ensure you are making actual progress.",
                     suffix_names.join(" -> ")
                ));
            }
        }

        None
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
        assert!(detector.record_call(tools::GREP_FILE, &args).is_none());
        assert!(detector.record_call(tools::GREP_FILE, &args).is_none());

        // Third identical call - hard stop
        let warning = detector.record_call(tools::GREP_FILE, &args);
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
            json!({"path": "././"}),
            json!({"path": "//"}),
            json!({}),
        ];

        for path in &paths[..2] {
            assert!(detector.record_call(tools::LIST_FILES, path).is_none());
        }

        // Third call with any root variation should trigger
        let warning = detector.record_call(tools::LIST_FILES, &paths[2]);
        assert!(warning.is_some());

        // Further root-only variations should continue to warn
        for path in &paths[3..] {
            assert!(detector.record_call(tools::LIST_FILES, path).is_some());
        }
    }

    #[test]
    fn test_detects_repeated_calls() {
        let mut detector = LoopDetector::new();
        let args = json!({"path": "/src"});

        // list_files uses MAX_READONLY_TOOL_CALLS (10)
        for i in 0..MAX_READONLY_TOOL_CALLS {
            let warning = detector.record_call(tools::LIST_FILES, &args);
            if i < MAX_READONLY_TOOL_CALLS - 1 {
                assert!(warning.is_none());
            } else {
                assert!(warning.is_some());
                assert!(warning.unwrap().contains("Loop detected"));
            }
        }
    }

    #[test]
    fn test_hard_limit_enforcement() {
        let mut detector = LoopDetector::new();
        let args = json!({"pattern": "test"});

        // grep_file uses MAX_READONLY_TOOL_CALLS (10), hard limit is 20
        let hard_limit = MAX_READONLY_TOOL_CALLS * HARD_LIMIT_MULTIPLIER;
        for i in 0..hard_limit {
            let result = detector.record_call(tools::GREP_FILE, &args);
            if i >= hard_limit - 1 {
                assert!(result.is_some());
                assert!(result.unwrap().contains("CRITICAL"));
            }
        }

        assert!(detector.is_hard_limit_exceeded(tools::GREP_FILE));
    }

    #[test]
    fn test_different_tools_no_warning() {
        let mut detector = LoopDetector::new();

        detector.record_call(tools::LIST_FILES, &json!({"path": "/src"}));
        detector.record_call(tools::GREP_FILE, &json!({"pattern": "test"}));
        detector.record_call(tools::READ_FILE, &json!({"path": "main.rs"}));

        assert_eq!(detector.tool_counts.len(), 3);
    }

    #[test]
    fn test_non_root_paths_distinct() {
        let mut detector = LoopDetector::new();

        // These should be treated as different calls
        detector.record_call(tools::LIST_FILES, &json!({"path": "src"}));
        detector.record_call(tools::LIST_FILES, &json!({"path": "docs"}));
        detector.record_call(tools::LIST_FILES, &json!({"path": "tests"}));

        // Count for each should be 1
        assert_eq!(
            detector
                .tool_counts
                .get(tools::LIST_FILES)
                .copied()
                .unwrap_or(0),
            3
        );
    }

    #[test]
    fn test_identical_calls_trigger_hard_limit() {
        let mut detector = LoopDetector::new();
        let args = json!({"path": "."});

        assert!(detector.record_call(tools::READ_FILE, &args).is_none());
        assert!(detector.record_call(tools::READ_FILE, &args).is_none());

        let warning = detector.record_call(tools::READ_FILE, &args);
        assert!(warning.is_some());
        assert!(detector.is_hard_limit_exceeded(tools::READ_FILE));
    }

    #[test]
    fn test_normalize_args_removes_pagination() {
        let args = json!({"path": "src", "page": 1, "per_page": 10});
        let normalized = normalize_args_for_detection(tools::LIST_FILES, &args);

        assert!(normalized.get("page").is_none());
        assert!(normalized.get("per_page").is_none());
        assert_eq!(normalized.get("path").and_then(|v| v.as_str()), Some("src"));
    }

    #[test]
    fn test_reset_tool_clears_specific_tool() {
        let mut detector = LoopDetector::new();
        let args = json!({"path": "src"});

        // Record calls for multiple tools
        detector.record_call(tools::LIST_FILES, &args);
        detector.record_call(tools::LIST_FILES, &args);
        detector.record_call(tools::GREP_FILE, &json!({"pattern": "test"}));

        assert_eq!(detector.get_call_count(tools::LIST_FILES), 2);
        assert_eq!(detector.get_call_count(tools::GREP_FILE), 1);

        // Reset only list_files
        detector.reset_tool(tools::LIST_FILES);

        assert_eq!(detector.get_call_count(tools::LIST_FILES), 0);
        assert_eq!(detector.get_call_count(tools::GREP_FILE), 1);
    }

    #[test]
    fn test_suggest_alternative_for_list_files() {
        let detector = LoopDetector::new();
        let suggestion = detector.suggest_alternative(tools::LIST_FILES);

        assert!(suggestion.is_some());
        let msg = suggestion.unwrap();
        assert!(msg.contains(tools::GREP_FILE));
        assert!(msg.contains("subdirectories"));
    }

    #[test]
    fn test_suggest_alternative_for_grep_file() {
        let detector = LoopDetector::new();
        let suggestion = detector.suggest_alternative(tools::GREP_FILE);

        assert!(suggestion.is_some());
        let msg = suggestion.unwrap();
        assert!(msg.contains(tools::READ_FILE));
        assert!(msg.contains("pattern"));
    }

    #[test]
    fn test_suggest_alternative_for_unknown_tool() {
        let detector = LoopDetector::new();
        let suggestion = detector.suggest_alternative("unknown_tool");

        assert!(suggestion.is_some());
        let msg = suggestion.unwrap();
        assert!(msg.contains("different tool"));
    }

    #[test]
    fn test_faster_detection_with_lower_limit() {
        let mut detector = LoopDetector::new();
        let args = json!({"path": "src"});

        // First call - no warning
        assert!(detector.record_call(tools::LIST_FILES, &args).is_none());

        // Second call - no warning
        assert!(detector.record_call(tools::LIST_FILES, &args).is_none());

        // Third call - should trigger warning (soft limit = 3)
        let warning = detector.record_call(tools::LIST_FILES, &args);
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("Loop detected"));
    }
}
