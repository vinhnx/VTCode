//! AST-grep tool implementation for VTCode
//!
//! This module provides a tool interface for the AST-grep engine,
//! allowing it to be used as a standard agent tool.

use super::ast_grep::{AstGrepEngine, AstGrepSearchOutput};
use super::traits::Tool;
use crate::config::constants::tools;
use crate::tools::ast_grep_format::{extract_matches_with_metadata, matches_to_concise};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

/// AST-grep tool that provides syntax-aware code search and transformation
pub struct AstGrepTool {
    /// The underlying AST-grep engine
    engine: Arc<AstGrepEngine>,
    /// Workspace root for path resolution
    workspace_root: PathBuf,
}

impl AstGrepTool {
    /// Create a new AST-grep tool
    pub fn new(workspace_root: PathBuf) -> Result<Self> {
        let engine =
            Arc::new(AstGrepEngine::new().context("Failed to initialize AST-grep engine")?);

        Ok(Self {
            engine,
            workspace_root,
        })
    }

    /// Get the workspace root
    pub fn workspace_root(&self) -> &PathBuf {
        &self.workspace_root
    }

    /// Validate and normalize a path relative to workspace
    fn normalize_path(&self, path: &str) -> Result<String> {
        let path_buf = PathBuf::from(path);

        // If path is absolute, check if it's within workspace
        if path_buf.is_absolute() {
            if !path_buf.starts_with(&self.workspace_root) {
                return Err(anyhow::anyhow!(
                    "Path {} is outside workspace root {}",
                    path,
                    self.workspace_root.display()
                ));
            }
            Ok(path.to_string())
        } else {
            // Relative path - resolve relative to workspace
            let resolved = self.workspace_root.join(path);
            Ok(resolved.to_string_lossy().to_string())
        }
    }
}

#[async_trait]
impl Tool for AstGrepTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let operation = args
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("search");

        match operation {
            "search" => self.search(args).await,
            "transform" => self.transform(args).await,
            "lint" => self.lint(args).await,
            "refactor" => self.refactor(args).await,
            "custom" => self.custom(args).await,
            _ => Err(anyhow::anyhow!("Unknown AST-grep operation: {}", operation)),
        }
    }

    fn name(&self) -> &'static str {
        tools::AST_GREP_SEARCH
    }

    fn description(&self) -> &'static str {
        "Advanced syntax-aware code search, transformation, and analysis using AST-grep patterns"
    }

    fn validate_args(&self, args: &Value) -> Result<()> {
        if let Some(operation) = args.get("operation").and_then(|v| v.as_str()) {
            match operation {
                "search" => {
                    if args.get("pattern").is_none() {
                        return Err(anyhow::anyhow!(
                            "'pattern' is required for search operation"
                        ));
                    }
                    if args.get("path").is_none() {
                        return Err(anyhow::anyhow!("'path' is required for search operation"));
                    }
                }
                "transform" => {
                    if args.get("pattern").is_none() {
                        return Err(anyhow::anyhow!(
                            "'pattern' is required for transform operation"
                        ));
                    }
                    if args.get("replacement").is_none() {
                        return Err(anyhow::anyhow!(
                            "'replacement' is required for transform operation"
                        ));
                    }
                    if args.get("path").is_none() {
                        return Err(anyhow::anyhow!(
                            "'path' is required for transform operation"
                        ));
                    }
                }
                "refactor" => {
                    if args.get("path").is_none() {
                        return Err(anyhow::anyhow!("'path' is required for refactor operation"));
                    }
                    if args.get("refactor_type").is_none() {
                        return Err(anyhow::anyhow!(
                            "'refactor_type' is required for refactor operation"
                        ));
                    }
                }
                _ => {} // Other operations may have different requirements
            }
        }

        Ok(())
    }
}

impl AstGrepTool {
    /// Execute search operation
    async fn search(&self, args: Value) -> Result<Value> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .context("'pattern' is required")?;

        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("'path' is required")?;

        let path = self.normalize_path(path)?;

        let language = args.get("language").and_then(|v| v.as_str());
        let context_lines = args
            .get("context_lines")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        if let Some(limit) = max_results
            && limit == 0
        {
            return Err(anyhow::anyhow!("'max_results' must be greater than zero"));
        }

        let response_format = args
            .get("response_format")
            .and_then(|v| v.as_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| "concise".to_string());

        let format = match response_format.as_str() {
            "concise" => ResponseFormat::Concise,
            "detailed" => ResponseFormat::Detailed,
            other => {
                return Err(anyhow::anyhow!(
                    "Unsupported 'response_format': {}. Use 'concise' or 'detailed'",
                    other
                ));
            }
        };

        let AstGrepSearchOutput {
            matches,
            truncated,
            limit,
        } = self
            .engine
            .search(pattern, &path, language, context_lines, max_results)
            .await?;

        let match_count = matches.len();

        let formatted_matches = match format {
            ResponseFormat::Concise => {
                Value::Array(matches_to_concise(&matches, &self.workspace_root))
            }
            ResponseFormat::Detailed => Value::Array(matches.clone()),
        };

        let mut body = json!({
            "success": true,
            "matches": formatted_matches,
            "mode": "search",
            "response_format": response_format,
            "match_count": match_count,
        });

        if truncated {
            body["truncated"] = Value::Bool(true);
            body["message"] = json!(format!(
                "Showing {} matches (limit {}). Narrow the query or raise 'max_results' to see more.",
                match_count, limit
            ));
        }

        Ok(body)
    }

    /// Execute transform operation
    async fn transform(&self, args: Value) -> Result<Value> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .context("'pattern' is required")?;

        let replacement = args
            .get("replacement")
            .and_then(|v| v.as_str())
            .context("'replacement' is required")?;

        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("'path' is required")?;

        let path = self.normalize_path(path)?;

        let language = args.get("language").and_then(|v| v.as_str());
        let preview_only = args
            .get("preview_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let update_all = args
            .get("update_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        self.engine
            .transform(
                pattern,
                replacement,
                &path,
                language,
                preview_only,
                update_all,
            )
            .await
    }

    /// Execute lint operation
    async fn lint(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("'path' is required")?;

        let path = self.normalize_path(path)?;

        let language = args.get("language").and_then(|v| v.as_str());
        let severity_filter = args.get("severity_filter").and_then(|v| v.as_str());

        self.engine
            .lint(&path, language, severity_filter, None)
            .await
    }

    /// Execute refactor operation
    async fn refactor(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("'path' is required")?;

        let path = self.normalize_path(path)?;

        let language = args.get("language").and_then(|v| v.as_str());
        let refactor_type = args
            .get("refactor_type")
            .and_then(|v| v.as_str())
            .context("'refactor_type' is required")?;

        self.engine.refactor(&path, language, refactor_type).await
    }

    /// Execute custom operation
    async fn custom(&self, args: Value) -> Result<Value> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .context("'pattern' is required")?;

        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .context("'path' is required")?;

        let path = self.normalize_path(path)?;

        let language = args.get("language").and_then(|v| v.as_str());
        let rewrite = args.get("rewrite").and_then(|v| v.as_str());
        let context_lines = args
            .get("context_lines")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        let interactive = args
            .get("interactive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let update_all = args
            .get("update_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if let Some(limit) = max_results
            && limit == 0
        {
            return Err(anyhow::anyhow!("'max_results' must be greater than zero"));
        }

        let response_format = args
            .get("response_format")
            .and_then(|v| v.as_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| "concise".to_string());

        let format = match response_format.as_str() {
            "concise" => ResponseFormat::Concise,
            "detailed" => ResponseFormat::Detailed,
            other => {
                return Err(anyhow::anyhow!(
                    "Unsupported 'response_format': {}. Use 'concise' or 'detailed'",
                    other
                ));
            }
        };

        let result = self
            .engine
            .run_custom(
                pattern,
                &path,
                language,
                rewrite,
                context_lines,
                max_results,
                interactive,
                update_all,
            )
            .await?;

        let (matches, metadata) = extract_matches_with_metadata(result.get("results"));

        let match_count = matches.len();

        let formatted_matches = match format {
            ResponseFormat::Concise => {
                Value::Array(matches_to_concise(&matches, &self.workspace_root))
            }
            ResponseFormat::Detailed => Value::Array(matches.clone()),
        };

        let mut body = json!({
            "success": true,
            "matches": formatted_matches,
            "mode": "custom",
            "response_format": response_format,
            "match_count": match_count,
        });

        if let Value::Object(body_obj) = &mut body {
            for (key, value) in metadata {
                body_obj.entry(key).or_insert(value);
            }
        }

        Ok(body)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResponseFormat {
    Concise,
    Detailed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ast_grep_format::matches_to_concise;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn test_transform_ast_grep_matches_to_concise() {
        let workspace = PathBuf::from("/home/user/project");
        let raw_matches = vec![json!({
            "file": "/home/user/project/src/lib.rs",
            "lines": "fn main() { println!(\"hi\"); }\n",
            "range": {
                "start": {"line": 4, "column": 0},
                "end": {"line": 4, "column": 25},
                "byteOffset": {"start": 0, "end": 25}
            }
        })];

        let concise = matches_to_concise(&raw_matches, &workspace);
        assert_eq!(concise.len(), 1);
        assert_eq!(concise[0]["path"], "src/lib.rs");
        assert_eq!(concise[0]["line_number"], 5);
        assert_eq!(concise[0]["text"], "fn main() { println!(\"hi\"); }");
    }
}
