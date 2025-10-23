use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::PathBuf;

use super::ToolRegistry;
use super::utils;
use crate::tools::ast_grep_format::matches_to_concise;

enum ResponseFormat {
    Concise,
    Detailed,
}

impl ToolRegistry {
    pub(super) async fn execute_ast_grep(&self, args: Value) -> Result<Value> {
        let engine = self
            .ast_grep_engine()
            .ok_or_else(|| anyhow!("AST-grep engine not available"))?;

        let operation = args
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("search");

        let response_format = args
            .get("response_format")
            .and_then(|v| v.as_str())
            .unwrap_or("concise")
            .to_lowercase();

        let format = match response_format.as_str() {
            "concise" => ResponseFormat::Concise,
            "detailed" => ResponseFormat::Detailed,
            other => {
                return Err(anyhow!(
                    "Unsupported 'response_format': {}. Use 'concise' or 'detailed'",
                    other
                ));
            }
        };

        let out = match operation {
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

                if let Some(limit) = max_results
                    && limit == 0
                {
                    return Err(anyhow!("'max_results' must be greater than zero"));
                }

                let search_output = engine
                    .search(pattern, &path, language, context_lines, max_results)
                    .await?;

                let matches_value = match format {
                    ResponseFormat::Concise => Value::Array(matches_to_concise(
                        &search_output.matches,
                        self.workspace_root().as_path(),
                    )),
                    ResponseFormat::Detailed => Value::Array(search_output.matches.clone()),
                };

                let mut body = json!({
                    "success": true,
                    "matches": matches_value,
                    "mode": "search",
                    "response_format": response_format,
                    "match_count": search_output.matches.len(),
                });

                if search_output.truncated {
                    body["truncated"] = json!(true);
                    body["message"] = json!(format!(
                        "Showing {} matches (limit {}). Narrow the query or raise 'max_results' to see more.",
                        search_output.matches.len(),
                        search_output.limit
                    ));
                }

                body
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
                    .await?
            }
            "lint" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .context("'path' is required")?;

                let path = self.normalize_path(path)?;

                let language = args.get("language").and_then(|v| v.as_str());
                let severity_filter = args.get("severity_filter").and_then(|v| v.as_str());

                engine.lint(&path, language, severity_filter, None).await?
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

                engine.refactor(&path, language, refactor_type).await?
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

                if let Some(limit) = max_results
                    && limit == 0
                {
                    return Err(anyhow!("'max_results' must be greater than zero"));
                }

                let result = engine
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

                let mut metadata_sources: Vec<&serde_json::Map<String, Value>> = Vec::new();

                if let Some(obj) = result.as_object() {
                    metadata_sources.push(obj);
                }

                let (matches_vec, matches_value) = match result.get("results") {
                    Some(Value::Array(arr)) => (Some(arr.clone()), Value::Array(arr.clone())),
                    Some(Value::Object(obj)) => {
                        metadata_sources.push(obj);
                        let inner_matches = obj
                            .get("results")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.clone());
                        let matches_value = if let Some(ref inner) = inner_matches {
                            Value::Array(inner.clone())
                        } else {
                            Value::Object(obj.clone())
                        };
                        (inner_matches, matches_value)
                    }
                    Some(other) => (None, other.clone()),
                    None => (None, Value::Null),
                };

                let match_count = matches_vec
                    .as_ref()
                    .map(|arr| arr.len())
                    .or_else(|| matches_value.as_array().map(|arr| arr.len()))
                    .unwrap_or(0);

                let formatted_matches = match format {
                    ResponseFormat::Concise => {
                        if let Some(matches) = matches_vec.as_ref() {
                            Value::Array(matches_to_concise(
                                matches,
                                self.workspace_root().as_path(),
                            ))
                        } else {
                            matches_value.clone()
                        }
                    }
                    ResponseFormat::Detailed => matches_value.clone(),
                };

                let mut body = json!({
                    "success": true,
                    "matches": formatted_matches,
                    "mode": "custom",
                    "response_format": response_format,
                    "match_count": match_count,
                });

                if let Value::Object(ref mut map) = body {
                    for source in metadata_sources {
                        for (key, value) in source {
                            if key != "results" {
                                map.insert(key.clone(), value.clone());
                            }
                        }
                    }
                }

                body
            }
            _ => return Err(anyhow!("Unknown AST-grep operation: {}", operation)),
        };

        match format {
            ResponseFormat::Concise => {
                // For non-search/custom operations we still normalize using legacy helpers.
                if !matches!(operation, "search" | "custom") {
                    let mut out = out;
                    if let Some(matches) = out.get_mut("matches") {
                        let concise = utils::astgrep_to_concise(matches.take());
                        out["matches"] = concise;
                    } else if let Some(results) = out.get_mut("results") {
                        let concise = utils::astgrep_to_concise(results.take());
                        out["results"] = concise;
                    } else if let Some(issues) = out.get_mut("issues") {
                        let concise = utils::astgrep_issues_to_concise(issues.take());
                        out["issues"] = concise;
                    } else if let Some(suggestions) = out.get_mut("suggestions") {
                        let concise = utils::astgrep_changes_to_concise(suggestions.take());
                        out["suggestions"] = concise;
                    } else if let Some(changes) = out.get_mut("changes") {
                        let concise = utils::astgrep_changes_to_concise(changes.take());
                        out["changes"] = concise;
                    }
                    out["response_format"] = json!("concise");
                    Ok(out)
                } else {
                    Ok(out)
                }
            }
            ResponseFormat::Detailed => {
                let mut out = out;
                out["response_format"] = json!("detailed");
                Ok(out)
            }
        }
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
