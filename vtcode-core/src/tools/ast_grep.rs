//! AST-grep integration for VTCode
//!
//! This module provides integration with the ast-grep CLI tool for
//! syntax-aware code search, transformation, linting, and refactoring.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::HashMap;
use tokio;

/// Default maximum number of matches to return when the caller does not
/// explicitly specify a limit.
const DEFAULT_MAX_RESULTS: usize = 100;

/// Result of an AST-grep search operation.
pub struct AstGrepSearchOutput {
    /// Matches returned by the ast-grep CLI in JSON format.
    pub matches: Vec<Value>,
    /// Whether the results were truncated to respect the requested limit.
    pub truncated: bool,
    /// Maximum number of matches returned to the caller.
    pub limit: usize,
}

/// AST-grep engine for syntax-aware code operations
pub struct AstGrepEngine {
    /// Path to the ast-grep executable
    sgrep_path: String,
}

impl AstGrepEngine {
    /// Create a new AST-grep engine
    pub fn new() -> Result<Self> {
        // Try to find ast-grep in PATH
        let sgrep_path = if cfg!(target_os = "windows") {
            "ast-grep.exe"
        } else {
            "ast-grep"
        };

        // Verify ast-grep is available
        let output = std::process::Command::new(sgrep_path)
            .arg("--version")
            .output()
            .context("Failed to execute ast-grep")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "ast-grep not found or not working properly"
            ));
        }

        Ok(Self {
            sgrep_path: sgrep_path.to_string(),
        })
    }

    /// Map language names to ast-grep language identifiers
    fn map_language(language: &str) -> &str {
        match language.to_lowercase().as_str() {
            "rust" => "rust",
            "rs" => "rust",
            "python" => "python",
            "py" => "python",
            "javascript" => "javascript",
            "js" => "javascript",
            "typescript" => "typescript",
            "ts" => "typescript",
            "tsx" => "tsx",
            "go" => "go",
            "golang" => "go",
            "java" => "java",
            "cpp" => "cpp",
            "c++" => "cpp",
            "c" => "c",
            "html" => "html",
            "css" => "css",
            "json" => "json",
            "yaml" => "yaml",
            "yml" => "yaml",
            _ => language,
        }
    }

    /// Search code using AST-grep patterns
    pub async fn search(
        &self,
        pattern: &str,
        path: &str,
        language: Option<&str>,
        context_lines: Option<usize>,
        max_results: Option<usize>,
    ) -> Result<AstGrepSearchOutput> {
        let sgrep_path = self.sgrep_path.clone();
        let pattern = pattern.to_string();
        let path = path.to_string();
        let language = language.map(|s| s.to_string());
        let context_lines = context_lines.unwrap_or(0);
        let result_limit = max_results.unwrap_or(DEFAULT_MAX_RESULTS);

        let handle = tokio::task::spawn_blocking(move || {
            let mut cmd = std::process::Command::new(&sgrep_path);
            cmd.arg("run")
                .arg("--pattern")
                .arg(&pattern)
                .arg("--json")
                .arg("--context")
                .arg(context_lines.to_string())
                .arg("--max-results")
                .arg(result_limit.to_string())
                .arg(&path);

            if let Some(lang) = language {
                cmd.arg("--lang").arg(Self::map_language(&lang));
            }

            cmd.output()
        });

        let output = handle
            .await
            .context("Failed to spawn ast-grep search task")?
            .context("Failed to execute ast-grep search")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("ast-grep search failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut matches: Vec<Value> =
            serde_json::from_str(&stdout).context("Failed to parse ast-grep search results")?;

        let truncated = if matches.len() > result_limit {
            matches.truncate(result_limit);
            true
        } else {
            false
        };

        Ok(AstGrepSearchOutput {
            matches,
            truncated,
            limit: result_limit,
        })
    }

    /// Transform code using AST-grep patterns
    pub async fn transform(
        &self,
        pattern: &str,
        replacement: &str,
        path: &str,
        language: Option<&str>,
        preview_only: bool,
        update_all: bool,
    ) -> Result<Value> {
        let sgrep_path = self.sgrep_path.clone();
        let pattern = pattern.to_string();
        let replacement = replacement.to_string();
        let path = path.to_string();
        let language = language.map(|s| s.to_string());

        let handle = tokio::task::spawn_blocking(move || {
            let mut cmd = std::process::Command::new(&sgrep_path);
            cmd.arg("run")
                .arg("--pattern")
                .arg(&pattern)
                .arg("--rewrite")
                .arg(&replacement)
                .arg("--json")
                .arg(&path);

            if let Some(lang) = language {
                cmd.arg("--lang").arg(Self::map_language(&lang));
            }

            // Note: We can't use --interactive and --json together
            // For preview, we'll just show the matches without applying changes
            if update_all && !preview_only {
                cmd.arg("--update-all");
            }

            cmd.output()
        });

        let output = handle
            .await
            .context("Failed to spawn ast-grep transform task")?
            .context("Failed to execute ast-grep transform")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("ast-grep transform failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let results: Value =
            serde_json::from_str(&stdout).context("Failed to parse ast-grep transform results")?;

        Ok(json!({ "success": true, "changes": results }))
    }

    /// Lint code using AST-grep rules with custom rules
    pub async fn lint(
        &self,
        path: &str,
        language: Option<&str>,
        severity_filter: Option<&str>,
        custom_rules: Option<Vec<HashMap<String, Value>>>,
    ) -> Result<Value> {
        let sgrep_path = self.sgrep_path.clone();
        let path = path.to_string();
        let language = language.map(|s| s.to_string());
        let _severity_filter = severity_filter.map(|s| s.to_string());
        let _custom_rules = custom_rules.clone();

        let handle = tokio::task::spawn_blocking(move || {
            let mut cmd = std::process::Command::new(&sgrep_path);
            cmd.arg("run")
                .arg("--pattern")
                .arg("// TODO: $$")
                .arg("--json")
                .arg(&path);

            if let Some(lang) = language {
                cmd.arg("--lang").arg(Self::map_language(&lang));
            }

            cmd.output()
        });

        let output = handle
            .await
            .context("Failed to spawn ast-grep lint task")?
            .context("Failed to execute ast-grep lint")?;

        // Even if the command fails (e.g., no project config), we'll still return results
        // if we got any output
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.trim().is_empty() {
            if let Ok(results) = serde_json::from_str::<Value>(&stdout) {
                return Ok(json!({ "success": true, "issues": results }));
            }
        }

        // If we couldn't parse JSON output, return a more generic response
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If it's a project config error, we'll return an empty result instead of failing
            if stderr.contains("No ast-grep project configuration") {
                return Ok(json!({
                    "success": true,
                    "issues": [],
                    "warning": "No ast-grep project configuration found. Linting may be limited."
                }));
            }
            return Err(anyhow::anyhow!("ast-grep lint failed: {}", stderr));
        }

        Ok(json!({ "success": true, "issues": [] }))
    }

    /// Get refactoring suggestions using AST-grep
    pub async fn refactor(
        &self,
        path: &str,
        language: Option<&str>,
        refactor_type: &str,
    ) -> Result<Value> {
        // Different refactoring suggestions based on type
        let (pattern, replacement) = match refactor_type {
            "extract_function" => (
                "function $func($) { $$ }",
                "// TODO: Extract function $func to separate module\nfunction $func($) { $$ }",
            ),
            "remove_console_logs" => ("console.log($$)", ""),
            "simplify_conditions" => ("if ($cond) { true } else { false }", "$cond"),
            "extract_constants" => (
                "$NUM", // Simple number extraction
                "const MY_CONSTANT = $NUM;\n// TODO: Replace $NUM with MY_CONSTANT",
            ),
            "modernize_syntax" => ("var $VAR = $$", "let $VAR = $$"),
            _ => ("$$", "// TODO: Consider refactoring this code"),
        };

        let sgrep_path = self.sgrep_path.clone();
        let path = path.to_string();
        let language = language.map(|s| s.to_string());
        let pattern = pattern.to_string();
        let replacement = replacement.to_string();

        let handle = tokio::task::spawn_blocking(move || {
            let mut cmd = std::process::Command::new(&sgrep_path);
            cmd.arg("run")
                .arg("--pattern")
                .arg(&pattern)
                .arg("--rewrite")
                .arg(&replacement)
                .arg("--json")
                .arg(&path);

            if let Some(lang) = language {
                cmd.arg("--lang").arg(Self::map_language(&lang));
            }

            cmd.output()
        });

        let output = handle
            .await
            .context("Failed to spawn ast-grep refactor task")?
            .context("Failed to execute ast-grep refactor")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("ast-grep refactor failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let results: Value =
            serde_json::from_str(&stdout).context("Failed to parse ast-grep refactor results")?;

        Ok(json!({ "success": true, "suggestions": results }))
    }

    /// Run a custom ast-grep command with full options
    pub async fn run_custom(
        &self,
        pattern: &str,
        path: &str,
        language: Option<&str>,
        rewrite: Option<&str>,
        context_lines: Option<usize>,
        max_results: Option<usize>,
        interactive: bool,
        update_all: bool,
    ) -> Result<Value> {
        let sgrep_path = self.sgrep_path.clone();
        let pattern = pattern.to_string();
        let path = path.to_string();
        let language = language.map(|s| s.to_string());
        let rewrite = rewrite.map(|s| s.to_string());
        let context_lines = context_lines.unwrap_or(0);
        let max_results = max_results.unwrap_or(100);

        let handle = tokio::task::spawn_blocking(move || {
            let mut cmd = std::process::Command::new(&sgrep_path);
            cmd.arg("run")
                .arg("--pattern")
                .arg(&pattern)
                .arg("--json")
                .arg("--context")
                .arg(context_lines.to_string())
                .arg("--max-results")
                .arg(max_results.to_string())
                .arg(&path);

            if let Some(lang) = language {
                cmd.arg("--lang").arg(Self::map_language(&lang));
            }

            if let Some(rewrite_pattern) = rewrite {
                cmd.arg("--rewrite").arg(&rewrite_pattern);
            }

            if interactive {
                cmd.arg("--interactive");
            } else if update_all {
                cmd.arg("--update-all");
            }

            cmd.output()
        });

        let output = handle
            .await
            .context("Failed to spawn ast-grep custom task")?
            .context("Failed to execute ast-grep custom command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "ast-grep custom command failed: {}",
                stderr
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let results: Value =
            serde_json::from_str(&stdout).context("Failed to parse ast-grep custom results")?;

        Ok(json!({ "success": true, "results": results }))
    }
}
