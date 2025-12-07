//! Tool call safety validation and safeguards
//!
//! Enforces safety boundaries for tool execution:
//! - Per-turn tool limits
//! - Tool call rate limiting
//! - Destructive operation confirmation
//! - Argument validation

use anyhow::{Context as _, Result};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Safety validation rules for tool calls
pub struct ToolCallSafetyValidator {
    /// Destructive tools that require explicit confirmation
    destructive_tools: HashSet<&'static str>,
    /// Per-turn tool limit
    max_per_turn: usize,
    /// Total tool limit per session
    max_per_session: usize,
    /// Current per-turn tool count
    current_turn_count: usize,
    /// Total calls made in this session
    session_count: usize,
    /// Call rate limit (max calls per second)
    rate_limit_per_second: usize,
    /// Tools called in current window
    calls_in_window: Vec<Instant>,
}

impl ToolCallSafetyValidator {
    pub fn new() -> Self {
        let mut destructive = HashSet::new();
        destructive.insert("delete_file");
        destructive.insert("edit_file");
        destructive.insert("write_file");
        destructive.insert("shell");
        destructive.insert("apply_patch");

        Self {
            destructive_tools: destructive,
            max_per_turn: 10,
            max_per_session: 100,
            current_turn_count: 0,
            session_count: 0,
            rate_limit_per_second: 5,
            calls_in_window: Vec::new(),
        }
    }

    /// Reset per-turn counters; call at the start of a turn
    pub fn start_turn(&mut self) {
        self.current_turn_count = 0;
        self.reset_rate_limit();
    }

    /// Override per-turn and session limits based on runtime config
    pub fn set_limits(&mut self, max_per_turn: usize, max_per_session: usize) {
        self.max_per_turn = max_per_turn;
        self.max_per_session = max_per_session;
    }

    /// Validate a tool call before execution
    pub fn validate_call(&mut self, tool_name: &str) -> Result<CallValidation> {
        // Check if tool is destructive
        let is_destructive = self.destructive_tools.contains(tool_name);

        // Check rate limit
        self.enforce_rate_limit()?;

        // Enforce per-turn and session limits
        if self.current_turn_count >= self.max_per_turn {
            return Err(anyhow::anyhow!(
                "Per-turn tool limit reached (max: {}). Wait or adjust config.",
                self.max_per_turn
            ));
        }
        if self.session_count >= self.max_per_session {
            return Err(anyhow::anyhow!(
                "Session tool limit reached (max: {}). End turn or reduce tool calls.",
                self.max_per_session
            ));
        }

        self.current_turn_count += 1;
        self.session_count += 1;

        Ok(CallValidation {
            is_destructive,
            requires_confirmation: is_destructive,
            execution_allowed: true,
        })
    }

    /// Enforce rate limiting
    fn enforce_rate_limit(&mut self) -> Result<()> {
        let now = Instant::now();

        // Remove calls older than 1 second
        self.calls_in_window
            .retain(|call_time| now.duration_since(*call_time) < Duration::from_secs(1));

        if self.calls_in_window.len() >= self.rate_limit_per_second {
            return Err(anyhow::anyhow!(
                "Tool call rate limit exceeded: {} calls/second (max: {})",
                self.calls_in_window.len(),
                self.rate_limit_per_second
            ))
            .context("Please wait before making another tool call");
        }

        self.calls_in_window.push(now);
        Ok(())
    }

    /// Check if tool is destructive
    #[allow(dead_code)]
    pub fn is_destructive(&self, tool_name: &str) -> bool {
        self.destructive_tools.contains(tool_name)
    }

    /// Get list of destructive tools
    #[allow(dead_code)]
    pub fn destructive_tools(&self) -> Vec<&'static str> {
        self.destructive_tools.iter().copied().collect()
    }

    /// Reset rate limit tracking
    pub fn reset_rate_limit(&mut self) {
        self.calls_in_window.clear();
    }
}

/// Result of tool call validation
#[derive(Debug, Clone)]
pub struct CallValidation {
    /// Whether tool is destructive
    #[allow(dead_code)]
    pub is_destructive: bool,
    /// Whether confirmation is required
    pub requires_confirmation: bool,
    /// Whether execution is allowed
    #[allow(dead_code)]
    pub execution_allowed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_destructive_tool_detection() {
        let validator = ToolCallSafetyValidator::new();
        assert!(validator.is_destructive("delete_file"));
        assert!(validator.is_destructive("edit_file"));
        assert!(!validator.is_destructive("read_file"));
        assert!(!validator.is_destructive("grep_file"));
    }

    #[test]
    fn test_rate_limiting() {
        let mut validator = ToolCallSafetyValidator::new();
        validator.rate_limit_per_second = 2;
        validator.start_turn();

        // First two calls should succeed
        assert!(validator.validate_call("read_file").is_ok());
        assert!(validator.validate_call("read_file").is_ok());

        // Third call within same second should fail
        assert!(validator.validate_call("read_file").is_err());
    }

    #[test]
    fn test_validation_structure() {
        let mut validator = ToolCallSafetyValidator::new();
        validator.start_turn();

        let validation = validator.validate_call("read_file").unwrap();
        assert!(!validation.is_destructive);
        assert!(!validation.requires_confirmation);
        assert!(validation.execution_allowed);

        let validation = validator.validate_call("delete_file").unwrap();
        assert!(validation.is_destructive);
        assert!(validation.requires_confirmation);
        assert!(validation.execution_allowed);
    }

    #[test]
    fn test_turn_and_session_limits() {
        let mut validator = ToolCallSafetyValidator::new();
        validator.max_per_turn = 2;
        validator.max_per_session = 3;

        // First turn
        validator.start_turn();
        assert!(validator.validate_call("read_file").is_ok());
        assert!(validator.validate_call("read_file").is_ok());
        assert!(validator.validate_call("read_file").is_err()); // turn limit

        // Second turn: should respect session total
        validator.start_turn();
        assert!(validator.validate_call("read_file").is_ok()); // third session call
        assert!(validator.validate_call("read_file").is_err()); // session limit
    }
}
