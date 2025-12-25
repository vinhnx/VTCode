//! Example agent implementations and usage patterns

/// Example agent configurations and usage patterns
pub struct AgentExamples;

/// Basic agent usage example
impl AgentExamples {
    /// Create a simple agent example
    pub fn basic_example() -> &'static str {
        r#"
# Basic VT Code Usage Example

This example shows how to use VT Code for code analysis and tool execution.

## Available Tools:
- grep_file: Ripgrep-backed code search
- bash: Bash-like commands with PTY support
- run_pty_cmd: Terminal command execution

## Example Workflow:
1. Use grep_file for code search
2. Use bash for system operations
3. Use run_pty_cmd for complex terminal tasks
"#
    }

    /// Advanced agent usage example
    pub fn advanced_example() -> &'static str {
        r#"
# Advanced VT Code Usage

## Tool Integration:
- All tools now support PTY for terminal emulation
- AST-based code analysis and transformation
"#
    }
}
