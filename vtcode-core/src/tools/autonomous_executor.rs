//! Autonomous tool execution with safety checks
//!
//! Implements safe autonomous execution following AGENTS.md principles:
//! - Act, don't ask (for safe operations)
//! - Verify before destructive operations
//! - Loop detection and prevention
//! - Context-aware decision making

use crate::config::constants::tools;
use crate::core::loop_detector::LoopDetector;
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::warn;

/// Tools that are always safe to execute autonomously
const SAFE_AUTONOMOUS_TOOLS: &[&str] = &[
    tools::GREP_FILE,
    tools::LIST_FILES,
    tools::READ_FILE,
    tools::SEARCH_TOOLS,
    tools::LIST_PTY_SESSIONS,
    tools::READ_PTY_SESSION,
];

/// Tools that require verification before execution
const VERIFICATION_REQUIRED_TOOLS: &[&str] = &[
    tools::WRITE_FILE,
    tools::EDIT_FILE,
    "shell",
    tools::RUN_PTY_CMD,
    tools::CREATE_PTY_SESSION,
];

/// Tools that are destructive and need explicit confirmation
const DESTRUCTIVE_TOOLS: &[&str] = &[tools::APPLY_PATCH];

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

/// Autonomous tool executor with safety checks
pub struct AutonomousExecutor {
    safe_tools: HashSet<String>,
    verification_tools: HashSet<String>,
    destructive_tools: HashSet<String>,
    loop_detector: Arc<RwLock<LoopDetector>>,
    execution_stats: Arc<RwLock<HashMap<String, ToolStats>>>,
    workspace_dir: Option<PathBuf>,
    rate_limit_window: Duration,
    rate_limit_max_calls: usize,
    rate_history: Arc<RwLock<HashMap<String, VecDeque<Instant>>>>,
}

impl AutonomousExecutor {
    pub fn new() -> Self {
        Self::with_loop_detector(Arc::new(RwLock::new(LoopDetector::new())))
    }

    pub fn with_loop_detector(loop_detector: Arc<RwLock<LoopDetector>>) -> Self {
        Self {
            safe_tools: SAFE_AUTONOMOUS_TOOLS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            verification_tools: VERIFICATION_REQUIRED_TOOLS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            destructive_tools: DESTRUCTIVE_TOOLS.iter().map(|s| s.to_string()).collect(),
            loop_detector,
            execution_stats: Arc::new(RwLock::new(HashMap::new())),
            workspace_dir: std::env::var("WORKSPACE_DIR")
                .ok()
                .map(PathBuf::from)
                .or_else(|| std::env::current_dir().ok()),
            rate_limit_window: Duration::from_secs(10),
            rate_limit_max_calls: 5,
            rate_history: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set workspace directory for boundary validation
    pub fn set_workspace_dir(&mut self, dir: PathBuf) {
        self.workspace_dir = Some(dir);
    }

    /// Determine execution policy for a tool
    pub fn get_policy(&self, tool_name: &str, args: &Value) -> AutonomousPolicy {
        // Check for destructive patterns in arguments
        if self.is_destructive_operation(tool_name, args) {
            return AutonomousPolicy::RequireConfirmation;
        }

        // Safe tools execute immediately
        if self.safe_tools.contains(tool_name) {
            return AutonomousPolicy::AutoExecute;
        }

        // Verification tools show preview first
        if self.verification_tools.contains(tool_name) {
            return AutonomousPolicy::VerifyThenExecute;
        }

        // Unknown tools require confirmation
        AutonomousPolicy::RequireConfirmation
    }

    /// Check if tool should be blocked due to loop detection
    /// Returns Some(message) if blocked, None if allowed
    pub fn should_block(&self, tool_name: &str, _args: &Value) -> Option<String> {
        if self.is_rate_limited(tool_name) {
            return Some(format!(
                "Tool '{}' temporarily blocked: rate limit exceeded ({} calls in {:?}).",
                tool_name, self.rate_limit_max_calls, self.rate_limit_window
            ));
        }

        // Use try_read to avoid blocking on contested locks
        match self.loop_detector.try_read() {
            Ok(detector) => {
                // Check if hard limit already exceeded
                if detector.is_hard_limit_exceeded(tool_name) {
                    return Some(format!(
                        "Tool '{}' blocked: hard limit exceeded. Agent is stuck in a loop.",
                        tool_name
                    ));
                }

                // Check call count and provide early warning
                let count = detector.get_call_count(tool_name);
                if count >= 3
                    && let Some(suggestion) = detector.suggest_alternative(tool_name)
                {
                    return Some(format!(
                        "Tool '{}' called {} times. Consider alternative approach:\n{}",
                        tool_name, count, suggestion
                    ));
                }
            }
            Err(_) => {
                // If we can't get the lock, don't block execution
                tracing::debug!(
                    "Could not acquire loop detector read lock for {}",
                    tool_name
                );
            }
        }
        None
    }

    /// Record tool call in loop detector
    /// Returns warning message if loop detected
    pub fn record_tool_call(&self, tool_name: &str, args: &Value) -> Option<String> {
        self.record_rate_history(tool_name);
        if let Ok(mut detector) = self.loop_detector.write() {
            detector.record_call(tool_name, args)
        } else {
            None
        }
    }

    /// Check if operation is destructive based on tool and arguments
    fn is_destructive_operation(&self, tool_name: &str, args: &Value) -> bool {
        // Explicitly destructive tools
        if self.destructive_tools.contains(tool_name) {
            return true;
        }

        // Check for destructive shell commands
        if (tool_name == "shell" || tool_name == tools::RUN_PTY_CMD)
            && let Some(cmd) = args.get("command").and_then(|v| v.as_str())
        {
            return self.is_destructive_command(cmd);
        }

        false
    }

    /// Check if shell command is destructive
    fn is_destructive_command(&self, cmd: &str) -> bool {
        let cmd_lower = cmd.to_lowercase();

        // Enhanced destructive patterns
        let destructive_patterns = [
            // File deletion
            "rm -rf",
            "rm -fr",
            "rm -r",
            "rm --recursive",
            // Git destructive operations
            "git reset --hard",
            "git push --force",
            "git push -f",
            "git clean -fd",
            "git clean -fdx",
            "git branch -D",
            // System operations
            "truncate",
            "dd if=",
            "> /dev/",
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
            "chmod -R",
            "chown -R",
        ];

        destructive_patterns
            .iter()
            .any(|pattern| cmd_lower.contains(pattern))
    }

    /// Validate tool arguments for safety
    pub fn validate_args(&self, tool_name: &str, args: &Value) -> Result<()> {
        if tool_name == tools::WRITE_FILE || tool_name == tools::EDIT_FILE {
            self.validate_file_path(args.get("path"))?;
        } else if tool_name == "shell" || tool_name == tools::RUN_PTY_CMD {
            self.validate_command(args.get("command"))?;
        } else if tool_name == tools::LIST_FILES {
            self.validate_list_files_args(args)?;
        }
        Ok(())
    }

    /// Validate file path is within workspace boundaries
    fn validate_file_path(&self, path: Option<&Value>) -> Result<()> {
        let path_str = path
            .and_then(|v| v.as_str())
            .context("Missing or invalid 'path' argument")?;

        let path_obj = Path::new(path_str);

        // Check for absolute paths
        if path_obj.is_absolute() {
            // Allow /tmp/vtcode paths
            if path_str.starts_with("/tmp/vtcode") {
                return Ok(());
            }

            // Check if within workspace
            if let Some(workspace) = &self.workspace_dir {
                let canonical_path = path_obj.canonicalize().ok();
                let canonical_workspace = workspace.canonicalize().ok();

                if let (Some(p), Some(w)) = (canonical_path, canonical_workspace)
                    && p.starts_with(&w)
                {
                    return Ok(());
                }
            }

            anyhow::bail!(
                "Absolute path outside workspace boundary: {}. \
                 Only paths within WORKSPACE_DIR or /tmp/vtcode are allowed.",
                path_str
            );
        }

        // Prevent parent directory traversal that could escape workspace
        if path_str.contains("..") {
            warn!("Path contains parent directory traversal: {}", path_str);

            // Resolve the path and check if it stays within workspace
            if let Some(workspace) = &self.workspace_dir {
                let resolved = workspace.join(path_str);
                if let Ok(canonical) = resolved.canonicalize()
                    && let Ok(canonical_workspace) = workspace.canonicalize()
                    && !canonical.starts_with(&canonical_workspace)
                {
                    anyhow::bail!("Path traversal escapes workspace boundary: {}", path_str);
                }
            } else {
                anyhow::bail!(
                    "Path traversal blocked: workspace boundary is unknown for '{}'",
                    path_str
                );
            }
        }

        // If workspace directory is unknown, conservatively block writes to avoid escaping boundaries.
        if self.workspace_dir.is_none() {
            anyhow::bail!(
                "Workspace directory is not set; refusing to write to relative path '{}'. \
                 Set WORKSPACE_DIR or call set_workspace_dir().",
                path_str
            );
        }

        Ok(())
    }

    /// Validate shell command for safety
    fn validate_command(&self, cmd: Option<&Value>) -> Result<()> {
        let cmd_str = cmd
            .and_then(|v| v.as_str())
            .context("Missing or invalid 'command' argument")?;

        if self.is_destructive_command(cmd_str) {
            anyhow::bail!(
                "Destructive command requires explicit confirmation: {}",
                cmd_str
            );
        }

        Ok(())
    }

    /// Validate list_files arguments to prevent root listing loops
    fn validate_list_files_args(&self, args: &Value) -> Result<()> {
        if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
            let normalized = path.trim_start_matches("./").trim_start_matches('/');
            if normalized.is_empty() || normalized == "." {
                anyhow::bail!(
                    "list_files on root directory is blocked (causes loops). \
                     Use specific subdirectories like 'src/', 'vtcode-core/src/', etc."
                );
            }
        } else {
            // No path = root
            anyhow::bail!(
                "list_files requires explicit path. \
                 Root directory listing is blocked to prevent loops."
            );
        }
        Ok(())
    }

    /// Generate dry-run preview for verification
    pub fn generate_preview(&self, tool_name: &str, args: &Value) -> String {
        if tool_name == tools::WRITE_FILE {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let lines = content.lines().count();
            let size_kb = content.len() / 1024;

            let preview = if lines > 10 {
                let first_lines: Vec<_> = content.lines().take(5).collect();
                format!(
                    "\n  {}\n  ... ({} more lines)",
                    first_lines.join("\n  "),
                    lines - 5
                )
            } else {
                format!("\n  {}", content.lines().collect::<Vec<_>>().join("\n  "))
            };

            format!(
                "Will write {} lines ({} KB) to: {}\nPreview:{}",
                lines, size_kb, path, preview
            )
        } else if tool_name == tools::EDIT_FILE {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let old_str = args.get("old_str").and_then(|v| v.as_str()).unwrap_or("");
            let new_str = args.get("new_str").and_then(|v| v.as_str()).unwrap_or("");

            format!(
                "Will edit file: {}\nReplacing:\n  {}\nWith:\n  {}",
                path,
                old_str.lines().take(3).collect::<Vec<_>>().join("\n  "),
                new_str.lines().take(3).collect::<Vec<_>>().join("\n  ")
            )
        } else if tool_name == "shell" || tool_name == tools::RUN_PTY_CMD {
            let cmd = args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let is_destructive = self.is_destructive_command(cmd);

            let warning = if is_destructive {
                "\n[WARN] WARNING: This command is potentially destructive!"
            } else {
                ""
            };

            format!("Will execute: {}{}", cmd, warning)
        } else if tool_name == tools::APPLY_PATCH {
            let patch = args.get("patch").and_then(|v| v.as_str()).unwrap_or("");
            let lines = patch.lines().count();
            format!("Will apply patch with {} lines of changes", lines)
        } else {
            format!("Will execute: {} with args: {:?}", tool_name, args)
        }
    }

    /// Record execution result for statistics tracking
    pub fn record_execution(&self, tool_name: &str, success: bool) {
        if let Ok(mut stats) = self.execution_stats.write() {
            let entry = stats.entry(tool_name.to_string()).or_default();
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
            stats
                .get(tool_name)
                .map(|s| s.success_rate())
                .unwrap_or(0.0)
        } else {
            0.0
        }
    }

    /// Get execution statistics for a tool
    pub fn get_tool_stats(&self, tool_name: &str) -> Option<(usize, usize, usize)> {
        if let Ok(stats) = self.execution_stats.read() {
            stats.get(tool_name).map(|s| {
                (
                    s.total_attempts,
                    s.successful_executions,
                    s.failed_executions,
                )
            })
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
            let entries = history.entry(tool_name.to_string()).or_default();
            entries.push_back(now);
            while let Some(front) = entries.front()
                && now.duration_since(*front) > self.rate_limit_window
            {
                entries.pop_front();
            }
        }
    }

    fn is_rate_limited(&self, tool_name: &str) -> bool {
        let now = Instant::now();

        // First, try with a read lock to check without modifying
        // This is the common fast path when there are no expired entries
        if let Ok(history) = self.rate_history.read() {
            if let Some(entries) = history.get(tool_name) {
                // Quick check: if all entries are within window and at limit, we're rate limited
                if entries
                    .front()
                    .is_some_and(|front| now.duration_since(*front) <= self.rate_limit_window)
                    && entries.len() >= self.rate_limit_max_calls
                {
                    return true;
                }
                // If entries exist but are under limit and no expired entries, not rate limited
                if entries
                    .front()
                    .is_some_and(|front| now.duration_since(*front) <= self.rate_limit_window)
                    && entries.len() < self.rate_limit_max_calls
                {
                    return false;
                }
            } else {
                // No entries for this tool, definitely not rate limited
                return false;
            }
        }

        // Fall back to write lock only when we need to clean up expired entries
        if let Ok(mut history) = self.rate_history.write() {
            let entries = history.entry(tool_name.to_string()).or_default();
            while let Some(front) = entries.front()
                && now.duration_since(*front) > self.rate_limit_window
            {
                entries.pop_front();
            }
            return entries.len() >= self.rate_limit_max_calls;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_safe_tools_auto_execute() {
        let executor = AutonomousExecutor::new();

        for tool in SAFE_AUTONOMOUS_TOOLS {
            let policy = executor.get_policy(tool, &json!({}));
            assert_eq!(policy, AutonomousPolicy::AutoExecute);
        }
    }

    #[test]
    fn test_destructive_commands_require_confirmation() {
        let executor = AutonomousExecutor::new();

        let destructive_cmds = vec![
            "rm -rf /tmp/test",
            "git reset --hard HEAD~1",
            "git push --force origin main",
            "git clean -fdx",
            "chmod -R 777 /",
        ];

        for cmd in destructive_cmds {
            let args = json!({"command": cmd});
            let policy = executor.get_policy("shell", &args);
            assert_eq!(policy, AutonomousPolicy::RequireConfirmation);
        }
    }

    #[test]
    fn test_list_files_root_blocked() {
        let executor = AutonomousExecutor::new();

        let root_variations = vec![
            json!({"path": "."}),
            json!({"path": ""}),
            json!({"path": "./"}),
            json!({}),
        ];

        for args in root_variations {
            let result = executor.validate_args(tools::LIST_FILES, &args);
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("root directory"));
        }
    }

    #[test]
    fn test_list_files_specific_path_allowed() {
        let executor = AutonomousExecutor::new();

        let args = json!({"path": "src/core/"});
        let result = executor.validate_args(tools::LIST_FILES, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verification_tools_need_preview() {
        let executor = AutonomousExecutor::new();

        for tool in VERIFICATION_REQUIRED_TOOLS {
            let policy = executor.get_policy(tool, &json!({}));
            assert_eq!(policy, AutonomousPolicy::VerifyThenExecute);
        }
    }

    #[test]
    fn test_loop_detection_integration() {
        let executor = AutonomousExecutor::new();
        let args = json!({"path": "src/"});

        // First two calls should not block
        assert!(executor.should_block(tools::GREP_FILE, &args).is_none());
        executor.record_tool_call(tools::GREP_FILE, &args);

        assert!(executor.should_block(tools::GREP_FILE, &args).is_none());
        executor.record_tool_call(tools::GREP_FILE, &args);

        // Third call should trigger warning
        executor.record_tool_call(tools::GREP_FILE, &args);
        let block_msg = executor.should_block(tools::GREP_FILE, &args);
        assert!(block_msg.is_some());
        assert!(block_msg.unwrap().contains("alternative"));
    }

    #[test]
    fn test_execution_stats_tracking() {
        let executor = AutonomousExecutor::new();

        // Record some executions
        executor.record_execution(tools::GREP_FILE, true);
        executor.record_execution(tools::GREP_FILE, true);
        executor.record_execution(tools::GREP_FILE, false);

        // Check stats
        let (total, success, failed) = executor.get_tool_stats(tools::GREP_FILE).unwrap();
        assert_eq!(total, 3);
        assert_eq!(success, 2);
        assert_eq!(failed, 1);

        // Check success rate
        let rate = executor.get_success_rate(tools::GREP_FILE);
        assert!((rate - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_workspace_boundary_validation() {
        let mut executor = AutonomousExecutor::new();
        let temp_dir = std::env::temp_dir();
        executor.set_workspace_dir(temp_dir.clone());

        // Absolute path outside workspace should fail
        let args = json!({"path": "/etc/passwd"});
        let result = executor.validate_args(tools::WRITE_FILE, &args);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("workspace boundary")
        );

        // /tmp/vtcode should be allowed
        let args = json!({"path": "/tmp/vtcode/test.txt"});
        let result = executor.validate_args(tools::WRITE_FILE, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_enhanced_destructive_patterns() {
        let executor = AutonomousExecutor::new();

        let destructive_cmds = vec![
            "rm -r somedir",
            "git branch -D feature",
            "npm uninstall -g package",
            "cargo uninstall tool",
        ];

        for cmd in destructive_cmds {
            assert!(executor.is_destructive_command(cmd));
        }
    }

    #[test]
    fn test_enhanced_preview_generation() {
        let executor = AutonomousExecutor::new();

        // Test write_file preview
        let args = json!({
            "path": "test.rs",
            "content": "line1\nline2\nline3"
        });
        let preview = executor.generate_preview(tools::WRITE_FILE, &args);
        assert!(preview.contains("3 lines"));
        assert!(preview.contains("test.rs"));

        // Test edit_file preview
        let args = json!({
            "path": "main.rs",
            "old_str": "old code",
            "new_str": "new code"
        });
        let preview = executor.generate_preview(tools::EDIT_FILE, &args);
        assert!(preview.contains("main.rs"));
        assert!(preview.contains("old code"));
        assert!(preview.contains("new code"));

        // Test destructive command preview
        let args = json!({"command": "rm -rf /tmp/test"});
        let preview = executor.generate_preview("shell", &args);
        assert!(preview.contains("WARNING"));
        assert!(preview.contains("destructive"));
    }

    #[test]
    fn test_parent_traversal_detection() {
        let mut executor = AutonomousExecutor::new();
        let workspace = PathBuf::from("/workspace");
        executor.set_workspace_dir(workspace);

        // Path with .. that stays in workspace should be allowed (with warning)
        let args = json!({"path": "src/../lib/file.rs"});
        let result = executor.validate_args(tools::WRITE_FILE, &args);
        // This should succeed but log a warning
        assert!(result.is_ok());
    }
}
