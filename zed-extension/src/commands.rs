/// VTCode Command Implementations
///
/// This module implements all VTCode commands that are exposed through
/// Zed's command palette.
use crate::executor::execute_command;

/// Response from a VTCode command operation
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

/// Ask the VTCode agent an arbitrary question
pub fn ask_agent(query: &str) -> CommandResponse {
    match execute_command("ask", &["--query", query]) {
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
    let mut args = vec!["--query", "Analyze this code:"];

    if let Some(lang) = language {
        args.extend_from_slice(&["--language", lang]);
    }

    // Note: This would need to pipe the code as stdin or use a temp file
    // For now, we'll construct a query that includes the code
    let query = format!("Analyze this {} code:\n{}", language.unwrap_or(""), code);

    ask_agent(&query)
}

/// Analyze the entire workspace
pub fn analyze_workspace() -> CommandResponse {
    match execute_command("analyze", &[]) {
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
    match execute_command("chat", &[]) {
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

/// Check VTCode CLI installation and status
pub fn check_status() -> CommandResponse {
    match execute_command("--version", &[]) {
        Ok(result) => {
            if result.is_success() {
                CommandResponse::ok(format!("VTCode CLI is available\n{}", result.stdout))
            } else {
                CommandResponse::err("VTCode CLI check failed".to_string())
            }
        }
        Err(e) => CommandResponse::err(format!("VTCode CLI not found: {}", e)),
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
