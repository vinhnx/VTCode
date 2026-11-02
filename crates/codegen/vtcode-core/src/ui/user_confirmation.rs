//! User confirmation utilities for safety-critical operations
//!
//! This module provides utilities for asking user confirmation before
//! performing operations that may be expensive or require explicit consent.

use crate::utils::colors::style;
use anyhow::Result;
use dialoguer::{Confirm, Input, Select};

/// User confirmation utilities for safety-critical operations
pub struct UserConfirmation;

/// Result of a tool confirmation prompt
#[derive(Debug, Clone, PartialEq)]
pub enum ToolConfirmationResult {
    /// Allow this specific execution
    Yes,
    /// Allow this and all future executions of this tool
    YesAutoAccept,
    /// Deny this execution
    No,
    /// Deny and provide feedback to the agent
    Feedback(String),
}

impl UserConfirmation {
    /// Simple yes/no confirmation with custom message
    #[must_use = "confirmation action failure is silently ignored"]
    pub fn confirm_action(message: &str, default: bool) -> Result<bool> {
        Confirm::new()
            .with_prompt(message)
            .default(default)
            .interact()
            .map_err(Into::into)
    }

    /// Ask for detailed confirmation for tool usage
    pub fn confirm_tool_usage(tool_name: &str, tool_args: Option<&str>) -> Result<ToolConfirmationResult> {
        println!("{}", style("Tool Execution Confirmation").cyan().bold());
        println!("Tool: {}", style(tool_name).cyan().bold());
        if let Some(args) = tool_args {
            println!("Args: {}", style(args).dim());
        }
        println!();

        let options = vec![
            "Yes - Allow this execution",
            "Yes - Always allow this tool (Auto-accept)",
            "No - Deny this execution",
            "No - Deny and provide feedback to agent",
        ];

        let selection = Select::new()
            .with_prompt("How would you like to proceed?")
            .default(0)
            .items(&options)
            .interact()?;

        match selection {
            0 => Ok(ToolConfirmationResult::Yes),
            1 => Ok(ToolConfirmationResult::YesAutoAccept),
            2 => Ok(ToolConfirmationResult::No),
            3 => {
                let feedback: String = Input::new()
                    .with_prompt("Enter feedback for the agent")
                    .allow_empty(false)
                    .interact_text()?;
                Ok(ToolConfirmationResult::Feedback(feedback))
            }
            _ => Ok(ToolConfirmationResult::No),
        }
    }
}
