//! Grep files handler (from Codex pattern).
//!
//! Searches for patterns in files using ripgrep-like functionality.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::orchestrator::{Approvable, Sandboxable, SandboxablePreference};
use super::tool_handler::{
    ToolCallError, ToolHandler, ToolInvocation, ToolKind, ToolOutput, ToolPayload,
};
use crate::utils::file_utils::read_file_with_context;

/// Maximum number of matches to return.
const MAX_MATCHES: usize = 100;

/// Maximum context lines before/after match.
const MAX_CONTEXT_LINES: usize = 5;

/// Arguments for grep_files tool.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrepFilesArgs {
    /// Pattern to search for.
    pub pattern: String,
    /// Path to search in (file or directory).
    pub path: Option<String>,
    /// Include pattern for files (glob).
    pub include: Option<String>,
    /// Exclude pattern for files (glob).
    pub exclude: Option<String>,
    /// Whether pattern is a regex.
    #[serde(default)]
    pub is_regex: bool,
    /// Case-insensitive search.
    #[serde(default)]
    pub case_insensitive: bool,
    /// Number of context lines before match.
    pub context_before: Option<usize>,
    /// Number of context lines after match.
    pub context_after: Option<usize>,
    /// Maximum number of results.
    pub max_results: Option<usize>,
}

/// A single match result.
#[derive(Clone, Debug, Serialize)]
pub struct GrepMatch {
    pub file: String,
    pub line_number: usize,
    pub line_content: String,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
}

/// Handler for searching files.
pub struct GrepFilesHandler {
    pub max_matches: usize,
}

impl Default for GrepFilesHandler {
    fn default() -> Self {
        Self {
            max_matches: MAX_MATCHES,
        }
    }
}

impl GrepFilesHandler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse arguments from payload.
    fn parse_args(&self, invocation: &ToolInvocation) -> Result<GrepFilesArgs, ToolCallError> {
        match &invocation.payload {
            ToolPayload::Function { arguments } => serde_json::from_str(arguments)
                .map_err(|e| ToolCallError::respond(format!("Invalid grep_files arguments: {e}"))),
            _ => Err(ToolCallError::respond(
                "Invalid payload type for grep_files handler",
            )),
        }
    }

    /// Search for pattern in files.
    async fn search(
        &self,
        args: &GrepFilesArgs,
        search_path: &PathBuf,
    ) -> Result<Vec<GrepMatch>, ToolCallError> {
        let pattern = if args.is_regex {
            args.pattern.clone()
        } else {
            regex::escape(&args.pattern)
        };

        let regex = regex::RegexBuilder::new(&pattern)
            .case_insensitive(args.case_insensitive)
            .build()
            .map_err(|e| ToolCallError::respond(format!("Invalid pattern: {e}")))?;

        let max_results = args
            .max_results
            .unwrap_or(self.max_matches)
            .min(self.max_matches);
        let context_before = args.context_before.unwrap_or(0).min(MAX_CONTEXT_LINES);
        let context_after = args.context_after.unwrap_or(0).min(MAX_CONTEXT_LINES);

        let mut matches = Vec::new();

        // Collect files to search
        let files = self.collect_files(search_path, args).await?;

        for file_path in files {
            if matches.len() >= max_results {
                break;
            }

            match read_file_with_context(&file_path, "grep search file").await {
                Ok(content) => {
                    let lines: Vec<&str> = content.lines().collect();

                    for (idx, line) in lines.iter().enumerate() {
                        if matches.len() >= max_results {
                            break;
                        }

                        if regex.is_match(line) {
                            let context_before_lines: Vec<String> =
                                (idx.saturating_sub(context_before)..idx)
                                    .map(|i| lines[i].to_string())
                                    .collect();

                            let context_after_lines: Vec<String> = ((idx + 1)
                                ..(idx + 1 + context_after).min(lines.len()))
                                .map(|i| lines[i].to_string())
                                .collect();

                            matches.push(GrepMatch {
                                file: file_path.to_string_lossy().to_string(),
                                line_number: idx + 1,
                                line_content: line.to_string(),
                                context_before: context_before_lines,
                                context_after: context_after_lines,
                            });
                        }
                    }
                }
                Err(_) => continue, // Skip files we can't read
            }
        }

        Ok(matches)
    }

    /// Collect files to search based on path and filters.
    async fn collect_files(
        &self,
        search_path: &PathBuf,
        args: &GrepFilesArgs,
    ) -> Result<Vec<PathBuf>, ToolCallError> {
        let mut files = Vec::new();

        if search_path.is_file() {
            files.push(search_path.clone());
        } else if search_path.is_dir() {
            self.collect_files_recursive(search_path, args, &mut files)
                .await?;
        } else {
            return Err(ToolCallError::respond(format!(
                "Path does not exist: {}",
                search_path.display()
            )));
        }

        Ok(files)
    }

    /// Recursively collect files from directory.
    async fn collect_files_recursive(
        &self,
        dir: &PathBuf,
        args: &GrepFilesArgs,
        files: &mut Vec<PathBuf>,
    ) -> Result<(), ToolCallError> {
        let mut entries = tokio::fs::read_dir(dir)
            .await
            .map_err(|e| ToolCallError::respond(format!("Cannot read directory: {e}")))?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();

            // Skip hidden files and common ignore patterns
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            if file_name.starts_with('.')
                || file_name == "node_modules"
                || file_name == "target"
                || file_name == "__pycache__"
            {
                continue;
            }

            if path.is_dir() {
                Box::pin(self.collect_files_recursive(&path, args, files)).await?;
            } else if path.is_file() {
                // Apply include/exclude filters
                if self.should_include_file(&path, args) {
                    files.push(path);
                }
            }
        }

        Ok(())
    }

    /// Check if a file should be included based on filters.
    fn should_include_file(&self, path: &Path, args: &GrepFilesArgs) -> bool {
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();

        // Check include pattern
        if let Some(include) = &args.include
            && !glob::Pattern::new(include)
                .map(|p| p.matches(&file_name))
                .unwrap_or(false)
        {
            return false;
        }

        // Check exclude pattern
        if let Some(exclude) = &args.exclude
            && glob::Pattern::new(exclude)
                .map(|p| p.matches(&file_name))
                .unwrap_or(false)
        {
            return false;
        }

        true
    }
}

impl Sandboxable for GrepFilesHandler {
    fn sandbox_preference(&self) -> SandboxablePreference {
        SandboxablePreference::Auto
    }
}

impl<R> Approvable<R> for GrepFilesHandler {
    type ApprovalKey = String;

    fn approval_key(&self, _req: &R) -> Self::ApprovalKey {
        "grep_files".to_string()
    }
}

#[async_trait]
impl ToolHandler for GrepFilesHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        false
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolCallError> {
        let args = self.parse_args(&invocation)?;
        let search_path = invocation.turn.resolve_path_ref(args.path.as_deref());

        let matches = self.search(&args, &search_path).await?;

        if matches.is_empty() {
            return Ok(ToolOutput::simple("No matches found."));
        }

        // Format results
        let mut output = String::new();
        for m in &matches {
            if !m.context_before.is_empty() {
                for (i, line) in m.context_before.iter().enumerate() {
                    let line_num = m.line_number - m.context_before.len() + i;
                    output.push_str(&format!("{}:{}: {}\n", m.file, line_num, line));
                }
            }
            output.push_str(&format!(
                "{}:{}> {}\n",
                m.file, m.line_number, m.line_content
            ));
            if !m.context_after.is_empty() {
                for (i, line) in m.context_after.iter().enumerate() {
                    output.push_str(&format!("{}:{}: {}\n", m.file, m.line_number + 1 + i, line));
                }
            }
            output.push('\n');
        }

        Ok(ToolOutput::simple(output.trim()))
    }
}

/// Create the grep_files tool specification.
pub fn create_grep_files_tool() -> super::tool_handler::ToolSpec {
    use super::tool_handler::{JsonSchema, ResponsesApiTool, ToolSpec};
    use std::collections::BTreeMap;

    let mut properties = BTreeMap::new();
    properties.insert(
        "pattern".to_string(),
        JsonSchema::String {
            description: Some("Pattern to search for (literal text or regex)".to_string()),
        },
    );
    properties.insert(
        "path".to_string(),
        JsonSchema::String {
            description: Some(
                "Path to search in (file or directory, defaults to workspace)".to_string(),
            ),
        },
    );
    properties.insert(
        "include".to_string(),
        JsonSchema::String {
            description: Some("Glob pattern for files to include (e.g., '*.rs')".to_string()),
        },
    );
    properties.insert(
        "exclude".to_string(),
        JsonSchema::String {
            description: Some("Glob pattern for files to exclude".to_string()),
        },
    );
    properties.insert(
        "is_regex".to_string(),
        JsonSchema::Boolean {
            description: Some("Whether pattern is a regex (default: false)".to_string()),
        },
    );
    properties.insert(
        "case_insensitive".to_string(),
        JsonSchema::Boolean {
            description: Some("Case-insensitive search (default: false)".to_string()),
        },
    );
    properties.insert(
        "context_before".to_string(),
        JsonSchema::Number {
            description: Some("Lines of context before each match (max: 5)".to_string()),
        },
    );
    properties.insert(
        "context_after".to_string(),
        JsonSchema::Number {
            description: Some("Lines of context after each match (max: 5)".to_string()),
        },
    );
    properties.insert(
        "max_results".to_string(),
        JsonSchema::Number {
            description: Some("Maximum number of results (default: 100)".to_string()),
        },
    );

    ToolSpec::Function(ResponsesApiTool {
        name: "grep_files".to_string(),
        description: "Search for a pattern in files. Returns matching lines with context."
            .to_string(),
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["pattern".to_string()]),
            additional_properties: Some(false.into()),
        },
        strict: false,
    })
}

#[cfg(test)]
mod tests {
    use super::super::tool_handler::ToolSpec;
    use super::*;

    #[test]
    fn test_grep_files_handler_kind() {
        let handler = GrepFilesHandler::new();
        assert_eq!(handler.kind(), ToolKind::Function);
    }

    #[test]
    fn test_create_grep_files_tool_spec() {
        let spec = create_grep_files_tool();
        assert_eq!(spec.name(), "grep_files");
        if let ToolSpec::Function(tool) = &spec {
            assert!(!tool.description.is_empty());
        } else {
            panic!("Expected ToolSpec::Function");
        }
    }

    #[test]
    fn test_should_include_file() {
        let handler = GrepFilesHandler::new();

        let args = GrepFilesArgs {
            pattern: "test".to_string(),
            path: None,
            include: Some("*.rs".to_string()),
            exclude: None,
            is_regex: false,
            case_insensitive: false,
            context_before: None,
            context_after: None,
            max_results: None,
        };

        assert!(handler.should_include_file(&PathBuf::from("test.rs"), &args));
        assert!(!handler.should_include_file(&PathBuf::from("test.py"), &args));
    }

    #[test]
    fn test_should_exclude_file() {
        let handler = GrepFilesHandler::new();

        let args = GrepFilesArgs {
            pattern: "test".to_string(),
            path: None,
            include: None,
            exclude: Some("*.lock".to_string()),
            is_regex: false,
            case_insensitive: false,
            context_before: None,
            context_after: None,
            max_results: None,
        };

        assert!(handler.should_include_file(&PathBuf::from("test.rs"), &args));
        assert!(!handler.should_include_file(&PathBuf::from("Cargo.lock"), &args));
    }
}
