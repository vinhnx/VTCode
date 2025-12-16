use crate::config::PtyConfig;
use crate::mcp::{DetailLevel, ToolDiscovery};
use crate::tools::apply_patch::{Patch, PatchOperation};
use crate::tools::editing::PatchLine;
use crate::tools::grep_file::GrepSearchInput;
use crate::tools::traits::Tool;
use crate::tools::types::VTCodePtySession;
use crate::tools::{PlanUpdateResult, UpdatePlanArgs};

use crate::utils::diff::{DiffOptions, compute_diff};
use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use chrono::prelude::Utc;
use futures::future::BoxFuture;
use portable_pty::PtySize;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use shell_words::{join, split};
use std::fmt::Write as _;
use std::{
    env,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::fs;
use tokio::time::sleep;
use tracing::{debug, info, trace, warn};

use crate::config::constants::defaults::{
    DEFAULT_PTY_OUTPUT_BYTE_FUSE, DEFAULT_PTY_OUTPUT_MAX_TOKENS,
};

const RUN_PTY_POLL_TIMEOUT_SECS: u64 = 5;
// For known long-running commands, wait longer before returning partial output
const RUN_PTY_POLL_TIMEOUT_LONG_RUNNING: u64 = 30;
const SEARCH_REPLACE_MAX_BYTES: usize = 2 * 1024 * 1024;

// Conservative PTY command policy inspired by bash allow/deny defaults.
const PTY_DENY_PREFIXES: &[&str] = &[
    "bash -i",
    "sh -i",
    "zsh -i",
    "fish -i",
    "python -i",
    "python3 -i",
    "ipython",
    "nano",
    "vim",
    "vi",
    "emacs",
    "top",
    "htop",
    "less",
    "more",
    "screen",
    "tmux",
];

const PTY_DENY_STANDALONE: &[&str] = &["python", "python3", "bash", "sh", "zsh", "fish"];

#[allow(dead_code)]
const PTY_ALLOW_PREFIXES: &[&str] = &[
    "pwd",
    "whoami",
    "ls",
    "git status",
    "git diff",
    "git log",
    "stat",
    "which",
    "echo",
    "cat",
];

fn enforce_pty_command_policy(display_command: &str, confirm: bool) -> Result<()> {
    let lower = display_command.to_ascii_lowercase();
    let trimmed = lower.trim();
    let is_standalone = trimmed.split_whitespace().count() == 1;

    let deny_match = PTY_DENY_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix));
    let standalone_denied = is_standalone && PTY_DENY_STANDALONE.contains(&trimmed);

    if deny_match || standalone_denied {
        if confirm {
            return Ok(());
        }
        return Err(anyhow!(
            "Command '{}' is blocked by PTY safety policy. Set confirm=true to force execution.",
            display_command
        ));
    }

    // Allowlisted commands are simply allowed; we rely on general policy for others.
    Ok(())
}

fn matches_context(
    content: &str,
    idx: usize,
    search_len: usize,
    before: Option<&str>,
    after: Option<&str>,
) -> bool {
    if let Some(prefix) = before
        && !content[..idx].ends_with(prefix)
    {
        return false;
    }

    if let Some(suffix) = after {
        let end = idx.saturating_add(search_len);
        if end > content.len() || !content[end..].starts_with(suffix) {
            return false;
        }
    }

    true
}

const LONG_RUNNING_COMMANDS: &[&str] = &[
    "cargo", "npm", "yarn", "pnpm", "pip", "python", "make", "docker",
];

/// Commands that produce structured build output (errors, warnings)
/// For these, we apply smarter extraction that prioritizes error lines.
const BUILD_OUTPUT_COMMANDS: &[&str] = &[
    "cargo", "rustc", "npm", "yarn", "pnpm", "tsc", "eslint", "make", "gcc", "clang",
];

use super::ToolRegistry;

impl ToolRegistry {
    #[allow(dead_code)]
    pub(super) fn get_errors_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            #[derive(serde::Deserialize)]
            struct Args {
                #[serde(default = "default_scope")]
                scope: String,
                #[serde(default = "default_limit")]
                limit: usize,
                #[serde(default = "default_detailed")]
                detailed: bool,
                #[serde(default)]
                pattern: Option<String>,
            }

            fn default_scope() -> String {
                "archive".into()
            }

            const fn default_limit() -> usize {
                5
            }

            const fn default_detailed() -> bool {
                false
            }

            let parsed: Args = serde_json::from_value(args).unwrap_or(Args {
                scope: default_scope(),
                limit: default_limit(),
                detailed: default_detailed(),
                pattern: None,
            });

            // Use Cow to avoid allocation when possible
            let workspace_root = self.workspace_root_str();

            // Initialize comprehensive error report with pre-allocated vectors
            let mut error_report = serde_json::json!({
                "timestamp": Utc::now().to_rfc3339(),
                "workspace": workspace_root,
                "scope": parsed.scope,
                "detailed": parsed.detailed,
                "total_errors": 0,
                "recent_errors": Vec::<Value>::with_capacity(parsed.limit.min(100)), // Cap capacity to prevent excessive allocation
                "suggestions": Vec::<String>::with_capacity(20), // Pre-allocate for common suggestions
                "diagnostics": {
                    "tool_execution_failures": Vec::<Value>::with_capacity(10),
                    "recent_tool_calls": Vec::<Value>::with_capacity(20),
                    "system_state": {}
                }
            });

            if parsed.scope == "archive" || parsed.scope == "all" {
                // Search in session archives
                let sessions =
                    match crate::utils::session_archive::list_recent_sessions(parsed.limit).await {
                        Ok(list) => list,
                        Err(err) => {
                            tracing::warn!("Failed to list session archives: {}", err);
                            Vec::with_capacity(0) // Use with_capacity(0) instead of Vec::new()
                        }
                    };

                let mut issues = Vec::with_capacity(parsed.limit.min(100)); // Cap capacity to prevent excessive allocation
                let mut total_errors = 0usize;

                for listing in sessions {
                    for message in listing.snapshot.messages {
                        // Check assistant messages for error-like content
                        if message.role == crate::llm::provider::MessageRole::Assistant {
                            let text = message.content.as_text();
                            let lower = text.to_lowercase();

                            // Use shared error detection patterns
                            let error_patterns = crate::tools::constants::ERROR_DETECTION_PATTERNS;

                            let matches_pattern = if let Some(ref pattern) = parsed.pattern {
                                lower.contains(&pattern.to_lowercase())
                            } else {
                                error_patterns.iter().any(|&pat| lower.contains(pat))
                            };

                            if matches_pattern {
                                total_errors += 1;
                                issues.push(serde_json::json!({
                                    "type": "session_error",
                                    "workspace": listing.snapshot.metadata.workspace_label,
                                    "path": listing.snapshot.metadata.workspace_path,
                                    "message": text.trim(),
                                    "timestamp": listing.snapshot.ended_at.to_rfc3339(),
                                }));
                            }
                        }
                    }
                }

                error_report["recent_errors"] = serde_json::to_value(issues)
                    .unwrap_or_else(|_| serde_json::Value::Array(vec![]));
                error_report["total_errors"] = serde_json::to_value(total_errors)
                    .unwrap_or_else(|_| serde_json::Value::Number(serde_json::Number::from(0)));
            }

            // Enhanced suggestions with self-fix capabilities
            let mut suggestions: Vec<String> = Vec::with_capacity(10);
            let total_errors = error_report["total_errors"]
                .as_u64()
                .unwrap_or(0)
                .try_into()
                .unwrap_or(0_usize);

            if total_errors > 0 {
                suggestions.push(
                    "Review recent assistant tool calls and session archives for more details"
                        .into(),
                );

                if parsed.detailed {
                    suggestions
                        .push("Consider running 'debug_agent' for more system diagnostics".into());
                    suggestions
                        .push("Try 'analyze_agent' to understand current behavior patterns".into());
                    suggestions.push(
                        "Use 'search_tools' to find specific tools for error handling".into(),
                    );
                }

                // Self-fix suggestions based on common error patterns
                // Extract error messages to check for patterns
                let empty_vec = Vec::with_capacity(0); // Use with_capacity(0) instead of Vec::new()
                let recent_errors_array = error_report["recent_errors"]
                    .as_array()
                    .unwrap_or(&empty_vec);
                let error_messages: Vec<String> = recent_errors_array
                    .iter()
                    .filter_map(|err| err.get("message").and_then(|m| m.as_str()))
                    .map(|s| s.to_lowercase())
                    .collect();

                // File not found errors
                if error_messages.iter().any(|msg| {
                    msg.contains("not found")
                        || msg.contains("no such file")
                        || msg.contains("file does not exist")
                }) {
                    suggestions.extend_from_slice(&[
                        String::from("File not found errors: Verify file paths exist and are accessible"),
                        String::from("Try using 'list_files' to check directory contents before accessing files"),
                        String::from("Consider creating missing files with 'create_file' or 'write_file' tools"),
                    ]);
                }

                // Permission errors
                if error_messages.iter().any(|msg| {
                    msg.contains("permission")
                        || msg.contains("access denied")
                        || msg.contains("forbidden")
                }) {
                    suggestions.push(
                        "Permission errors: Check file permissions and workspace access".into(),
                    );
                    suggestions.push(
                        "Consider running with appropriate permissions or changing file ownership"
                            .into(),
                    );
                }

                // Command execution errors
                if error_messages.iter().any(|msg| {
                    msg.contains("command not found")
                        || msg.contains("command failed")
                        || msg.contains("exit code")
                }) {
                    suggestions.push("Command execution errors: Verify command availability with 'list_files' or check PATH environment".into());
                    suggestions.push(
                        "Use 'run_pty_cmd' to test commands manually before automation".into(),
                    );
                }

                // Git-related errors
                if error_messages.iter().any(|msg| {
                    msg.contains("git") && (msg.contains("error") || msg.contains("fatal"))
                }) {
                    suggestions
                        .push("Git errors: Check repository status and Git configuration".into());
                    suggestions.push(
                        "Run 'run_pty_cmd' with 'git status' to diagnose repository issues".into(),
                    );
                }

                // Network/HTTP errors
                if error_messages.iter().any(|msg| {
                    msg.contains("connection")
                        || msg.contains("timeout")
                        || msg.contains("network")
                        || msg.contains("http")
                        || msg.contains("ssl")
                        || msg.contains("tls")
                }) {
                    suggestions.push(
                        "Network/HTTP errors: Check internet connectivity and proxy settings"
                            .into(),
                    );
                    suggestions.push(
                        "Verify API endpoints and credentials if using external services".into(),
                    );
                    suggestions.push(
                        "Consider using 'web_fetch' with proper error handling for web requests"
                            .into(),
                    );
                }

                // Memory/resource errors
                if error_messages.iter().any(|msg| {
                    msg.contains("memory")
                        || msg.contains("oom")
                        || msg.contains("out of")
                        || msg.contains("resource")
                        || msg.contains("too large")
                }) {
                    suggestions.push(
                        "Memory/resource errors: Consider processing data in smaller chunks".into(),
                    );
                    suggestions.push("Use 'execute_code' with memory-efficient algorithms when handling large files".into());
                }

                // Add a general recommendation to use the enhanced get_errors
                suggestions.push(
                    "For more detailed diagnostics, run 'get_errors' with detailed=true parameter"
                        .into(),
                );
            } else {
                suggestions.push("No obvious errors discovered in recent sessions".into());
                if parsed.detailed {
                    suggestions.push(
                        "Run 'debug_agent' or 'analyze_agent' for proactive system checks".into(),
                    );
                    suggestions.push("Consider performing routine maintenance tasks if working with large projects".into());
                }
            }

            error_report["suggestions"] = serde_json::to_value(suggestions)
                .unwrap_or_else(|_| serde_json::Value::Array(vec![]));

            // Add system diagnostics if detailed mode
            if parsed.detailed {
                let available_tools = self.available_tools().await;

                // Get actual recent tool execution history
                let recent_executions = self.get_recent_tool_executions(20); // Last 20 executions
                let recent_failures = self.get_recent_tool_failures(10); // Last 10 failures

                // Convert to JSON format for the report with capacity planning
                let recent_tool_calls: Vec<Value> = recent_executions
                    .iter()
                    .map(|record| {
                        json!({
                            "tool_name": &record.tool_name, // Use reference to avoid clone
                            "timestamp": record.timestamp.duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0),
                            "success": record.success,
                        })
                    })
                    .collect();

                // Convert failures to JSON format with capacity planning
                let tool_execution_failures: Vec<Value> = recent_failures
                    .iter()
                    .map(|record| {
                        json!({
                            "tool_name": &record.tool_name, // Use reference to avoid clone
                            "timestamp": record.timestamp.duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0),
                            "error": match &record.result {
                                Ok(_) => "Unexpected success in failure list".to_string(),
                                Err(e) => e.clone(),
                            },
                            "args": &record.args, // Use reference to avoid clone
                        })
                    })
                    .collect();

                // Use Cow to avoid allocation
                let workspace_root = self.workspace_root_str();
                let system_state = json!({
                    "available_tools_count": available_tools.len(),
                    "workspace_root": workspace_root,
                    "recent_tool_calls": recent_tool_calls
                });

                // Self-diagnosis logic
                let mut self_diagnosis_issues: Vec<String> = Vec::with_capacity(5);

                // Check for common system issues
                if available_tools.is_empty() {
                    self_diagnosis_issues.push("No tools are currently available - this may indicate a system initialization issue".into());
                }

                // Check workspace status
                let workspace_path = self.workspace_root();
                if !workspace_path.exists() {
                    self_diagnosis_issues.push(format!(
                        "Workspace directory does not exist: {}",
                        workspace_path.display()
                    ));
                } else if !workspace_path.is_dir() {
                    self_diagnosis_issues.push(format!(
                        "Workspace path is not a directory: {}",
                        workspace_path.display()
                    ));
                }

                // Check for execution failures in history
                if !recent_failures.is_empty() {
                    let failure_count = recent_failures.len();
                    self_diagnosis_issues.push(format!(
                        "Found {} recent tool execution failures that need attention",
                        failure_count
                    ));
                }

                // Provide self-fix suggestions
                let mut self_fix_suggestions: Vec<String> = Vec::with_capacity(5);
                if !self_diagnosis_issues.is_empty() {
                    self_fix_suggestions
                        .push("Run system initialization to ensure proper setup".into());
                    self_fix_suggestions.push("Verify workspace directory and permissions".into());
                    self_fix_suggestions
                        .push("Check that all required tools are properly configured".into());

                    if !recent_failures.is_empty() {
                        self_fix_suggestions
                            .push("Review recent tool failures and their error messages".into());
                        self_fix_suggestions.push(
                            "Consider retrying failed operations with corrected parameters".into(),
                        );
                    }
                } else if total_errors == 0 && recent_failures.is_empty() {
                    self_fix_suggestions
                        .push("System appears healthy. No immediate issues detected.".into());
                    if parsed.scope != "all" {
                        self_fix_suggestions.push(
                            "Consider running with scope='all' for comprehensive check".into(),
                        );
                    }
                } else {
                    self_fix_suggestions.push(
                        "Based on the errors found, review the suggestions provided above".into(),
                    );
                    self_fix_suggestions.push(
                        "Consider running 'debug_agent' for additional system insights".into(),
                    );

                    if !recent_failures.is_empty() {
                        self_fix_suggestions.push(
                            "Examine recent tool execution failures in the diagnostics section"
                                .into(),
                        );
                    }
                }

                let self_diagnosis_summary = if !self_diagnosis_issues.is_empty() {
                    format!(
                        "Self-diagnosis found {} potential system issues. {}",
                        self_diagnosis_issues.len(),
                        self_diagnosis_issues.join("; ")
                    )
                } else {
                    "Self-diagnosis: System appears healthy with no critical issues detected".into()
                };

                error_report["diagnostics"] = json!({
                    "tool_execution_failures": tool_execution_failures,
                    "recent_tool_calls_count": recent_executions.len(),
                    "recent_tool_failures_count": recent_failures.len(),
                    "recent_tool_calls": recent_tool_calls,
                    "system_state": system_state,
                    "self_diagnosis": self_diagnosis_summary,
                    "self_diagnosis_issues": self_diagnosis_issues,
                    "self_fix_suggestions": self_fix_suggestions
                });
            }

            Ok(error_report)
        })
    }

    pub(super) fn debug_agent_executor(&mut self, _args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            // Lightweight snapshot of registry state for diagnostics; this will not include full session context.
            let tools = self.available_tools().await;
            let workspace_root = self.workspace_root_str();
            let stats = json!({
                "tools_registered": tools,
                "workspace_root": workspace_root,
            });
            Ok(stats)
        })
    }

    pub(super) fn analyze_agent_executor(&mut self, _args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            // Aggregate some simple analysis metrics for the agent's behavior
            let available_tools = self.available_tools().await;
            Ok(json!({
                "available_tools_count": available_tools.len(),
                "available_tools": available_tools,
            }))
        })
    }
}

impl ToolRegistry {
    pub(super) fn grep_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let manager = self.inventory.grep_file_manager();
        Box::pin(async move {
            #[derive(Debug, Deserialize)]
            struct GrepArgs {
                pattern: String,
                #[serde(default = "default_grep_path", alias = "root", alias = "search_path")]
                path: String,
                #[serde(default)]
                max_results: Option<usize>,
                #[serde(default)]
                case_sensitive: Option<bool>,
                #[serde(default)]
                literal: Option<bool>,
                #[serde(default)]
                glob_pattern: Option<String>,
                #[serde(default)]
                context_lines: Option<usize>,
                #[serde(default)]
                include_hidden: Option<bool>,
                #[serde(default)]
                respect_ignore_files: Option<bool>,
                #[serde(default)]
                max_file_size: Option<usize>,
                #[serde(default)]
                search_hidden: Option<bool>,
                #[serde(default)]
                search_binary: Option<bool>,
                #[serde(default)]
                files_with_matches: Option<bool>,
                #[serde(default)]
                type_pattern: Option<String>,
                #[serde(default)]
                invert_match: Option<bool>,
                #[serde(default)]
                word_boundaries: Option<bool>,
                #[serde(default)]
                line_number: Option<bool>,
                #[serde(default)]
                column: Option<bool>,
                #[serde(default)]
                only_matching: Option<bool>,
                #[serde(default)]
                trim: Option<bool>,
                #[serde(default)]
                max_result_bytes: Option<usize>,
                #[serde(default)]
                timeout_secs: Option<u64>,
                #[serde(default)]
                extra_ignore_globs: Option<Vec<String>>,
            }

            fn default_grep_path() -> String {
                ".".into()
            }

            let payload: GrepArgs = serde_json::from_value(args).context(
                "Invalid 'grep_file' arguments. Expected JSON object with: \n\
                 - pattern (required, string): regex pattern to search for\n\
                 - path (optional, string): directory to search (defaults to '.')\n\
                 - max_results (optional, number): max results to return\n\
                 Example: {\"pattern\": \"TODO\", \"path\": \"src\", \"max_results\": 5}",
            )?;

            // Validate pattern parameter
            if payload.pattern.is_empty() {
                return Err(anyhow!("pattern cannot be empty"));
            }

            // Validate regex pattern syntax if not using literal matching
            if payload.literal != Some(true) {
                if let Err(e) = regex::Regex::new(&payload.pattern) {
                    return Err(anyhow!(
                        "Invalid regex pattern: {}. If you meant to match a literal string, set literal: true",
                        e
                    ));
                }
            }

            // Validate the path parameter to avoid security issues
            if payload.path.contains("..") || payload.path.starts_with('/') {
                return Err(anyhow!(
                    "Path must be a relative path and cannot contain '..' or start with '/'"
                ));
            }

            // Validate and enforce hard limits
            if let Some(max_results) = payload.max_results {
                // Enforce a reasonable upper limit to prevent excessive resource usage
                const MAX_ALLOWED_RESULTS: usize = 1000;
                if max_results > MAX_ALLOWED_RESULTS {
                    return Err(anyhow!(
                        "max_results ({}) exceeds the maximum allowed value of {}",
                        max_results,
                        MAX_ALLOWED_RESULTS
                    ));
                }
                if max_results == 0 {
                    return Err(anyhow!("max_results must be greater than 0"));
                }
            }

            if let Some(max_file_size) = payload.max_file_size {
                // Enforce a reasonable upper limit for file size (100MB)
                const MAX_ALLOWED_FILE_SIZE: usize = 100 * 1024 * 1024; // 100MB in bytes
                if max_file_size > MAX_ALLOWED_FILE_SIZE {
                    return Err(anyhow!(
                        "max_file_size ({}) exceeds the maximum allowed value of {} bytes (100MB)",
                        max_file_size,
                        MAX_ALLOWED_FILE_SIZE
                    ));
                }
                if max_file_size == 0 {
                    return Err(anyhow!("max_file_size must be greater than 0"));
                }
            }

            // Validate context_lines to prevent excessive context
            if let Some(context_lines) = payload.context_lines {
                const MAX_ALLOWED_CONTEXT: usize = 20; // Increased from 10 to 20 for more flexibility
                if context_lines > MAX_ALLOWED_CONTEXT {
                    return Err(anyhow!(
                        "context_lines ({}) exceeds the maximum allowed value of {}",
                        context_lines,
                        MAX_ALLOWED_CONTEXT
                    ));
                }
                if (context_lines as i32) < 0 {
                    return Err(anyhow!("context_lines must not be negative"));
                }
            }

            // Validate glob_pattern for security
            if let Some(glob_pattern) = &payload.glob_pattern
                && (glob_pattern.contains("..") || glob_pattern.starts_with('/'))
            {
                return Err(anyhow!(
                    "glob_pattern must be a relative path and cannot contain '..' or start with '/'"
                ));
            }

            // Validate type_pattern for basic security (only allow alphanumeric, hyphens, underscores)
            if let Some(type_pattern) = &payload.type_pattern
                && !type_pattern
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            {
                return Err(anyhow!(
                    "type_pattern can only contain alphanumeric characters, hyphens, and underscores"
                ));
            }

            let input = GrepSearchInput {
                pattern: payload.pattern.clone(),
                path: payload.path.clone(),
                case_sensitive: payload.case_sensitive,
                literal: payload.literal,
                glob_pattern: payload.glob_pattern,
                context_lines: payload.context_lines,
                include_hidden: payload.include_hidden,
                max_results: payload.max_results,
                respect_ignore_files: payload.respect_ignore_files,
                max_file_size: payload.max_file_size,
                search_hidden: payload.search_hidden,
                search_binary: payload.search_binary,
                files_with_matches: payload.files_with_matches,
                type_pattern: payload.type_pattern,
                invert_match: payload.invert_match,
                word_boundaries: payload.word_boundaries,
                line_number: payload.line_number,
                column: payload.column,
                only_matching: payload.only_matching,
                trim: payload.trim,
                max_result_bytes: payload.max_result_bytes,
                timeout: payload.timeout_secs.map(Duration::from_secs),
                extra_ignore_globs: payload.extra_ignore_globs,
            };

            let result = manager
                .perform_search(input)
                .await
                .with_context(|| format!("grep_file failed for pattern '{}'", payload.pattern))?;

            // Add overflow indication if we have more results than the limit
            let matches_count = result.matches.len();
            let max_results = payload.max_results.unwrap_or(5); // Default to 5 per AGENTS.md
            let overflow_indication = if matches_count > max_results {
                format!("[+{} more matches]", matches_count - max_results)
            } else {
                String::new()
            };

            Ok(json!({
                "success": true,
                "query": result.query,
                "matches": result.matches,
                "overflow": overflow_indication,
            }))
        })
    }

    pub(super) fn list_files_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        let workspace_root = self.inventory.workspace_root().to_path_buf();
        Box::pin(async move {
            // Helper to discover top-level directories
            fn discover_directories(workspace_root: &std::path::Path) -> Vec<String> {
                let mut dirs = Vec::new();
                if let Ok(entries) = std::fs::read_dir(workspace_root) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir()
                            && let Some(name) = entry.file_name().to_str()
                        {
                            // Skip hidden directories and common non-code dirs
                            if !name.starts_with('.')
                                && name != "target"
                                && name != "node_modules"
                                && name != "dist"
                                && name != "__pycache__"
                                && name != "build"
                            {
                                dirs.push(name.to_string());
                            }
                        }
                    }
                }
                dirs.sort();
                dirs.truncate(8);
                dirs
            }

            // Check if path is root or missing
            let is_root_path = if let Some(path) = args.get("path").and_then(|p| p.as_str()) {
                let normalized = path.trim_start_matches("./").trim_start_matches('/');
                normalized.is_empty() || normalized == "." || normalized == "/"
            } else {
                true // No path = root
            };

            if is_root_path {
                let dirs = discover_directories(&workspace_root);

                // Auto-correct: use first available directory instead of blocking
                if !dirs.is_empty() {
                    let default_path = dirs.first().unwrap_or(&"src".to_string()).clone();
                    let mut corrected_args = args.clone();
                    corrected_args["path"] = serde_json::json!(default_path);
                    return tool.execute(corrected_args).await;
                } else {
                    // No suitable directories found, provide helpful error
                    return Err(anyhow!(
                        "Cannot list root directory. No standard source directories found. Available options: use run_pty_cmd with {{\"command\": \"ls -la\"}} to explore manually."
                    ));
                }
            }

            tool.execute(args).await
        })
    }

    pub(super) fn run_pty_cmd_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_run_pty_command(args).await })
    }

    pub(super) fn create_pty_session_executor(
        &mut self,
        args: Value,
    ) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_create_pty_session(args).await })
    }

    pub(super) fn list_pty_sessions_executor(
        &mut self,
        _args: Value,
    ) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_list_pty_sessions().await })
    }

    pub(super) fn close_pty_session_executor(
        &mut self,
        args: Value,
    ) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_close_pty_session(args).await })
    }

    pub(super) fn send_pty_input_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_send_pty_input(args).await })
    }

    pub(super) fn read_pty_session_executor(
        &mut self,
        args: Value,
    ) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_read_pty_session(args).await })
    }

    pub(super) fn resize_pty_session_executor(
        &mut self,
        args: Value,
    ) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_resize_pty_session(args).await })
    }

    pub(super) fn web_fetch_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        use crate::tools::web_fetch::WebFetchTool;
        // Get config from policy gateway or use defaults
        let mode = "restricted".to_string(); // Default mode
        let blocked_domains = Vec::with_capacity(10); // Pre-allocate for common blocked domains
        let blocked_patterns = Vec::with_capacity(10); // Pre-allocate for common blocked patterns
        let allowed_domains = Vec::with_capacity(10); // Pre-allocate for common allowed domains
        let strict_https_only = true;

        let tool = WebFetchTool::with_config(
            mode,
            blocked_domains,
            blocked_patterns,
            allowed_domains,
            strict_https_only,
        );
        Box::pin(async move { tool.execute(args).await })
    }

    pub(super) fn read_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.read_file(args).await })
    }

    pub(super) fn write_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.write_file(args).await })
    }

    pub(super) fn create_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.create_file(args).await })
    }

    pub(super) fn delete_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.delete_file(args).await })
    }

    pub(super) fn edit_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.edit_file(args).await })
    }

    pub(super) fn apply_patch_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_apply_patch(args).await })
    }

    pub(super) fn search_replace_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let file_ops = self.inventory.file_ops_tool().clone();
        Box::pin(async move {
            #[derive(Debug, Deserialize)]
            struct SearchReplaceInput {
                #[serde(alias = "file_path", alias = "filepath", alias = "target_path")]
                path: String,
                #[serde(alias = "query", alias = "pattern")]
                search: String,
                #[serde(alias = "replacement", alias = "new_text")]
                replace: String,
                #[serde(default)]
                max_replacements: Option<usize>,
                #[serde(default = "default_backup")]
                backup: bool,
                #[serde(default)]
                before: Option<String>,
                #[serde(default)]
                after: Option<String>,
            }

            const fn default_backup() -> bool {
                true
            }

            let input: SearchReplaceInput = serde_json::from_value(args)
                .context("search_replace requires path, search, replace")?;

            if input.search.is_empty() {
                return Err(anyhow!("search_replace requires non-empty 'search' string"));
            }

            let path = file_ops
                .normalize_user_path(&input.path)
                .await
                .with_context(|| format!("Failed to resolve path '{}'", input.path))?;

            let metadata = fs::metadata(&path)
                .await
                .with_context(|| format!("Failed to read metadata for '{}'", path.display()))?;
            if metadata.len() as usize > SEARCH_REPLACE_MAX_BYTES {
                return Err(anyhow!(
                    "File '{}' exceeds search/replace safety limit ({} bytes)",
                    path.display(),
                    SEARCH_REPLACE_MAX_BYTES
                ));
            }

            let original = fs::read_to_string(&path)
                .await
                .with_context(|| format!("Failed to read '{}'", path.display()))?;

            let mut replaced = String::with_capacity(original.len());
            let mut last_index = 0usize;
            let mut replacements = 0usize;
            let search_len = input.search.len();

            for (idx, _) in original.match_indices(&input.search) {
                if let Some(max) = input.max_replacements
                    && replacements >= max
                {
                    break;
                }

                if !matches_context(
                    &original,
                    idx,
                    search_len,
                    input.before.as_deref(),
                    input.after.as_deref(),
                ) {
                    continue;
                }

                replaced.push_str(&original[last_index..idx]);
                replaced.push_str(&input.replace);
                last_index = idx + search_len;
                replacements += 1;
            }

            replaced.push_str(&original[last_index..]);

            if replacements == 0 {
                return Ok(json!({
                    "success": true,
                    "replacements": 0,
                    "unchanged": true,
                }));
            }

            if input.backup {
                let mut backup_path = path.clone();
                let backup_ext = backup_path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| format!("{ext}.bak"))
                    .unwrap_or_else(|| "bak".to_string());
                backup_path.set_extension(backup_ext);
                fs::write(&backup_path, &original).await.with_context(|| {
                    format!("Failed to write backup '{}'", backup_path.display())
                })?;
            }

            fs::write(&path, &replaced).await.with_context(|| {
                format!("Failed to write updated content to '{}'", path.display())
            })?;

            Ok(json!({
                "success": true,
                "replacements": replacements,
                "path": path.display().to_string(),
                "backup_created": input.backup,
            }))
        })
    }

    pub(super) fn update_plan_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let manager = self.inventory.plan_manager();
        Box::pin(async move {
            let parsed: UpdatePlanArgs = serde_json::from_value(args)
                .context("update_plan requires plan items with step and status")?;
            let updated_plan = manager
                .update_plan(parsed)
                .context("failed to update plan state")?;
            let payload = PlanUpdateResult::success(updated_plan);
            serde_json::to_value(payload).context("failed to serialize plan update result")
        })
    }

    pub(super) fn search_tools_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let mcp_client = self.mcp_client.clone();
        let workspace_root = self.workspace_root_owned();
        Box::pin(async move {
            #[derive(Debug, Deserialize)]
            struct SearchArgs {
                keyword: String,
                #[serde(default)]
                detail_level: Option<String>,
            }

            let parsed: SearchArgs = serde_json::from_value(args)
                .context("search_tools requires 'keyword' and optional 'detail_level'")?;

            let detail_level = match parsed.detail_level.as_deref() {
                Some("name-only") | Some("name") => DetailLevel::NameOnly,
                Some("full") => DetailLevel::Full,
                Some("name-and-description") | Some("description") | None => {
                    DetailLevel::NameAndDescription
                }
                Some(invalid) => {
                    return Err(anyhow!(
                        "Invalid detail_level: '{}'. Must be one of: name-only, name-and-description, full",
                        invalid
                    ));
                }
            };

            // Search MCP tools
            let mut all_results = vec![];

            if let Some(mcp_client) = mcp_client {
                let discovery = ToolDiscovery::new(mcp_client);
                if let Ok(results) = discovery.search_tools(&parsed.keyword, detail_level).await {
                    all_results.extend(results);
                }
            }

            // Also search local skills (using EnhancedSkillLoader for .claude/skills/ with SKILL.md)
            use crate::skills::{EnhancedSkillLoader, SkillContext};
            let mut loader = EnhancedSkillLoader::new(workspace_root);
            if let Ok(discovery_result) = loader.discover_all_skills().await {
                let skill_contexts = discovery_result.traditional_skills;
                let query_lower = parsed.keyword.to_lowercase();
                let filtered: Vec<_> = skill_contexts
                    .into_iter()
                    .filter_map(|ctx| {
                        if let SkillContext::MetadataOnly(manifest) = ctx {
                            let name_matches = manifest.name.to_lowercase().contains(&query_lower);
                            let desc_matches =
                                manifest.description.to_lowercase().contains(&query_lower);
                            if name_matches || desc_matches {
                                Some(manifest)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect();

                // Convert skill results to tool format for consistent response
                for manifest in filtered {
                    all_results.push(crate::mcp::ToolDiscoveryResult {
                        name: manifest.name.clone(),
                        provider: "skill".to_string(),
                        description: manifest.description.clone(),
                        relevance_score: 1.0,
                        input_schema: None,
                    });
                }
            }

            if all_results.is_empty() {
                return Ok(json!({
                    "keyword": parsed.keyword,
                    "matched": 0,
                    "results": [],
                    "note": "Use 'skill' tool to load available skills directly by name"
                }));
            }

            let tools_json: Vec<Value> = all_results
                .iter()
                .map(|r| r.to_json(detail_level))
                .collect();

            // Implement AGENTS.md pattern for large result sets
            let matched = all_results.len();
            let overflow_indication = if matched > 50 {
                format!("[+{} more tools]", matched - 5)
            } else {
                String::new()
            };

            let mut response = json!({
                "keyword": parsed.keyword,
                "matched": matched,
                "detail_level": detail_level.as_str(),
                "results": tools_json,
            });

            if !overflow_indication.is_empty() {
                response["overflow"] = json!(overflow_indication);
            }

            Ok(response)
        })
    }

    pub(super) fn skill_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let workspace_root = self.workspace_root_owned();
        Box::pin(async move {
            #[derive(Debug, Deserialize)]
            struct SkillArgs {
                name: String,
            }

            let parsed: SkillArgs =
                serde_json::from_value(args).context("skill requires 'name' field")?;

            // Load skill using EnhancedSkillLoader (reads .claude/skills/*/SKILL.md)
            use crate::skills::{EnhancedSkillLoader, loader::EnhancedSkill};
            let mut loader = EnhancedSkillLoader::new(workspace_root.clone());

            // Ensure skills are discovered before trying to load them
            // This fixes the issue where skills loaded via CLI aren't available to the skill tool
            match loader.discover_all_skills().await {
                Ok(_) => {
                    // Skills discovered successfully, continue with loading
                }
                Err(e) => {
                    tracing::warn!("Failed to discover skills: {}. Proceeding anyway.", e);
                    // Continue even if discovery fails - skill might still be loadable
                }
            }

            match loader.get_skill(&parsed.name).await {
                Ok(enhanced_skill) => {
                    // Extract traditional skill from enhanced skill
                    if let EnhancedSkill::Traditional(skill) = enhanced_skill {
                        let mut resources = json!({});

                        // Include resources if available
                        for (path, resource) in skill.resources.iter() {
                            resources[path] = json!({
                                "type": format!("{:?}", resource.resource_type),
                                "path": resource.path,
                            });
                        }

                        // Format output to emphasize instructions for agent to follow
                        let mut output = format!(
                            "=== Skill Loaded: {} ===\n\n{}\n\n=== Resources Available ===\n{}\n",
                            skill.manifest.name,
                            skill.instructions,
                            if resources.as_object().map(|r| r.is_empty()).unwrap_or(true) {
                                "No additional resources".to_string()
                            } else {
                                serde_json::to_string_pretty(&resources).unwrap_or_default()
                            }
                        );

                        // Add file tracking information if the skill mentions file generation
                        use crate::skills::skill_file_tracker::SkillFileTracker;
                        let _tracker = SkillFileTracker::new(workspace_root.clone());

                        // Scan the instructions for file generation patterns
                        if skill.instructions.contains("output")
                            || skill.instructions.contains("generate")
                            || skill.instructions.contains("create")
                            || skill.instructions.contains(".pdf")
                            || skill.instructions.contains(".xlsx")
                            || skill.instructions.contains(".csv")
                        {
                            output.push_str("
=== Auto File Tracking ===
This skill generates files. After execution, file locations will be automatically detected and reported.
");
                        }

                        output.push_str("\n⚠️  IMPORTANT: Follow the instructions above to complete the task. Do NOT call this tool again.");

                        Ok(json!({
                            "success": true,
                            "name": skill.manifest.name,
                            "description": skill.manifest.description,
                            "version": skill.manifest.version,
                            "author": skill.manifest.author,
                            "instructions": skill.instructions,
                            "resources": resources,
                            "output": output,
                            "message": format!("Skill '{}' loaded. Read 'output' field for complete instructions.", skill.manifest.name),
                            "file_tracking_enabled": true  // NEW: Flag indicating auto-tracking is available
                        }))
                    } else {
                        Ok(json!({
                            "success": false,
                            "error": format!("Skill '{}' is not a traditional skill and cannot be loaded directly", parsed.name),
                            "message": "CLI tool skills must be executed through the tool system, not loaded as instructions"
                        }))
                    }
                }
                Err(e) => Ok(json!({
                    "success": false,
                    "error": format!("Failed to load skill '{}': {}", parsed.name, e),
                    "hint": "Use search_tools to discover available skills"
                })),
            }
        })
    }

    pub(super) fn execute_code_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let mcp_client = self.mcp_client.clone();
        let workspace_root = self.workspace_root_owned();
        let file_tracker = crate::tools::file_tracker::FileTracker::new(workspace_root.clone());

        Box::pin(async move {
            use crate::exec::code_executor::{CodeExecutor, Language};

            #[derive(Debug, Deserialize)]
            struct ExecuteCodeArgs {
                code: String,
                language: String,
                #[serde(default)]
                timeout_secs: Option<u64>,
                #[serde(default)]
                track_files: Option<bool>,
            }

            let parsed: ExecuteCodeArgs = serde_json::from_value(args)
                .context("execute_code requires 'code' and 'language' fields")?;

            // Record timestamp before execution for file tracking
            let execution_start = std::time::SystemTime::now();

            // SECURITY FIX: Warn if code appears to be calling tool invocation methods
            // This is a heuristic check - documents expectations that tool calls are not supported
            let code_lower = parsed.code.to_lowercase();
            if code_lower.contains("_call_tool") || code_lower.contains("\"tool_name\"") {
                tracing::warn!(
                    "Code execution contains potential tool invocation attempt. \
                    User code should use documented APIs only."
                );
            }

            // Validate language
            let language = match parsed.language.as_str() {
                "python3" | "python" => Language::Python3,
                "javascript" | "js" => Language::JavaScript,
                invalid => {
                    return Err(anyhow!(
                        "Invalid language: '{}'. Must be 'python3' or 'javascript'",
                        invalid
                    ));
                }
            };

            // Get MCP client for code execution
            let result = match mcp_client {
                Some(mcp_client) => {
                    // Build execution config
                    let mut config: crate::exec::code_executor::ExecutionConfig =
                        Default::default();
                    if let Some(timeout_secs) = parsed.timeout_secs {
                        config.timeout_secs = timeout_secs;
                    }

                    // Create and configure code executor
                    let executor = CodeExecutor::new(language, mcp_client, workspace_root.clone())
                        .with_config(config);

                    // Execute the code
                    executor
                        .execute(&parsed.code)
                        .await
                        .context("code execution failed")?
                }
                None => {
                    debug!("MCP client not configured, attempting direct code execution");

                    // Attempt direct code execution without MCP if no client available
                    let code = parsed.code.clone();

                    // Create a direct code executor using process execution
                    use std::io::Write;
                    use std::process::Command;
                    use tempfile::NamedTempFile;

                    match language {
                        Language::Python3 => {
                            let output = Command::new("python3")
                                .arg("-c")
                                .arg(&code)
                                .current_dir(&workspace_root)
                                .output()
                                .context("failed to execute Python code")?;

                            crate::exec::code_executor::ExecutionResult {
                                exit_code: output.status.code().unwrap_or(1) as i32,
                                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                                duration_ms: 0, // Not tracked in this fallback
                                json_result: None,
                            }
                        }
                        Language::JavaScript => {
                            // Create a temporary file for JavaScript execution
                            let mut temp_file = NamedTempFile::new_in(&workspace_root)
                                .context("failed to create temp file for JavaScript execution")?;
                            temp_file
                                .write_all(code.as_bytes())
                                .context("failed to write JavaScript code to temp file")?;

                            let output = Command::new("node")
                                .arg(temp_file.path())
                                .current_dir(&workspace_root)
                                .output()
                                .context("failed to execute JavaScript code")?;

                            crate::exec::code_executor::ExecutionResult {
                                exit_code: output.status.code().unwrap_or(1),
                                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                                duration_ms: 0, // Not tracked in this fallback
                                json_result: None,
                            }
                        }
                    }
                }
            };

            debug!(
                exit_code = result.exit_code,
                duration_ms = result.duration_ms,
                has_output = !result.stdout.is_empty(),
                has_error = !result.stderr.is_empty(),
                has_json_result = result.json_result.is_some(),
                "Code execution completed"
            );

            // File tracking: detect newly created files if enabled
            let mut file_tracking_info = None;
            if parsed.track_files.unwrap_or(true) {
                match file_tracker.detect_new_files(execution_start).await {
                    Ok(new_files) if !new_files.is_empty() => {
                        let summary = file_tracker.generate_file_summary(&new_files);
                        info!("File tracking detected {} new files", new_files.len());
                        file_tracking_info = Some(json!({
                            "files": new_files.iter().map(|f| f.to_json()).collect::<Vec<_>>(),
                            "summary": summary,
                            "count": new_files.len(),
                        }));
                    }
                    Ok(_) => {
                        debug!("File tracking: no new files detected");
                    }
                    Err(e) => {
                        warn!("File tracking failed: {}", e);
                    }
                }
            }

            // Implement AGENTS.md pattern for large outputs
            const MAX_OUTPUT_CHARS: usize = 4000; // Reasonable limit for context windows
            let stdout_chars = result.stdout.chars().count();
            let stderr_chars = result.stderr.chars().count();

            let (stdout_output, stdout_overflow) = if stdout_chars > MAX_OUTPUT_CHARS {
                let truncated: String = result.stdout.chars().take(MAX_OUTPUT_CHARS).collect();
                let overflow = format!("[+{} more characters]", stdout_chars - MAX_OUTPUT_CHARS);
                (truncated, Some(overflow))
            } else {
                (result.stdout, None)
            };

            let (stderr_output, stderr_overflow) = if stderr_chars > MAX_OUTPUT_CHARS {
                let truncated: String = result.stderr.chars().take(MAX_OUTPUT_CHARS).collect();
                let overflow = format!("[+{} more characters]", stderr_chars - MAX_OUTPUT_CHARS);
                (truncated, Some(overflow))
            } else {
                (result.stderr, None)
            };

            // Build response with overflow indicators
            let mut response = json!({
                "exit_code": result.exit_code,
                "duration_ms": result.duration_ms,
                "stdout": stdout_output,
                "stderr": stderr_output,
            });

            // Add overflow indicators if present
            if stdout_overflow.is_some() || stderr_overflow.is_some() {
                let mut overflow_info = serde_json::Map::new();
                if let Some(overflow) = stdout_overflow {
                    overflow_info.insert("stdout".to_string(), json!(overflow));
                }
                if let Some(overflow) = stderr_overflow {
                    overflow_info.insert("stderr".to_string(), json!(overflow));
                }
                response["overflow"] = json!(overflow_info);
            }

            // Include JSON result if present
            if let Some(json_result) = result.json_result {
                response["result"] = json_result;
            }

            // Include file tracking info if available
            if let Some(file_info) = file_tracking_info {
                response["generated_files"] = file_info;
            }

            Ok(response)
        })
    }

    #[allow(dead_code)]
    pub(super) fn save_skill_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let workspace_root = self.workspace_root_owned();
        Box::pin(async move {
            use crate::exec::{Skill, SkillManager, SkillMetadata};

            #[derive(Debug, Deserialize)]
            struct SaveSkillArgs {
                name: String,
                code: String,
                language: String,
                description: String,
                #[serde(default)]
                inputs: Option<Vec<serde_json::Value>>,
                output: String,
                #[serde(default)]
                tags: Option<Vec<String>>,
                #[serde(default)]
                examples: Option<Vec<String>>,
            }

            let parsed: SaveSkillArgs = serde_json::from_value(args)
                .context("save_skill requires name, code, language, description, and output")?;

            // Parse inputs
            let inputs = if let Some(input_values) = parsed.inputs {
                input_values
                    .iter()
                    .map(|v| {
                        let obj = v.as_object().context("input must be an object")?;
                        Ok(crate::exec::skill_manager::ParameterDoc {
                            name: obj
                                .get("name")
                                .and_then(|v| v.as_str())
                                .context("input.name required")?
                                .to_string(),
                            r#type: obj
                                .get("type")
                                .and_then(|v| v.as_str())
                                .context("input.type required")?
                                .to_string(),
                            description: obj
                                .get("description")
                                .and_then(|v| v.as_str())
                                .context("input.description required")?
                                .to_string(),
                            required: obj
                                .get("required")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false),
                        })
                    })
                    .collect::<Result<Vec<_>>>()
                    .context("failed to parse inputs")?
            } else {
                Vec::with_capacity(0) // Use with_capacity(0) instead of Vec::new()
            };

            let metadata = SkillMetadata {
                name: parsed.name.clone(),
                description: parsed.description,
                language: parsed.language,
                inputs,
                output: parsed.output,
                examples: parsed.examples.unwrap_or_default(),
                tags: parsed.tags.unwrap_or_default(),
                created_at: chrono::Utc::now().to_rfc3339(),
                modified_at: chrono::Utc::now().to_rfc3339(),
                tool_dependencies: vec![],
            };

            let skill = Skill {
                metadata,
                code: parsed.code,
            };

            let manager = SkillManager::new(&workspace_root);
            manager.save_skill(skill).await?;

            Ok(json!({
                "success": true,
                "message": format!("Skill '{}' saved successfully", parsed.name)
            }))
        })
    }

    #[allow(dead_code)]
    pub(super) fn load_skill_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let workspace_root = self.workspace_root_owned();
        Box::pin(async move {
            use crate::exec::SkillManager;

            #[derive(Debug, Deserialize)]
            struct LoadSkillArgs {
                name: String,
            }

            let parsed: LoadSkillArgs =
                serde_json::from_value(args).context("load_skill requires 'name' field")?;

            let manager = SkillManager::new(&workspace_root);
            let skill = manager.load_skill(&parsed.name).await?;

            Ok(json!({
                "name": skill.metadata.name,
                "code": skill.code,
                "language": skill.metadata.language,
                "description": skill.metadata.description,
                "inputs": skill.metadata.inputs,
                "output": skill.metadata.output,
                "examples": skill.metadata.examples,
                "tags": skill.metadata.tags,
                "created_at": skill.metadata.created_at,
                "modified_at": skill.metadata.modified_at,
            }))
        })
    }

    #[allow(dead_code)]
    pub(super) fn list_skills_executor(&mut self, _args: Value) -> BoxFuture<'_, Result<Value>> {
        let workspace_root = self.workspace_root_owned();
        Box::pin(async move {
            use crate::exec::SkillManager;

            let manager = SkillManager::new(&workspace_root);
            let skills = manager.list_skills().await?;

            Ok(json!({
                "skills": skills,
                "count": skills.len(),
            }))
        })
    }

    #[allow(dead_code)]
    pub(super) fn search_skills_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let workspace_root = self.workspace_root_owned();
        Box::pin(async move {
            use crate::exec::SkillManager;

            #[derive(Debug, Deserialize)]
            struct SearchSkillsArgs {
                query: String,
            }

            let parsed: SearchSkillsArgs =
                serde_json::from_value(args).context("search_skills requires 'query' field")?;

            let manager = SkillManager::new(&workspace_root);
            let results = manager.search_skills(&parsed.query).await?;

            Ok(json!({
                "query": parsed.query,
                "results": results,
                "count": results.len(),
            }))
        })
    }

    pub(super) async fn execute_apply_patch(&self, args: Value) -> Result<Value> {
        let patch_source = args
            .get("input")
            .or_else(|| args.get("patch"))
            .or_else(|| args.get("diff"));

        let input = patch_source.and_then(|v| v.as_str()).ok_or_else(|| {
            anyhow!(
                "Error: Invalid 'apply_patch' arguments. Expected JSON object with: input (required, string with patch content). Aliases for input: 'patch', 'diff'. Example: {{\"input\": \"--- a/file.txt\\n+++ b/file.txt\\n@@ ... \"}}"
            )
        })?;
        let patch = Patch::parse(input)?;
        let delete_ops = patch
            .operations()
            .iter()
            .filter(|op| matches!(op, crate::tools::editing::PatchOperation::DeleteFile { .. }))
            .count();
        let add_ops = patch
            .operations()
            .iter()
            .filter(|op| matches!(op, crate::tools::editing::PatchOperation::AddFile { .. }))
            .count();

        if delete_ops > 0 && add_ops > 0 {
            tracing::warn!(
                delete_ops,
                add_ops,
                "apply_patch will delete and recreate files; ensure backups or incremental edits"
            );

            // Emit telemetry event for destructive operation detection
            // This addresses the Codex issue review recommendation to track
            // cascading delete/recreate sequences
            //
            // Reference: docs/research/codex_issue_review.md - apply_patch Tool Reliability
            let affected_files: Vec<String> = patch
                .operations()
                .iter()
                .filter_map(|op| match op {
                    crate::tools::editing::PatchOperation::DeleteFile { path } => {
                        Some(path.clone())
                    }
                    crate::tools::editing::PatchOperation::AddFile { path, .. } => {
                        Some(path.clone())
                    }
                    _ => None,
                })
                .collect();

            // Check if we're in a git repository (simple heuristic for backup detection)
            let has_git_backup = self.workspace_root().join(".git").exists();

            let event = crate::tools::registry::ToolTelemetryEvent::delete_and_recreate_warning(
                "apply_patch",
                affected_files.clone(),
                has_git_backup,
            );

            // Log the telemetry event (structured logging for observability)
            debug!(
                event = ?event,
                "Emitting destructive operation telemetry"
            );

            // Check if confirmation is needed (destructive operations without backup)
            let skip_confirmations = env::var("VTCODE_SKIP_CONFIRMATIONS")
                .ok()
                .and_then(|v| v.parse::<bool>().ok())
                .unwrap_or(false);

            // Always prompt for confirmation if no git backup and not skipping confirmations
            let requires_confirmation = !skip_confirmations && !has_git_backup;

            if requires_confirmation {
                let file_list = affected_files
                    .iter()
                    .take(10) // Show first 10 files; truncate if more
                    .map(|f| format!("  - {}", f))
                    .collect::<Vec<_>>()
                    .join("\n");

                let file_count_suffix = if affected_files.len() > 10 {
                    format!("\n  ... and {} more file(s)", affected_files.len() - 10)
                } else {
                    String::new()
                };

                let backup_warning = if has_git_backup {
                    "\nGit backup detected - can be recovered if needed."
                } else {
                    "\nNo git backup detected - deletion is permanent!"
                };

                let prompt_msg = format!(
                    "apply_patch will delete and recreate {} file(s):{}{}{}\n\nContinue?",
                    affected_files.len(),
                    file_list,
                    file_count_suffix,
                    backup_warning
                );

                // Check if running in TUI mode
                let in_tui_mode = env::var("VTCODE_TUI_MODE").is_ok();

                if in_tui_mode {
                    // TUI mode: Return error for runloop to handle with modal confirmation
                    return Err(anyhow!("CONFIRMATION_REQUIRED: {}", prompt_msg));
                } else {
                    // CLI mode: Use dialoguer for confirmation prompt
                    let confirmed = dialoguer::Confirm::new()
                        .with_prompt(prompt_msg)
                        .default(false)
                        .interact()
                        .context("Failed to get user confirmation")?;

                    if !confirmed {
                        return Ok(json!({
                            "success": false,
                            "error": "Operation cancelled by user",
                            "affected_files": affected_files,
                            "cancelled_at": std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .ok()
                                .map(|d| d.as_secs())
                        }));
                    }
                }
            }
        }

        // Generate enhanced diff preview with proper git-style diffs
        let mut diff_preview = String::new();

        for op in patch.operations() {
            match op {
                PatchOperation::AddFile { path, content } => {
                    // For new files, create a proper unified diff format
                    let structured_diff = compute_diff(
                        "",
                        content,
                        DiffOptions {
                            context_lines: 3,
                            old_label: Some("/dev/null"),
                            new_label: Some(path),
                            missing_newline_hint: true,
                        },
                    );

                    diff_preview.push_str(&structured_diff.formatted);
                    if !structured_diff.formatted.is_empty() {
                        diff_preview.push('\n');
                    }
                }
                PatchOperation::DeleteFile { path } => {
                    // For deleted files, try to read the current content to show what will be deleted
                    let full_path = self.workspace_root().join(path);
                    let current_content = if full_path.exists() {
                        std::fs::read_to_string(&full_path).unwrap_or_default()
                    } else {
                        String::new()
                    };

                    // Create a structured diff for the renderer
                    let structured_diff = compute_diff(
                        &current_content,
                        "",
                        DiffOptions {
                            context_lines: 3,
                            old_label: Some(path),
                            new_label: Some(path),
                            missing_newline_hint: true,
                        },
                    );

                    diff_preview.push_str(&structured_diff.formatted);
                    if !structured_diff.formatted.is_empty() {
                        diff_preview.push('\n');
                    }
                }
                PatchOperation::UpdateFile { path, chunks, .. } => {
                    // For updated files, read the current content and properly apply the patch
                    let full_path = self.workspace_root().join(path);
                    let old_content = if full_path.exists() {
                        fs::read_to_string(&full_path).await.unwrap_or_default()
                    } else {
                        String::new() // If file doesn't exist, treat as empty for an add operation
                    };

                    // Reconstruct the new content by applying the patch changes
                    let old_lines: Vec<&str> = old_content.lines().collect();
                    let mut new_lines = Vec::with_capacity(old_lines.len() + chunks.len() * 10); // Pre-allocate for typical patch size
                    let mut current_old_line_idx = 0;

                    for chunk in chunks {
                        // Add context lines from the original file before this chunk
                        if let Some(ctx) = &chunk.change_context {
                            // Extract the line numbers from the context string (e.g., "@@ -1,5 +1,6 @@")
                            if ctx.starts_with("@@") {
                                // Parse context to find at which line to apply the changes
                                // Format is typically: @@ -old_start,old_count +new_start,new_count @@
                                let parts: Vec<&str> = ctx.split_whitespace().collect();
                                if parts.len() >= 3
                                    && let Some(old_part) = parts.get(1)
                                    && let Some(range_str) = old_part.strip_prefix('-')
                                {
                                    let range_parts: Vec<&str> = range_str.split(',').collect();
                                    if let (Some(start_str), Some(_count_str)) =
                                        (range_parts.first(), range_parts.get(1))
                                        && let Ok(start_line) = start_str.parse::<usize>()
                                    {
                                        let start_idx = start_line.saturating_sub(1); // Convert to 0-indexed

                                        // Add lines from old content up to this chunk position
                                        while current_old_line_idx < start_idx
                                            && current_old_line_idx < old_lines.len()
                                        {
                                            new_lines
                                                .push(old_lines[current_old_line_idx].to_string());
                                            current_old_line_idx += 1;
                                        }
                                    }
                                }
                            }
                        }

                        // Process lines in the chunk
                        for line in &chunk.lines {
                            match line {
                                PatchLine::Addition(text) => {
                                    new_lines.push(text.clone());
                                }
                                PatchLine::Context(text) => {
                                    new_lines.push(text.clone());
                                    current_old_line_idx += 1;
                                }
                                PatchLine::Removal(_) => {
                                    current_old_line_idx += 1; // Skip this line from old content
                                }
                            }
                        }
                    }

                    // Add any remaining lines from the original file
                    while current_old_line_idx < old_lines.len() {
                        new_lines.push(old_lines[current_old_line_idx].to_string());
                        current_old_line_idx += 1;
                    }

                    let new_content = new_lines.join("\n");

                    // Create a structured diff using the same approach as generate_unified_diff
                    let structured_diff = compute_diff(
                        &old_content,
                        &new_content,
                        DiffOptions {
                            context_lines: 3,
                            old_label: Some(path),
                            new_label: Some(path),
                            missing_newline_hint: true,
                        },
                    );

                    diff_preview.push_str(&structured_diff.formatted);
                    if !structured_diff.formatted.is_empty() {
                        diff_preview.push('\n');
                    }
                }
            }
        }

        let results = match patch.apply(self.workspace_root()).await {
            Ok(results) => results,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "apply_patch failed; consider falling back to incremental edits"
                );
                return Err(err);
            }
        };
        Ok(json!({
            "success": true,
            "applied": results,
            "diff_preview": {
                "content": diff_preview,
                "truncated": false
            },
        }))
    }

    async fn execute_run_pty_command(&mut self, args: Value) -> Result<Value> {
        let payload = value_as_object(&args, "run_pty_cmd expects an object payload")?;
        let setup = self.prepare_ephemeral_pty_command(payload).await?;

        // Guard: ensure command is not empty - this should not happen if parse_command_parts worked correctly
        if setup.command.is_empty() {
            let debug_info = format!(
                "Available keys in payload: {:?}",
                payload.keys().collect::<Vec<_>>()
            );
            return Err(anyhow!(
                "Internal error: prepared PTY command is empty after parsing. {}. \
                 Please ensure 'command' parameter is a non-empty string or array in run_pty_cmd call.",
                debug_info
            ));
        }

        self.run_ephemeral_pty_command(setup).await
    }

    async fn prepare_ephemeral_pty_command(
        &self,
        payload: &Map<String, Value>,
    ) -> Result<PtyCommandSetup> {
        let mut command = parse_command_parts(
            payload,
            "run_pty_cmd requires a 'command' value",
            "PTY command cannot be empty",
        )?;

        let raw_command = payload
            .get("raw_command")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        let shell_program = resolve_shell_preference(
            payload.get("shell").and_then(|value| value.as_str()),
            self.pty_config(),
        );
        let login_shell = payload
            .get("login")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        let confirm = payload
            .get("confirm")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        {
            let normalized_shell = normalized_shell_name(&shell_program);
            let existing_shell = command
                .first()
                .map(|existing| normalized_shell_name(existing));
            if existing_shell != Some(normalized_shell.clone()) {
                let command_string =
                    build_shell_command_string(raw_command.as_deref(), &command, &shell_program);

                let mut shell_invocation = Vec::with_capacity(4);
                shell_invocation.push(shell_program.clone());

                if login_shell && !should_use_windows_command_tokenizer(Some(&shell_program)) {
                    shell_invocation.push("-l".to_string());
                }

                let command_flag = if should_use_windows_command_tokenizer(Some(&shell_program)) {
                    match normalized_shell.as_str() {
                        "cmd" | "cmd.exe" => "/C".to_string(),
                        "powershell" | "powershell.exe" | "pwsh" => "-Command".to_string(),
                        _ => "-c".to_string(),
                    }
                } else {
                    "-c".to_string()
                };

                shell_invocation.push(command_flag);
                shell_invocation.push(command_string);
                command = shell_invocation;
            }
        }

        let timeout_secs = parse_timeout_secs(
            payload.get("timeout_secs"),
            self.pty_config().command_timeout_seconds,
        )?;
        let rows =
            parse_pty_dimension("rows", payload.get("rows"), self.pty_config().default_rows)?;
        let cols =
            parse_pty_dimension("cols", payload.get("cols"), self.pty_config().default_cols)?;

        let working_dir_path = self
            .pty_manager()
            .resolve_working_dir(payload.get("working_dir").and_then(|value| value.as_str()))
            .await?;
        let working_dir_display = self.pty_manager().describe_working_dir(&working_dir_path);

        // Parse max_tokens for output truncation (defaults to DEFAULT_PTY_OUTPUT_MAX_TOKENS)
        let max_tokens = payload
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_PTY_OUTPUT_MAX_TOKENS);

        let display_command = if should_use_windows_command_tokenizer(Some(&shell_program)) {
            join_windows_command(&command)
        } else {
            join(command.iter().map(|part| part.as_str()))
        };

        Ok(PtyCommandSetup {
            command,
            display_command,
            working_dir_path,
            working_dir_display,
            session_id: generate_session_id("run"),
            rows,
            cols,
            timeout_secs,
            max_tokens,
            confirm,
        })
    }

    async fn run_ephemeral_pty_command(&mut self, setup: PtyCommandSetup) -> Result<Value> {
        // Guard: ensure command is not empty before attempting execution
        if setup.command.is_empty() {
            return Err(anyhow!("PTY command cannot be empty"));
        }

        enforce_pty_command_policy(&setup.display_command, setup.confirm)?;

        // Execute the PTY command exactly once.
        // We do NOT retry on exit code (application error) because:
        // 1. It causes "multi retry" behavior where a single failed command runs 3 times.
        // 2. Retrying permanent errors (like 127 Command Not Found) is futile.
        // 3. The agent should decide whether to retry based on the error message.

        // We pass 0 as retry_count since we are not retrying.
        let result = self.execute_single_pty_attempt(&setup, 0).await?;

        let mut capture = result.1;
        let snapshot = result.2;
        let mut truncated = false;

        // Apply smart truncation to prevent context overflow
        // This is critical for commands like `cargo clippy` that can produce 8000+ lines
        if setup.max_tokens > 0 && !capture.output.is_empty() {
            let original_len = capture.output.len();
            let original_lines = capture.output.lines().count();

            // Check if this is build tool output that benefits from error extraction
            let is_build_output = setup.command.iter().any(|arg| {
                let lower = arg.to_lowercase();
                BUILD_OUTPUT_COMMANDS.iter().any(|cmd| lower.contains(cmd))
            });

            if is_build_output {
                // Smart extraction: prioritize errors/warnings for build output
                capture.output =
                    extract_build_errors_and_summary(&capture.output, setup.max_tokens);
                truncated = true;
            } else {
                // Generic head+tail truncation for other commands
                use crate::core::agent::runloop::token_trunc::truncate_content_by_tokens;
                use crate::core::token_budget::TokenBudgetManager;

                let token_budget = TokenBudgetManager::default();
                let (truncated_output, was_truncated) =
                    truncate_content_by_tokens(&capture.output, setup.max_tokens, &token_budget)
                        .await;

                if was_truncated {
                    capture.output = truncated_output;
                    truncated = true;
                }
            }

            // Apply byte fuse as secondary safeguard
            if capture.output.len() > DEFAULT_PTY_OUTPUT_BYTE_FUSE {
                use crate::core::agent::runloop::token_trunc::safe_truncate_to_bytes_with_marker;
                capture.output = safe_truncate_to_bytes_with_marker(
                    &capture.output,
                    DEFAULT_PTY_OUTPUT_BYTE_FUSE,
                );
                truncated = true;
            }

            // Add truncation notice if output was reduced
            let final_lines = capture.output.lines().count();
            if original_lines > final_lines || original_len > capture.output.len() {
                capture.output = format!(
                    "{}\n\n[Output truncated: {} lines / {} bytes → {} lines / {} bytes]",
                    capture.output,
                    original_lines,
                    original_len,
                    final_lines,
                    capture.output.len()
                );
            }
        }

        let response = build_ephemeral_pty_response(
            &setup,
            capture,
            snapshot,
            truncated,
            pty_capabilities_from_config(self.pty_config()),
        );
        Ok(response)
    }

    async fn execute_single_pty_attempt(
        &mut self,
        setup: &PtyCommandSetup,
        retry_count: u32,
    ) -> Result<(Option<i32>, PtyEphemeralCapture, VTCodePtySession)> {
        let mut lifecycle = PtySessionLifecycle::start(self)?;

        self.pty_manager()
            .create_session(
                setup.session_id.clone(),
                setup.command.clone(),
                setup.working_dir_path.clone(),
                setup.size(),
            )
            .with_context(|| {
                format!(
                    "failed to create PTY session '{}' for command {:?} (attempt {})",
                    setup.session_id,
                    setup.command,
                    retry_count + 1
                )
            })?;
        lifecycle.commit();

        // Use adaptive timeout: longer for known long-running commands
        let poll_timeout = if is_long_running_command(&setup.command) {
            Duration::from_secs(RUN_PTY_POLL_TIMEOUT_LONG_RUNNING)
        } else {
            Duration::from_secs(RUN_PTY_POLL_TIMEOUT_SECS)
        };

        // Wait for full command completion, not just initial output
        let capture = self
            .wait_for_pty_completion(&setup.session_id, poll_timeout)
            .await;

        let snapshot = self
            .pty_manager()
            .snapshot_session(&setup.session_id)
            .with_context(|| format!("failed to snapshot PTY session '{}'", setup.session_id))?;

        Ok((capture.exit_code, capture, snapshot))
    }

    async fn wait_for_pty_completion(
        &self,
        session_id: &str,
        poll_timeout: Duration,
    ) -> PtyEphemeralCapture {
        let mut output = String::new();
        let start = Instant::now();
        let poll_interval = Duration::from_millis(50);
        let min_wait = Duration::from_millis(200);

        loop {
            // Read any available output
            if let Ok(Some(new_output)) = self.pty_manager().read_session_output(session_id, true)
                && !new_output.is_empty()
            {
                output.push_str(&new_output);
            }

            // Check if session has completed
            if let Ok(Some(code)) = self.pty_manager().is_session_completed(session_id) {
                // Drain any remaining output
                if let Ok(Some(final_output)) =
                    self.pty_manager().read_session_output(session_id, true)
                {
                    output.push_str(&final_output);
                }

                return PtyEphemeralCapture {
                    output,
                    exit_code: Some(code),
                    completed: true,
                    duration: start.elapsed(),
                };
            }

            let elapsed = start.elapsed();

            // Minimum wait before returning
            if !output.is_empty() && elapsed > min_wait {
                // For fast commands (< 2s), wait for completion
                if elapsed < Duration::from_secs(2) {
                    sleep(poll_interval).await;
                    continue;
                }
            }

            // For agent loop: if we're dealing with long-running commands, wait much longer
            // The original poll_timeout was too aggressive for build processes
            let effective_timeout =
                if poll_timeout >= Duration::from_secs(RUN_PTY_POLL_TIMEOUT_LONG_RUNNING) {
                    // For long-running commands (like cargo, npm), use extended timeout
                    Duration::from_secs(600) // 10 minutes for long-running commands
                } else {
                    // For regular commands, use original timeout behavior
                    Duration::from_secs(60) // 1 minute for regular commands
                };

            // Check if we've exceeded the effective timeout
            if elapsed > effective_timeout {
                debug!(
                    "PTY command exceeded timeout of {:?} (original: {:?}), returning partial output",
                    effective_timeout, poll_timeout
                );
                return PtyEphemeralCapture {
                    output,
                    exit_code: None, // Indicate still running
                    completed: false,
                    duration: elapsed,
                };
            }

            sleep(poll_interval).await;
        }
    }

    async fn execute_create_pty_session(&mut self, args: Value) -> Result<Value> {
        let payload = value_as_object(&args, "create_pty_session expects an object payload")?;
        let session_id =
            parse_session_id(payload, "create_pty_session requires a 'session_id' string")?;

        let mut command_parts = parse_command_parts(
            payload,
            "create_pty_session requires a 'command' value",
            "PTY session command cannot be empty",
        )?;

        let login_shell = payload
            .get("login")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        let shell_program = resolve_shell_preference(
            payload.get("shell").and_then(|value| value.as_str()),
            self.pty_config(),
        );
        let should_replace = payload.get("shell").is_some()
            || (command_parts.len() == 1 && is_default_shell_placeholder(&command_parts[0]));
        if should_replace {
            command_parts = vec![shell_program];
        }

        if login_shell
            && !command_parts.is_empty()
            && !should_use_windows_command_tokenizer(Some(&command_parts[0]))
            && !command_parts.iter().skip(1).any(|arg| arg == "-l")
        {
            command_parts.push("-l".to_string());
        }

        let working_dir = self
            .pty_manager()
            .resolve_working_dir(payload.get("working_dir").and_then(|value| value.as_str()))
            .await?;

        let rows =
            parse_pty_dimension("rows", payload.get("rows"), self.pty_config().default_rows)?;
        let cols =
            parse_pty_dimension("cols", payload.get("cols"), self.pty_config().default_cols)?;

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        debug!(
            target: "vtcode::pty",
            session_id = %session_id,
            command = ?command_parts,
            working_dir = %working_dir.display(),
            rows,
            cols,
            "creating PTY session"
        );

        // Start PTY session and store guard to keep session alive for the duration
        let _guard = self.start_pty_session()?;
        let result = match self.pty_manager().create_session(
            session_id.clone(),
            command_parts.clone(),
            working_dir.clone(),
            size,
        ) {
            Ok(meta) => meta,
            Err(error) => {
                // Guard will be dropped here, automatically decrementing session count
                // Attempt to cleanup the failed session if it was created
                let _ = self.pty_manager().close_session(&session_id);
                return Err(error).with_context(|| {
                    format!(
                        "Failed to create PTY session '{}' with command {:?} in {}",
                        session_id,
                        command_parts,
                        working_dir.display()
                    )
                });
            }
        };

        // Check if the session is still running (should be, since we just created it)
        let is_completed = match self.pty_manager().is_session_completed(&session_id) {
            Ok(Some(exit_code)) => {
                // Process has exited immediately - likely command not found or permission denied
                // This is often caused by:
                // - Command not found in PATH
                // - Permission denied (executable not marked as +x)
                // - Shell not found
                // Try to capture any output to help diagnose
                let output = self
                    .pty_manager()
                    .read_session_output(&session_id, false)
                    .unwrap_or(None)
                    .unwrap_or_default();

                if exit_code != 0 {
                    debug!(
                        target: "vtcode::pty",
                        session_id = %session_id,
                        exit_code = exit_code,
                        output = %output,
                        "PTY session exited immediately after creation"
                    );
                }

                Some(exit_code)
            }
            Ok(None) => {
                // Process is still running
                None
            }
            Err(_) => {
                // Error checking status, assume completed
                Some(-1) // Use -1 to indicate error state
            }
        };

        let mut response = snapshot_to_map(result, PtySnapshotViewOptions::default());
        response.insert("success".to_string(), Value::Bool(true));

        // Add status information
        match is_completed {
            Some(exit_code) => {
                response.insert("is_exited".to_string(), Value::Bool(true));
                response.insert("exit_code".to_string(), Value::Number(exit_code.into()));
            }
            None => {
                response.insert("is_exited".to_string(), Value::Bool(false)); // Still running
                response.insert("exit_code".to_string(), Value::Null); // No exit code yet
            }
        }

        let mut value = Value::Object(response);
        let stdout = value
            .get("scrollback")
            .or_else(|| value.get("screen_contents"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        add_unified_metadata(
            &mut value,
            stdout,
            is_completed,
            Some(is_completed.is_some()),
            None,
            Some(false),
            pty_capabilities_from_config(self.pty_config()),
        );

        Ok(value)
    }

    async fn execute_list_pty_sessions(&self) -> Result<Value> {
        let sessions = self.pty_manager().list_sessions();
        let mut identifiers = Vec::with_capacity(sessions.len());
        let mut details = Vec::with_capacity(sessions.len());
        for session in sessions {
            identifiers.push(session.id.clone());
            details.push(Value::Object(snapshot_to_map(
                session,
                PtySnapshotViewOptions::default(),
            )));
        }

        let mut value = json!({
            "success": true,
            "sessions": identifiers,
            "details": details,
        });
        add_unified_metadata(
            &mut value,
            None,
            None,
            None,
            None,
            Some(false),
            pty_capabilities_from_config(self.pty_config()),
        );
        Ok(value)
    }

    async fn execute_close_pty_session(&mut self, args: Value) -> Result<Value> {
        let payload = value_as_object(&args, "close_pty_session expects an object payload")?;
        let session_id =
            parse_session_id(payload, "close_pty_session requires a 'session_id' string")?;

        // Check if the session is still running BEFORE closing
        // (after close, the session will no longer be accessible)
        let exit_code = match self.pty_manager().is_session_completed(session_id.as_str()) {
            Ok(Some(code)) => Some(code),
            Ok(None) => None,   // Process was still running when we checked
            Err(_) => Some(-1), // Error state
        };

        let metadata = self
            .pty_manager()
            .close_session(session_id.as_str())
            .with_context(|| format!("failed to close PTY session '{session_id}'"))?;
        self.end_pty_session();

        let mut response = snapshot_to_map(metadata, PtySnapshotViewOptions::default());
        response.insert("success".to_string(), Value::Bool(true));

        // Report the exit code we captured before closing
        match exit_code {
            Some(code) => {
                response.insert("is_exited".to_string(), Value::Bool(true));
                response.insert("exit_code".to_string(), Value::Number(code.into()));
            }
            None => {
                response.insert("is_exited".to_string(), Value::Bool(false));
                response.insert("exit_code".to_string(), Value::Null);
            }
        }

        let mut value = Value::Object(response);
        let stdout = value
            .get("scrollback")
            .or_else(|| value.get("screen_contents"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        add_unified_metadata(
            &mut value,
            stdout,
            exit_code,
            Some(exit_code.is_some()),
            None,
            Some(false),
            pty_capabilities_from_config(self.pty_config()),
        );

        Ok(value)
    }

    async fn execute_send_pty_input(&mut self, args: Value) -> Result<Value> {
        let payload = value_as_object(&args, "send_pty_input expects an object payload")?;
        let input = PtyInputPayload::from_map(payload)?;

        // Check if session exists and is still running before attempting to send input
        let session_exists = self
            .pty_manager()
            .snapshot_session(input.session_id.as_str())
            .is_ok();
        if !session_exists {
            return Err(anyhow!(
                "PTY session '{}' does not exist. Create it first with create_pty_session.",
                input.session_id
            ));
        }

        // Check if session has already exited
        let is_completed = self
            .pty_manager()
            .is_session_completed(input.session_id.as_str())
            .unwrap_or(Some(-1));

        if let Some(exit_code) = is_completed {
            return Err(anyhow!(
                "PTY session '{}' has already exited with code {}. Cannot send input to completed session.",
                input.session_id,
                exit_code
            ));
        }

        let written = self
            .pty_manager()
            .send_input_to_session(
                input.session_id.as_str(),
                &input.buffer,
                input.append_newline,
            )
            .with_context(|| format!(
                "failed to write to PTY session '{}'. Session may have been closed or may not be writable.",
                input.session_id
            ))?;

        if input.wait_ms > 0 {
            sleep(Duration::from_millis(input.wait_ms)).await;
        }

        let output = self
            .pty_manager()
            .read_session_output(input.session_id.as_str(), input.drain_output)
            .with_context(|| format!("failed to read PTY session '{}' output", input.session_id))?;
        let snapshot = self
            .pty_manager()
            .snapshot_session(input.session_id.as_str())
            .with_context(|| format!("failed to snapshot PTY session '{}'", input.session_id))?;

        // Check if the session is still running
        let is_completed = match self
            .pty_manager()
            .is_session_completed(input.session_id.as_str())
        {
            Ok(Some(exit_code)) => {
                // Process has exited with code
                Some(exit_code)
            }
            Ok(None) => {
                // Process is still running
                None
            }
            Err(_) => {
                // Error checking status, assume completed
                Some(-1) // Use -1 to indicate error state
            }
        };

        let mut response = snapshot_to_map(snapshot, PtySnapshotViewOptions::default());
        response.insert("success".to_string(), Value::Bool(true));
        response.insert("written_bytes".to_string(), Value::from(written));
        response.insert(
            "appended_newline".to_string(),
            Value::Bool(input.append_newline),
        );
        if let Some(output) = output {
            response.insert("output".to_string(), Value::String(strip_ansi(&output)));
        }

        // Add the input that was sent as stdin (if it's valid UTF-8)
        if !input.buffer.is_empty()
            && let Ok(input_str) = std::str::from_utf8(&input.buffer)
        {
            response.insert("stdin".to_string(), Value::String(input_str.to_string()));
        }

        // Add status information
        match is_completed {
            Some(exit_code) => {
                response.insert("is_exited".to_string(), Value::Bool(true));
                response.insert("exit_code".to_string(), Value::Number(exit_code.into()));
            }
            None => {
                response.insert("is_exited".to_string(), Value::Bool(false)); // Still running
                response.insert("exit_code".to_string(), Value::Null); // No exit code yet
            }
        }

        let mut value = Value::Object(response);
        let stdout = value
            .get("output")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        add_unified_metadata(
            &mut value,
            stdout,
            is_completed,
            Some(is_completed.is_some()),
            None,
            Some(false),
            pty_capabilities_from_config(self.pty_config()),
        );

        Ok(value)
    }

    async fn execute_read_pty_session(&mut self, args: Value) -> Result<Value> {
        let payload = value_as_object(&args, "read_pty_session expects an object payload")?;
        let view_args = PtySessionViewArgs::from_map(payload)?;

        let output = self
            .pty_manager()
            .read_session_output(view_args.session_id.as_str(), view_args.drain_output)
            .with_context(|| {
                format!(
                    "failed to read PTY session '{}' output",
                    view_args.session_id
                )
            })?;
        let snapshot = self
            .pty_manager()
            .snapshot_session(view_args.session_id.as_str())
            .with_context(|| {
                format!("failed to snapshot PTY session '{}'", view_args.session_id)
            })?;

        // Check if the session is still running
        let is_completed = match self
            .pty_manager()
            .is_session_completed(view_args.session_id.as_str())
        {
            Ok(Some(exit_code)) => {
                // Process has exited with code
                Some(exit_code)
            }
            Ok(None) => {
                // Process is still running
                None
            }
            Err(_) => {
                // Error checking status, assume completed
                Some(-1) // Use -1 to indicate error state
            }
        };

        let mut response = snapshot_to_map(snapshot, view_args.view);
        response.insert("success".to_string(), Value::Bool(true));

        // Apply max_tokens truncation if specified
        let processed_output = if let Some(output) = output {
            if let Some(max_tokens) = view_args.max_tokens {
                if max_tokens > 0 {
                    use crate::core::agent::runloop::token_trunc::truncate_content_by_tokens;
                    use crate::core::token_budget::TokenBudgetManager;

                    // Use a temporary token budget manager for truncation
                    let token_budget = TokenBudgetManager::default();

                    // Since we're already in an async context, we can await directly
                    let (truncated_output, _) =
                        truncate_content_by_tokens(&output, max_tokens, &token_budget).await;

                    format!(
                        "{}\n[... truncated by max_tokens: {} ...]",
                        truncated_output, max_tokens
                    )
                } else {
                    output // Keep original if max_tokens is not valid
                }
            } else {
                output // Keep original if no max_tokens specified
            }
        } else {
            String::new() // No output to process
        };

        if !processed_output.is_empty() {
            response.insert(
                "output".to_string(),
                Value::String(strip_ansi(&processed_output)),
            );
        }

        // Add status information
        match is_completed {
            Some(exit_code) => {
                response.insert("is_exited".to_string(), Value::Bool(true));
                response.insert("exit_code".to_string(), Value::Number(exit_code.into()));
            }
            None => {
                response.insert("is_exited".to_string(), Value::Bool(false)); // Still running
                response.insert("exit_code".to_string(), Value::Null); // No exit code yet
            }
        }

        let mut value = Value::Object(response);
        let stdout = value
            .get("output")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        add_unified_metadata(
            &mut value,
            stdout,
            is_completed,
            Some(is_completed.is_some()),
            None,
            Some(false),
            pty_capabilities_from_config(self.pty_config()),
        );

        Ok(value)
    }

    async fn execute_resize_pty_session(&mut self, args: Value) -> Result<Value> {
        let payload = value_as_object(&args, "resize_pty_session expects an object payload")?;
        let session_id =
            parse_session_id(payload, "resize_pty_session requires a 'session_id' string")?;

        let current = self
            .pty_manager()
            .snapshot_session(session_id.as_str())
            .with_context(|| format!("failed to snapshot PTY session '{session_id}'"))?;

        let rows = parse_pty_dimension("rows", payload.get("rows"), current.rows)?;
        let cols = parse_pty_dimension("cols", payload.get("cols"), current.cols)?;

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let snapshot = self
            .pty_manager()
            .resize_session(session_id.as_str(), size)
            .with_context(|| format!("failed to resize PTY session '{session_id}'"))?;

        let mut response = snapshot_to_map(snapshot, PtySnapshotViewOptions::default());
        response.insert("success".to_string(), Value::Bool(true));

        Ok(Value::Object(response))
    }
}

fn parse_timeout_secs(value: Option<&Value>, fallback: u64) -> Result<u64> {
    let parsed = value
        .map(|raw| {
            raw.as_u64()
                .ok_or_else(|| anyhow!("timeout_secs must be a positive integer"))
        })
        .transpose()?;
    validated_timeout_secs(parsed, fallback)
}

fn validated_timeout_secs(raw: Option<u64>, fallback: u64) -> Result<u64> {
    let timeout_secs = raw.unwrap_or(fallback);
    if timeout_secs == 0 {
        return Err(anyhow!("timeout_secs must be greater than zero"));
    }
    Ok(timeout_secs)
}

fn value_as_object<'a>(value: &'a Value, context: &str) -> Result<&'a Map<String, Value>> {
    value.as_object().ok_or_else(|| anyhow!("{}", context))
}

fn parse_command_parts(
    payload: &Map<String, Value>,
    missing_error: &str,
    empty_error: &str,
) -> Result<Vec<String>> {
    let mut parts = match payload.get("command") {
        Some(Value::String(command)) => {
            // Use the same tokenization logic as terminal commands to handle "cargo fmt" correctly
            tokenize_command_string(command, None)?
        }
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(|part| part.to_string())
                    .ok_or_else(|| anyhow!("command array must contain only strings"))
            })
            .collect::<Result<Vec<_>>>()?,
        Some(_) => {
            return Err(anyhow!("command must be a string or string array"));
        }
        None => Vec::with_capacity(0), // Use with_capacity(0) instead of Vec::new()
    };

    // If we didn't get a command array or string, try to pick up dotted command.N args
    if parts.is_empty() {
        let mut entries: Vec<(usize, String)> = Vec::with_capacity(50); // Pre-allocate for typical patch size
        for (k, v) in payload.iter() {
            if let Some(idx_str) = k.strip_prefix("command.")
                && let Ok(idx) = idx_str.parse::<usize>()
            {
                let Some(seg) = v.as_str() else {
                    return Err(anyhow!("command array must contain only strings"));
                };
                entries.push((idx, seg.to_string()));
            }
        }
        if !entries.is_empty() {
            entries.sort_unstable_by_key(|(idx, _)| *idx);
            let min_index = entries.first().unwrap().0;
            let max_index = entries.last().unwrap().0;

            // Validate that command starts at index 0 or 1 (not after gaps)
            if min_index > 1 {
                return Err(anyhow!(
                    "command array from dotted notation must start at command.0 or command.1, got command.{}",
                    min_index
                ));
            }

            let mut computed_parts = vec![String::new(); max_index + 1 - min_index];
            for (idx, seg) in entries.into_iter() {
                let position = if min_index == 1 { idx - 1 } else { idx };
                if position >= computed_parts.len() {
                    computed_parts.resize(position + 1, String::new());
                }
                computed_parts[position] = seg;
            }
            if computed_parts.is_empty() {
                return Err(anyhow!("{}", missing_error));
            }
            if computed_parts[0].trim().is_empty() {
                return Err(anyhow!("{}", empty_error));
            }
            parts = computed_parts;
        }
    }

    if let Some(args_value) = payload.get("args") {
        let args_array = args_value
            .as_array()
            .ok_or_else(|| anyhow!("args must be an array of strings"))?;
        for value in args_array {
            let Some(part) = value.as_str() else {
                return Err(anyhow!("args array must contain only strings"));
            };
            parts.push(part.to_string());
        }
    }

    if parts.is_empty() {
        return Err(anyhow!(
            "Error: Invalid 'run_pty_cmd' arguments. Expected JSON object with 'command' (string or array). Optional: 'args' (array). \
             Format 1 (string command): {{\"command\": \"ls -la\"}} \
             Format 2 (array command): {{\"command\": [\"ls\", \"-la\"]}} \
             Format 3 (command + args): {{\"command\": \"cargo\", \"args\": [\"build\", \"--release\"]}}. \
             {}",
            empty_error
        ));
    }

    if parts[0].trim().is_empty() {
        return Err(anyhow!(
            "{}\n\nThe first element of the command array cannot be empty or whitespace-only.\n\
             Got: {:?}",
            empty_error,
            parts
        ));
    }

    Ok(parts)
}

fn parse_pty_dimension(name: &str, value: Option<&Value>, default: u16) -> Result<u16> {
    let Some(raw) = value else {
        return Ok(default);
    };

    let numeric = raw
        .as_u64()
        .ok_or_else(|| anyhow!("{name} must be an integer"))?;
    if numeric == 0 {
        return Err(anyhow!("{name} must be greater than zero"));
    }
    if numeric > u16::MAX as u64 {
        return Err(anyhow!("{name} exceeds maximum value {}", u16::MAX));
    }

    Ok(numeric as u16)
}

fn bool_from_map(map: &Map<String, Value>, key: &str, default: bool) -> bool {
    map.get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(default)
}

fn parse_session_id(payload: &Map<String, Value>, missing_error: &str) -> Result<String> {
    let raw_id = payload
        .get("session_id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow!(missing_error.to_string()))?;
    let trimmed = raw_id.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("session_id cannot be empty"));
    }

    Ok(trimmed.to_string())
}

struct PtyCommandSetup {
    command: Vec<String>,
    display_command: String,
    working_dir_path: PathBuf,
    working_dir_display: String,
    session_id: String,
    rows: u16,
    cols: u16,
    timeout_secs: u64,
    /// Maximum tokens for output truncation. Defaults to DEFAULT_PTY_OUTPUT_MAX_TOKENS.
    /// Set to 0 to disable truncation (not recommended for large outputs).
    max_tokens: usize,
    confirm: bool,
}

impl PtyCommandSetup {
    fn size(&self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PtySnapshotViewOptions {
    include_screen: bool,
    include_scrollback: bool,
}

impl PtySnapshotViewOptions {
    fn new(include_screen: bool, include_scrollback: bool) -> Self {
        Self {
            include_screen,
            include_scrollback,
        }
    }
}

impl Default for PtySnapshotViewOptions {
    fn default() -> Self {
        Self {
            include_screen: true,
            include_scrollback: true,
        }
    }
}

fn snapshot_to_map(
    snapshot: VTCodePtySession,
    options: PtySnapshotViewOptions,
) -> Map<String, Value> {
    let VTCodePtySession {
        id,
        command,
        args,
        working_dir,
        rows,
        cols,
        screen_contents,
        scrollback,
    } = snapshot;

    let mut response = Map::new();
    response.insert("session_id".to_string(), Value::String(id));
    response.insert("command".to_string(), Value::String(command));
    response.insert(
        "args".to_string(),
        Value::Array(args.into_iter().map(Value::String).collect()),
    );
    let working_directory = working_dir.unwrap_or_else(|| ".".into());
    response.insert(
        "working_directory".to_string(),
        Value::String(working_directory),
    );
    response.insert("rows".to_string(), Value::from(rows));
    response.insert("cols".to_string(), Value::from(cols));

    if options.include_screen
        && let Some(screen) = screen_contents
    {
        response.insert(
            "screen_contents".to_string(),
            Value::String(filter_pty_output(&strip_ansi(&screen))),
        );
    }

    if options.include_scrollback
        && let Some(scrollback) = scrollback
    {
        response.insert(
            "scrollback".to_string(),
            Value::String(filter_pty_output(&strip_ansi(&scrollback))),
        );
    }

    response
}

fn filter_pty_output(text: &str) -> String {
    // Filter out macOS malloc debugging messages
    // These appear as: "process_name(PID) MallocStackLogging: message"
    let lines: Vec<&str> = text.lines().collect();
    let filtered: Vec<&str> = lines
        .iter()
        .filter(|line| {
            !line.contains("MallocStackLogging:")
                && !line.contains("malloc: enabling abort()")
                && !line.contains("can't turn off malloc stack logging")
        })
        .copied()
        .collect();
    filtered.join("\n")
}

fn strip_ansi(text: &str) -> String {
    crate::utils::ansi_parser::strip_ansi(text)
}

fn compute_output_stats(text: &str) -> Value {
    json!({
        "bytes": text.len(),
        "lines": text.lines().count(),
        "unicode_metrics": Value::Null,
    })
}

fn add_unified_metadata(
    value: &mut Value,
    stdout: Option<String>,
    exit_code: Option<i32>,
    is_exited: Option<bool>,
    duration_ms: Option<u128>,
    truncated: Option<bool>,
    capabilities: Value,
) {
    if let Value::Object(obj) = value {
        if let Some(stdout) = stdout {
            obj.entry("stdout".to_string())
                .or_insert(Value::String(stdout.clone()));
            obj.entry("stats".to_string())
                .or_insert(compute_output_stats(&stdout));
        } else if !obj.contains_key("stats") {
            obj.insert(
                "stats".to_string(),
                json!({"bytes": 0, "lines": 0, "unicode_metrics": Value::Null}),
            );
        }

        if let Some(code) = exit_code {
            obj.entry("exit_code".to_string())
                .or_insert(Value::Number(code.into()));
        }

        if let Some(done) = is_exited {
            obj.entry("is_exited".to_string())
                .or_insert(Value::Bool(done));
        }

        if let Some(ms) = duration_ms {
            let ms_u64 = ms as u64;
            obj.entry("duration_ms".to_string())
                .or_insert(Value::Number(ms_u64.into()));
        }

        if let Some(flag) = truncated {
            obj.entry("truncated".to_string())
                .or_insert(Value::Bool(flag));
        }

        obj.entry("capabilities".to_string())
            .or_insert(capabilities);
    }
}

fn pty_capabilities_from_config(config: &PtyConfig) -> Value {
    json!({
        "platform": std::env::consts::OS,
        "supports_login_shell": !cfg!(windows),
        "supports_resize": true,
        "max_sessions": config.max_sessions,
        "max_scrollback_bytes": config.max_scrollback_bytes,
        "supports_unicode_metrics": true,
    })
}

#[cfg(test)]
mod unified_metadata_tests {
    use super::*;

    #[test]
    fn adds_stats_and_capabilities_when_stdout_present() {
        let mut value = json!({});
        add_unified_metadata(
            &mut value,
            Some("hello\nworld".to_string()),
            Some(0),
            Some(true),
            Some(123),
            Some(true),
            json!({"supports_resize": true}),
        );

        let obj = value.as_object().expect("object");
        assert_eq!(
            obj.get("stdout").and_then(|v| v.as_str()),
            Some("hello\nworld")
        );
        let stats = obj.get("stats").and_then(|v| v.as_object()).expect("stats");
        assert_eq!(stats.get("bytes").and_then(|v| v.as_u64()), Some(11));
        assert_eq!(stats.get("lines").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(obj.get("exit_code").and_then(|v| v.as_i64()), Some(0));
        assert_eq!(obj.get("is_exited").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(obj.get("duration_ms").and_then(|v| v.as_u64()), Some(123));
        assert_eq!(obj.get("truncated").and_then(|v| v.as_bool()), Some(true));
        assert!(obj.get("capabilities").is_some());
    }

    #[test]
    fn adds_defaults_when_stdout_missing() {
        let mut value = json!({});
        add_unified_metadata(
            &mut value,
            None,
            None,
            None,
            None,
            Some(false),
            json!({"supports_resize": true}),
        );

        let obj = value.as_object().expect("object");
        let stats = obj.get("stats").and_then(|v| v.as_object()).expect("stats");
        assert_eq!(stats.get("bytes").and_then(|v| v.as_u64()), Some(0));
        assert_eq!(stats.get("lines").and_then(|v| v.as_u64()), Some(0));
        assert_eq!(obj.get("truncated").and_then(|v| v.as_bool()), Some(false));
        assert!(obj.get("capabilities").is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use base64::engine::general_purpose::STANDARD as BASE64;
    use serde_json::{Map, json};

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi("hello"), "hello");
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(strip_ansi("\x1b[1;32mbold green\x1b[0m"), "bold green");
        assert_eq!(
            strip_ansi("Checking \x1b[0m\x1b[1m\x1b[32mvtcode\x1b[0m"),
            "Checking vtcode"
        );
    }

    #[test]
    fn windows_tokenizer_preserves_paths_with_spaces() {
        let command = r#""C:\Program Files\Git\bin\bash.exe" -lc "echo hi""#;
        let tokens = tokenize_command_string(command, Some("cmd.exe")).expect("tokens");
        assert_eq!(
            tokens,
            vec![
                r"C:\Program Files\Git\bin\bash.exe".to_string(),
                "-lc".to_string(),
                "echo hi".to_string(),
            ]
        );
    }

    #[test]
    fn windows_tokenizer_handles_empty_arguments() {
        let tokens = tokenize_windows_command("\"\"").expect("tokens");
        assert_eq!(tokens, vec![String::new()]);
    }

    #[test]
    fn windows_tokenizer_errors_on_unterminated_quotes() {
        let err = tokenize_windows_command("\"unterminated").unwrap_err();
        assert!(err.to_string().contains("unterminated"));
    }

    #[test]
    fn windows_join_quotes_arguments_with_spaces() {
        let parts = vec![
            r"C:\Program Files\Git\bin\git.exe".to_string(),
            "--version".to_string(),
        ];
        let joined = join_windows_command(&parts);
        assert_eq!(
            joined,
            r#""C:\Program Files\Git\bin\git.exe" --version"#.to_string()
        );
    }

    #[test]
    fn windows_join_leaves_simple_arguments_unquoted() {
        let parts = vec!["cmd".to_string(), "/C".to_string(), "dir".to_string()];
        let joined = join_windows_command(&parts);
        assert_eq!(joined, "cmd /C dir");
    }

    #[test]
    fn pty_input_prefers_base64_over_plain_text() {
        let mut payload = Map::new();
        payload.insert(
            "session_id".to_string(),
            Value::String("test-session".into()),
        );
        payload.insert("append_newline".to_string(), Value::Bool(false));
        payload.insert("input".to_string(), Value::String("plain".into()));
        let encoded = BASE64.encode(b"decoded");
        payload.insert("input_base64".to_string(), Value::String(encoded));

        let parsed = PtyInputPayload::from_map(&payload).expect("pty payload");
        assert_eq!(parsed.buffer, b"decoded");
        assert!(!parsed.append_newline);
    }

    #[test]
    fn pty_input_rejects_empty_payload_without_newline() {
        let mut payload = Map::new();
        payload.insert(
            "session_id".to_string(),
            Value::String("empty-session".into()),
        );

        let err = PtyInputPayload::from_map(&payload).expect_err("expected failure");
        assert!(
            err.to_string()
                .contains("send_pty_input requires 'input' or 'input_base64'")
        );
    }

    #[test]
    fn tokenizer_uses_posix_rules_for_posix_shells() {
        let tokens =
            tokenize_command_string("echo 'hello world'", Some("/bin/bash")).expect("tokens");
        assert_eq!(tokens, vec!["echo", "hello world"]);
    }

    #[test]
    fn tokenizer_strips_markdown_single_backticks() {
        // LLMs sometimes wrap commands in backticks from markdown formatting
        let tokens = tokenize_command_string("`cargo clippy`", None).expect("tokens");
        assert_eq!(tokens, vec!["cargo", "clippy"]);
    }

    #[test]
    fn tokenizer_strips_markdown_triple_backticks() {
        let tokens = tokenize_command_string("```\ncargo test\n```", None).expect("tokens");
        assert_eq!(tokens, vec!["cargo", "test"]);
    }

    #[test]
    fn tokenizer_strips_markdown_triple_backticks_with_language() {
        let tokens = tokenize_command_string("```bash\ncargo build\n```", None).expect("tokens");
        assert_eq!(tokens, vec!["cargo", "build"]);
    }

    #[test]
    fn tokenizer_handles_plain_commands() {
        // Plain commands without backticks should work as before
        let tokens = tokenize_command_string("cargo fmt", None).expect("tokens");
        assert_eq!(tokens, vec!["cargo", "fmt"]);
    }

    #[test]
    fn strip_markdown_preserves_backticks_in_shell_commands() {
        // Backticks used for command substitution should be preserved
        // This case: `echo $(date)` - inner backticks are command substitution
        let result = strip_markdown_code_formatting("echo `date`");
        assert_eq!(result, "echo `date`");
    }

    #[test]
    fn normalize_natural_language_git_diff_on_file() {
        // Test "git diff on file.rs" -> "git diff file.rs"
        let result = normalize_natural_language_command("git diff on vtcode-core/src/mcp/mod.rs");
        assert_eq!(result, "git diff vtcode-core/src/mcp/mod.rs");

        // Test "git log on src/" -> "git log src/"
        let result = normalize_natural_language_command("git log on src/");
        assert_eq!(result, "git log src/");

        // Test "git status on ." -> "git status ."
        let result = normalize_natural_language_command("git status on .");
        assert_eq!(result, "git status .");

        // Test that normal commands are not affected
        let result = normalize_natural_language_command("git diff --cached");
        assert_eq!(result, "git diff --cached");

        // Test that "on" in other contexts is not affected
        let result = normalize_natural_language_command("echo on");
        assert_eq!(result, "echo on");
    }

    #[test]
    fn normalize_trailing_and_report_clause() {
        let result = normalize_natural_language_command("cargo clippy and report");
        assert_eq!(result, "cargo clippy");

        let result = normalize_natural_language_command("npm test and show output");
        assert_eq!(result, "npm test");
    }

    #[test]
    fn detects_windows_shell_name_variants() {
        assert!(should_use_windows_command_tokenizer(Some(
            "C:/Windows/System32/cmd.exe"
        )));
        assert!(should_use_windows_command_tokenizer(Some("pwsh")));
        assert_eq!(normalized_shell_name("/bin/bash"), "bash");
    }

    #[test]
    fn resolve_shell_preference_uses_explicit_value() {
        let mut config = PtyConfig::default();
        config.preferred_shell = Some("/bin/bash".to_string());
        let resolved = super::resolve_shell_preference(Some("/custom/zsh"), &config);
        assert_eq!(resolved, "/custom/zsh");
    }

    #[test]
    fn resolve_shell_preference_uses_config_value() {
        let mut config = PtyConfig::default();
        config.preferred_shell = Some("/bin/zsh".to_string());
        let resolved = super::resolve_shell_preference(None, &config);
        assert_eq!(resolved, "/bin/zsh");
    }

    #[test]
    fn resolve_shell_preference_always_returns_value() {
        let config = PtyConfig::default();
        let resolved = super::resolve_shell_preference(None, &config);
        // Should never return empty string - guaranteed to have a fallback
        assert!(!resolved.is_empty());
    }

    #[test]
    fn pty_input_prefers_base64_over_plain_text_v2() {
        let map: Map<String, Value> = json!({
            "session_id": "pty-1",
            "input": "ls",
            "input_base64": BASE64.encode("pwd"),
            "append_newline": false,
        })
        .as_object()
        .unwrap()
        .clone();

        let payload = PtyInputPayload::from_map(&map).expect("payload");
        assert_eq!(payload.buffer, b"pwd");
        assert!(!payload.append_newline);
    }

    #[test]
    fn pty_input_uses_plain_text_when_base64_missing() {
        let map: Map<String, Value> = json!({
            "session_id": "pty-2",
            "input": "echo hello",
        })
        .as_object()
        .unwrap()
        .clone();

        let payload = PtyInputPayload::from_map(&map).expect("payload");
        assert_eq!(payload.buffer, b"echo hello");
        assert!(!payload.append_newline);
    }

    #[test]
    fn pty_input_rejects_empty_without_newline() {
        let map: Map<String, Value> = json!({
            "session_id": "pty-3",
            "input": "",
            "append_newline": false,
        })
        .as_object()
        .unwrap()
        .clone();

        let err = PtyInputPayload::from_map(&map).unwrap_err();
        assert!(
            err.to_string()
                .contains("send_pty_input requires 'input' or 'input_base64'")
        );
    }

    #[test]
    fn pty_input_allows_empty_when_newline_requested() {
        let map: Map<String, Value> = json!({
            "session_id": "pty-4",
            "input": "",
            "append_newline": true,
        })
        .as_object()
        .unwrap()
        .clone();

        let payload = PtyInputPayload::from_map(&map).expect("payload");
        assert!(payload.buffer.is_empty());
        assert!(payload.append_newline);
    }
}

struct PtySessionViewArgs {
    session_id: String,
    drain_output: bool,
    view: PtySnapshotViewOptions,
    max_tokens: Option<usize>,
}

impl PtySessionViewArgs {
    fn from_map(map: &Map<String, Value>) -> Result<Self> {
        let session_id = parse_session_id(map, "read_pty_session requires a 'session_id' string")?;
        let drain_output = bool_from_map(map, "drain", false);
        let include_screen = bool_from_map(map, "include_screen", true);
        let include_scrollback = bool_from_map(map, "include_scrollback", true);
        let max_tokens = map
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        Ok(Self {
            session_id,
            drain_output,
            view: PtySnapshotViewOptions::new(include_screen, include_scrollback),
            max_tokens,
        })
    }
}

#[derive(Debug)]
struct PtyInputPayload {
    session_id: String,
    buffer: Vec<u8>,
    append_newline: bool,
    wait_ms: u64,
    drain_output: bool,
}

impl PtyInputPayload {
    fn from_map(map: &Map<String, Value>) -> Result<Self> {
        let session_id = parse_session_id(map, "send_pty_input requires a 'session_id' string")?;
        let append_newline = bool_from_map(map, "append_newline", false);
        let wait_ms = map
            .get("wait_ms")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let drain_output = bool_from_map(map, "drain", true);

        let input_text = map.get("input").and_then(Value::as_str);
        let input_base64_text = map.get("input_base64").and_then(Value::as_str);
        let input_preview = input_text.map(Self::preview_string);
        let input_base64_preview = input_base64_text.map(Self::preview_string);

        debug!(
            target: "vtcode::pty",
            session_id = %session_id,
            append_newline,
            wait_ms,
            drain_output,
            input_len = input_text.map(|text| text.len()).unwrap_or(0),
            input_preview = input_preview.as_deref(),
            input_base64_len = input_base64_text.map(|text| text.len()).unwrap_or(0),
            input_base64_preview = input_base64_preview.as_deref(),
            "received send_pty_input payload"
        );

        let mut buffer = Vec::with_capacity(4096); // Pre-allocate 4KB buffer for typical PTY output

        // Prefer input_base64 if present, else use input
        if let Some(encoded) = map
            .get("input_base64")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
        {
            let decoded = BASE64_STANDARD
                .decode(encoded.as_bytes())
                .context("input_base64 must be valid base64")?;
            buffer.extend_from_slice(&decoded);
        } else if let Some(text) = map.get("input").and_then(|value| value.as_str()) {
            buffer.extend_from_slice(text.as_bytes());
        }

        debug!(
            target: "vtcode::pty",
            session_id = %session_id,
            buffer_len = buffer.len(),
            buffer_preview = %Self::preview_bytes(&buffer),
            "prepared PTY input buffer"
        );

        if buffer.is_empty() && !append_newline {
            debug!(
                target: "vtcode::pty",
                session_id = %session_id,
                "rejecting empty PTY input without append_newline"
            );
            return Err(anyhow!(
                "send_pty_input requires 'input' or 'input_base64' unless append_newline is true"
            ));
        }

        trace!(
            target: "vtcode::pty",
            session_id = session_id.as_str(),
            append_newline,
            wait_ms,
            drain_output,
            has_input = map.contains_key("input"),
            has_input_base64 = map.contains_key("input_base64"),
            buffer_len = buffer.len(),
            "parsed PTY input payload"
        );

        Ok(Self {
            session_id,
            buffer,
            append_newline,
            wait_ms,
            drain_output,
        })
    }

    fn preview_string(text: &str) -> String {
        const MAX_PREVIEW: usize = 64;
        if text.len() <= MAX_PREVIEW {
            text.to_string()
        } else {
            format!("{}…", &text[..MAX_PREVIEW])
        }
    }

    fn preview_bytes(bytes: &[u8]) -> String {
        const MAX_BYTES: usize = 64;
        if let Ok(text) = std::str::from_utf8(bytes) {
            return Self::preview_string(text);
        }

        let mut hex = String::new();
        for byte in bytes.iter().take(MAX_BYTES / 2) {
            use std::fmt::Write as _;
            let _ = write!(hex, "{:02x}", byte);
        }
        if bytes.len() > MAX_BYTES / 2 {
            hex.push('…');
        }
        format!("hex:{}", hex)
    }
}

struct PtyEphemeralCapture {
    output: String,
    exit_code: Option<i32>,
    completed: bool,
    duration: Duration,
}

struct PtyFollowUp {
    summary: String,
    warnings: Vec<String>,
    errors: Vec<String>,
    prompt: Option<String>,
}

fn summarize_pty_output(
    output: &str,
    exit_code: Option<i32>,
    command: &[String],
    duration: Duration,
    completed: bool,
) -> PtyFollowUp {
    const MAX_SCAN_LINES: usize = 200;
    const MAX_FINDINGS: usize = 5;

    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    for line in output.lines().take(MAX_SCAN_LINES) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let lower = trimmed.to_lowercase();
        if lower.contains("warning") && warnings.len() < MAX_FINDINGS {
            warnings.push(trimmed.to_string());
            continue;
        }

        if lower.contains("error") && errors.len() < MAX_FINDINGS {
            errors.push(trimmed.to_string());
        }
    }

    let command_display = if command.is_empty() {
        "<empty command>".to_string()
    } else {
        join(command.iter().map(|part| part.as_str()))
    };

    let status = if !completed {
        "running".to_string()
    } else if exit_code == Some(0) {
        "succeeded".to_string()
    } else if let Some(code) = exit_code {
        format!("finished with exit code {code}")
    } else {
        "finished".to_string()
    };

    let mut summary = format!("{command_display}: {status}");
    if completed {
        summary.push_str(&format!(" ({:.1}s)", duration.as_secs_f32()));
    }
    if !warnings.is_empty() {
        summary.push_str(&format!(" | warnings: {}", warnings.len()));
    }
    if !errors.is_empty() {
        summary.push_str(&format!(" | errors: {}", errors.len()));
    }

    let prompt = if !completed {
        None
    } else if exit_code == Some(0) && !warnings.is_empty() {
        Some(format!(
            "{} warning{} detected. Should I address them now?",
            warnings.len(),
            if warnings.len() == 1 { "" } else { "s" }
        ))
    } else if exit_code.is_some_and(|code| code != 0) {
        Some("Command failed. Investigate and fix the errors?".to_string())
    } else {
        None
    };

    PtyFollowUp {
        summary,
        warnings,
        errors,
        prompt,
    }
}

fn build_ephemeral_pty_response(
    setup: &PtyCommandSetup,
    capture: PtyEphemeralCapture,
    snapshot: VTCodePtySession,
    truncated: bool,
    capabilities: Value,
) -> Value {
    let PtyEphemeralCapture {
        output,
        exit_code,
        completed,
        duration,
    } = capture;

    let session_reference = if completed {
        None
    } else {
        Some(setup.session_id.clone())
    };
    let code = if completed { exit_code } else { None };
    let status = if completed { "completed" } else { "running" };

    // Build a clear message for the agent based on status
    let (mut message, output_override) = if completed {
        if let Some(exit_code) = code {
            if exit_code == 0 {
                ("Command completed successfully".to_string(), None)
            } else if exit_code == 127 {
                // Command not found - provide immediate, actionable guidance
                // IMPORTANT: We used to replace the output, but that hides the actual shell error (e.g. "zsh: command not found: cargo")
                // Now we preserve the output but provide the helpful message in the "message" field and "critical_note"

                // Try to extract the actual command that failed if it was wrapped in a shell
                let mut cmd_name = setup
                    .command
                    .first()
                    .map(|s| s.as_str())
                    .unwrap_or("command");

                if setup.command.len() >= 3 {
                    let shell = std::path::Path::new(&setup.command[0]);
                    let shell_name = shell.file_name().and_then(|n| n.to_str()).unwrap_or("");

                    if shell_name.contains("sh")
                        || shell_name.contains("cmd")
                        || shell_name.contains("powershell")
                        || shell_name.contains("pwsh")
                    {
                        // Scan arguments for the execution flag
                        for (i, arg) in setup.command.iter().enumerate().skip(1) {
                            if arg == "-c" || arg == "/C" || arg == "-Command" || arg == "-lc" {
                                // The next argument should be the command string
                                if let Some(cmd_str) = setup.command.get(i + 1) {
                                    cmd_name =
                                        cmd_str.split_whitespace().next().unwrap_or("command");
                                    break;
                                }
                            }
                        }
                    }
                }

                let helpful_msg = generate_command_not_found_message(cmd_name);
                (helpful_msg.clone(), None)
            } else {
                (format!("Command failed with exit code {}", exit_code), None)
            }
        } else {
            ("Command completed".to_string(), None)
        }
    } else {
        (
            "Command is still running. Backend continues polling automatically. Do NOT call read_pty_session."
                .to_string(),
            None,
        )
    };

    // Use the override output if available (for exit code 127), otherwise use actual output
    let final_output = output_override.as_ref().unwrap_or(&output);

    let follow_up = summarize_pty_output(final_output, code, &setup.command, duration, completed);
    if completed && code == Some(0) && !follow_up.warnings.is_empty() {
        message = format!(
            "Command completed successfully with {} warning{}",
            follow_up.warnings.len(),
            if follow_up.warnings.len() == 1 {
                ""
            } else {
                "s"
            }
        );
    } else if completed && code.is_some_and(|c| c != 0 && c != 127) && !follow_up.errors.is_empty()
    {
        message = format!(
            "Command failed with exit code {} ({} error{})",
            code.unwrap_or_default(),
            follow_up.errors.len(),
            if follow_up.errors.len() == 1 { "" } else { "s" }
        );
    }

    if let Some(prompt) = &follow_up.prompt {
        message = format!("{} · {}", message, prompt);
    }

    // Detect if this is a git diff command - output is already rendered visually
    let has_git = setup
        .command
        .iter()
        .any(|arg| arg.to_lowercase().contains("git"));
    let has_diff_cmd = setup.command.iter().any(|arg| {
        let lower = arg.to_lowercase();
        lower == "diff"
            || lower == "show"
            || lower == "log"
            || lower.contains("git diff")
            || lower.contains("git show")
            || lower.contains("git log")
    });
    let is_git_diff = has_git && has_diff_cmd;

    // Build annotated output for the agent (keeps raw content + follow-up hints)
    let mut cleaned_output = strip_ansi(final_output);
    let mut follow_lines: Vec<String> = Vec::new();
    follow_lines.push(format!("Follow-up: {}", follow_up.summary));
    if !follow_up.warnings.is_empty() {
        let mut shown = 0usize;
        for warning in follow_up.warnings.iter().take(3) {
            follow_lines.push(format!("warning: {}", warning));
            shown += 1;
        }
        if follow_up.warnings.len() > shown {
            follow_lines.push(format!(
                "(+{} more warnings)",
                follow_up.warnings.len() - shown
            ));
        }
    }
    if !follow_up.errors.is_empty() {
        let mut shown = 0usize;
        for error in follow_up.errors.iter().take(3) {
            follow_lines.push(format!("error: {}", error));
            shown += 1;
        }
        if follow_up.errors.len() > shown {
            follow_lines.push(format!("(+{} more errors)", follow_up.errors.len() - shown));
        }
    }
    if let Some(prompt) = &follow_up.prompt {
        follow_lines.push(format!("Next: {}", prompt));
    }
    if !follow_lines.is_empty() {
        cleaned_output.push_str("\n\n");
        cleaned_output.push_str(&follow_lines.join("\n"));
    }

    let mut response = json!({
        "success": true,
        "command": setup.command.clone(),
        "output": cleaned_output,
        "code": code,
        "exit_code": code,
        "status": status,
        "message": message,
        "mode": "pty",
        "is_exited": completed,
        "session_id": session_reference,
        "id": setup.session_id.clone(),
        "rows": snapshot.rows,
        "cols": snapshot.cols,
        "pty": {
            "rows": snapshot.rows,
            "cols": snapshot.cols,
        },
        "working_directory": setup.working_dir_display.clone(),
        "working_dir": setup.working_dir_display.clone(),
        "timeout_secs": setup.timeout_secs,
        "duration_ms": if completed { duration.as_millis() } else { 0 },
        "output_already_rendered": is_git_diff,
    });

    if let Value::Object(ref mut obj) = response {
        obj.insert(
            "follow_up".to_string(),
            json!({
                "summary": follow_up.summary,
                "warnings": follow_up.warnings,
                "errors": follow_up.errors,
                "prompt": follow_up.prompt,
            }),
        );
        if let Some(prompt) = follow_up.prompt {
            obj.insert("follow_up_prompt".to_string(), Value::String(prompt));
        }
    }

    // Add CRITICAL signals for exit code 127 to ABSOLUTELY PREVENT retry loops
    if completed
        && code == Some(127)
        && let Value::Object(ref mut obj) = response
    {
        // Add multiple explicit "STOP" signals to prevent agent from trying workarounds
        obj.insert("fatal_error".to_string(), Value::Bool(true));
        obj.insert("do_not_attempt_workarounds".to_string(), Value::Bool(true));
        obj.insert("do_not_retry".to_string(), Value::Bool(true));
        obj.insert("final".to_string(), Value::Bool(true));

        // CRITICAL: Direct instruction to agent not to retry
        obj.insert(
                "agent_instruction".to_string(),
                Value::String(
                    "AGENT: Do NOT run any more commands related to this tool. Do NOT try different shells. \
                     Do NOT try bash -lc, sh -lc, or workarounds. The tool is not installed. \
                     STOP all command attempts. Follow the solution in the output field. \
                     If you are tempted to retry or try a workaround, STOP. You already have the answer."
                        .to_string(),
                ),
            );

        // Comprehensive explanation of why retrying won't work
        obj.insert(
            "critical_note".to_string(),
            Value::String(
                "EXIT CODE 127 IS FINAL. The operating system shell searched the entire PATH \
                     and could not find this executable. This is NOT a shell compatibility issue. \
                     Retrying with bash, sh, zsh, or different escaping WILL FAIL. \
                     The ONLY solutions are: (1) Install the tool, (2) Add its directory to PATH, \
                     (3) Use a different tool. Read the 'output' field for specific instructions."
                    .to_string(),
            ),
        );
        obj.insert(
            "suggestion".to_string(),
            Value::String(
                "DO NOT RETRY with: different shells, diagnostic commands (which/--version), \
                     shell wrappers (bash -lc, sh -lc), or any command variations. \
                     This will cause an infinite loop of exit code 127 errors."
                    .to_string(),
            ),
        );
        obj.insert(
                "error_explanation".to_string(),
                Value::String(
                    "Exit code 127: Command not found. The OS searched PATH. The tool is not installed \
                     or not in a directory that's in PATH. No shell workaround will fix this."
                        .to_string(),
                ),
            );
    }

    add_unified_metadata(
        &mut response,
        Some(strip_ansi(final_output)),
        code,
        Some(completed),
        Some(duration.as_millis()),
        Some(truncated),
        capabilities,
    );

    response
}

/// Generate helpful error message for exit code 127 (command not found)
fn generate_command_not_found_message(cmd: &str) -> String {
    match cmd {
        "cargo" | "rustfmt" | "clippy" | "cargo-clippy" => {
            "Command not found: This is a Rust tool. Try: source $HOME/.cargo/env && <command>. \
             If Rust is not installed: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
                .to_string()
        }
        "git" => {
            "Command not found: git is not installed or not in PATH. Install git for your system.".to_string()
        }
        "npm" | "node" => {
            "Command not found: Node.js/npm is not installed or not in PATH. Install Node.js from https://nodejs.org".to_string()
        }
        "python" | "python3" => {
            "Command not found: Python is not installed or not in PATH. Install Python 3 for your system.".to_string()
        }
        _ => {
            format!(
                "Command not found: '{}' is not in PATH or not installed. \
                 Verify installation or add its directory to PATH. Do NOT retry with diagnostic commands.",
                cmd
            )
        }
    }
}

/// Extract errors, warnings, and summary from build tool output.
/// Prioritizes error messages over verbose compilation progress.
///
/// For cargo/rustc output, extracts:
/// - All error lines (error[E...]:) with 2 lines of context
/// - All warning lines (warning:) with 1 line of context
/// - Summary lines (Finished, error: could not compile, etc.)
///
/// This dramatically reduces output size while preserving actionable information.
fn extract_build_errors_and_summary(output: &str, max_tokens: usize) -> String {
    use crate::core::token_constants::TOKENS_PER_CHARACTER;

    let lines: Vec<&str> = output.lines().collect();
    let total_lines = lines.len();

    // If output is small enough, return as-is
    let estimated_tokens = (output.len() as f64 / TOKENS_PER_CHARACTER).ceil() as usize;
    if estimated_tokens <= max_tokens {
        return output.to_string();
    }

    let mut extracted = Vec::with_capacity(100); // Pre-allocate for typical extraction size
    let mut i = 0;

    // Patterns that indicate important lines to keep
    let error_patterns = ["error[E", "error:", "Error:", "ERROR:"];
    let warning_patterns = ["warning:", "Warning:", "WARN:"];
    let summary_patterns = [
        "Finished",
        "error: could not compile",
        "error: aborting",
        "Build failed",
        "npm ERR!",
        "failed to compile",
        "generated",
        "warning:",
        "error:",
    ];

    while i < total_lines {
        let line = lines[i];
        let is_error = error_patterns.iter().any(|p| line.contains(p));
        let is_warning = warning_patterns.iter().any(|p| line.contains(p));
        let is_summary = summary_patterns.iter().any(|p| line.contains(p));

        if is_error {
            // Include 2 lines before and after error for context
            let start = i.saturating_sub(2);
            let end = (i + 3).min(total_lines);
            for j in start..end {
                if !extracted.contains(&j) {
                    extracted.push(j);
                }
            }
        } else if is_warning {
            // Include 1 line before and after warning
            let start = i.saturating_sub(1);
            let end = (i + 2).min(total_lines);
            for j in start..end {
                if !extracted.contains(&j) {
                    extracted.push(j);
                }
            }
        } else if is_summary && !extracted.contains(&i) {
            extracted.push(i);
        }

        i += 1;
    }

    // Always include the last 10 lines (usually contains summary)
    let tail_start = total_lines.saturating_sub(10);
    for j in tail_start..total_lines {
        if !extracted.contains(&j) {
            extracted.push(j);
        }
    }

    // Sort to maintain original order
    extracted.sort();

    // Build output with markers for skipped sections
    let mut result = String::new();
    let mut last_idx: Option<usize> = None;

    for &idx in &extracted {
        if let Some(last) = last_idx
            && idx > last + 1
        {
            let skipped = idx - last - 1;
            let _ = writeln!(result, "\n[... {} lines skipped ...]", skipped);
        }
        result.push_str(lines[idx]);
        result.push('\n');
        last_idx = Some(idx);
    }

    // If we extracted nothing useful, fall back to head+tail
    if extracted.is_empty() || result.trim().is_empty() {
        let head_lines = 50.min(total_lines / 3);
        let tail_lines = 30.min(total_lines / 3);

        result.clear();
        for line in lines.iter().take(head_lines) {
            result.push_str(line);
            result.push('\n');
        }
        if total_lines > head_lines + tail_lines {
            let _ = writeln!(
                result,
                "\n[... {} lines skipped ...]\n",
                total_lines - head_lines - tail_lines
            );
        }
        for line in lines.iter().skip(total_lines.saturating_sub(tail_lines)) {
            result.push_str(line);
            result.push('\n');
        }
    }

    result
}

fn build_shell_command_string(
    raw_command: Option<&str>,
    parts: &[String],
    shell_hint: &str,
) -> String {
    if let Some(raw) = raw_command {
        return raw.to_string();
    }

    if should_use_windows_command_tokenizer(Some(shell_hint)) {
        return join_windows_command(parts);
    }

    join(parts.iter().map(|part| part.as_str()))
}

fn join_windows_command(parts: &[String]) -> String {
    parts
        .iter()
        .map(|part| quote_windows_argument(part))
        .collect::<Vec<_>>()
        .join(" ")
}

#[allow(dead_code)]
fn tokenize_windows_command(command: &str) -> Result<Vec<String>> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut backslashes = 0usize;

    for ch in command.chars() {
        match ch {
            '"' => {
                // In Windows parsing, an even number of preceding backslashes escapes them,
                // an odd number escapes the quote.
                if backslashes.is_multiple_of(2) {
                    in_quotes = !in_quotes;
                }
                current.push('"');
                backslashes = 0;
            }
            '\\' => {
                backslashes += 1;
                current.push(ch);
            }
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    parts.push(current.trim_matches('"').to_string());
                    current.clear();
                }
                backslashes = 0;
            }
            _ => {
                if backslashes > 0 {
                    backslashes = 0;
                }
                current.push(ch);
            }
        }
    }

    if in_quotes {
        return Err(anyhow!("unterminated quotes in command"));
    }

    if !current.is_empty() {
        parts.push(current.trim_matches('"').to_string());
    }

    Ok(parts)
}

fn quote_windows_argument(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }

    let requires_quotes = arg
        .chars()
        .any(|c| c.is_whitespace() || c == '"' || c == '\t');
    if !requires_quotes {
        return arg.to_string();
    }

    let mut result = String::with_capacity(arg.len() + 2);
    result.push('"');

    let mut backslashes = 0;
    for ch in arg.chars() {
        match ch {
            '\\' => {
                backslashes += 1;
            }
            '"' => {
                result.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                result.push('"');
                backslashes = 0;
            }
            _ => {
                if backslashes > 0 {
                    result.extend(std::iter::repeat_n('\\', backslashes));
                    backslashes = 0;
                }
                result.push(ch);
            }
        }
    }

    if backslashes > 0 {
        result.extend(std::iter::repeat_n('\\', backslashes * 2));
    }

    result.push('"');
    result
}

fn tokenize_command_string(command: &str, _shell_hint: Option<&str>) -> Result<Vec<String>> {
    // Sanitize markdown formatting (backticks) that LLMs sometimes include
    let sanitized = strip_markdown_code_formatting(command);

    // Normalize natural language patterns before tokenization
    let normalized = normalize_natural_language_command(&sanitized);

    split(&normalized).map_err(|err| anyhow!(err))
}

/// Normalize natural language command patterns to proper shell syntax.
/// Handles common patterns like "git diff on file.rs" -> "git diff file.rs"
fn normalize_natural_language_command(command: &str) -> String {
    let trimmed = command.trim();

    // Drop trailing natural-language clauses like "and report" that should not be
    // forwarded to the shell command.
    const TRAILING_CONNECTORS: [&str; 6] = [
        " and report",
        " and show",
        " and display",
        " and tell me",
        " and give",
        " and summarize",
    ];
    let lowered = trimmed.to_ascii_lowercase();
    for connector in TRAILING_CONNECTORS {
        if let Some(idx) = lowered.rfind(connector)
            && idx > 0
        {
            let candidate = trimmed[..idx].trim_end();
            if !candidate.is_empty() {
                return candidate.to_string();
            }
        }
    }

    // Pattern: "git <subcommand> on <path>" -> "git <subcommand> <path>"
    // Examples: "git diff on file.rs", "git log on src/", "git status on ."
    if let Some(git_idx) = trimmed.find("git ")
        && let Some(on_idx) = trimmed.find(" on ")
    {
        // Ensure "on" comes after "git" and is not part of another word
        if on_idx > git_idx {
            let before_on = &trimmed[..on_idx];
            let after_on = &trimmed[on_idx + 4..]; // Skip " on "

            // Only normalize if "on" is followed by a path-like argument
            // (not empty and doesn't look like another command)
            if !after_on.trim().is_empty() && !after_on.trim().starts_with('-') {
                return format!("{} {}", before_on, after_on);
            }
        }
    }

    trimmed.to_string()
}

/// Strip common markdown code formatting from command strings.
/// LLMs sometimes include backticks when generating tool calls from user prompts.
fn strip_markdown_code_formatting(input: &str) -> String {
    let trimmed = input.trim();

    // Handle triple backticks with optional language identifier (```bash, ```sh, etc.)
    if trimmed.starts_with("```") {
        let without_opening = trimmed.strip_prefix("```").unwrap_or(trimmed);
        // Skip language identifier on first line if present
        let content = if let Some(newline_pos) = without_opening.find('\n') {
            &without_opening[newline_pos + 1..]
        } else {
            without_opening
        };
        // Remove closing backticks
        let result = content.trim_end().strip_suffix("```").unwrap_or(content);
        return result.trim().to_string();
    }

    // Handle single backticks wrapping the entire command
    if trimmed.starts_with('`') && trimmed.ends_with('`') && trimmed.len() > 2 {
        // Check if it's not just backticks at start and end of different words
        let inner = &trimmed[1..trimmed.len() - 1];
        // Only strip if there are no internal backticks (it's a single wrapped command)
        if !inner.contains('`') {
            return inner.to_string();
        }
    }

    // Strip leading/trailing backticks that might be attached to the command
    let mut result = trimmed.to_string();

    // Handle case like "`cargo clippy`" -> "cargo clippy"
    if result.starts_with('`') {
        result = result[1..].to_string();
    }
    if result.ends_with('`') {
        result = result[..result.len() - 1].to_string();
    }

    result
}

fn should_use_windows_command_tokenizer(shell_hint: Option<&str>) -> bool {
    if let Some(shell) = shell_hint
        && is_windows_shell(shell)
    {
        return true;
    }

    cfg!(windows)
}

fn resolve_shell_preference(explicit: Option<&str>, config: &PtyConfig) -> String {
    explicit
        .and_then(sanitize_shell_candidate)
        .or_else(|| {
            config
                .preferred_shell
                .as_deref()
                .and_then(sanitize_shell_candidate)
        })
        .or_else(|| {
            env::var("SHELL")
                .ok()
                .and_then(|value| sanitize_shell_candidate(&value))
        })
        .or_else(detect_posix_shell_candidate)
        .unwrap_or_else(|| resolve_shell_candidate().display().to_string())
}

fn resolve_shell_candidate() -> PathBuf {
    // Resolve the preferred shell for command execution
    // Detects available shells based on platform
    if cfg!(windows) {
        // Windows: prefer PowerShell if available, fall back to cmd.exe
        if Path::new("C:\\Windows\\System32\\pwsh.exe").exists() {
            PathBuf::from("C:\\Windows\\System32\\pwsh.exe")
        } else if Path::new("C:\\Program Files\\PowerShell\\7\\pwsh.exe").exists() {
            PathBuf::from("C:\\Program Files\\PowerShell\\7\\pwsh.exe")
        } else {
            PathBuf::from("cmd.exe")
        }
    } else {
        // POSIX systems: use detected shell or default to /bin/sh
        detect_posix_shell_candidate()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/bin/sh"))
    }
}

fn sanitize_shell_candidate(shell: &str) -> Option<String> {
    let trimmed = shell.trim();
    if trimmed.is_empty() {
        None
    } else if Path::new(trimmed).exists() {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn detect_posix_shell_candidate() -> Option<String> {
    if cfg!(windows) {
        return None;
    }

    const CANDIDATES: [&str; 6] = [
        "/bin/bash",
        "/usr/bin/bash",
        "/bin/zsh",
        "/usr/bin/zsh",
        "/bin/sh",
        "/usr/bin/sh",
    ];

    for candidate in CANDIDATES {
        if Path::new(candidate).exists() {
            return Some(candidate.to_string());
        }
    }

    None
}

fn is_default_shell_placeholder(program: &str) -> bool {
    matches!(normalized_shell_name(program).as_str(), "bash" | "sh")
}

fn is_windows_shell(shell: &str) -> bool {
    matches!(
        normalized_shell_name(shell).as_str(),
        "cmd" | "cmd.exe" | "powershell" | "powershell.exe" | "pwsh"
    )
}

fn normalized_shell_name(shell: &str) -> String {
    Path::new(shell)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(shell)
        .to_ascii_lowercase()
}

fn generate_session_id(prefix: &str) -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis();
    format!("{prefix}-{timestamp}")
}

struct PtySessionLifecycle<'a> {
    registry: &'a ToolRegistry,
    active: bool,
}

impl<'a> PtySessionLifecycle<'a> {
    fn start(registry: &'a ToolRegistry) -> Result<Self> {
        registry.start_pty_session()?;
        Ok(Self {
            registry,
            active: true,
        })
    }

    fn commit(&mut self) {
        self.active = false;
    }
}

impl Drop for PtySessionLifecycle<'_> {
    fn drop(&mut self) {
        if self.active {
            self.registry.end_pty_session();
        }
    }
}

/// Detects if a command is known to be long-running (build tools, package managers, etc.)
fn is_long_running_command(command_parts: &[String]) -> bool {
    if let Some(first) = command_parts.first() {
        let cmd = first.to_lowercase();
        let basename = std::path::Path::new(&cmd)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Check if it's a long-running command
        LONG_RUNNING_COMMANDS
            .iter()
            .any(|&long_cmd| basename.starts_with(long_cmd) || basename == long_cmd)
    } else {
        false
    }
}

#[cfg(test)]
mod search_replace_exec_tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[tokio::test]
    async fn search_replace_replaces_and_backups() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path().to_path_buf();
        let file_path = workspace_root.join("sample.txt");

        fs::write(&file_path, "hello world\nhello again\n")
            .await
            .expect("write fixture");

        let mut registry = ToolRegistry::new(workspace_root.clone()).await;
        registry.allow_all_tools().await.ok();

        let response = registry
            .search_replace_executor(json!({
                "path": file_path.to_string_lossy(),
                "search": "hello",
                "replace": "hi",
                "max_replacements": 1
            }))
            .await?;

        assert_eq!(
            response.get("replacements").and_then(|v| v.as_u64()),
            Some(1)
        );
        let updated = fs::read_to_string(&file_path)
            .await
            .expect("read updated file");
        assert!(updated.contains("hi"));
        assert!(updated.contains("hello again"));

        let backup_exists = file_path
            .with_extension("txt.bak")
            .try_exists()
            .unwrap_or(false);
        assert!(backup_exists);

        Ok(())
    }
}
