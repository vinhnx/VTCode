//! List directory handler (from Codex pattern).
//!
//! Lists directory contents with metadata.

use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::orchestrator::{Approvable, Sandboxable, SandboxablePreference};
use super::tool_handler::{
    ToolCallError, ToolHandler, ToolInvocation, ToolKind, ToolOutput, ToolPayload,
};

use crate::utils::formatting::format_size;

/// Maximum entries to return.
const MAX_ENTRIES: usize = 500;

/// Arguments for list_dir tool.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListDirArgs {
    /// Path to list (defaults to current directory).
    pub path: Option<String>,
    /// Include hidden files.
    #[serde(default)]
    pub show_hidden: bool,
    /// Recursive listing.
    #[serde(default)]
    pub recursive: bool,
    /// Maximum depth for recursive listing.
    pub max_depth: Option<usize>,
    /// Pattern to filter entries (glob).
    pub pattern: Option<String>,
}

/// A directory entry.
#[derive(Clone, Debug, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified: Option<String>,
}

/// Handler for listing directories.
pub struct ListDirHandler {
    pub max_entries: usize,
}

impl Default for ListDirHandler {
    fn default() -> Self {
        Self {
            max_entries: MAX_ENTRIES,
        }
    }
}

impl ListDirHandler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse arguments from payload.
    fn parse_args(&self, invocation: &ToolInvocation) -> Result<ListDirArgs, ToolCallError> {
        match &invocation.payload {
            ToolPayload::Function { arguments } => serde_json::from_str(arguments)
                .map_err(|e| ToolCallError::respond(format!("Invalid list_dir arguments: {e}"))),
            _ => Err(ToolCallError::respond(
                "Invalid payload type for list_dir handler",
            )),
        }
    }

    /// List directory contents.
    async fn list_directory(
        &self,
        path: &PathBuf,
        args: &ListDirArgs,
        depth: usize,
    ) -> Result<Vec<DirEntry>, ToolCallError> {
        if !path.exists() {
            return Err(ToolCallError::respond(format!(
                "Directory not found: {}",
                path.display()
            )));
        }

        if !path.is_dir() {
            return Err(ToolCallError::respond(format!(
                "Not a directory: {}",
                path.display()
            )));
        }

        let max_depth = args.max_depth.unwrap_or(3);
        if args.recursive && depth >= max_depth {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        let mut dir_entries = tokio::fs::read_dir(path)
            .await
            .map_err(|e| ToolCallError::respond(format!("Cannot read directory: {e}")))?;

        while let Ok(Some(entry)) = dir_entries.next_entry().await {
            if entries.len() >= self.max_entries {
                break;
            }

            let entry_path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files unless requested
            if !args.show_hidden && file_name.starts_with('.') {
                continue;
            }

            // Apply pattern filter
            if let Some(pattern) = &args.pattern
                && !glob::Pattern::new(pattern)
                    .map(|p| p.matches(&file_name))
                    .unwrap_or(true)
            {
                continue;
            }

            let metadata = entry.metadata().await.ok();
            let is_dir = entry_path.is_dir();

            let dir_entry = DirEntry {
                name: file_name.clone(),
                path: entry_path.to_string_lossy().to_string(),
                is_dir,
                size: metadata.as_ref().map(|m| m.len()),
                modified: metadata.as_ref().and_then(|m| {
                    m.modified().ok().map(|t| {
                        chrono::DateTime::<chrono::Utc>::from(t)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    })
                }),
            };

            entries.push(dir_entry);

            // Recurse into subdirectories
            if args.recursive && is_dir && entries.len() < self.max_entries {
                let sub_entries =
                    Box::pin(self.list_directory(&entry_path, args, depth + 1)).await?;
                entries.extend(sub_entries);
            }
        }

        // Sort entries: directories first, then alphabetically
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        Ok(entries)
    }
}

impl Sandboxable for ListDirHandler {
    fn sandbox_preference(&self) -> SandboxablePreference {
        SandboxablePreference::Auto
    }
}

impl<R> Approvable<R> for ListDirHandler {
    type ApprovalKey = String;

    fn approval_key(&self, _req: &R) -> Self::ApprovalKey {
        "list_dir".to_string()
    }
}

#[async_trait]
impl ToolHandler for ListDirHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        false
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolCallError> {
        let args = self.parse_args(&invocation)?;
        let path = invocation.turn.resolve_path_ref(args.path.as_deref());

        let entries = self.list_directory(&path, &args, 0).await?;

        if entries.is_empty() {
            return Ok(ToolOutput::simple("Directory is empty."));
        }

        // Format as tree-like output
        let mut output = String::new();
        for entry in &entries {
            let prefix = if entry.is_dir { "📁 " } else { "📄 " };
            let suffix = if entry.is_dir { "/" } else { "" };

            if let Some(size) = entry.size {
                if !entry.is_dir {
                    output.push_str(&format!(
                        "{}{}{} ({})\n",
                        prefix,
                        entry.name,
                        suffix,
                        format_size(size)
                    ));
                } else {
                    output.push_str(&format!("{}{}{}\n", prefix, entry.name, suffix));
                }
            } else {
                output.push_str(&format!("{}{}{}\n", prefix, entry.name, suffix));
            }
        }

        Ok(ToolOutput::simple(output.trim()))
    }
}

/// Create the list_dir tool specification.
pub fn create_list_dir_tool() -> super::tool_handler::ToolSpec {
    use super::tool_handler::{JsonSchema, ResponsesApiTool, ToolSpec};
    use std::collections::BTreeMap;

    let mut properties = BTreeMap::new();
    properties.insert(
        "path".to_string(),
        JsonSchema::String {
            description: Some("Path to list (defaults to current directory)".to_string()),
        },
    );
    properties.insert(
        "show_hidden".to_string(),
        JsonSchema::Boolean {
            description: Some("Include hidden files (default: false)".to_string()),
        },
    );
    properties.insert(
        "recursive".to_string(),
        JsonSchema::Boolean {
            description: Some("List recursively (default: false)".to_string()),
        },
    );
    properties.insert(
        "max_depth".to_string(),
        JsonSchema::Number {
            description: Some("Maximum depth for recursive listing (default: 3)".to_string()),
        },
    );
    properties.insert(
        "pattern".to_string(),
        JsonSchema::String {
            description: Some("Glob pattern to filter entries".to_string()),
        },
    );

    ToolSpec::Function(ResponsesApiTool {
        name: "list_dir".to_string(),
        description:
            "List the contents of a directory. Returns file and directory names with metadata."
                .to_string(),
        parameters: JsonSchema::Object {
            properties,
            required: None,
            additional_properties: Some(false.into()),
        },
        strict: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_dir_handler_kind() {
        let handler = ListDirHandler::new();
        assert_eq!(handler.kind(), ToolKind::Function);
    }

    #[test]
    fn test_create_list_dir_tool_spec() {
        let spec = create_list_dir_tool();
        assert_eq!(spec.name(), "list_dir");
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500B");
        assert_eq!(format_size(1024), "1.0KB");
        assert_eq!(format_size(1536), "1.5KB");
        assert_eq!(format_size(1048576), "1.0MB");
        assert_eq!(format_size(1073741824), "1.0GB");
    }
}
