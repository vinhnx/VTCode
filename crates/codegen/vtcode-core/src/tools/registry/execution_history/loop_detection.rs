//! Repeated-call detection and rate-limit windows.
use super::*;

/// Default window size for loop detection.
///
/// A larger window gives the detector more context across turns, reducing
/// false positives when the model retries a call after a transient failure.
pub(super) const DEFAULT_LOOP_DETECT_WINDOW: usize = 8;
/// Minimum limit for identical readonly operations.
///
/// Read/search calls are cheap to reuse but can become stale across unrelated
/// turns. The threshold must be high enough to allow one legitimate retry
/// (e.g. after an ast-grep parse error or a transient network failure) before
/// the loop detector fires.  A limit of 2 means the same identical call must
/// appear 2 times in the detection window before it is flagged.
const MIN_READONLY_IDENTICAL_LIMIT: usize = 2;
/// Maximum limit for identical readonly operations.
///
/// Mirrors the hard-block threshold in `execution_facade.rs`. Keeping this
/// in sync ensures `set_loop_detection_limits` can never raise the read-only
/// limit so high that every identical call immediately hard-blocks.
const MAX_READONLY_IDENTICAL_LIMIT: usize = 4;

impl ToolExecutionHistory {
    /// Get the current loop limit.
    pub fn loop_limit(&self) -> usize {
        self.identical_limit.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get the effective loop limit for a specific tool.
    pub fn loop_limit_for(&self, tool_name: &str, args: &Value) -> usize {
        self.effective_identical_limit_for_call(tool_name, args)
    }

    /// Get the rate limit per minute if configured.
    pub fn rate_limit_per_minute(&self) -> Option<usize> {
        let val = self.rate_limit_per_minute.load(std::sync::atomic::Ordering::Relaxed);
        (val != 0).then_some(val)
    }

    fn effective_identical_limit_for_call(&self, tool_name: &str, args: &Value) -> usize {
        let base_limit = self.identical_limit.load(std::sync::atomic::Ordering::Relaxed);
        if is_read_style_tool_call(tool_name, args) || tool_name_matches(tool_name, tools::CODE_SEARCH) {
            // Read-only tools: clamp to [MIN, MAX] so the limit cannot grow
            // unbounded via set_loop_detection_limits, while still allowing
            // callers to lower it below the default for aggressive dedup.
            base_limit.clamp(MIN_READONLY_IDENTICAL_LIMIT, MAX_READONLY_IDENTICAL_LIMIT)
        } else {
            base_limit
        }
    }

    /// Count calls within a time window.
    pub fn calls_in_window(&self, window: Duration) -> usize {
        let cutoff = SystemTime::now().checked_sub(window).unwrap_or(SystemTime::UNIX_EPOCH);

        let Ok(records) = self.records.read() else {
            return 0;
        };
        records.iter().rev().take_while(|record| record.timestamp >= cutoff).count()
    }

    /// Detect if the agent is stuck in a loop.
    ///
    /// Returns a `LoopDetectionResult` indicating whether a loop was detected.
    pub fn detect_loop(&self, tool_name: &str, args: &Value) -> LoopDetectionResult {
        let limit = self.effective_identical_limit_for_call(tool_name, args);
        if limit == 0 {
            return LoopDetectionResult {
                detected: false,
                repeat_count: 0,
                tool_name: tool_name.to_string(),
            };
        }

        let detect_window = self.detect_window.load(std::sync::atomic::Ordering::Relaxed);
        let window = detect_window.max(limit.saturating_mul(2)).max(1);

        let Ok(records) = self.records.read() else {
            return LoopDetectionResult {
                detected: false,
                repeat_count: 0,
                tool_name: tool_name.to_string(),
            };
        };
        let recent: Vec<&ToolExecutionRecord> = records.iter().rev().take(window).collect();

        if recent.is_empty() {
            return LoopDetectionResult {
                detected: false,
                repeat_count: 0,
                tool_name: tool_name.to_string(),
            };
        }

        // Count how many recent calls match this tool's loop identity.
        // CRITICAL FIX: Only count SUCCESSFUL calls to avoid cascade blocking
        let mut identical_count = 0;
        let is_tracker = tool_name == tools::TASK_TRACKER;
        let tracker_start = is_tracker
            .then(|| self.tracker_loop_start.read().ok().and_then(|start| *start))
            .flatten();
        for record in &recent {
            if tracker_start.is_some_and(|start| record.timestamp < start) {
                break;
            }
            if !record.success {
                continue;
            }
            // Tracker state is live: intervening work or a different tracker
            // operation starts a new observation/update streak. Do not carry
            // old polls across a checklist update or verification step.
            if is_tracker && (record.tool_name != tool_name || record.args != *args) {
                break;
            }
            let same_args = if tool_name_matches(tool_name, tools::CODE_SEARCH) {
                crate::tools::normalised_code_search_loop_identity(&record.args)
                    == crate::tools::normalised_code_search_loop_identity(args)
            } else {
                record.args == *args
            };
            if record.tool_name == tool_name && same_args {
                identical_count += 1;
            }
        }

        let detected = identical_count >= limit;
        LoopDetectionResult {
            detected,
            repeat_count: identical_count,
            tool_name: tool_name.to_string(),
        }
    }
}
