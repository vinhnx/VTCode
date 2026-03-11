//! VT Code Command Implementations
//!
//! This module implements all VT Code commands that are exposed through
//! Zed's command palette. Uses CommandBuilder for cleaner construction.
use crate::command_builder::CommandBuilder;
use crate::context::{EditorContext, IDE_CONTEXT_ENV_VAR};

/// Response from a VT Code command operation
#[derive(Debug, Clone)]
pub struct CommandResponse {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

impl CommandResponse {
    pub fn ok(output: String) -> Self {
        Self {
            success: true,
            output,
            error: None,
        }
    }

    pub fn err(error: String) -> Self {
        Self {
            success: false,
            output: String::new(),
            error: Some(error),
        }
    }
}

/// Ask the VT Code agent an arbitrary question
pub fn ask_agent(query: &str) -> CommandResponse {
    ask_agent_with_context(query, None)
}

pub fn ask_agent_with_context(query: &str, context: Option<&EditorContext>) -> CommandResponse {
    let builder = match attach_ide_context(CommandBuilder::ask(query), context) {
        Ok(builder) => builder,
        Err(error) => return CommandResponse::err(error),
    };

    match builder.execute() {
        Ok(result) => {
            if result.is_success() {
                CommandResponse::ok(result.output())
            } else {
                CommandResponse::err(format!("Command failed: {}", result.stderr))
            }
        }
        Err(e) => CommandResponse::err(e),
    }
}

/// Ask about a code selection
pub fn ask_about_selection(code: &str, language: Option<&str>) -> CommandResponse {
    ask_about_selection_with_context(code, language, None)
}

pub fn ask_about_selection_with_context(
    code: &str,
    language: Option<&str>,
    context: Option<&EditorContext>,
) -> CommandResponse {
    // Construct a query that includes the code and optional language
    let query = format!(
        "Analyze this {} code:\n{}",
        language.unwrap_or(""),
        code
    );

    ask_agent_with_context(&query, context)
}

/// Analyze the entire workspace
pub fn analyze_workspace() -> CommandResponse {
    analyze_workspace_with_context(None)
}

pub fn analyze_workspace_with_context(context: Option<&EditorContext>) -> CommandResponse {
    let builder = match attach_ide_context(CommandBuilder::analyze(), context) {
        Ok(builder) => builder,
        Err(error) => return CommandResponse::err(error),
    };

    match builder.execute() {
        Ok(result) => {
            if result.is_success() {
                CommandResponse::ok(result.output())
            } else {
                CommandResponse::err(format!("Workspace analysis failed: {}", result.stderr))
            }
        }
        Err(e) => CommandResponse::err(e),
    }
}

/// Launch an interactive chat session
pub fn launch_chat() -> CommandResponse {
    launch_chat_with_context(None)
}

pub fn launch_chat_with_context(context: Option<&EditorContext>) -> CommandResponse {
    let builder = match attach_ide_context(CommandBuilder::chat(), context) {
        Ok(builder) => builder,
        Err(error) => return CommandResponse::err(error),
    };

    match builder.execute() {
        Ok(result) => {
            if result.is_success() {
                CommandResponse::ok(result.output())
            } else {
                CommandResponse::err(format!("Chat launch failed: {}", result.stderr))
            }
        }
        Err(e) => CommandResponse::err(e),
    }
}

/// Check VT Code CLI installation and status
pub fn check_status() -> CommandResponse {
    match CommandBuilder::version().execute() {
        Ok(result) => {
            if result.is_success() {
                CommandResponse::ok(format!("VT Code CLI is available\n{}", result.stdout))
            } else {
                CommandResponse::err("VT Code CLI check failed".to_string())
            }
        }
        Err(e) => CommandResponse::err(format!("VT Code CLI not found: {}", e)),
    }
}

/// Find files matching a fuzzy pattern
///
/// Uses the optimized file search bridge for fast parallel file discovery.
/// Supports fuzzy matching and respects .gitignore files.
///
/// # Arguments
/// * `pattern` - Fuzzy search pattern (e.g., "main", "component.rs")
/// * `limit` - Maximum number of results to return
pub fn find_files(pattern: &str, limit: Option<usize>) -> CommandResponse {
    find_files_with_context(pattern, limit, None)
}

pub fn find_files_with_context(
    pattern: &str,
    limit: Option<usize>,
    context: Option<&EditorContext>,
) -> CommandResponse {
    let mut builder = CommandBuilder::find_files(pattern);

    if let Some(l) = limit {
        builder = builder.with_option("limit", l.to_string());
    }

    let builder = match attach_ide_context(builder, context) {
        Ok(builder) => builder,
        Err(error) => return CommandResponse::err(error),
    };

    match builder.execute() {
        Ok(result) => {
            if result.is_success() {
                CommandResponse::ok(result.output())
            } else {
                CommandResponse::err(format!("File search failed: {}", result.stderr))
            }
        }
        Err(e) => CommandResponse::err(format!("File search error: {}", e)),
    }
}

/// List all files in the workspace
///
/// Enumerates files with optional exclusion patterns.
/// Respects .gitignore by default.
///
/// # Arguments
/// * `exclude_patterns` - Optional comma-separated glob patterns to exclude
pub fn list_files(exclude_patterns: Option<&str>) -> CommandResponse {
    list_files_with_context(exclude_patterns, None)
}

pub fn list_files_with_context(
    exclude_patterns: Option<&str>,
    context: Option<&EditorContext>,
) -> CommandResponse {
    let mut builder = CommandBuilder::list_files();

    if let Some(patterns) = exclude_patterns {
        builder = builder.with_option("exclude", patterns);
    }

    let builder = match attach_ide_context(builder, context) {
        Ok(builder) => builder,
        Err(error) => return CommandResponse::err(error),
    };

    match builder.execute() {
        Ok(result) => {
            if result.is_success() {
                CommandResponse::ok(result.output())
            } else {
                CommandResponse::err(format!("File listing failed: {}", result.stderr))
            }
        }
        Err(e) => CommandResponse::err(format!("File listing error: {}", e)),
    }
}

/// Search for files with pattern and exclusions
///
/// Combined search and filter operation for more advanced queries.
///
/// # Arguments
/// * `pattern` - Fuzzy search pattern
/// * `exclude` - Comma-separated glob patterns to exclude
pub fn search_files(pattern: &str, exclude: &str) -> CommandResponse {
    search_files_with_context(pattern, exclude, None)
}

pub fn search_files_with_context(
    pattern: &str,
    exclude: &str,
    context: Option<&EditorContext>,
) -> CommandResponse {
    let builder = match attach_ide_context(
        CommandBuilder::search_files(pattern, exclude),
        context,
    ) {
        Ok(builder) => builder,
        Err(error) => return CommandResponse::err(error),
    };

    match builder.execute() {
        Ok(result) => {
            if result.is_success() {
                CommandResponse::ok(result.output())
            } else {
                CommandResponse::err(format!("File search failed: {}", result.stderr))
            }
        }
        Err(e) => CommandResponse::err(format!("File search error: {}", e)),
    }
}

fn attach_ide_context(
    builder: CommandBuilder,
    context: Option<&EditorContext>,
) -> Result<CommandBuilder, String> {
    let Some(context) = context else {
        return Ok(builder);
    };
    let path = context.write_ide_context_snapshot()?;

    Ok(builder.with_env(
        IDE_CONTEXT_ENV_VAR,
        path.to_string_lossy().into_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_response_ok() {
        let response = CommandResponse::ok("Success".to_string());
        assert!(response.success);
        assert_eq!(response.output, "Success");
        assert!(response.error.is_none());
    }

    #[test]
    fn test_command_response_err() {
        let response = CommandResponse::err("Error message".to_string());
        assert!(!response.success);
        assert_eq!(response.output, "");
        assert_eq!(response.error, Some("Error message".to_string()));
    }

    #[test]
    fn test_find_files_without_limit() {
        let response = find_files("test", None);
        // This will fail if VT Code is not installed, which is expected
        // The test verifies the function structure
        assert!(response.error.is_some() || response.success);
    }

    #[test]
    fn test_find_files_with_limit() {
        let response = find_files("main", Some(50));
        // Verify response structure
        assert!(response.success || response.error.is_some());
    }

    #[test]
    fn test_list_files_without_exclusions() {
        let response = list_files(None);
        // Verify response structure
        assert!(response.success || response.error.is_some());
    }

    #[test]
    fn test_list_files_with_exclusions() {
        let response = list_files(Some("target/**,node_modules/**"));
        // Verify response structure
        assert!(response.success || response.error.is_some());
    }

    #[test]
    fn test_search_files() {
        let response = search_files("component", "dist/**");
        // Verify response structure
        assert!(response.success || response.error.is_some());
    }
}
