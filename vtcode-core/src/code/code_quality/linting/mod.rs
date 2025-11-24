pub mod clippy;
pub mod eslint;
pub mod pylint;

use crate::code::code_quality::config::{LintConfig, LintSeverity};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
// use anyhow::Result;

/// Individual lint finding
#[derive(Debug, Clone)]
pub struct LintFinding {
    pub file_path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub severity: LintSeverity,
    pub rule: String,
    pub message: String,
    pub suggestion: Option<String>,
}

/// Result of linting operation
#[derive(Debug, Clone)]
pub struct LintResult {
    pub success: bool,
    pub findings: Vec<LintFinding>,
    pub error_message: Option<String>,
    pub tool_used: String,
}

/// Linting orchestrator that manages multiple linters
pub struct LintingOrchestrator {
    configs: Vec<LintConfig>,
}

impl Default for LintingOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl LintingOrchestrator {
    pub fn new() -> Self {
        let mut orchestrator = Self {
            configs: Vec::new(),
        };

        // Register default linters
        orchestrator.register(LintConfig::clippy());
        orchestrator.register(LintConfig::eslint());
        orchestrator.register(LintConfig::pylint());

        orchestrator
    }

    /// Register a linting configuration
    pub fn register(&mut self, config: LintConfig) {
        self.configs.push(config);
    }

    /// Lint a file or directory
    pub async fn lint_path(&self, path: &Path) -> Vec<LintResult> {
        let mut results = Vec::new();

        for config in &self.configs {
            if config.enabled
                && let Some(result) = self.run_linter(config, path).await
            {
                results.push(result);
            }
        }

        results
    }

    async fn run_linter(&self, config: &LintConfig, path: &Path) -> Option<LintResult> {
        // Execute the actual linting tool
        let mut cmd = Command::new(&config.command[0]);

        // Add arguments
        for arg in &config.args {
            cmd.arg(arg);
        }

        // Add the path as the last argument
        cmd.arg(path);

        match cmd.output() {
            Ok(output) => {
                if output.status.success() {
                    // Parse the lint output based on the tool
                    let findings = self.parse_lint_output(config, &output.stdout, path);

                    Some(LintResult {
                        success: true,
                        findings,
                        error_message: None,
                        tool_used: config.tool_name.clone(),
                    })
                } else {
                    let error_msg = String::from_utf8_lossy(&output.stderr).to_string();
                    Some(LintResult {
                        success: false,
                        findings: Vec::new(),
                        error_message: Some(error_msg),
                        tool_used: config.tool_name.clone(),
                    })
                }
            }
            Err(e) => Some(LintResult {
                success: false,
                findings: Vec::new(),
                error_message: Some(format!("Failed to execute {}: {}", config.tool_name, e)),
                tool_used: config.tool_name.clone(),
            }),
        }
    }

    fn parse_lint_output(
        &self,
        config: &LintConfig,
        output: &[u8],
        base_path: &Path,
    ) -> Vec<LintFinding> {
        let output_str = String::from_utf8_lossy(output);

        // Parse based on the tool used
        match config.tool_name.as_str() {
            "clippy" => self.parse_clippy_output(&output_str, base_path),
            "eslint" => self.parse_eslint_output(&output_str, base_path),
            "pylint" => self.parse_pylint_output(&output_str, base_path),
            _ => Vec::new(), // Unknown tool, return empty findings
        }
    }

    fn parse_clippy_output(&self, output: &str, base_path: &Path) -> Vec<LintFinding> {
        let mut findings = Vec::new();
        for line in output.lines() {

            if let Ok(json) = serde_json::from_str::<Value>(line)
                && json.get("reason").and_then(Value::as_str) == Some("compiler-message")
                && let Some(message) = json.get("message")
                && let Some(spans) = message.get("spans").and_then(Value::as_array)
            {
                for span in spans {
                    if span.get("is_primary").and_then(Value::as_bool) == Some(true) {
                                let file =
                                    span.get("file_name").and_then(Value::as_str).unwrap_or("");
                                let line_num =
                                    span.get("line_start").and_then(Value::as_u64).unwrap_or(0);
                                let column = span
                                    .get("column_start")
                                    .and_then(Value::as_u64)
                                    .unwrap_or(0);
                                let rule = message
                                    .get("code")
                                    .and_then(|c| c.get("code"))
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string();
                                let severity = match message
                                    .get("level")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                {
                                    "error" => LintSeverity::Error,
                                    "warning" => LintSeverity::Warning,
                                    _ => LintSeverity::Info,
                                };
                                let msg = message
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string();
                                findings.push(LintFinding {
                                    file_path: base_path.join(file),
                                    line: line_num as usize,
                                    column: column as usize,
                                    severity,
                                    rule,
                                    message: msg,
                                    suggestion: None,
                                });
                            }
                        }

            }
        }
        findings
    }

    fn parse_eslint_output(&self, output: &str, base_path: &Path) -> Vec<LintFinding> {
        let mut findings = Vec::new();
        if let Ok(json) = serde_json::from_str::<Value>(output)
            && let Some(arr) = json.as_array()
        {
            for file in arr {
                let path = file.get("filePath").and_then(Value::as_str).unwrap_or("");
                if let Some(messages) = file.get("messages").and_then(Value::as_array) {
                    for m in messages {
                        let line = m.get("line").and_then(Value::as_u64).unwrap_or(0);
                        let column = m.get("column").and_then(Value::as_u64).unwrap_or(0);
                        let rule = m
                            .get("ruleId")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let severity = match m.get("severity").and_then(Value::as_u64).unwrap_or(0)
                        {
                            2 => LintSeverity::Error,
                            1 => LintSeverity::Warning,
                            _ => LintSeverity::Info,
                        };
                        let msg = m
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        findings.push(LintFinding {
                            file_path: base_path.join(path),
                            line: line as usize,
                            column: column as usize,
                            severity,
                            rule,
                            message: msg,
                            suggestion: m.get("fix").map(|_| "fix available".to_string()),
                        });
                    }
                }
            }
        }
        findings
    }

    fn parse_pylint_output(&self, output: &str, base_path: &Path) -> Vec<LintFinding> {
        let mut findings = Vec::new();
        if let Ok(json) = serde_json::from_str::<Value>(output)
            && let Some(arr) = json.as_array()
        {
            for item in arr {
                let path = item.get("path").and_then(Value::as_str).unwrap_or("");
                let line = item.get("line").and_then(Value::as_u64).unwrap_or(0);
                let column = item.get("column").and_then(Value::as_u64).unwrap_or(0);
                let rule = item
                    .get("symbol")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let msg = item
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let severity = match item.get("type").and_then(Value::as_str).unwrap_or("") {
                    "error" | "fatal" => LintSeverity::Error,
                    "warning" => LintSeverity::Warning,
                    _ => LintSeverity::Info,
                };
                findings.push(LintFinding {
                    file_path: base_path.join(path),
                    line: line as usize,
                    column: column as usize,
                    severity,
                    rule,
                    message: msg,
                    suggestion: None,
                });
            }
        }
        findings
    }
}
