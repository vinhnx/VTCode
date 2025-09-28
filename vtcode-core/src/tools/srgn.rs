//! Srgn (code surgeon) tool integration for VTCode
//!
//! This tool provides access to srgn, a grep-like tool that understands source code
//! syntax and allows for manipulation in addition to search. It supports various
//! programming languages and provides precise code modification capabilities.
//!
//! ## Supported Languages and Prepared Queries
//!
//! ### Rust
//! - `comments` - Comments (line and block styles; excluding doc comments)
//! - `doc-comments` - Doc comments (comment chars included)
//! - `uses` - Use statements (paths only; excl. `use`/`as`/`*`)
//! - `strings` - Strings (regular, raw, byte; includes interpolation parts)
//! - `attribute` - Attributes like `#[attr]`
//! - `struct` - `struct` definitions
//! - `struct~<PATTERN>` - Structs whose name matches PATTERN
//! - `enum` - `enum` definitions
//! - `enum~<PATTERN>` - Enums whose name matches PATTERN
//! - `fn` - Function definitions
//! - `fn~<PATTERN>` - Functions whose name matches PATTERN
//! - `unsafe` - `unsafe` keyword usages
//!
//! ### Python
//! - `comments` - Comments
//! - `strings` - Strings (raw, byte, f-strings; interpolation not included)
//! - `imports` - Module names in imports
//! - `doc-strings` - Docstrings
//! - `function-names` - Function names at definition site
//! - `function-calls` - Function calls
//! - `class` - Class definitions
//! - `def` - All function definitions
//! - `methods` - Function definitions inside classes
//!
//! ### JavaScript/TypeScript
//! - `comments` - Comments
//! - `strings` - Strings (literal, template)
//! - `imports` - Imports (module specifiers)
//! - `function` - Function definitions
//! - `class` - Class definitions
//! - `interface` - Interface definitions
//!
//! ### Go
//! - `comments` - Comments
//! - `strings` - Strings (interpreted and raw)
//! - `imports` - Imports
//! - `struct` - Struct type definitions
//! - `struct~<PATTERN>` - Structs whose name matches PATTERN
//! - `func` - Function definitions
//! - `func~<PATTERN>` - Functions whose name matches PATTERN
//!
//! ### C/C++/C#
//! - `comments` - Comments
//! - `strings` - Strings
//! - `function` - Function definitions
//! - `struct` - Struct definitions
//! - `class` - Class definitions
//!
//! ### HCL (Terraform)
//! - `comments` - Comments
//! - `strings` - Literal strings
//! - `variable` - Variable blocks
//! - `resource` - Resource blocks
//! - `data` - Data blocks
//!
//! ## Usage Examples
//!
//! ```rust
//! // Replace println with eprintln in Rust functions
//! SrgnInput {
//!     path: "*.rs".to_string(),
//!     language_scope: Some("rust fn".to_string()),
//!     scope: Some("println".to_string()),
//!     replacement: Some("eprintln".to_string()),
//!     action: SrgnAction::Replace,
//!     dry_run: true,
//!     ..Default::default()
//! }
//!
//! // Find all unsafe Rust code
//! SrgnInput {
//!     path: "*.rs".to_string(),
//!     language_scope: Some("rust unsafe".to_string()),
//!     fail_any: true,
//!     ..Default::default()
//! }
//! ```

use super::traits::{FileTool, Tool};
use crate::utils::vtcodegitignore::should_exclude_file;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::SystemTime;
use tokio::process::Command;

/// Input structure for srgn operations
#[derive(Debug, Deserialize)]
pub struct SrgnInput {
    /// File path or glob pattern to operate on
    pub path: String,
    /// Scope pattern (regex or literal string)
    pub scope: Option<String>,
    /// Replacement string (for replace operations)
    pub replacement: Option<String>,
    /// Language-specific scope (e.g., "rust fn", "python class")
    pub language_scope: Option<String>,
    /// Action to perform
    pub action: SrgnAction,
    /// Whether to use literal string matching instead of regex
    #[serde(default)]
    pub literal_string: bool,
    /// Whether to perform a dry run (show changes without applying)
    #[serde(default)]
    pub dry_run: bool,
    /// Whether to invert the operation (where applicable)
    #[serde(default)]
    pub invert: bool,
    /// Custom tree-sitter query (for advanced users)
    pub custom_query: Option<String>,
    /// Custom tree-sitter query from file
    pub custom_query_file: Option<String>,
    /// Additional srgn flags
    pub flags: Option<Vec<String>>,
    /// Fail if anything matches (for linting/checking)
    #[serde(default)]
    pub fail_any: bool,
    /// Fail if nothing matches
    #[serde(default)]
    pub fail_none: bool,
    /// Join multiple language scopes with OR instead of AND
    #[serde(default)]
    pub join_language_scopes: bool,
    /// Ignore hidden files and directories
    #[serde(default)]
    pub hidden: bool,
    /// Don't ignore .gitignored files
    #[serde(default)]
    pub gitignored: bool,
    /// Process files in sorted order
    #[serde(default)]
    pub sorted: bool,
    /// Number of threads to use (0 = auto)
    pub threads: Option<usize>,
    /// Whether to fail if no files are found
    #[serde(default)]
    pub fail_no_files: bool,
    /// German-specific options
    pub german_options: Option<GermanOptions>,
}

/// German-specific options for srgn
#[derive(Debug, Deserialize)]
pub struct GermanOptions {
    /// Prefer original spelling when multiple are valid
    #[serde(default)]
    pub prefer_original: bool,
    /// Use naive replacement (don't check word validity)
    #[serde(default)]
    pub naive: bool,
}

/// Available srgn actions
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SrgnAction {
    /// Replace content in scope
    Replace,
    /// Delete content in scope
    Delete,
    /// Convert to uppercase
    Upper,
    /// Convert to lowercase
    Lower,
    /// Convert to titlecase
    Titlecase,
    /// Normalize Unicode
    Normalize,
    /// German umlaut substitutions
    German,
    /// Symbol substitutions (ASCII art to Unicode)
    Symbols,
    /// Squeeze consecutive occurrences
    Squeeze,
}

/// Srgn tool implementation
#[derive(Clone)]
pub struct SrgnTool {
    workspace_root: PathBuf,
}

impl SrgnTool {
    /// Create a new SrgnTool instance
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    /// Build srgn command arguments from input
    fn build_command_args(&self, input: &SrgnInput) -> Result<Vec<String>> {
        let mut args = Vec::new();

        // Add global flags first
        if input.dry_run {
            args.push("--dry-run".to_string());
        }

        if input.invert {
            args.push("--invert".to_string());
        }

        if input.fail_any {
            args.push("--fail-any".to_string());
        }

        if input.fail_none {
            args.push("--fail-none".to_string());
        }

        if input.join_language_scopes {
            args.push("--join-language-scopes".to_string());
        }

        if input.hidden {
            args.push("--hidden".to_string());
        }

        if input.gitignored {
            args.push("--gitignored".to_string());
        }

        if input.sorted {
            args.push("--sorted".to_string());
        }

        if input.fail_no_files {
            args.push("--fail-no-files".to_string());
        }

        if let Some(threads) = input.threads
            && threads > 0
        {
            args.push("--threads".to_string());
            args.push(threads.to_string());
        }

        // Add German-specific options
        if let Some(german_opts) = &input.german_options {
            if german_opts.prefer_original {
                args.push("--german-prefer-original".to_string());
            }
            if german_opts.naive {
                args.push("--german-naive".to_string());
            }
        }

        // Add file path/glob
        args.push("--glob".to_string());
        args.push(input.path.clone());

        // Handle different input combinations for scope
        match (
            &input.scope,
            &input.language_scope,
            &input.custom_query,
            &input.custom_query_file,
        ) {
            // Custom query from file takes highest precedence
            (_, _, _, Some(query_file)) => {
                // Determine language from scope or default to rust
                let lang = if let Some(lang_scope) = &input.language_scope {
                    let parts: Vec<String> = lang_scope
                        .split_whitespace()
                        .map(|s| s.to_string())
                        .collect();
                    parts.first().unwrap_or(&"rust".to_string()).clone()
                } else {
                    "rust".to_string()
                };

                let query_flag = match lang.as_str() {
                    "rust" | "rs" => "--rust-query-file",
                    "python" | "py" => "--python-query-file",
                    "javascript" | "js" | "typescript" | "ts" => "--typescript-query-file",
                    "go" => "--go-query-file",
                    "c" => "--c-query-file",
                    "csharp" | "cs" | "c#" => "--csharp-query-file",
                    "hcl" => "--hcl-query-file",
                    _ => {
                        return Err(anyhow!(
                            "Unsupported language for custom query file: {}",
                            lang
                        ));
                    }
                };

                args.push(query_flag.to_string());
                args.push(query_file.clone());
            }
            // Custom query takes precedence
            (_, _, Some(query), None) => {
                // Determine language from scope or default to rust
                let lang = if let Some(lang_scope) = &input.language_scope {
                    let parts: Vec<String> = lang_scope
                        .split_whitespace()
                        .map(|s| s.to_string())
                        .collect();
                    parts.first().unwrap_or(&"rust".to_string()).clone()
                } else {
                    "rust".to_string()
                };

                let query_flag = match lang.as_str() {
                    "rust" | "rs" => "--rust-query",
                    "python" | "py" => "--python-query",
                    "javascript" | "js" | "typescript" | "ts" => "--typescript-query",
                    "go" => "--go-query",
                    "c" => "--c-query",
                    "csharp" | "cs" | "c#" => "--csharp-query",
                    "hcl" => "--hcl-query",
                    _ => return Err(anyhow!("Unsupported language for custom query: {}", lang)),
                };

                args.push(query_flag.to_string());
                args.push(query.clone());
            }
            // Language scope takes precedence
            (_, Some(lang_scope), None, None) => {
                // Parse language and scope (e.g., "rust fn", "python class", "go struct~Test")
                let parts: Vec<&str> = lang_scope.split_whitespace().collect();
                if parts.len() >= 2 {
                    let lang = parts[0];
                    let scope = parts[1];

                    // Map language to srgn flag
                    let lang_flag = match lang {
                        "rust" | "rs" => "--rust",
                        "python" | "py" => "--python",
                        "javascript" | "js" => "--typescript", // srgn uses typescript for js
                        "typescript" | "ts" => "--typescript",
                        "go" => "--go",
                        "c" => "--c",
                        "csharp" | "cs" | "c#" => "--csharp",
                        "hcl" => "--hcl",
                        _ => return Err(anyhow!("Unsupported language: {}", lang)),
                    };

                    args.push(lang_flag.to_string());
                    args.push(scope.to_string());

                    // Add additional scope parts if present (for dynamic patterns like "struct~Test")
                    // The "~" separator is used by srgn for dynamic patterns (e.g., "struct~Test" matches only structs named "Test")
                    if parts.len() > 2 {
                        for part in &parts[2..] {
                            args.push(part.to_string());
                        }
                    }
                } else {
                    return Err(anyhow!(
                        "Invalid language scope format. Expected 'language scope' or 'language scope~pattern', got: {}",
                        lang_scope
                    ));
                }
            }
            // Regular scope
            (Some(scope), None, None, None) => {
                if input.literal_string {
                    args.push("--literal-string".to_string());
                }
                args.push(scope.clone());
            }
            // No scope specified
            (None, None, None, None) => {
                // Use global scope (empty string)
                args.push(".*".to_string());
            }
        }

        // Add action-specific flags
        match &input.action {
            SrgnAction::Replace => {
                if let Some(replacement) = &input.replacement {
                    args.push("--".to_string());
                    args.push(replacement.clone());
                } else {
                    return Err(anyhow!("Replacement string required for replace action"));
                }
            }
            SrgnAction::Delete => {
                args.push("--delete".to_string());
            }
            SrgnAction::Upper => {
                args.push("--upper".to_string());
            }
            SrgnAction::Lower => {
                args.push("--lower".to_string());
            }
            SrgnAction::Titlecase => {
                args.push("--titlecase".to_string());
            }
            SrgnAction::Normalize => {
                args.push("--normalize".to_string());
            }
            SrgnAction::German => {
                args.push("--german".to_string());
            }
            SrgnAction::Symbols => {
                args.push("--symbols".to_string());
            }
            SrgnAction::Squeeze => {
                args.push("--squeeze".to_string());
            }
        }

        // Add any additional flags
        if let Some(flags) = &input.flags {
            args.extend(flags.clone());
        }

        Ok(args)
    }

    /// Sanitize and validate file path within workspace
    fn validate_path(&self, path: &str) -> Result<PathBuf> {
        let full_path = self.workspace_root.join(path);
        let canonical =
            std::fs::canonicalize(&full_path).with_context(|| format!("Invalid path: {}", path))?;
        if !canonical.starts_with(&self.workspace_root) {
            return Err(anyhow!("Path '{}' is outside workspace", path));
        }
        Ok(canonical)
    }

    /// Check if a file was modified by comparing timestamps
    fn was_file_modified(&self, path: &Path, before_time: SystemTime) -> Result<bool> {
        let metadata = std::fs::metadata(path)?;
        let modified_time = metadata.modified()?;
        Ok(modified_time > before_time)
    }

    /// Execute srgn command
    async fn execute_srgn(&self, args: &[String]) -> Result<String> {
        // For file-modifying operations, capture file paths and timestamps for verification
        let file_paths: Vec<PathBuf> = args
            .iter()
            .filter(|arg| arg.contains('.') && !arg.starts_with('-'))
            .map(|arg| self.validate_path(arg))
            .collect::<Result<Vec<_>>>()?;
        let before_times: Vec<SystemTime> = file_paths
            .iter()
            .map(|path| {
                std::fs::metadata(path)
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH)
            })
            .collect();

        let output = Command::new("srgn")
            .args(args)
            .current_dir(&self.workspace_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .with_context(|| format!("Failed to execute srgn command with args: {:?}", args))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Err(anyhow!(
                "srgn command failed with exit code {}: {}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            ));
        }

        // Verify file modifications for non-dry-run operations
        if !args.contains(&"--dry-run".to_string()) && !file_paths.is_empty() {
            for (i, path) in file_paths.iter().enumerate() {
                if !self.was_file_modified(path, before_times[i])? {
                    return Err(anyhow!(
                        "File '{}' was not modified as expected",
                        path.display()
                    ));
                }
            }
        }

        // Return combined output
        if stdout.is_empty() {
            Ok(stderr)
        } else if stderr.is_empty() {
            Ok(stdout)
        } else {
            Ok(format!("{}\n{}", stdout.trim(), stderr.trim()))
        }
    }

    /// Validate input parameters
    fn validate_input(&self, input: &SrgnInput) -> Result<()> {
        // Check if path exists or is a valid glob
        let path = self.workspace_root.join(&input.path);
        if !path.exists() && !input.path.contains('*') && !input.path.contains('?') {
            return Err(anyhow!("Path '{}' does not exist", input.path));
        }

        // Validate action-specific requirements
        match &input.action {
            SrgnAction::Replace => {
                if input.replacement.is_none() {
                    return Err(anyhow!("Replacement action requires a replacement string"));
                }
            }
            SrgnAction::Delete => {
                if input.scope.is_none() && input.language_scope.is_none() {
                    return Err(anyhow!(
                        "Delete action requires either a scope pattern or language scope"
                    ));
                }
            }
            _ => {}
        }

        Ok(())
    }
}

#[async_trait]
impl Tool for SrgnTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let input: SrgnInput = serde_json::from_value(args)
            .with_context(|| "Failed to parse SrgnInput from arguments")?;

        // Validate input
        self.validate_input(&input)?;

        // Build command arguments
        let cmd_args = self.build_command_args(&input)?;

        // Extract potential file paths for git diff confirmation
        let modified_files: Vec<String> = cmd_args
            .iter()
            .filter(|arg| arg.contains('.') && !arg.starts_with('-') && !arg.starts_with('*'))
            .cloned()
            .collect();

        // Execute srgn command
        let output = self.execute_srgn(&cmd_args).await?;

        // Return result with modified files info
        Ok(json!({
            "success": true,
            "output": output,
            "command": format!("srgn {}", cmd_args.join(" ")),
            "dry_run": input.dry_run,
            "modified_files": if input.dry_run { Vec::<String>::new() } else { modified_files }
        }))
    }

    fn name(&self) -> &'static str {
        "srgn"
    }

    fn description(&self) -> &'static str {
        "Code surgeon tool for precise source code manipulation using srgn. Supports syntax-aware search and replace operations across multiple programming languages."
    }
}

#[async_trait]
impl FileTool for SrgnTool {
    fn workspace_root(&self) -> &PathBuf {
        &self.workspace_root
    }

    async fn should_exclude(&self, path: &Path) -> bool {
        should_exclude_file(path).await
    }
}
