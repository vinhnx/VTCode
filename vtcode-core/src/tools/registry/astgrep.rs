use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::PathBuf;

use super::ToolRegistry;
use super::utils;

impl ToolRegistry {
    pub(super) async fn execute_ast_grep(&self, args: Value) -> Result<Value> {
        let engine = self
            .ast_grep_engine()
            .ok_or_else(|| anyhow!("AST-grep engine not available"))?;

        let operation = args
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("search");

        let mut out = match operation {
            "search" => {
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

                engine
                    .search(pattern, &path, language, context_lines, max_results)
                    .await
            }
            "transform" => {
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

                engine
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
            "lint" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .context("'path' is required")?;

                let path = self.normalize_path(path)?;

                let language = args.get("language").and_then(|v| v.as_str());
                let severity_filter = args.get("severity_filter").and_then(|v| v.as_str());

                engine.lint(&path, language, severity_filter, None).await
            }
            "refactor" => {
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

                engine.refactor(&path, language, refactor_type).await
            }
            "custom" => {
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

                engine
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
                    .await
            }
            _ => Err(anyhow!("Unknown AST-grep operation: {}", operation)),
        }?;

        let fmt = args
            .get("response_format")
            .and_then(|v| v.as_str())
            .unwrap_or("concise");
        if fmt.eq_ignore_ascii_case("concise") {
            if let Some(matches) = out.get_mut("matches") {
                let concise = utils::astgrep_to_concise(matches.take());
                out["matches"] = concise;
                out["response_format"] = json!("concise");
            } else if let Some(results) = out.get_mut("results") {
                let concise = utils::astgrep_to_concise(results.take());
                out["results"] = concise;
                out["response_format"] = json!("concise");
            } else if let Some(issues) = out.get_mut("issues") {
                let concise = utils::astgrep_issues_to_concise(issues.take());
                out["issues"] = concise;
                out["response_format"] = json!("concise");
            } else if let Some(suggestions) = out.get_mut("suggestions") {
                let concise = utils::astgrep_changes_to_concise(suggestions.take());
                out["suggestions"] = concise;
                out["response_format"] = json!("concise");
            } else if let Some(changes) = out.get_mut("changes") {
                let concise = utils::astgrep_changes_to_concise(changes.take());
                out["changes"] = concise;
                out["response_format"] = json!("concise");
            }
        } else {
            out["response_format"] = json!("detailed");
        }

        Ok(out)
    }

    pub(super) fn normalize_path(&self, path: &str) -> Result<String> {
        let path_buf = PathBuf::from(path);

        if path_buf.is_absolute() {
            if !path_buf.starts_with(self.workspace_root()) {
                return Err(anyhow!(
                    "Path {} is outside workspace root {}",
                    path,
                    self.workspace_root().display()
                ));
            }
            Ok(path.to_string())
        } else {
            let resolved = self.workspace_root().join(path);
            Ok(resolved.to_string_lossy().to_string())
        }
    }
}
