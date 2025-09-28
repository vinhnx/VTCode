//! Search tool implementation with multiple modes

use super::traits::{CacheableTool, ModeTool, Tool};
use crate::config::constants::tools;
use crate::tools::grep_search::{GrepSearchInput, GrepSearchManager};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

/// Unified search tool with multiple modes
#[derive(Clone)]
pub struct SearchTool {
    workspace_root: PathBuf,
    grep_search: Arc<GrepSearchManager>,
}

impl SearchTool {
    pub fn new(workspace_root: PathBuf, grep_search: Arc<GrepSearchManager>) -> Self {
        Self {
            workspace_root,
            grep_search,
        }
    }

    /// Execute exact search mode
    async fn execute_exact(&self, args: Value) -> Result<Value> {
        let pattern = args
            .get("pattern")
            .and_then(|p| p.as_str())
            .ok_or_else(|| anyhow!("Error: Missing 'pattern'. Example: grep_search({{\"pattern\": \"TODO|FIXME\", \"path\": \"src\"}})"))?;

        let input = GrepSearchInput {
            pattern: pattern.to_string(),
            path: args
                .get("path")
                .and_then(|p| p.as_str())
                .unwrap_or(".")
                .to_string(),
            max_results: Some(
                args.get("max_results")
                    .and_then(|m| m.as_u64())
                    .unwrap_or(100) as usize,
            ),
            case_sensitive: Some(
                args.get("case_sensitive")
                    .and_then(|c| c.as_bool())
                    .unwrap_or(true),
            ),
            literal: Some(false),
            glob_pattern: None,
            context_lines: Some(0),
            include_hidden: Some(false),
        };

        let result = self.grep_search.perform_search(input.clone()).await?;

        // Response formatting
        let concise = args
            .get("response_format")
            .and_then(|v| v.as_str())
            .map(|s| s.eq_ignore_ascii_case("concise"))
            .unwrap_or(true);

        let mut body = if concise {
            let concise_matches = transform_matches_to_concise(&result.matches);
            json!({
                "success": true,
                "matches": concise_matches,
                "mode": "exact",
                "response_format": "concise"
            })
        } else {
            json!({
                "success": true,
                "matches": result.matches,
                "mode": "exact",
                "response_format": "detailed"
            })
        };

        if let Some(max) = input.max_results {
            // Heuristic: if we hit the cap, hint pagination/filtering
            if let Some(arr) = body.get("matches").and_then(|m| m.as_array())
                && arr.len() >= max
            {
                body["message"] = json!(format!(
                    "Showing {} results (limit). Narrow your query or use more specific patterns to reduce tokens.",
                    max
                ));
            }
        }
        Ok(body)
    }

    /// Execute fuzzy search mode
    async fn execute_fuzzy(&self, args: Value) -> Result<Value> {
        let pattern = args
            .get("pattern")
            .and_then(|p| p.as_str())
            .ok_or_else(|| anyhow!("Error: Missing 'pattern'. Example: grep_search({{\"mode\": \"fuzzy\", \"pattern\": \"todo\", \"path\": \"src\"}})"))?;

        let input = GrepSearchInput {
            pattern: pattern.to_string(),
            path: args
                .get("path")
                .and_then(|p| p.as_str())
                .unwrap_or(".")
                .to_string(),
            max_results: Some(
                args.get("max_results")
                    .and_then(|m| m.as_u64())
                    .unwrap_or(100) as usize,
            ),
            case_sensitive: Some(
                args.get("case_sensitive")
                    .and_then(|c| c.as_bool())
                    .unwrap_or(false), // Default to case-insensitive for fuzzy search
            ),
            literal: Some(false),
            glob_pattern: None,
            context_lines: Some(0),
            include_hidden: Some(false),
        };

        let result = self.grep_search.perform_search(input.clone()).await?;

        // Response formatting
        let concise = args
            .get("response_format")
            .and_then(|v| v.as_str())
            .map(|s| s.eq_ignore_ascii_case("concise"))
            .unwrap_or(true);

        let mut body = if concise {
            let concise_matches = transform_matches_to_concise(&result.matches);
            json!({
                "success": true,
                "matches": concise_matches,
                "mode": "fuzzy",
                "case_sensitive": false,
                "response_format": "concise"
            })
        } else {
            json!({
                "success": true,
                "matches": result.matches,
                "mode": "fuzzy",
                "case_sensitive": false,
                "response_format": "detailed"
            })
        };

        if let Some(max) = input.max_results {
            // Heuristic: if we hit the cap, hint pagination/filtering
            if let Some(arr) = body.get("matches").and_then(|m| m.as_array())
                && arr.len() >= max
            {
                body["message"] = json!(format!(
                    "Showing {} results (limit). Narrow your query or use more specific patterns to reduce tokens.",
                    max
                ));
            }
        }
        Ok(body)
    }

    /// Execute multi-pattern search mode
    async fn execute_multi(&self, args: Value) -> Result<Value> {
        let args_obj = args
            .as_object()
            .ok_or_else(|| anyhow!("Error: Invalid 'multi' arguments. Required: {{ patterns: string[] }}. Optional: {{ logic: 'AND'|'OR' }}. Example: grep_search({{\"mode\": \"multi\", \"patterns\": [\"fn \\w+\", \"use \\w+\"], \"logic\": \"AND\"}})"))?;

        let patterns = args_obj
            .get("patterns")
            .and_then(|p| p.as_array())
            .ok_or_else(|| anyhow!("Missing patterns array for multi mode"))?;

        let logic = args_obj
            .get("logic")
            .and_then(|l| l.as_str())
            .unwrap_or("AND");

        let mut all_results = Vec::new();

        // Execute search for each pattern
        for pattern in patterns {
            if let Some(pattern_str) = pattern.as_str() {
                let mut pattern_args = args.clone();
                if let Some(obj) = pattern_args.as_object_mut() {
                    obj.insert("pattern".to_string(), json!(pattern_str));
                }

                match self.execute_exact(pattern_args).await {
                    Ok(result) => {
                        if let Some(matches) = result.get("matches").and_then(|m| m.as_array()) {
                            all_results.extend(matches.clone());
                        }
                    }
                    Err(_) => continue, // Skip failed patterns
                }
            }
        }

        // Apply logic (AND/OR) to combine results
        let final_results = if logic == "AND" {
            self.apply_and_logic(all_results, patterns.len())
        } else {
            self.apply_or_logic(all_results)
        };

        Ok(json!({
            "success": true,
            "matches": final_results,
            "mode": "multi",
            "logic": logic,
            "pattern_count": patterns.len()
        }))
    }

    /// Execute similarity search mode
    async fn execute_similarity(&self, args: Value) -> Result<Value> {
        let args_obj = args
            .as_object()
            .ok_or_else(|| anyhow!("Error: Invalid 'similarity' arguments. Required: {{ reference_file: string }}. Optional: {{ content_type: 'structure'|'imports'|'functions'|'all' }}. Example: grep_search({{\"mode\": \"similarity\", \"reference_file\": \"src/lib.rs\", \"content_type\": \"functions\"}})"))?;

        let reference_file = args_obj
            .get("reference_file")
            .and_then(|f| f.as_str())
            .ok_or_else(|| anyhow!("Error: Missing 'reference_file'. Example: grep_search({{\"mode\": \"similarity\", \"reference_file\": \"src/main.rs\"}})"))?;

        let content_type = args_obj
            .get("content_type")
            .and_then(|c| c.as_str())
            .unwrap_or("all");

        // Read reference file to extract patterns
        let ref_path = self.workspace_root.join(reference_file);
        let ref_content = tokio::fs::read_to_string(&ref_path).await.map_err(|e| {
            anyhow!(
                "Error: Failed to read reference file '{}': {}",
                reference_file,
                e
            )
        })?;

        // Extract patterns based on content type
        let patterns = self.extract_similarity_patterns(&ref_content, content_type)?;

        // Execute multi-pattern search with OR logic
        let mut search_args = args.clone();
        if let Some(obj) = search_args.as_object_mut() {
            obj.insert("patterns".to_string(), json!(patterns));
            obj.insert("logic".to_string(), json!("OR"));
        }

        self.execute_multi(search_args).await
    }

    /// Apply AND logic to search results
    fn apply_and_logic(&self, results: Vec<Value>, pattern_count: usize) -> Vec<Value> {
        use std::collections::HashMap;

        let mut file_matches: HashMap<String, Vec<Value>> = HashMap::new();

        // Group matches by file
        for result in results {
            if let Some(path) = result.get("path").and_then(|p| p.as_str()) {
                file_matches
                    .entry(path.to_string())
                    .or_default()
                    .push(result);
            }
        }

        // Only include files that have matches for all patterns
        file_matches
            .into_iter()
            .filter(|(_, matches)| matches.len() >= pattern_count)
            .flat_map(|(_, matches)| matches)
            .collect()
    }

    /// Apply OR logic to search results (remove duplicates)
    fn apply_or_logic(&self, results: Vec<Value>) -> Vec<Value> {
        use std::collections::HashSet;

        let mut seen = HashSet::new();
        let mut unique_results = Vec::new();

        for result in results {
            let key = format!(
                "{}:{}:{}",
                result.get("path").and_then(|p| p.as_str()).unwrap_or(""),
                result
                    .get("line_number")
                    .and_then(|l| l.as_u64())
                    .unwrap_or(0),
                result.get("column").and_then(|c| c.as_u64()).unwrap_or(0)
            );

            if seen.insert(key) {
                unique_results.push(result);
            }
        }

        unique_results
    }

    /// Extract patterns for similarity search
    fn extract_similarity_patterns(
        &self,
        content: &str,
        content_type: &str,
    ) -> Result<Vec<String>> {
        let mut patterns = Vec::new();

        match content_type {
            "functions" => {
                // Extract function signatures
                for line in content.lines() {
                    if (line.trim_start().starts_with("fn ")
                        || line.trim_start().starts_with("pub fn "))
                        && let Some(name) = self.extract_function_name(line)
                    {
                        patterns.push(format!("fn {}", name));
                    }
                }
            }
            "imports" => {
                // Extract import statements
                for line in content.lines() {
                    if line.trim_start().starts_with("use ") {
                        patterns.push(line.trim().to_string());
                    }
                }
            }
            "structure" => {
                // Extract struct/enum definitions
                for line in content.lines() {
                    let trimmed = line.trim_start();
                    if trimmed.starts_with("struct ") || trimmed.starts_with("enum ") {
                        patterns.push(
                            trimmed
                                .split_whitespace()
                                .take(2)
                                .collect::<Vec<_>>()
                                .join(" "),
                        );
                    }
                }
            }
            _ => {
                // Extract all significant keywords
                patterns.extend(self.extract_keywords(content));
            }
        }

        if patterns.is_empty() {
            return Err(anyhow!(
                "No patterns extracted from reference file. Try content_type='all' or provide a different reference_file."
            ));
        }

        Ok(patterns)
    }

    /// Extract function name from function definition
    fn extract_function_name(&self, line: &str) -> Option<String> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        for (i, part) in parts.iter().enumerate() {
            if *part == "fn" && i + 1 < parts.len() {
                let name = parts[i + 1];
                if let Some(paren_pos) = name.find('(') {
                    return Some(name[..paren_pos].to_string());
                }
                return Some(name.to_string());
            }
        }
        None
    }

    /// Extract keywords from content
    fn extract_keywords(&self, content: &str) -> Vec<String> {
        let keywords = ["fn ", "struct ", "enum ", "impl ", "trait ", "use ", "mod "];
        let mut patterns = Vec::new();

        for line in content.lines() {
            for keyword in &keywords {
                if line.contains(keyword) {
                    patterns.push(keyword.trim().to_string());
                }
            }
        }

        patterns.sort();
        patterns.dedup();
        patterns
    }
}

#[async_trait]
impl Tool for SearchTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let args_clone = args.clone();
        let mode = args_clone
            .get("mode")
            .and_then(|m| m.as_str())
            .unwrap_or("exact");

        self.execute_mode(mode, args).await
    }

    fn name(&self) -> &'static str {
        tools::GREP_SEARCH
    }

    fn description(&self) -> &'static str {
        "Enhanced unified search tool with multiple modes: exact (default), fuzzy, multi-pattern, and similarity search"
    }
}

#[async_trait]
impl ModeTool for SearchTool {
    fn supported_modes(&self) -> Vec<&'static str> {
        vec!["exact", "fuzzy", "multi", "similarity"]
    }

    async fn execute_mode(&self, mode: &str, args: Value) -> Result<Value> {
        match mode {
            "exact" => self.execute_exact(args).await,
            "fuzzy" => self.execute_fuzzy(args).await,
            "multi" => self.execute_multi(args).await,
            "similarity" => self.execute_similarity(args).await,
            _ => Err(anyhow!("Unsupported search mode: {}", mode)),
        }
    }
}

#[async_trait]
impl CacheableTool for SearchTool {
    fn cache_key(&self, args: &Value) -> String {
        format!(
            "search:{}:{}",
            args.get("pattern").and_then(|p| p.as_str()).unwrap_or(""),
            args.get("mode").and_then(|m| m.as_str()).unwrap_or("exact")
        )
    }

    fn should_cache(&self, args: &Value) -> bool {
        // Cache exact and fuzzy searches, but not multi/similarity (too dynamic)
        let mode = args.get("mode").and_then(|m| m.as_str()).unwrap_or("exact");
        matches!(mode, "exact" | "fuzzy")
    }
}

/// Transform ripgrep JSON event stream into a concise, agent-friendly structure
/// keeping only meaningful context for downstream actions.
pub(crate) fn transform_matches_to_concise(events: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for ev in events {
        if ev.get("type").and_then(|t| t.as_str()) != Some("match") {
            continue;
        }
        if let Some(data) = ev.get("data") {
            let path = data
                .get("path")
                .and_then(|p| p.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            let line = data
                .get("line_number")
                .and_then(|n| n.as_u64())
                .unwrap_or(0);
            let preview = data
                .get("lines")
                .and_then(|l| l.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .trim_end_matches(['\r', '\n']);

            out.push(json!({
                "path": path,
                "line_number": line,
                "text": preview,
            }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_matches_to_concise() {
        let raw = vec![
            json!({
                "type": "match",
                "data": {
                    "path": {"text": "src/main.rs"},
                    "line_number": 10,
                    "lines": {"text": "fn main() {}\n"}
                }
            }),
            json!({"type": "begin"}),
        ];
        let concise = transform_matches_to_concise(&raw);
        assert_eq!(concise.len(), 1);
        assert_eq!(concise[0]["path"], "src/main.rs");
        assert_eq!(concise[0]["line_number"], 10);
        assert_eq!(concise[0]["text"], "fn main() {}");
    }
}
