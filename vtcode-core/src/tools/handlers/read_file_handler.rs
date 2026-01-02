//! Read file handler (from Codex pattern).
//!
//! Reads file contents with line range support and encoding detection.

use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::orchestrator::{Approvable, Sandboxable, SandboxablePreference};
use super::tool_handler::{
    ToolCallError, ToolHandler, ToolInvocation, ToolKind, ToolOutput, ToolPayload,
};

/// Maximum file size to read (1MB).
const MAX_FILE_SIZE: u64 = 1_048_576;

/// Maximum lines to return in a single read.
const MAX_LINES: usize = 2000;

/// Arguments for read_file tool.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadFileArgs {
    /// Path to the file to read.
    pub path: String,
    /// Starting line number (1-indexed, optional).
    pub start_line: Option<usize>,
    /// Ending line number (1-indexed, inclusive, optional).
    pub end_line: Option<usize>,
    /// Number of lines to read (alternative to end_line).
    pub num_lines: Option<usize>,
}

/// Handler for reading files.
pub struct ReadFileHandler {
    /// Maximum file size allowed.
    pub max_file_size: u64,
    /// Maximum lines to return.
    pub max_lines: usize,
}

impl Default for ReadFileHandler {
    fn default() -> Self {
        Self {
            max_file_size: MAX_FILE_SIZE,
            max_lines: MAX_LINES,
        }
    }
}

impl ReadFileHandler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse arguments from payload.
    fn parse_args(&self, invocation: &ToolInvocation) -> Result<ReadFileArgs, ToolCallError> {
        match &invocation.payload {
            ToolPayload::Function { arguments } => serde_json::from_str(arguments)
                .map_err(|e| ToolCallError::respond(format!("Invalid read_file arguments: {e}"))),
            _ => Err(ToolCallError::respond(
                "Invalid payload type for read_file handler",
            )),
        }
    }

    /// Resolve the file path relative to workspace.
    fn resolve_path(&self, path: &str, invocation: &ToolInvocation) -> PathBuf {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            invocation.turn.cwd.join(path)
        }
    }

    /// Read file contents with optional line range.
    async fn read_file_contents(
        &self,
        path: &PathBuf,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> Result<(String, FileMetadata), ToolCallError> {
        // Check if file exists
        if !path.exists() {
            return Err(ToolCallError::respond(format!(
                "File not found: {}",
                path.display()
            )));
        }

        // Check file size
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| ToolCallError::respond(format!("Cannot read file metadata: {e}")))?;

        if metadata.len() > self.max_file_size {
            return Err(ToolCallError::respond(format!(
                "File too large: {} bytes (max: {} bytes)",
                metadata.len(),
                self.max_file_size
            )));
        }

        // Read file
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| ToolCallError::respond(format!("Cannot read file: {e}")))?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        // Apply line range
        let start = start_line.unwrap_or(1).saturating_sub(1);
        let end = end_line.unwrap_or(total_lines).min(total_lines);

        if start >= total_lines {
            return Err(ToolCallError::respond(format!(
                "Start line {} exceeds file length ({} lines)",
                start + 1,
                total_lines
            )));
        }

        let selected_lines: Vec<&str> = lines[start..end].to_vec();
        let truncated = selected_lines.len() > self.max_lines;
        let final_lines: Vec<&str> = if truncated {
            selected_lines[..self.max_lines].to_vec()
        } else {
            selected_lines
        };

        // Format with line numbers
        let formatted: Vec<String> = final_lines
            .iter()
            .enumerate()
            .map(|(i, line)| format!("L{}: {}", start + i + 1, line))
            .collect();

        let file_meta = FileMetadata {
            total_lines,
            returned_lines: final_lines.len(),
            start_line: start + 1,
            end_line: start + final_lines.len(),
            truncated,
            file_size: metadata.len(),
        };

        Ok((formatted.join("\n"), file_meta))
    }
}

/// Metadata about the file read.
#[derive(Clone, Debug, Serialize)]
pub struct FileMetadata {
    pub total_lines: usize,
    pub returned_lines: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub truncated: bool,
    pub file_size: u64,
}

impl Sandboxable for ReadFileHandler {
    fn sandbox_preference(&self) -> SandboxablePreference {
        SandboxablePreference::Auto
    }
}

impl<R> Approvable<R> for ReadFileHandler {
    type ApprovalKey = String;

    fn approval_key(&self, _req: &R) -> Self::ApprovalKey {
        "read_file".to_string()
    }
}

#[async_trait]
impl ToolHandler for ReadFileHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        false // Reading is not mutating
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolCallError> {
        let args = self.parse_args(&invocation)?;
        let path = self.resolve_path(&args.path, &invocation);

        // Calculate end_line from num_lines if provided
        let end_line = args
            .end_line
            .or_else(|| args.num_lines.map(|n| args.start_line.unwrap_or(1) + n - 1));

        let (content, _metadata) = self
            .read_file_contents(&path, args.start_line, end_line)
            .await?;

        Ok(ToolOutput::simple(content))
    }
}

/// Create the read_file tool specification.
pub fn create_read_file_tool() -> super::tool_handler::ToolSpec {
    use super::tool_handler::{JsonSchema, ResponsesApiTool, ToolSpec};
    use std::collections::BTreeMap;

    let mut properties = BTreeMap::new();
    properties.insert(
        "path".to_string(),
        JsonSchema::String {
            description: Some(
                "Path to the file to read (absolute or relative to workspace)".to_string(),
            ),
        },
    );
    properties.insert(
        "start_line".to_string(),
        JsonSchema::Number {
            description: Some("Starting line number (1-indexed, optional)".to_string()),
        },
    );
    properties.insert(
        "end_line".to_string(),
        JsonSchema::Number {
            description: Some("Ending line number (1-indexed, inclusive, optional)".to_string()),
        },
    );
    properties.insert(
        "num_lines".to_string(),
        JsonSchema::Number {
            description: Some(
                "Number of lines to read from start_line (alternative to end_line)".to_string(),
            ),
        },
    );

    ToolSpec::Function(ResponsesApiTool {
        name: "read_file".to_string(),
        description: "Read the contents of a file. Returns content with line numbers. Use start_line/end_line for specific ranges.".to_string(),
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["path".to_string()]),
            additional_properties: Some(false.into()),
        },
        strict: false,
    })
}

#[cfg(test)]
mod tests {
    use super::super::tool_handler::{ApprovalPolicy, ToolSpec};
    use super::*;

    #[test]
    fn test_read_file_handler_kind() {
        let handler = ReadFileHandler::new();
        assert_eq!(handler.kind(), ToolKind::Function);
    }

    #[test]
    fn test_create_read_file_tool_spec() {
        let spec = create_read_file_tool();
        assert_eq!(spec.name(), "read_file");
        if let ToolSpec::Function(tool) = &spec {
            assert!(!tool.description.is_empty());
        } else {
            panic!("Expected ToolSpec::Function");
        }
    }

    #[tokio::test]
    async fn test_read_file_handler_is_not_mutating() {
        let handler = ReadFileHandler::new();
        // Read operations are not mutating
        assert!(!handler.is_mutating(&create_dummy_invocation()).await);
    }

    fn create_dummy_invocation() -> ToolInvocation {
        use super::super::tool_handler::{
            ShellEnvironmentPolicy, ToolEvent, ToolSession, TurnContext,
        };
        use std::sync::Arc;

        struct DummySession;

        #[async_trait]
        impl ToolSession for DummySession {
            fn cwd(&self) -> &PathBuf {
                static CWD: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
                CWD.get_or_init(|| PathBuf::from("/tmp"))
            }
            fn workspace_root(&self) -> &PathBuf {
                self.cwd()
            }
            async fn record_warning(&self, _message: String) {}
            fn user_shell(&self) -> &str {
                "/bin/bash"
            }
            async fn send_event(&self, _event: ToolEvent) {}
        }

        ToolInvocation {
            session: Arc::new(DummySession),
            turn: Arc::new(TurnContext {
                cwd: PathBuf::from("/tmp"),
                turn_id: "test".to_string(),
                sub_id: None,
                shell_environment_policy: ShellEnvironmentPolicy::Inherit,
                approval_policy: ApprovalPolicy::Never,
                codex_linux_sandbox_exe: None,
                sandbox_policy: Default::default(),
            }),
            tracker: None,
            call_id: "test-call".to_string(),
            tool_name: "read_file".to_string(),
            payload: ToolPayload::Function {
                arguments: r#"{"path": "/tmp/test.txt"}"#.to_string(),
            },
        }
    }
}
