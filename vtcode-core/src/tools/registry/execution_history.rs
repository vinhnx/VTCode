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
use crate::tools::tool_intent;

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
}

impl ToolExecutionRecord {
    /// Create a new failed execution record.
    #[allow(clippy::too_many_arguments)]
    #[inline]
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
        }
    }

    /// Create a new successful execution record.
    #[allow(clippy::too_many_arguments)]
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
        }
    }
}

/// Default window size for loop detection.
const DEFAULT_LOOP_DETECT_WINDOW: usize = 5;
/// Minimum limit for identical readonly operations.
const MIN_READONLY_IDENTICAL_LIMIT: usize = 5;

fn tool_rate_limit_from_env() -> Option<usize> {
    env::var("VTCODE_TOOL_CALLS_PER_MIN")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
}

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
    for key in ["path", "file_path", "filepath", "target_path"] {
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
    name.trim().to_ascii_lowercase().replace(' ', "_")
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
        return tool_intent::unified_file_action(args)
            .unwrap_or("read")
            .eq_ignore_ascii_case("read");
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

fn value_to_usize(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .or_else(|| value.as_str().and_then(|s| s.parse::<usize>().ok()))
}

fn is_read_file_style_record(record: &ToolExecutionRecord) -> bool {
    if is_read_file_tool_name(&record.tool_name) {
        return true;
    }

    if !is_unified_file_tool_name(&record.tool_name) {
        return false;
    }

    tool_intent::unified_file_action(&record.args)
        .unwrap_or("read")
        .eq_ignore_ascii_case("read")
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
                tool_rate_limit_from_env().unwrap_or(0),
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
                && result
                    .get("spooled_to_file")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
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
                if result
                    .get("spooled_to_file")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
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

    /// Find continuation info from a recent chunked file-read call for the same path.
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

            let Some(next_offset) = result.get("next_offset").and_then(value_to_usize) else {
                continue;
            };
            let chunk_limit = result
                .get("chunk_limit")
                .and_then(value_to_usize)
                .unwrap_or(40)
                .max(1);
            return Some((next_offset.max(1), chunk_limit));
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
        if val == 0 { None } else { Some(val) }
    }

    fn effective_identical_limit_for_call(&self, tool_name: &str, args: &Value) -> usize {
        let base_limit = self
            .identical_limit
            .load(std::sync::atomic::Ordering::Relaxed);
        if is_read_style_tool_call(tool_name, args)
            || tool_name_matches(tool_name, tools::GREP_FILE)
            || tool_name_matches(tool_name, tools::LIST_FILES)
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
    /// Returns (is_loop, repeat_count, tool_name) if a loop is detected.
    pub fn detect_loop(&self, tool_name: &str, args: &Value) -> (bool, usize, String) {
        let limit = self.effective_identical_limit_for_call(tool_name, args);
        if limit == 0 {
            return (false, 0, String::new());
        }

        let detect_window = self
            .detect_window
            .load(std::sync::atomic::Ordering::Relaxed);
        let window = detect_window.max(limit.saturating_mul(2)).max(1);

        let Ok(records) = self.records.read() else {
            return (false, 0, String::new());
        };
        let recent: Vec<&ToolExecutionRecord> = records.iter().rev().take(window).collect();

        if recent.is_empty() {
            return (false, 0, String::new());
        }

        // Count how many of the recent calls match this exact tool + args combo
        // CRITICAL FIX: Only count SUCCESSFUL calls to avoid cascade blocking
        let mut identical_count = 0;
        for record in &recent {
            if record.tool_name == tool_name && record.args == *args && record.success {
                identical_count += 1;
            }
        }

        let is_loop = identical_count >= limit;
        (is_loop, identical_count, tool_name.to_string())
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
            "spooled_to_file": true,
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
            json!({"content": "small", "spooled_to_file": false}),
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
            "spooled_to_file": true,
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
            "spooled_to_file": true,
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
            "next_offset": 41,
            "chunk_limit": 40
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
            "next_offset": 81,
            "chunk_limit": 40
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
        let abs_path = std::env::current_dir().unwrap().join(rel_path);
        let args = json!({
            "path": abs_path,
            "offset": 1,
            "limit": 40
        });
        let result = json!({
            "success": true,
            "spool_chunked": true,
            "has_more": true,
            "next_offset": 41,
            "chunk_limit": 40
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
            "next_offset": 121,
            "chunk_limit": 40
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
}
