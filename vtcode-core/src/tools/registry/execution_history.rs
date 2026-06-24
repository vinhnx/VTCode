//! Tool execution history and records.
//!
//! This module provides thread-safe recording and querying of tool executions,
//! including loop detection and rate limiting.

use std::collections::VecDeque;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use crate::config::constants::{defaults, tools};
use crate::tools::continuation::read_chunk_progress_from_result;
use crate::tools::tool_intent;

/// Result of loop detection analysis.
#[derive(Debug, Clone)]
pub struct LoopDetectionResult {
    /// Whether a loop was detected.
    pub detected: bool,
    /// Number of identical consecutive calls found.
    pub repeat_count: usize,
    /// Name of the tool being checked.
    pub tool_name: String,
}

/// Snapshot of harness context for execution records.
#[derive(Debug, Clone)]
pub struct HarnessContextSnapshot {
    pub session_id: String,
    pub task_id: Option<String>,
}

impl HarnessContextSnapshot {
    /// Create a new harness context snapshot.
    pub fn new(session_id: String, task_id: Option<String>) -> Self {
        Self {
            session_id,
            task_id,
        }
    }

    /// Serialize snapshot for middleware/telemetry consumers without cloning callers.
    pub fn to_json(&self) -> Value {
        json!({
            "session_id": self.session_id,
            "task_id": self.task_id,
        })
    }
}

/// Record of a single tool execution for diagnostics.
#[derive(Debug, Clone)]
pub struct ToolExecutionRecord {
    pub tool_name: String,
    pub requested_name: String,
    pub is_mcp: bool,
    pub mcp_provider: Option<String>,
    pub args: Value,
    pub result: Result<Value, String>,
    pub timestamp: SystemTime,
    pub success: bool,
    pub context: HarnessContextSnapshot,
    pub timeout_category: Option<String>,
    pub base_timeout_ms: Option<u64>,
    pub adaptive_timeout_ms: Option<u64>,
    pub effective_timeout_ms: Option<u64>,
    pub circuit_breaker: bool,
    pub attempt: u32,
    pub retry_after_ms: Option<u64>,
    pub circuit_breaker_state: Option<String>,
}

impl ToolExecutionRecord {
    /// Create a new failed execution record.
    #[expect(clippy::too_many_arguments)]
    #[cold]
    pub fn failure(
        tool_name: String,
        requested_name: String,
        is_mcp: bool,
        mcp_provider: Option<String>,
        args: Value,
        error_msg: String,
        context: HarnessContextSnapshot,
        timeout_category: Option<String>,
        base_timeout_ms: Option<u64>,
        adaptive_timeout_ms: Option<u64>,
        effective_timeout_ms: Option<u64>,
        circuit_breaker: bool,
    ) -> Self {
        Self {
            tool_name,
            requested_name,
            is_mcp,
            mcp_provider,
            args,
            result: Err(error_msg),
            timestamp: SystemTime::now(),
            success: false,
            context,
            timeout_category,
            base_timeout_ms,
            adaptive_timeout_ms,
            effective_timeout_ms,
            circuit_breaker,
            attempt: 1,
            retry_after_ms: None,
            circuit_breaker_state: None,
        }
    }

    /// Create a new successful execution record.
    #[expect(clippy::too_many_arguments)]
    #[inline]
    pub fn success(
        tool_name: String,
        requested_name: String,
        is_mcp: bool,
        mcp_provider: Option<String>,
        args: Value,
        result: Value,
        context: HarnessContextSnapshot,
        timeout_category: Option<String>,
        base_timeout_ms: Option<u64>,
        adaptive_timeout_ms: Option<u64>,
        effective_timeout_ms: Option<u64>,
        circuit_breaker: bool,
    ) -> Self {
        Self {
            tool_name,
            requested_name,
            is_mcp,
            mcp_provider,
            args,
            result: Ok(result),
            timestamp: SystemTime::now(),
            success: true,
            context,
            timeout_category,
            base_timeout_ms,
            adaptive_timeout_ms,
            effective_timeout_ms,
            circuit_breaker,
            attempt: 1,
            retry_after_ms: None,
            circuit_breaker_state: None,
        }
    }

    #[inline]
    pub fn with_attempt(mut self, attempt: u32) -> Self {
        self.attempt = attempt.max(1);
        self
    }

    #[inline]
    pub fn with_retry_after(mut self, retry_after: Option<Duration>) -> Self {
        self.retry_after_ms =
            retry_after.map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64);
        self
    }

    #[inline]
    pub fn with_circuit_breaker_state(mut self, state: impl Into<String>) -> Self {
        self.circuit_breaker_state = Some(state.into());
        self
    }
}

/// Default window size for loop detection.
///
/// A larger window gives the detector more context across turns, reducing
/// false positives when the model retries a call after a transient failure.
const DEFAULT_LOOP_DETECT_WINDOW: usize = 8;
/// Minimum limit for identical readonly operations.
///
/// Read/search calls are cheap to reuse but can become stale across unrelated
/// turns. The threshold must be high enough to allow one legitimate retry
/// (e.g. after an ast-grep parse error or a transient network failure) before
/// the loop detector fires.  A limit of 3 means the same identical call must
/// appear 3 times in the detection window before it is flagged.
const MIN_READONLY_IDENTICAL_LIMIT: usize = 3;

fn spool_path_exists(result: &Value) -> bool {
    let Some(spool_path) = result.get("spool_path").and_then(|v| v.as_str()) else {
        return true;
    };

    let path = Path::new(spool_path);
    if path.is_absolute() {
        return path.exists();
    }

    path.exists()
        || env::current_dir()
            .ok()
            .is_some_and(|cwd| cwd.join(path).exists())
}

fn read_file_path_from_args(args: &Value) -> Option<&str> {
    let obj = args.as_object()?;
    for key in ["path", "file_path", "filepath", "target_path", "file"] {
        if let Some(path) = obj.get(key).and_then(|v| v.as_str()) {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn normalize_tool_name_for_match(name: &str) -> String {
    let normalized = name.trim().to_ascii_lowercase().replace(' ', "_");
    tool_intent::canonical_unified_exec_tool_name(&normalized)
        .unwrap_or(&normalized)
        .to_string()
}

fn is_read_file_tool_name(name: &str) -> bool {
    let normalized = normalize_tool_name_for_match(name);
    normalized == tools::READ_FILE || normalized.ends_with(".read_file")
}

fn is_unified_file_tool_name(name: &str) -> bool {
    let normalized = normalize_tool_name_for_match(name);
    normalized == tools::UNIFIED_FILE || normalized.ends_with(".unified_file")
}

fn tool_name_matches(name: &str, expected: &str) -> bool {
    let normalized = normalize_tool_name_for_match(name);
    normalized == expected || normalized.ends_with(&format!(".{expected}"))
}

fn is_read_style_tool_call(tool_name: &str, args: &Value) -> bool {
    if tool_name_matches(tool_name, tools::READ_FILE) {
        return true;
    }
    if is_unified_file_tool_name(tool_name) {
        return tool_intent::unified_file_action_is(args, "read");
    }
    false
}

fn normalize_path_for_match(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

fn to_absolute_path(path: &str) -> Option<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let raw = Path::new(trimmed);
    if raw.is_absolute() {
        return Some(raw.to_path_buf());
    }
    env::current_dir().ok().map(|cwd| cwd.join(raw))
}

fn paths_match(record_path: &str, expected_path: &str) -> bool {
    let lhs = normalize_path_for_match(record_path);
    let rhs = normalize_path_for_match(expected_path);
    if lhs == rhs {
        return true;
    }
    if lhs.ends_with(&format!("/{rhs}")) || rhs.ends_with(&format!("/{lhs}")) {
        return true;
    }

    match (
        to_absolute_path(record_path),
        to_absolute_path(expected_path),
    ) {
        (Some(abs_lhs), Some(abs_rhs)) => abs_lhs == abs_rhs,
        _ => false,
    }
}

fn is_read_file_style_record(record: &ToolExecutionRecord) -> bool {
    if is_read_file_tool_name(&record.tool_name) {
        return true;
    }

    if !is_unified_file_tool_name(&record.tool_name) {
        return false;
    }

    tool_intent::unified_file_action_is(&record.args, "read")
}

/// Thread-safe execution history for recording tool executions.
#[derive(Clone)]
pub struct ToolExecutionHistory {
    records: Arc<RwLock<VecDeque<ToolExecutionRecord>>>,
    max_records: usize,
    detect_window: Arc<std::sync::atomic::AtomicUsize>,
    identical_limit: Arc<std::sync::atomic::AtomicUsize>,
    rate_limit_per_minute: Arc<std::sync::atomic::AtomicUsize>,
}

impl ToolExecutionHistory {
    /// Create a new execution history with a maximum record count.
    pub fn new(max_records: usize) -> Self {
        Self {
            records: Arc::new(RwLock::new(VecDeque::new())),
            max_records,
            detect_window: Arc::new(std::sync::atomic::AtomicUsize::new(
                DEFAULT_LOOP_DETECT_WINDOW,
            )),
            identical_limit: Arc::new(std::sync::atomic::AtomicUsize::new(
                defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS,
            )),
            rate_limit_per_minute: Arc::new(std::sync::atomic::AtomicUsize::new(
                crate::tools::rate_limit_config::tool_calls_per_minute_from_env().unwrap_or(0),
            )),
        }
    }

    /// Add a record to the history.
    pub fn add_record(&self, record: ToolExecutionRecord) {
        let Ok(mut records) = self.records.write() else {
            return;
        };
        records.push_back(record);
        while records.len() > self.max_records {
            records.pop_front();
        }
    }

    /// Set loop detection parameters.
    pub fn set_loop_detection_limits(&self, detect_window: usize, identical_limit: usize) {
        self.detect_window
            .store(detect_window.max(1), std::sync::atomic::Ordering::Relaxed);
        self.identical_limit
            .store(identical_limit, std::sync::atomic::Ordering::Relaxed);
    }

    /// Set the rate limit for tool executions per minute.
    pub fn set_rate_limit_per_minute(&self, limit: Option<usize>) {
        self.rate_limit_per_minute.store(
            limit.filter(|v| *v > 0).unwrap_or(0),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    /// Get the most recent records.
    pub fn get_recent_records(&self, count: usize) -> Vec<ToolExecutionRecord> {
        let Ok(records) = self.records.read() else {
            return Vec::new();
        };
        let records_len = records.len();
        let start = records_len.saturating_sub(count);
        records.iter().skip(start).cloned().collect()
    }

    /// Get recent failures in chronological order.
    pub fn get_recent_failures(&self, count: usize) -> Vec<ToolExecutionRecord> {
        let Ok(records) = self.records.read() else {
            return Vec::new();
        };
        let mut failures: Vec<ToolExecutionRecord> = records
            .iter()
            .rev()
            .filter(|r| !r.success)
            .take(count)
            .cloned()
            .collect();
        failures.reverse();
        failures
    }

    /// Find the most recent spooled output for a tool call with identical args.
    pub fn find_recent_spooled_result(
        &self,
        tool_name: &str,
        args: &Value,
        max_age: Duration,
    ) -> Option<Value> {
        let records = self.records.read().ok()?;
        let now = SystemTime::now();

        for record in records.iter().rev() {
            if record.tool_name != tool_name || !record.success || record.args != *args {
                continue;
            }

            let age_ok = match now.duration_since(record.timestamp) {
                Ok(age) => age <= max_age,
                Err(_) => false,
            };
            if !age_ok {
                continue;
            }

            if let Ok(result) = &record.result
                && result.get("spool_path").and_then(|v| v.as_str()).is_some()
                && spool_path_exists(result)
            {
                return Some(result.clone());
            }
        }
        None
    }

    /// Find the most recent successful output for a tool call with identical args.
    pub fn find_recent_successful_result(
        &self,
        tool_name: &str,
        args: &Value,
        max_age: Duration,
    ) -> Option<Value> {
        let records = self.records.read().ok()?;
        let now = SystemTime::now();

        for record in records.iter().rev() {
            if record.tool_name != tool_name || !record.success || record.args != *args {
                continue;
            }

            let age_ok = match now.duration_since(record.timestamp) {
                Ok(age) => age <= max_age,
                Err(_) => false,
            };
            if !age_ok {
                continue;
            }

            if let Ok(result) = &record.result {
                if result.get("spool_path").and_then(|v| v.as_str()).is_some() {
                    let Some(spool_path) = result.get("spool_path").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    if !Path::new(spool_path).exists() {
                        continue;
                    }
                }
                return Some(result.clone());
            }
        }

        None
    }

    /// Find the most recent successful output for a read-only tool call that
    /// targets the same file path and compatible read shape. This enables
    /// cross-turn dedup only when the cached read covers the new request.
    ///
    /// Returns `None` for non-read-only tools or when no matching path can be
    /// extracted from the args.
    pub fn find_recent_successful_by_read_target(
        &self,
        tool_name: &str,
        query_args: &Value,
        max_age: Duration,
    ) -> Option<Value> {
        let query_path = Self::extract_read_target(tool_name, query_args)?;
        let records = self.records.read().ok()?;
        let now = SystemTime::now();

        for record in records.iter().rev() {
            if record.tool_name != tool_name || !record.success {
                continue;
            }
            let Some(record_path) = Self::extract_read_target(tool_name, &record.args) else {
                continue;
            };
            if record_path != query_path {
                continue;
            }
            // Read-shape check: only match if the cached result covers the
            // query's extent and has the same raw/summarized mode.  A query
            // asking for a larger limit, different offset, or raw content is a
            // materially different read — the model genuinely needs fresh
            // content, not a cached stub.
            if !Self::read_extent_matches(&record.args, query_args) {
                continue;
            }
            let age_ok = match now.duration_since(record.timestamp) {
                Ok(age) => age <= max_age,
                Err(_) => false,
            };
            if !age_ok {
                continue;
            }
            if let Ok(result) = &record.result {
                if result.get("spool_path").and_then(|v| v.as_str()).is_some() {
                    let Some(spool_path) = result.get("spool_path").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    if !Path::new(spool_path).exists() {
                        continue;
                    }
                }
                return Some(result.clone());
            }
        }
        None
    }

    /// Check whether the cached record's read shape covers the new query's shape.
    ///
    /// Returns `true` when both calls target the same offset and the cached
    /// limit is at least as large as the query limit, and both calls use the
    /// same raw mode. This prevents false loop detection when the model
    /// requests a larger limit, different offset, or exact raw content after a
    /// summarized read (issue #680).
    fn read_extent_matches(cached_args: &Value, query_args: &Value) -> bool {
        let cached_raw = cached_args
            .get("raw")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let query_raw = query_args
            .get("raw")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if cached_raw != query_raw {
            return false;
        }

        let cached_offset = cached_args
            .get("offset")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let query_offset = query_args
            .get("offset")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if cached_offset != query_offset {
            return false;
        }

        let cached_limit = cached_args.get("limit").and_then(Value::as_u64);
        let query_limit = query_args.get("limit").and_then(Value::as_u64);
        match (cached_limit, query_limit) {
            (Some(c), Some(q)) => c >= q,
            (None, None) => true,
            _ => false,
        }
    }

    /// Extract the read target from tool args for path-based matching.
    /// Returns `None` for non-read-only tools or when no path is found.
    ///
    /// For `unified_search` and `grep_file`, the key includes action+pattern
    /// so that two greps with different patterns on the same directory are NOT
    /// treated as duplicates.
    fn extract_read_target(tool_name: &str, args: &Value) -> Option<String> {
        let obj = args.as_object()?;
        let is_read = match tool_name {
            tools::READ_FILE | tools::GREP_FILE | tools::LIST_FILES => true,
            tools::UNIFIED_SEARCH => true,
            tools::UNIFIED_FILE => {
                matches!(obj.get("action").and_then(Value::as_str), Some("read"))
            }
            _ => false,
        };
        if !is_read {
            return None;
        }
        let path = Self::extract_path_from_args(obj)?;
        // For search tools, include action+pattern so different queries on the
        // same directory are not treated as duplicates.
        if tool_name == tools::UNIFIED_SEARCH || tool_name == tools::GREP_FILE {
            let action = obj.get("action").and_then(Value::as_str).unwrap_or("");
            let pattern = obj.get("pattern").and_then(Value::as_str).unwrap_or("");
            return Some(format!("{path}::{action}::{pattern}"));
        }
        Some(path)
    }

    fn extract_path_from_args(obj: &serde_json::Map<String, Value>) -> Option<String> {
        for key in ["path", "file_path", "filepath", "target_path", "file"] {
            if let Some(path) = obj.get(key).and_then(Value::as_str) {
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
        None
    }
    ///
    /// Supports both `read_file` and `unified_file` read action records.
    ///
    /// Returns `(next_offset, chunk_limit)` when the recent call indicates more chunks are
    /// available (`spool_chunked=true`, `has_more=true`).
    pub fn find_recent_read_file_spool_progress(
        &self,
        path: &str,
        max_age: Duration,
    ) -> Option<(usize, usize)> {
        let records = self.records.read().ok()?;
        let now = SystemTime::now();
        let expected_path = path.trim();

        for record in records.iter().rev() {
            if !record.success || !is_read_file_style_record(record) {
                continue;
            }

            let Some(record_path) = read_file_path_from_args(&record.args) else {
                continue;
            };
            if !paths_match(record_path, expected_path) {
                continue;
            }

            let age_ok = match now.duration_since(record.timestamp) {
                Ok(age) => age <= max_age,
                Err(_) => false,
            };
            if !age_ok {
                continue;
            }

            let Ok(result) = &record.result else {
                continue;
            };
            let chunked = result
                .get("spool_chunked")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let has_more = result
                .get("has_more")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !(chunked && has_more) {
                continue;
            }

            if let Some(progress) = read_chunk_progress_from_result(result) {
                return Some(progress);
            }
        }
        None
    }

    /// Clear all records.
    pub fn clear(&self) {
        if let Ok(mut records) = self.records.write() {
            records.clear();
        }
    }

    /// Total number of execution records currently stored.
    pub fn len(&self) -> usize {
        self.records.read().ok().map(|r| r.len()).unwrap_or(0)
    }

    /// Whether no execution records are currently stored.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the current loop limit.
    pub fn loop_limit(&self) -> usize {
        self.identical_limit
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get the effective loop limit for a specific tool.
    pub fn loop_limit_for(&self, tool_name: &str, args: &Value) -> usize {
        self.effective_identical_limit_for_call(tool_name, args)
    }

    /// Get the rate limit per minute if configured.
    pub fn rate_limit_per_minute(&self) -> Option<usize> {
        let val = self
            .rate_limit_per_minute
            .load(std::sync::atomic::Ordering::Relaxed);
        (val != 0).then_some(val)
    }

    fn effective_identical_limit_for_call(&self, tool_name: &str, args: &Value) -> usize {
        let base_limit = self
            .identical_limit
            .load(std::sync::atomic::Ordering::Relaxed);
        if is_read_style_tool_call(tool_name, args)
            || tool_name_matches(tool_name, tools::UNIFIED_SEARCH)
        {
            base_limit.max(MIN_READONLY_IDENTICAL_LIMIT)
        } else {
            base_limit
        }
    }

    /// Count calls within a time window.
    pub fn calls_in_window(&self, window: Duration) -> usize {
        let cutoff = SystemTime::now()
            .checked_sub(window)
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let Ok(records) = self.records.read() else {
            return 0;
        };
        records
            .iter()
            .rev()
            .take_while(|record| record.timestamp >= cutoff)
            .count()
    }

    /// Detect if the agent is stuck in a loop.
    ///
    /// Returns a [`LoopDetectionResult`] indicating whether a loop was detected.
    pub fn detect_loop(&self, tool_name: &str, args: &Value) -> LoopDetectionResult {
        let limit = self.effective_identical_limit_for_call(tool_name, args);
        if limit == 0 {
            return LoopDetectionResult {
                detected: false,
                repeat_count: 0,
                tool_name: tool_name.to_string(),
            };
        }

        let detect_window = self
            .detect_window
            .load(std::sync::atomic::Ordering::Relaxed);
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

        // Count how many of the recent calls match this exact tool + args combo
        // CRITICAL FIX: Only count SUCCESSFUL calls to avoid cascade blocking
        let mut identical_count = 0;
        for record in &recent {
            if record.tool_name == tool_name && record.args == *args && record.success {
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

impl Default for ToolExecutionHistory {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn make_snapshot() -> HarnessContextSnapshot {
        HarnessContextSnapshot::new("session_test".to_string(), None)
    }

    #[test]
    fn finds_recent_spooled_result() {
        let history = ToolExecutionHistory::new(10);
        let args = json!({"command": "git diff"});
        let temp = tempdir().unwrap();
        let spool_path = temp.path().join("spooled-output.txt");
        std::fs::write(&spool_path, "diff output").unwrap();
        let result = json!({
            "spool_path": spool_path,
            "success": true
        });

        history.add_record(ToolExecutionRecord::success(
            "run_pty_cmd".to_string(),
            "run_pty_cmd".to_string(),
            false,
            None,
            args.clone(),
            result.clone(),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        let found =
            history.find_recent_spooled_result("run_pty_cmd", &args, Duration::from_secs(60));
        assert_eq!(found, Some(result));
    }

    #[test]
    fn ignores_non_spooled_or_stale_results() {
        let history = ToolExecutionHistory::new(10);
        let args = json!({"path": "README.md"});

        let mut record = ToolExecutionRecord::success(
            "read_file".to_string(),
            "read_file".to_string(),
            false,
            None,
            args.clone(),
            json!({"content": "small"}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        );
        record.timestamp = SystemTime::UNIX_EPOCH;
        history.add_record(record);

        let found = history.find_recent_spooled_result("read_file", &args, Duration::from_secs(60));
        assert!(found.is_none());
    }

    #[test]
    fn ignores_spooled_result_when_spool_file_is_missing() {
        let history = ToolExecutionHistory::new(10);
        let args = json!({"command": "cargo clippy"});
        let missing_spool_path = tempdir().unwrap().path().join("missing_spool.txt");
        let result = json!({
            "spool_path": missing_spool_path,
            "success": true
        });

        history.add_record(ToolExecutionRecord::success(
            "run_pty_cmd".to_string(),
            "run_pty_cmd".to_string(),
            false,
            None,
            args.clone(),
            result,
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        let found =
            history.find_recent_spooled_result("run_pty_cmd", &args, Duration::from_secs(60));
        assert!(found.is_none());
    }

    #[test]
    fn find_recent_successful_result_skips_missing_spool_file() {
        let history = ToolExecutionHistory::new(10);
        let args = json!({"command": "cargo clippy"});
        let missing_spool_path = tempdir().unwrap().path().join("missing_spool.txt");
        let result = json!({
            "spool_path": missing_spool_path,
            "success": true
        });

        history.add_record(ToolExecutionRecord::success(
            "run_pty_cmd".to_string(),
            "run_pty_cmd".to_string(),
            false,
            None,
            args.clone(),
            result,
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        let found =
            history.find_recent_successful_result("run_pty_cmd", &args, Duration::from_secs(60));
        assert!(found.is_none());
    }

    #[test]
    fn len_tracks_records_and_clear() {
        let history = ToolExecutionHistory::new(10);
        assert_eq!(history.len(), 0);
        assert!(history.is_empty());

        history.add_record(ToolExecutionRecord::success(
            "read_file".to_string(),
            "read_file".to_string(),
            false,
            None,
            json!({"path": "README.md"}),
            json!({"success": true}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        assert_eq!(history.len(), 1);
        assert!(!history.is_empty());

        history.clear();
        assert_eq!(history.len(), 0);
        assert!(history.is_empty());
    }

    #[test]
    fn finds_recent_read_file_spool_progress() {
        let history = ToolExecutionHistory::new(10);
        let args = json!({"path": ".vtcode/context/tool_outputs/unified_exec_123.txt"});
        let result = json!({
            "success": true,
            "spool_chunked": true,
            "has_more": true,
            "next_read_args": {
                "path": ".vtcode/context/tool_outputs/unified_exec_123.txt",
                "offset": 41,
                "limit": 40
            }
        });

        history.add_record(ToolExecutionRecord::success(
            "read_file".to_string(),
            "read_file".to_string(),
            false,
            None,
            args,
            result,
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        let found = history.find_recent_read_file_spool_progress(
            ".vtcode/context/tool_outputs/unified_exec_123.txt",
            Duration::from_secs(60),
        );
        assert_eq!(found, Some((41, 40)));
    }

    #[test]
    fn finds_recent_unified_file_read_spool_progress() {
        let history = ToolExecutionHistory::new(10);
        let args = json!({
            "action": "read",
            "path": ".vtcode/context/tool_outputs/unified_exec_456.txt"
        });
        let result = json!({
            "success": true,
            "spool_chunked": true,
            "has_more": true,
            "next_read_args": {
                "path": ".vtcode/context/tool_outputs/unified_exec_456.txt",
                "offset": 81,
                "limit": 40
            }
        });

        history.add_record(ToolExecutionRecord::success(
            "unified_file".to_string(),
            "unified_file".to_string(),
            false,
            None,
            args,
            result,
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        let found = history.find_recent_read_file_spool_progress(
            ".vtcode/context/tool_outputs/unified_exec_456.txt",
            Duration::from_secs(60),
        );
        assert_eq!(found, Some((81, 40)));
    }

    #[test]
    fn matches_read_file_alias_name_and_abs_relative_spool_path() {
        let history = ToolExecutionHistory::new(10);
        let rel_path = ".vtcode/context/tool_outputs/unified_exec_789.txt";
        let abs_path = env::current_dir().unwrap().join(rel_path);
        let args = json!({
            "path": abs_path,
            "offset": 1,
            "limit": 40
        });
        let result = json!({
            "success": true,
            "spool_chunked": true,
            "has_more": true,
            "next_read_args": {
                "path": rel_path,
                "offset": 41,
                "limit": 40
            }
        });

        history.add_record(ToolExecutionRecord::success(
            "Read file".to_string(),
            "Read file".to_string(),
            false,
            None,
            args,
            result,
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        let found = history.find_recent_read_file_spool_progress(rel_path, Duration::from_secs(60));
        assert_eq!(found, Some((41, 40)));
    }

    #[test]
    fn matches_prefixed_read_file_tool_name() {
        let history = ToolExecutionHistory::new(10);
        let path = ".vtcode/context/tool_outputs/unified_exec_prefixed.txt";
        let args = json!({ "path": path });
        let result = json!({
            "success": true,
            "spool_chunked": true,
            "has_more": true,
            "next_read_args": {
                "path": path,
                "offset": 121,
                "limit": 40
            }
        });

        history.add_record(ToolExecutionRecord::success(
            "repo_browser.read_file".to_string(),
            "repo_browser.read_file".to_string(),
            false,
            None,
            args,
            result,
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        let found = history.find_recent_read_file_spool_progress(path, Duration::from_secs(60));
        assert_eq!(found, Some((121, 40)));
    }

    #[test]
    fn ignores_read_file_spool_progress_without_canonical_args() {
        let history = ToolExecutionHistory::new(10);
        let path = ".vtcode/context/tool_outputs/unified_exec_legacy.txt";
        let args = json!({"path": path});
        let result = json!({
            "success": true,
            "spool_chunked": true,
            "has_more": true,
            "next_offset": 33,
            "chunk_limit": 32
        });

        history.add_record(ToolExecutionRecord::success(
            "read_file".to_string(),
            "read_file".to_string(),
            false,
            None,
            args,
            result,
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        let found = history.find_recent_read_file_spool_progress(path, Duration::from_secs(60));
        assert_eq!(found, None);
    }

    #[test]
    fn readonly_unified_file_calls_use_lower_identical_limit() {
        let history = ToolExecutionHistory::new(10);
        history.set_loop_detection_limits(5, 2);

        let args = json!({
            "action": "read",
            "path": "vtcode-core/src/core/agent/runner/tests.rs"
        });

        // The effective limit is max(base_limit, MIN_READONLY_IDENTICAL_LIMIT).
        // Even when the configured base limit is 2, the readonly minimum (3)
        // takes precedence so that one legitimate retry is allowed before the
        // loop detector fires.
        assert_eq!(history.loop_limit_for("unified_file", &args), 3);
    }

    #[test]
    fn unified_search_exact_repeat_is_detected_after_three_successes() {
        let history = ToolExecutionHistory::new(10);
        history.set_loop_detection_limits(5, 2);

        let args = json!({
            "action": "grep",
            "pattern": "exec_only_policy",
            "path": "vtcode-core/src/core/agent/runner/tests.rs"
        });

        // With MIN_READONLY_IDENTICAL_LIMIT=3, three identical successful calls
        // are required before the loop detector fires.  Two calls (the old
        // threshold) should NOT be detected — the model is allowed one retry.
        for _ in 0..2 {
            history.add_record(ToolExecutionRecord::success(
                "unified_search".to_string(),
                "unified_search".to_string(),
                false,
                None,
                args.clone(),
                json!({"matches": []}),
                make_snapshot(),
                None,
                None,
                None,
                None,
                false,
            ));
        }

        let loop_result = history.detect_loop("unified_search", &args);
        assert!(
            !loop_result.detected,
            "two identical calls should not trigger loop detection with MIN_READONLY_IDENTICAL_LIMIT=3"
        );

        // A third identical call crosses the threshold.
        history.add_record(ToolExecutionRecord::success(
            "unified_search".to_string(),
            "unified_search".to_string(),
            false,
            None,
            args.clone(),
            json!({"matches": []}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        let loop_result = history.detect_loop("unified_search", &args);
        assert!(loop_result.detected);
        assert_eq!(loop_result.repeat_count, 3);
        assert_eq!(loop_result.tool_name, "unified_search");
    }

    #[test]
    fn find_recent_successful_by_read_target_matches_same_path_different_offset() {
        let history = ToolExecutionHistory::new(10);

        // Record 1: read src/lib.rs with offset=0
        history.add_record(ToolExecutionRecord::success(
            "unified_file".to_string(),
            "unified_file".to_string(),
            false,
            None,
            json!({"action":"read","path":"src/lib.rs","offset":0,"limit":100}),
            json!({"content":"file content"}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        // Record 2: read src/main.rs (different file)
        history.add_record(ToolExecutionRecord::success(
            "unified_file".to_string(),
            "unified_file".to_string(),
            false,
            None,
            json!({"action":"read","path":"src/main.rs","offset":0,"limit":100}),
            json!({"content":"main content"}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        // Query: same path, different offset — should NOT match (issue #680:
        // a different offset means the model is asking for a different slice
        // of the file, so it needs fresh content, not a cached stub).
        let result = history.find_recent_successful_by_read_target(
            "unified_file",
            &json!({"action":"read","path":"src/lib.rs","offset":500,"limit":200}),
            Duration::from_secs(600),
        );
        assert!(
            result.is_none(),
            "different offset should not match same path"
        );

        // Query: different path, same pagination — should match record 2
        let result2 = history.find_recent_successful_by_read_target(
            "unified_file",
            &json!({"action":"read","path":"src/main.rs","offset":0,"limit":100}),
            Duration::from_secs(600),
        );
        assert!(result2.is_some());
        assert_eq!(result2.unwrap(), json!({"content":"main content"}));

        // Query: non-existent path — should return None
        let result3 = history.find_recent_successful_by_read_target(
            "unified_file",
            &json!({"action":"read","path":"src/missing.rs"}),
            Duration::from_secs(600),
        );
        assert!(result3.is_none());

        // Query: write action — should return None (not read-only)
        let result4 = history.find_recent_successful_by_read_target(
            "unified_file",
            &json!({"action":"write","path":"src/lib.rs","content":"new"}),
            Duration::from_secs(600),
        );
        assert!(
            result4.is_none(),
            "write action should not match read records"
        );
    }

    #[test]
    fn find_recent_successful_by_read_target_extent_matters() {
        let history = ToolExecutionHistory::new(10);

        // Record: read AGENTS.md, offset=0, limit=200
        history.add_record(ToolExecutionRecord::success(
            "unified_file".to_string(),
            "unified_file".to_string(),
            false,
            None,
            json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
            json!({"output":"full file content line 1\nline2\n..."}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        // Query: same path, same offset, larger limit → should NOT match
        // (issue #680: the model asked for more lines than the cache has)
        let result = history.find_recent_successful_by_read_target(
            "unified_file",
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":220}),
            Duration::from_secs(600),
        );
        assert!(result.is_none(), "larger limit should not match same path");

        // Query: same path, same offset, same limit → should match (genuine repeat)
        let result = history.find_recent_successful_by_read_target(
            "unified_file",
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
            Duration::from_secs(600),
        );
        assert!(result.is_some(), "same path and same limit should match");

        // Query: same path, same offset, smaller limit → should match (subset)
        let result = history.find_recent_successful_by_read_target(
            "unified_file",
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":100}),
            Duration::from_secs(600),
        );
        assert!(
            result.is_some(),
            "smaller limit is a subset of cached extent"
        );
    }

    #[test]
    fn find_recent_successful_by_read_target_no_limit_uses_default() {
        let history = ToolExecutionHistory::new(10);

        // Record: read AGENTS.md with no explicit limit or offset (defaults)
        history.add_record(ToolExecutionRecord::success(
            "unified_file".to_string(),
            "unified_file".to_string(),
            false,
            None,
            json!({"action":"read","path":"AGENTS.md"}),
            json!({"output":"default content"}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        // Query: same path, also no explicit limit/offset → should match (both use defaults)
        let result = history.find_recent_successful_by_read_target(
            "unified_file",
            &json!({"action":"read","path":"AGENTS.md"}),
            Duration::from_secs(600),
        );
        assert!(
            result.is_some(),
            "both using default offset/limit should match"
        );

        // Query: same path, default offset but explicit limit → should NOT match
        // (one has explicit pagination, other doesn't — can't compare)
        let result = history.find_recent_successful_by_read_target(
            "unified_file",
            &json!({"action":"read","path":"AGENTS.md","limit":200}),
            Duration::from_secs(600),
        );
        assert!(
            result.is_none(),
            "mixed default/explicit limit should not match"
        );
    }

    #[test]
    fn find_recent_successful_by_read_target_raw_shape_matters() {
        let history = ToolExecutionHistory::new(10);

        // Record: non-raw read can be summarized for the model, so it must not
        // satisfy a later raw=true query that asks for exact content.
        history.add_record(ToolExecutionRecord::success(
            "unified_file".to_string(),
            "unified_file".to_string(),
            false,
            None,
            json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
            json!({"summary":"summarized guidance","summarized_for_model":true}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        let result = history.find_recent_successful_by_read_target(
            "unified_file",
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
            Duration::from_secs(600),
        );
        assert!(
            result.is_none(),
            "non-raw summarized read should not satisfy raw=true query"
        );

        // Record: raw=true read can satisfy the same raw=true shape.
        history.add_record(ToolExecutionRecord::success(
            "unified_file".to_string(),
            "unified_file".to_string(),
            false,
            None,
            json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
            json!({"output":"exact file content"}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));

        let result = history.find_recent_successful_by_read_target(
            "unified_file",
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
            Duration::from_secs(600),
        );
        assert_eq!(result, Some(json!({"output":"exact file content"})));
    }
}
