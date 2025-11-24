/// Centralized error messages for tool operations
///
/// This module provides consistent, reusable error messages across all tool implementations
/// to ensure uniformity in user-facing error reporting.
/// File operation error messages
pub mod file_ops {
    pub const FILE_NOT_FOUND: &str = "File not found";
    pub const FILE_NOT_ACCESSIBLE: &str = "File is not accessible from the workspace";
    pub const INVALID_PATH: &str = "Invalid file path";
    pub const PATH_TRAVERSAL_ATTEMPT: &str = "Path traversal outside workspace not allowed";
    pub const PERMISSION_DENIED: &str = "Permission denied";
    pub const IS_DIRECTORY: &str = "Path points to a directory, not a file";
    pub const FILE_TOO_LARGE: &str = "File is too large to read";
    pub const ENCODING_ERROR: &str = "Failed to decode file (invalid UTF-8)";
    pub const IO_ERROR: &str = "Input/output error";
    pub const DIRECTORY_NOT_FOUND: &str = "Directory not found";
}

/// Command execution error messages
pub mod command_ops {
    pub const COMMAND_NOT_FOUND: &str = "Command not found";
    pub const COMMAND_BLOCKED: &str = "Command is not allowed for security reasons";
    pub const INVALID_COMMAND: &str = "Invalid command format";
    pub const COMMAND_TIMEOUT: &str = "Command execution timed out";
    pub const COMMAND_FAILED: &str = "Command execution failed";
    pub const EMPTY_COMMAND: &str = "Command cannot be empty";
    pub const SESSION_NOT_FOUND: &str = "PTY session not found";
    pub const SESSION_ALREADY_EXISTS: &str = "PTY session already exists";
    pub const SESSION_CLOSED: &str = "PTY session is closed";
}

/// Network operation error messages
pub mod network_ops {
    pub const URL_BLOCKED: &str = "URL is blocked for security reasons";
    pub const URL_INVALID: &str = "Invalid URL format";
    pub const CONNECTION_FAILED: &str = "Failed to connect to URL";
    pub const TIMEOUT: &str = "Network request timed out";
    pub const HTTP_ERROR: &str = "HTTP error";
    pub const SSL_ERROR: &str = "SSL/TLS certificate verification failed";
}

/// Code execution error messages
pub mod code_ops {
    pub const INVALID_LANGUAGE: &str = "Unsupported programming language";
    pub const EXECUTION_FAILED: &str = "Code execution failed";
    pub const EXECUTION_TIMEOUT: &str = "Code execution timed out";
    pub const OUTPUT_TOO_LARGE: &str = "Output is too large to return";
    pub const RUNTIME_ERROR: &str = "Runtime error";
}

/// Validation error messages
pub mod validation {
    pub const REQUIRED_PARAMETER_MISSING: &str = "Required parameter is missing";
    pub const INVALID_PARAMETER_TYPE: &str = "Invalid parameter type";
    pub const INVALID_PARAMETER_VALUE: &str = "Invalid parameter value";
    pub const PARAMETER_OUT_OF_RANGE: &str = "Parameter value is out of allowed range";
}

/// Tool system error messages
pub mod tool_system {
    pub const TOOL_NOT_FOUND: &str = "Tool not found";
    pub const TOOL_NOT_AVAILABLE: &str = "Tool is not available in this context";
    pub const TOOL_EXECUTION_DENIED: &str = "Tool execution is denied by policy";
    pub const TOOL_EXECUTION_REQUIRES_CONFIRMATION: &str =
        "Tool execution requires user confirmation";
    pub const INTERNAL_ERROR: &str = "Internal tool system error";
}

/// Patch operation error messages
pub mod patch_ops {
    pub const PATCH_INVALID: &str = "Invalid patch format";
    pub const PATCH_CONFLICT: &str = "Patch application conflicts with file content";
    pub const PATCH_NO_MATCHES: &str = "Patch has no matching hunks in file";
    pub const PATCH_PARTIAL_APPLY: &str = "Patch applied with conflicts";
}

/// Skill management error messages
pub mod skill_ops {
    pub const SKILL_NOT_FOUND: &str = "Skill not found";
    pub const SKILL_ALREADY_EXISTS: &str = "Skill already exists";
    pub const INVALID_SKILL_FORMAT: &str = "Invalid skill format";
    pub const SKILL_SAVE_FAILED: &str = "Failed to save skill";
    pub const SKILL_LOAD_FAILED: &str = "Failed to load skill";
}

/// Diagnostic tool error messages
pub mod diagnostics {
    pub const NO_ERRORS_RECORDED: &str = "No errors recorded in session";
    pub const INVALID_SCOPE: &str = "Invalid error scope";
    pub const AGENT_STATE_UNAVAILABLE: &str = "Agent state is not available for inspection";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_messages_are_not_empty() {
        assert!(!file_ops::FILE_NOT_FOUND.is_empty());
        assert!(!command_ops::COMMAND_NOT_FOUND.is_empty());
        assert!(!network_ops::URL_BLOCKED.is_empty());
        assert!(!code_ops::EXECUTION_FAILED.is_empty());
        assert!(!validation::REQUIRED_PARAMETER_MISSING.is_empty());
        assert!(!tool_system::TOOL_NOT_FOUND.is_empty());
        assert!(!patch_ops::PATCH_INVALID.is_empty());
        assert!(!skill_ops::SKILL_NOT_FOUND.is_empty());
        assert!(!diagnostics::NO_ERRORS_RECORDED.is_empty());
    }
}
