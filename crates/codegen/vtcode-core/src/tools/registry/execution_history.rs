//! Tool execution history and records.
//!
//! This module provides thread-safe recording and querying of tool executions,
//! including loop detection and rate limiting.

use std::collections::VecDeque;
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use crate::config::constants::{defaults, tools};
use crate::tools::tool_intent;

use super::execution_kernel::PATH_ALIAS_KEYS;
mod loop_detection;
mod replay;
mod telemetry;
use loop_detection::DEFAULT_LOOP_DETECT_WINDOW;
pub use telemetry::ToolTaskTelemetrySnapshot;

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
        Self { session_id, task_id }
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
    #[expect(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
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
        Self::new_execution_record(
            tool_name,
            requested_name,
            is_mcp,
            mcp_provider,
            args,
            Err(error_msg),
            context,
            timeout_category,
            base_timeout_ms,
            adaptive_timeout_ms,
            effective_timeout_ms,
            circuit_breaker,
        )
    }

    /// Create a new successful execution record.
    #[expect(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
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
        Self::new_execution_record(
            tool_name,
            requested_name,
            is_mcp,
            mcp_provider,
            args,
            Ok(result),
            context,
            timeout_category,
            base_timeout_ms,
            adaptive_timeout_ms,
            effective_timeout_ms,
            circuit_breaker,
        )
    }

    /// Shared initializer: both constructors differ only in the `result`
    /// outcome, so this pins the single field list.
    #[expect(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    fn new_execution_record(
        tool_name: String,
        requested_name: String,
        is_mcp: bool,
        mcp_provider: Option<String>,
        args: Value,
        result: Result<Value, String>,
        context: HarnessContextSnapshot,
        timeout_category: Option<String>,
        base_timeout_ms: Option<u64>,
        adaptive_timeout_ms: Option<u64>,
        effective_timeout_ms: Option<u64>,
        circuit_breaker: bool,
    ) -> Self {
        let success = result.is_ok();
        Self {
            tool_name,
            requested_name,
            is_mcp,
            mcp_provider,
            args,
            result,
            timestamp: SystemTime::now(),
            success,
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
        self.retry_after_ms = retry_after.map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64);
        self
    }

    #[inline]
    pub fn with_circuit_breaker_state(mut self, state: impl Into<String>) -> Self {
        self.circuit_breaker_state = Some(state.into());
        self
    }
}

fn normalize_tool_name_for_match(name: &str) -> String {
    let normalized = name.trim().to_ascii_lowercase().replace(' ', "_");
    tool_intent::canonical_command_session_tool_name(&normalized)
        .unwrap_or(&normalized)
        .to_string()
}

fn is_read_file_tool_name(name: &str) -> bool {
    let normalized = normalize_tool_name_for_match(name);
    normalized == tools::READ_FILE || normalized.ends_with(".read_file")
}

fn is_file_operation_tool_name(name: &str) -> bool {
    let normalized = normalize_tool_name_for_match(name);
    normalized == tools::UNIFIED_FILE || normalized.ends_with(".file_operation")
}

fn tool_name_matches(name: &str, expected: &str) -> bool {
    let normalized = normalize_tool_name_for_match(name);
    normalized == expected || normalized.ends_with(&format!(".{expected}"))
}

fn is_read_style_tool_call(tool_name: &str, args: &Value) -> bool {
    if tool_name_matches(tool_name, tools::READ_FILE) {
        return true;
    }
    if is_file_operation_tool_name(tool_name) {
        return tool_intent::file_operation_action_is(args, "read");
    }
    false
}

/// Thread-safe execution history for recording tool executions.
#[derive(Clone)]
pub struct ToolExecutionHistory {
    records: Arc<RwLock<VecDeque<ToolExecutionRecord>>>,
    workspace_root: Arc<PathBuf>,
    max_records: usize,
    detect_window: Arc<std::sync::atomic::AtomicUsize>,
    identical_limit: Arc<std::sync::atomic::AtomicUsize>,
    rate_limit_per_minute: Arc<std::sync::atomic::AtomicUsize>,
    tracker_loop_start: Arc<RwLock<Option<SystemTime>>>,
}

impl ToolExecutionHistory {
    /// Create a new execution history with a maximum record count.
    pub fn new(max_records: usize) -> Self {
        Self::with_workspace_root(max_records, env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    pub(crate) fn with_workspace_root(max_records: usize, workspace_root: PathBuf) -> Self {
        Self {
            records: Arc::new(RwLock::new(VecDeque::with_capacity(max_records))),
            workspace_root: Arc::new(workspace_root),
            max_records,
            tracker_loop_start: Arc::new(RwLock::new(None)),
            detect_window: Arc::new(std::sync::atomic::AtomicUsize::new(DEFAULT_LOOP_DETECT_WINDOW)),
            identical_limit: Arc::new(std::sync::atomic::AtomicUsize::new(defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS)),
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

    pub(super) fn begin_tracker_request(&self) {
        if let Ok(mut start) = self.tracker_loop_start.write() {
            *start = Some(SystemTime::now());
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
        self.rate_limit_per_minute
            .store(limit.filter(|v| *v > 0).unwrap_or(0), std::sync::atomic::Ordering::Relaxed);
    }

    /// Get the most recent records.
    pub fn get_recent_records(&self, count: usize) -> Vec<ToolExecutionRecord> {
        self.with_records(|records| {
            let records_len = records.len();
            let start = records_len.saturating_sub(count);
            records.iter().skip(start).cloned().collect()
        })
    }

    /// Get recent failures in chronological order.
    pub fn get_recent_failures(&self, count: usize) -> Vec<ToolExecutionRecord> {
        self.with_records(|records| {
            let mut failures: Vec<ToolExecutionRecord> =
                records.iter().rev().filter(|r| !r.success).take(count).cloned().collect();
            failures.reverse();
            failures
        })
    }

    /// Run `f` against a shared read of the records; poisoned locks recover
    /// via `into_inner` without losing the history.
    fn with_records<T>(&self, f: impl FnOnce(&VecDeque<ToolExecutionRecord>) -> T) -> T {
        let guard = self.records.read().unwrap_or_else(|e| e.into_inner());
        f(&guard)
    }

    /// Clear all records.
    pub fn clear(&self) {
        if let Ok(mut records) = self.records.write() {
            records.clear();
        }
    }

    /// Total number of execution records currently stored.
    pub fn len(&self) -> usize {
        self.records.read().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Whether no execution records are currently stored.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ToolExecutionHistory {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests;
