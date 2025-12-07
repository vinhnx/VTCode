//! Autonomous tool execution with safety checks
//!
//! Implements safe autonomous execution following AGENTS.md principles:
//! - Act, don't ask (for safe operations)
//! - Verify before destructive operations
//! - Loop detection and prevention
//! - Context-aware decision making

use crate::config::constants::tools;
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashSet;
use tracing::warn;

/// Tools that are always safe to execute autonomously
const SAFE_AUTONOMOUS_TOOLS: &[&str] = &[
    tools::GREP_FILE,
    tools::LIST_FILES,
    tools::READ_FILE,
    tools::SEARCH_TOOLS,
    tools::GET_ERRORS,
    tools::DEBUG_AGENT,
    tools::ANALYZE_AGENT,
    tools::LIST_PTY_SESSIONS,
    tools::READ_PTY_SESSION,
    tools::UPDATE_PLAN,
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
const DESTRUCTIVE_TOOLS: &[&str] = &[
    tools::APPLY_PATCH,
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

/// Autonomous tool executor with safety checks
pub struct AutonomousExecutor {
    safe_tools: HashSet<String>,
    verification_tools: HashSet<String>,
    destructive_tools: HashSet<String>,
}

impl AutonomousExecutor {
    pub fn new() -> Self {
        Self {
            safe_tools: SAFE_AUTONOMOUS_TOOLS.iter().map(|s| s.to_string()).collect(),
            verification_tools: VERIFICATION_REQUIRED_TOOLS.iter().map(|s| s.to_string()).collect(),
            destructive_tools: DESTRUCTIVE_TOOLS.iter().map(|s| s.to_string()).collect(),
        }
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

    /// Check if operation is destructive based on tool and arguments
    fn is_destructive_operation(&self, tool_name: &str, args: &Value) -> bool {
        // Explicitly destructive tools
        if self.destructive_tools.contains(tool_name) {
            return true;
        }

        // Check for destructive shell commands
        if tool_name == "shell" || tool_name == tools::RUN_PTY_CMD {
            if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                return self.is_destructive_command(cmd);
            }
        }

        false
    }

    /// Check if shell command is destructive
    fn is_destructive_command(&self, cmd: &str) -> bool {
        let cmd_lower = cmd.to_lowercase();
        
        // Destructive patterns
        let destructive_patterns = [
            "rm -rf",
            "rm -fr",
            "git reset --hard",
            "git push --force",
            "git push -f",
            "truncate",
            "dd if=",
            "> /dev/",
            "mkfs",
            "fdisk",
        ];

        destructive_patterns.iter().any(|pattern| cmd_lower.contains(pattern))
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

    /// Validate file path is within workspace
    fn validate_file_path(&self, path: Option<&Value>) -> Result<()> {
        let path_str = path
            .and_then(|v| v.as_str())
            .context("Missing or invalid 'path' argument")?;

        // Prevent absolute paths outside workspace
        if path_str.starts_with('/') && !path_str.starts_with("/tmp/vtcode") {
            anyhow::bail!("Absolute paths outside workspace are not allowed: {}", path_str);
        }

        // Prevent parent directory traversal
        if path_str.contains("..") {
            warn!("Path contains parent directory traversal: {}", path_str);
        }

        Ok(())
    }

    /// Validate shell command for safety
    fn validate_command(&self, cmd: Option<&Value>) -> Result<()> {
        let cmd_str = cmd
            .and_then(|v| v.as_str())
            .context("Missing or invalid 'command' argument")?;

        if self.is_destructive_command(cmd_str) {
            anyhow::bail!("Destructive command requires explicit confirmation: {}", cmd_str);
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
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("unknown");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let lines = content.lines().count();
            format!("Will write {} lines to: {}", lines, path)
        } else if tool_name == tools::EDIT_FILE {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("unknown");
            format!("Will edit file: {}", path)
        } else if tool_name == "shell" || tool_name == tools::RUN_PTY_CMD {
            let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("unknown");
            format!("Will execute: {}", cmd)
        } else {
            format!("Will execute: {}", tool_name)
        }
    }
}

impl Default for AutonomousExecutor {
    fn default() -> Self {
        Self::new()
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
            let result = executor.validate_args("list_files", &args);
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("root directory"));
        }
    }

    #[test]
    fn test_list_files_specific_path_allowed() {
        let executor = AutonomousExecutor::new();
        
        let args = json!({"path": "src/core/"});
        let result = executor.validate_args("list_files", &args);
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
}
