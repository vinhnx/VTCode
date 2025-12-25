//! VT Code Command Implementations
//!
//! This module implements all VT Code commands that are exposed through
//! Zed's command palette. Uses CommandBuilder for cleaner construction.
use crate::command_builder::CommandBuilder;

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
    match CommandBuilder::ask(query).execute() {
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
    // Construct a query that includes the code and optional language
    let query = format!(
        "Analyze this {} code:\n{}",
        language.unwrap_or(""),
        code
    );

    ask_agent(&query)
}

/// Analyze the entire workspace
pub fn analyze_workspace() -> CommandResponse {
    match CommandBuilder::analyze().execute() {
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
    match CommandBuilder::chat().execute() {
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
}
