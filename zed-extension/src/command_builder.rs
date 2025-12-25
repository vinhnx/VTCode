//! Command Builder - Fluent API for constructing VT Code commands
//!
//! This module provides a builder pattern for constructing complex VTCode
//! commands with a clean, chainable API for better code readability.

use crate::executor::{execute_command, execute_command_with_timeout, CommandResult};
use std::time::Duration;

/// Fluent builder for VT Code commands
pub struct CommandBuilder {
    command: String,
    args: Vec<String>,
    timeout: Option<Duration>,
}

impl CommandBuilder {
    /// Create a new command builder
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            timeout: None,
        }
    }

    /// Add a single argument
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add multiple arguments
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(|a| a.into()));
        self
    }

    /// Add a key-value pair (e.g., --key value)
    pub fn with_option(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let key_str = key.into();
        let value_str = value.into();
        self.args.push(if key_str.starts_with("--") {
            key_str
        } else {
            format!("--{}", key_str)
        });
        self.args.push(value_str);
        self
    }

    /// Add a flag (e.g., --verbose)
    pub fn flag(mut self, flag: impl Into<String>) -> Self {
        let flag_str = flag.into();
        self.args.push(if flag_str.starts_with("--") {
            flag_str
        } else {
            format!("--{}", flag_str)
        });
        self
    }

    /// Set custom timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Execute the command
    pub fn execute(self) -> Result<CommandResult, String> {
        // Convert `Vec<String>` into `Vec<&str>` with a single preallocated buffer.
        let mut arg_refs: Vec<&str> = Vec::with_capacity(self.args.len());
        for s in &self.args {
            arg_refs.push(s.as_str());
        }

        if let Some(timeout) = self.timeout {
            execute_command_with_timeout(&self.command, &arg_refs, timeout)
        } else {
            execute_command(&self.command, &arg_refs)
        }
    }

    /// Execute and return output as string
    pub fn execute_output(self) -> Result<String, String> {
        self.execute().map(|result| result.output())
    }

    /// Get the final command for inspection/testing
    pub fn build_args(&self) -> Vec<String> {
        self.args.clone()
    }

    /// Get the command name
    pub fn get_command(&self) -> &str {
        &self.command
    }
}

/// Common command shortcuts
impl CommandBuilder {
    /// Create an "ask" command
    pub fn ask(query: impl Into<String>) -> Self {
        Self::new("ask").with_option("query", query)
    }

    /// Create an "analyze" command
    pub fn analyze() -> Self {
        Self::new("analyze")
    }

    /// Create a "chat" command
    pub fn chat() -> Self {
        Self::new("chat")
    }

    /// Create a "check-config" command
    pub fn check_config() -> Self {
        Self::new("check-config")
    }

    /// Create a version check command
    pub fn version() -> Self {
        Self::new("--version")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_builder_creation() {
        let builder = CommandBuilder::new("test");
        assert_eq!(builder.get_command(), "test");
        assert!(builder.build_args().is_empty());
    }

    #[test]
    fn test_add_single_argument() {
        let builder = CommandBuilder::new("ask").arg("hello");
        assert_eq!(builder.build_args(), vec!["hello"]);
    }

    #[test]
    fn test_add_multiple_arguments() {
        let builder = CommandBuilder::new("cmd").args(vec!["arg1", "arg2", "arg3"]);
        assert_eq!(builder.build_args().len(), 3);
    }

    #[test]
    fn test_with_option() {
        let builder = CommandBuilder::new("cmd").with_option("key", "value");
        let args = builder.build_args();
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], "--key");
        assert_eq!(args[1], "value");
    }

    #[test]
    fn test_with_option_double_dash() {
        let builder = CommandBuilder::new("cmd").with_option("--key", "value");
        let args = builder.build_args();
        assert_eq!(args[0], "--key");
    }

    #[test]
    fn test_flag() {
        let builder = CommandBuilder::new("cmd").flag("verbose");
        let args = builder.build_args();
        assert_eq!(args[0], "--verbose");
    }

    #[test]
    fn test_flag_double_dash() {
        let builder = CommandBuilder::new("cmd").flag("--debug");
        let args = builder.build_args();
        assert_eq!(args[0], "--debug");
    }

    #[test]
    fn test_chaining() {
        let builder = CommandBuilder::new("ask")
            .arg("test")
            .flag("verbose")
            .with_option("timeout", "30");

        let args = builder.build_args();
        assert_eq!(args.len(), 4);
        assert!(args.contains(&"test".to_string()));
        assert!(args.contains(&"--verbose".to_string()));
        assert!(args.contains(&"--timeout".to_string()));
        assert!(args.contains(&"30".to_string()));
    }

    #[test]
    fn test_timeout_setting() {
        let builder = CommandBuilder::new("analyze").timeout(Duration::from_secs(60));
        assert!(builder.timeout.is_some());
    }

    #[test]
    fn test_shortcut_ask() {
        let builder = CommandBuilder::ask("what is this?");
        assert_eq!(builder.get_command(), "ask");
        assert!(builder.build_args().len() >= 2);
    }

    #[test]
    fn test_shortcut_analyze() {
        let builder = CommandBuilder::analyze();
        assert_eq!(builder.get_command(), "analyze");
    }

    #[test]
    fn test_shortcut_chat() {
        let builder = CommandBuilder::chat();
        assert_eq!(builder.get_command(), "chat");
    }

    #[test]
    fn test_shortcut_version() {
        let builder = CommandBuilder::version();
        assert_eq!(builder.get_command(), "--version");
    }

    #[test]
    fn test_complex_command() {
        let builder = CommandBuilder::ask("explain this code")
            .with_option("language", "rust")
            .flag("detailed")
            .timeout(Duration::from_secs(45));

        let args = builder.build_args();
        assert!(args.len() >= 4);
    }
}
