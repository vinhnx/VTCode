//! Autonomous tool execution with safety checks
//!
//! Implements safe autonomous execution following AGENTS.md principles:
//! - Act, don't ask (for safe operations)
//! - Verify before destructive operations
//! - Loop detection and prevention
//! - Context-aware decision making

use crate::command_safety::shell_string_might_be_dangerous;
use crate::config::constants::tools;
use crate::core::loop_detector::LoopDetector;
use crate::tools::apply_patch::decode_apply_patch_input;
use crate::tools::command_args::{command_text, interactive_input_text};
use crate::tools::tool_intent::{
    self, classify_tool_intent, command_session_action, command_session_action_in, file_operation_action_in,
    file_operation_action_is,
};
use anyhow::{Context, Result};
use hashbrown::{HashMap, HashSet};
use serde_json::Value;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::warn;

/// Tools that require verification before execution
const VERIFICATION_REQUIRED_TOOLS: &[&str] = &[
    tools::WRITE_FILE,
    tools::EDIT_FILE,
    tools::UNIFIED_EXEC,
    tools::CREATE_PTY_SESSION,
];

/// Autonomous execution policy for a tool
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutonomousPolicy {
    /// Execute immediately without asking
    AutoExecute,
    /// Show dry-run/preview, then execute
    VerifyThenExecute,
    /// Always require explicit user confirmation
    RequireConfirmation,
}

/// Execution statistics for a tool
#[derive(Debug, Clone, Default)]
struct ToolStats {
    total_attempts: usize,
    successful_executions: usize,
    failed_executions: usize,
}

impl ToolStats {
    fn success_rate(&self) -> f64 {
        if self.total_attempts == 0 {
            0.0
        } else {
            self.successful_executions as f64 / self.total_attempts as f64
        }
    }
}

use crate::tools::circuit_breaker::CircuitBreaker;
use crate::tools::validation::paths::validate_path_safety;
use crate::utils::path::{normalize_path, resolve_workspace_path};

/// Autonomous tool executor with safety checks.
///
/// In the unified interactive VT Code runloop, higher-level turn code owns
/// user-visible loop recovery. The loop detector here remains a generic
/// safeguard for legacy and non-unified autonomous execution paths.
///
/// # Rate Limiting
///
/// This executor maintains its own sliding-window rate limiter (`rate_history`)
/// for **policy decisions** (auto-execute vs require confirmation). It does NOT
/// consume tokens -- it only checks whether the tool has been called too
/// frequently in the recent window.
///
/// The separate `PER_TOOL_RATE_LIMITER` (token-bucket) in `rate_limiter.rs`
/// handles **execution blocking** at the executor level. The `SafetyGateway`
/// rate limiter is disabled by the runloop in favor of these two systems.
pub struct AutonomousExecutor {
    verification_tools: HashSet<String>,
    loop_detector: Arc<RwLock<LoopDetector>>,
    execution_stats: Arc<RwLock<HashMap<String, ToolStats>>>,
    workspace_dir: Option<PathBuf>,
    rate_limit_window: Duration,
    rate_limit_max_calls: usize,
    rate_history: Arc<RwLock<HashMap<String, VecDeque<Instant>>>>,
    circuit_breaker: CircuitBreaker,
}

impl AutonomousExecutor {
    #[inline]
    fn canonical_tool_key(tool_name: &str) -> &str {
        tool_intent::canonical_command_session_tool_name(tool_name).unwrap_or(tool_name)
    }

    #[inline]
    fn is_command_session_run(tool_name: &str, args: &Value) -> bool {
        tool_intent::is_command_run_tool_call(tool_name, args)
            || (tool_name == tools::UNIFIED_EXEC && command_session_action(args).is_none())
    }

    pub fn new() -> Self {
        Self::with_loop_detector(Arc::new(RwLock::new(LoopDetector::new())))
    }

    pub fn with_loop_detector(loop_detector: Arc<RwLock<LoopDetector>>) -> Self {
        Self {
            verification_tools: VERIFICATION_REQUIRED_TOOLS.iter().map(|s| s.to_string()).collect(),
            loop_detector,
            execution_stats: Arc::new(RwLock::new(HashMap::new())),
            workspace_dir: std::env::var("WORKSPACE_DIR")
                .ok()
                .map(PathBuf::from)
                .or_else(|| std::env::current_dir().ok()),
            rate_limit_window: Duration::from_secs(10),
            rate_limit_max_calls: 5,
            rate_history: Arc::new(RwLock::new(HashMap::new())),
            circuit_breaker: CircuitBreaker::default(),
        }
    }

    /// Set workspace directory for boundary validation
    pub fn set_workspace_dir(&mut self, dir: PathBuf) {
        self.workspace_dir = Some(dir);
    }

    /// Configure loop detection thresholds
    pub fn configure_loop_limits(&self, limits: &HashMap<String, usize>) {
        if let Ok(mut detector) = self.loop_detector.write() {
            for (tool, limit) in limits {
                detector.set_tool_limit(Self::canonical_tool_key(tool), *limit);
            }
        } else {
            tracing::warn!("Failed to acquire loop detector lock for configuration");
        }
    }

    pub fn set_loop_limit(&self, tool_name: &str, limit: usize) {
        let tool_key = Self::canonical_tool_key(tool_name);
        if let Ok(mut detector) = self.loop_detector.write() {
            detector.set_tool_limit(tool_key, limit);
        } else {
            tracing::warn!("Failed to acquire loop detector lock for configuration");
        }
    }

    pub fn is_hard_limit_exceeded(&self, tool_name: &str) -> bool {
        let tool_key = Self::canonical_tool_key(tool_name);
        self.loop_detector
            .read()
            .map(|detector| detector.is_hard_limit_exceeded(tool_key))
            .unwrap_or(false)
    }

    /// Reset loop-detection streaks at the start of a new turn.
    pub fn reset_turn_loop_detection(&self) {
        if let Ok(mut detector) = self.loop_detector.write() {
            detector.reset();
        } else {
            tracing::warn!("Failed to acquire loop detector lock for turn reset");
        }
    }

    /// Determine execution policy for a tool
    pub fn get_policy(&self, tool_name: &str, args: &Value) -> AutonomousPolicy {
        if self.is_destructive_operation(tool_name, args) {
            return AutonomousPolicy::RequireConfirmation;
        }

        if !classify_tool_intent(tool_name, args).mutating {
            return AutonomousPolicy::AutoExecute;
        }

        if self.requires_preview(tool_name, args) {
            return AutonomousPolicy::VerifyThenExecute;
        }

        AutonomousPolicy::RequireConfirmation
    }

    /// Check if tool should be blocked due to loop detection or circuit breaker
    /// Returns Some(message) if blocked, None if allowed
    pub fn should_block(&self, tool_name: &str, _args: &Value) -> Option<String> {
        let tool_key = Self::canonical_tool_key(tool_name);

        // Check circuit breaker first (fail fast)
        if !self.circuit_breaker.allow_request_for_tool(tool_key) {
            return Some(format!(
                "Tool '{tool_key}' blocked by circuit breaker due to repeated failures. \
                 Cooling down before retrying."
            ));
        }

        if self.is_rate_limited(tool_key) {
            return Some(format!(
                "Tool '{}' temporarily blocked: rate limit exceeded ({} calls in {:?}).",
                tool_key, self.rate_limit_max_calls, self.rate_limit_window
            ));
        }

        // Use try_read to avoid blocking on contested locks
        match self.loop_detector.try_read() {
            Ok(detector) => {
                // Check if hard limit already exceeded
                if detector.is_hard_limit_exceeded(tool_key) {
                    return Some(format!("Tool '{tool_key}' blocked: hard limit exceeded. Agent is stuck in a loop."));
                }

                // Check call count and provide early warning
                let count = detector.get_call_count(tool_key);
                if count >= 3
                    && let Some(suggestion) = detector.suggest_alternative(tool_key)
                {
                    return Some(format!(
                        "Tool '{tool_key}' called {count} times. Consider alternative approach:\n{suggestion}"
                    ));
                }
            }
            Err(_) => {
                // If we can't get the lock, don't block execution
                tracing::debug!("Could not acquire loop detector read lock for {}", tool_key);
            }
        }
        None
    }

    /// Record tool call in loop detector
    /// Returns warning message if loop detected
    pub fn record_tool_call(&self, tool_name: &str, args: &Value) -> Option<String> {
        let tool_key = Self::canonical_tool_key(tool_name);
        self.record_rate_history(tool_key);
        if let Ok(mut detector) = self.loop_detector.write() {
            detector.record_call(tool_key, args)
        } else {
            None
        }
    }

    /// Check if operation is destructive based on tool and arguments
    fn is_destructive_operation(&self, tool_name: &str, args: &Value) -> bool {
        match tool_name {
            tools::APPLY_PATCH | tools::DELETE_FILE => true,
            tools::UNIFIED_FILE => file_operation_action_in(args, &["delete"]),
            _ if Self::is_command_session_run(tool_name, args) => command_text(args)
                .ok()
                .flatten()
                .is_some_and(|cmd| self.is_destructive_command(&cmd)),
            _ if tool_intent::is_command_session_tool(tool_name)
                && command_session_action_in(args, &["write", "continue"]) =>
            {
                interactive_input_text(args).is_some_and(|input| self.is_destructive_command(input))
            }
            _ => false,
        }
    }

    /// Check if shell command is destructive
    fn is_destructive_command(&self, cmd: &str) -> bool {
        if shell_string_might_be_dangerous(cmd) {
            return true;
        }

        let cmd_lower = cmd.to_lowercase();

        // Additional destructive patterns that are not captured by the centralized
        // command safety evaluator.
        let supplemental_patterns = [
            "truncate",
            "> /dev/",
            "dd if=",
            "mkfs",
            "fdisk",
            "format",
            // Overwrite operations
            ">/",
            "2>/",
            // Package managers (potentially destructive)
            "npm uninstall -g",
            "cargo uninstall",
            "pip uninstall",
            // Permissions
            "chmod -r",
            "chown -r",
        ];

        supplemental_patterns.iter().any(|pattern| cmd_lower.contains(pattern))
    }

    /// Validate tool arguments for safety
    pub fn validate_args(&self, tool_name: &str, args: &Value) -> Result<()> {
        match tool_name {
            tools::WRITE_FILE | tools::EDIT_FILE => self.validate_file_path(args.get("path"))?,
            _ if Self::is_command_session_run(tool_name, args) => {
                self.validate_command_text(
                    &command_text(args)
                        .map_err(anyhow::Error::msg)?
                        .context("Missing or invalid 'command' argument")?,
                )?;
            }
            _ if tool_intent::is_command_session_tool(tool_name)
                && command_session_action_in(args, &["write", "continue"]) =>
            {
                if let Some(input) = interactive_input_text(args) {
                    self.validate_command_text(input)?;
                }
            }
            tools::UNIFIED_FILE if file_operation_action_in(args, &["write", "edit", "delete"]) => {
                self.validate_file_path(args.get("path"))?;
            }
            tools::UNIFIED_FILE if file_operation_action_in(args, &["move", "copy"]) => {
                self.validate_file_path(args.get("path"))?;
                self.validate_file_path(args.get("destination"))?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Validate file path is within workspace boundaries.
    ///
    /// First checks for sensitive system paths via `validate_path_safety`,
    /// then enforces workspace boundary constraints.
    fn validate_file_path(&self, path: Option<&Value>) -> Result<()> {
        let path_str = path.and_then(|v| v.as_str()).context("Missing or invalid 'path' argument")?;

        // Check for sensitive system paths (e.g., /var/db/shadow, /etc/shadow)
        validate_path_safety(path_str)?;

        let path_obj = Path::new(path_str);

        // Check for absolute paths
        if path_obj.is_absolute() {
            // Allow paths beneath /tmp/vtcode. Use component-aware matching so
            // siblings like /tmp/vtcode-evil do not inherit the exception.
            if path_obj.starts_with("/tmp/vtcode") {
                return Ok(());
            }

            // Check if within workspace
            if let Some(workspace) = &self.workspace_dir
                && (resolve_workspace_path(workspace, path_obj).is_ok()
                    || is_within_workspace_lexically(workspace, path_obj))
            {
                return Ok(());
            }

            anyhow::bail!(
                "Absolute path outside workspace boundary: {path_str}. \
                 Only paths within WORKSPACE_DIR or /tmp/vtcode are allowed."
            );
        }

        // Prevent parent directory traversal that could escape workspace
        if path_str.contains("..") {
            warn!("Path contains parent directory traversal: {}", path_str);

            // Resolve the path and check if it stays within workspace
            if let Some(workspace) = &self.workspace_dir {
                let path_obj = Path::new(path_str);
                let canonical_ok = resolve_workspace_path(workspace, &workspace.join(path_obj)).is_ok();
                let lexical_ok = is_within_workspace_lexically(workspace, path_obj);
                if !canonical_ok && !lexical_ok {
                    anyhow::bail!("Path traversal escapes workspace boundary: {path_str}");
                }
            } else {
                anyhow::bail!("Path traversal blocked: workspace boundary is unknown for '{path_str}'");
            }
        }

        // If workspace directory is unknown, conservatively block writes to avoid escaping boundaries.
        if self.workspace_dir.is_none() {
            anyhow::bail!(
                "Workspace directory is not set; refusing to write to relative path '{path_str}'. \
                 Set WORKSPACE_DIR or call set_workspace_dir()."
            );
        }

        Ok(())
    }

    /// Validate shell command for safety
    fn validate_command_text(&self, cmd: &str) -> Result<()> {
        if self.is_destructive_command(cmd) {
            anyhow::bail!("Destructive command requires explicit confirmation: {cmd}");
        }

        Ok(())
    }

    /// Generate dry-run preview for verification
    pub fn generate_preview(&self, tool_name: &str, args: &Value) -> String {
        if tool_name == tools::WRITE_FILE
            || (tool_name == tools::UNIFIED_FILE && file_operation_action_is(args, "write"))
        {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("unknown");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let lines = content.lines().count();
            let size_kb = content.len() / 1024;

            let preview = if lines > 10 {
                let first_lines: Vec<_> = content.lines().take(5).collect();
                format!("\n  {}\n  ... ({} more lines)", first_lines.join("\n  "), lines - 5)
            } else {
                format!("\n  {}", content.lines().collect::<Vec<_>>().join("\n  "))
            };

            format!("Will write {lines} lines ({size_kb} KB) to: {path}\nPreview:{preview}")
        } else if tool_name == tools::EDIT_FILE
            || (tool_name == tools::UNIFIED_FILE && file_operation_action_is(args, "edit"))
        {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("unknown");
            let old_str = args.get("old_str").and_then(|v| v.as_str()).unwrap_or("");
            let new_str = args.get("new_str").and_then(|v| v.as_str()).unwrap_or("");

            format!(
                "Will edit file: {}\nReplacing:\n  {}\nWith:\n  {}",
                path,
                old_str.lines().take(3).collect::<Vec<_>>().join("\n  "),
                new_str.lines().take(3).collect::<Vec<_>>().join("\n  ")
            )
        } else if Self::is_command_session_run(tool_name, args) {
            let cmd = command_text(args).ok().flatten().unwrap_or_else(|| "unknown".to_string());
            let is_destructive = self.is_destructive_command(&cmd);

            let warning = if is_destructive {
                "\n[WARN] WARNING: This command is potentially destructive!"
            } else {
                ""
            };

            format!("Will execute: {cmd}{warning}")
        } else if tool_name == tools::APPLY_PATCH {
            let patch = decode_apply_patch_input(args)
                .ok()
                .flatten()
                .map(|patch| patch.text)
                .unwrap_or_default();
            let lines = patch.lines().count();
            format!("Will apply patch with {lines} lines of changes")
        } else {
            format!("Will execute: {tool_name} with args: {args:?}")
        }
    }

    fn requires_preview(&self, tool_name: &str, args: &Value) -> bool {
        let canonical_tool_name = Self::canonical_tool_key(tool_name);
        if self.verification_tools.contains(canonical_tool_name) {
            return true;
        }

        match canonical_tool_name {
            tools::UNIFIED_FILE => file_operation_action_in(args, &["write", "edit", "move", "copy"]),
            tools::UNIFIED_EXEC => command_session_action_in(args, &["run", "code", "close"]),
            _ => false,
        }
    }

    /// Record execution result for statistics tracking and circuit breaker
    pub fn record_execution(&self, tool_name: &str, success: bool) {
        let tool_key = Self::canonical_tool_key(tool_name);

        // Update circuit breaker
        if success {
            self.circuit_breaker.record_success_for_tool(tool_key);
        } else {
            // Note: We blindly treat all failures as circuit-breaking for now.
            // Ideally, the caller should specify if it's an arg error or system error.
            self.circuit_breaker.record_failure_for_tool(tool_key, false);
        }

        if let Ok(mut stats) = self.execution_stats.write() {
            let entry = stats.entry(tool_key.to_string()).or_default();
            entry.total_attempts += 1;
            if success {
                entry.successful_executions += 1;
            } else {
                entry.failed_executions += 1;
            }
        }
    }

    /// Get success rate for a tool
    pub fn get_success_rate(&self, tool_name: &str) -> f64 {
        if let Ok(stats) = self.execution_stats.read() {
            stats.get(tool_name).map(|s| s.success_rate()).unwrap_or(0.0)
        } else {
            0.0
        }
    }

    /// Get execution statistics for a tool
    pub fn get_tool_stats(&self, tool_name: &str) -> Option<(usize, usize, usize)> {
        if let Ok(stats) = self.execution_stats.read() {
            stats
                .get(tool_name)
                .map(|s| (s.total_attempts, s.successful_executions, s.failed_executions))
        } else {
            None
        }
    }
}

impl Default for AutonomousExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl AutonomousExecutor {
    fn record_rate_history(&self, tool_name: &str) {
        let now = Instant::now();
        if let Ok(mut history) = self.rate_history.write() {
            let entries = history.entry(Self::canonical_tool_key(tool_name).to_string()).or_default();
            entries.push_back(now);
            prune_expired_timestamps(entries, now, self.rate_limit_window);
        }
    }

    fn is_rate_limited(&self, tool_name: &str) -> bool {
        let tool_key = Self::canonical_tool_key(tool_name);
        let now = Instant::now();

        // First, try with a read lock to check without modifying
        // This is the common fast path when there are no expired entries
        if let Ok(history) = self.rate_history.read() {
            if let Some(entries) = history.get(tool_key) {
                // Quick check: if all entries are within window and at limit, we're rate limited
                let oldest_within_window = entries
                    .front()
                    .is_some_and(|front| now.duration_since(*front) <= self.rate_limit_window);
                if oldest_within_window {
                    return entries.len() >= self.rate_limit_max_calls;
                }
            } else {
                // No entries for this tool, definitely not rate limited
                return false;
            }
        }

        // Fall back to write lock only when we need to clean up expired entries
        if let Ok(mut history) = self.rate_history.write() {
            let entries = history.entry(tool_key.to_string()).or_default();
            prune_expired_timestamps(entries, now, self.rate_limit_window);
            return entries.len() >= self.rate_limit_max_calls;
        }
        false
    }
}

fn prune_expired_timestamps(entries: &mut VecDeque<Instant>, now: Instant, window: Duration) {
    while let Some(front) = entries.front() {
        if now.duration_since(*front) > window {
            entries.pop_front();
        } else {
            break;
        }
    }
}

fn is_within_workspace_lexically(workspace: &Path, candidate: &Path) -> bool {
    let normalized_workspace = normalize_path(workspace);
    let normalized_candidate = if candidate.is_absolute() {
        normalize_path(candidate)
    } else {
        normalize_path(&normalized_workspace.join(candidate))
    };
    normalized_candidate.starts_with(&normalized_workspace)
}

#[cfg(test)]
mod tests;
