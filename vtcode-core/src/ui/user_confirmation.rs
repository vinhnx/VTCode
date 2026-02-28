//! User confirmation utilities for safety-critical operations
//!
//! This module provides utilities for asking user confirmation before
//! performing operations that may be expensive or require explicit consent.

use crate::utils::colors::style;
use anyhow::Result;
use dialoguer::{Confirm, Input, Select};
// use std::io::Write;

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

/// Result of prompting for Gemini Pro model usage
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProModelConfirmationResult {
    /// Approve for this invocation only
    Yes,
    /// Approve this and future invocations in the current process
    YesAutoAccept,
    /// Deny and fallback to default model
    No,
}

impl UserConfirmation {
    /// Ask for confirmation before switching to the most capable model (Gemini 3 Pro)
    /// This is critical for ensuring user control over potentially expensive operations
    pub fn confirm_pro_model_usage(current_model: &str) -> Result<ProModelConfirmationResult> {
        use crate::config::constants::models;
        println!("{}", style("Model Upgrade Required").red().bold());
        println!("Current model: {}", style(current_model).cyan());
        println!(
            "Requested model: {}",
            style(models::google::GEMINI_3_1_PRO_PREVIEW).cyan().bold()
        );
        println!();
        println!("The Gemini 3 Pro model is the most capable but also:");
        println!("• More expensive per token");
        println!("• Slower response times");
        println!("• Higher resource usage");
        println!();

        let options = vec![
            "Yes - Use Pro model for this task",
            "Yes - Always use Pro model (Auto-accept)",
            "No - Use default model instead",
        ];

        let selection = Select::new()
            .with_prompt("How would you like to proceed?")
            .default(0)
            .items(&options)
            .interact()?;

        match selection {
            0 => {
                println!(
                    "{}",
                    style("✓ Using Gemini 3 Pro model for this task").green()
                );
                Ok(ProModelConfirmationResult::Yes)
            }
            1 => {
                println!(
                    "{}",
                    style("✓ Using Gemini 3 Pro model (will auto-accept in future)").green()
                );
                Ok(ProModelConfirmationResult::YesAutoAccept)
            }
            2 => {
                println!("{}", style("✗ Keeping current model").red());
                Ok(ProModelConfirmationResult::No)
            }
            _ => Ok(ProModelConfirmationResult::No),
        }
    }

    /// Present agent mode selection options to the user
    pub fn select_agent_mode() -> Result<AgentMode> {
        println!("{}", style("Agent Mode Selection").cyan().bold());
        println!(
            "VT Code now uses single-agent mode with Decision Ledger for reliable task execution."
        );

        Ok(AgentMode::SingleCoder)
    }

    /// Ask for task complexity assessment to determine agent mode
    pub fn assess_task_complexity(task_description: &str) -> Result<TaskComplexity> {
        println!("{}", style("Task Complexity Assessment").cyan().bold());
        println!("Task: {}", style(task_description).cyan());
        println!();

        let options = vec![
            "Simple (single file edit, basic question, straightforward task)",
            "Moderate (multiple files, refactoring, testing)",
            "Complex (architecture changes, cross-cutting concerns, large refactoring)",
        ];

        let selection = Select::new()
            .with_prompt("How would you classify this task's complexity?")
            .default(0)
            .items(&options)
            .interact()?;

        let complexity = match selection {
            0 => TaskComplexity::Simple,
            1 => TaskComplexity::Moderate,
            2 => TaskComplexity::Complex,
            _ => TaskComplexity::Simple, // Default fallback
        };

        match complexity {
            TaskComplexity::Simple => {
                println!(
                    "{}",
                    style("Simple task - Single agent recommended").green()
                );
            }
            TaskComplexity::Moderate => {
                println!(
                    "{}",
                    style("Moderate task - Single agent usually sufficient").cyan()
                );
            }
            TaskComplexity::Complex => {
                println!(
                    "{}",
                    style("Complex task detected - proceeding with single-agent mode").cyan()
                );
            }
        }

        Ok(complexity)
    }

    /// Simple yes/no confirmation with custom message
    pub fn confirm_action(message: &str, default: bool) -> Result<bool> {
        Confirm::new()
            .with_prompt(message)
            .default(default)
            .interact()
            .map_err(Into::into)
    }

    /// Display a warning message and wait for user acknowledgment
    pub fn show_warning(message: &str) -> Result<()> {
        println!("{}", style(" Warning").red().bold());
        println!("{}", message);
        println!();

        Confirm::new()
            .with_prompt("Press Enter to continue or Ctrl+C to cancel")
            .default(true)
            .interact()?;

        Ok(())
    }

    /// Ask for detailed confirmation for tool usage
    pub fn confirm_tool_usage(
        tool_name: &str,
        tool_args: Option<&str>,
    ) -> Result<ToolConfirmationResult> {
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

/// Available agent modes
#[derive(Debug, Clone, PartialEq)]
pub enum AgentMode {
    /// Single coder agent with Decision Ledger - reliable for all tasks
    SingleCoder,
}

/// Task complexity levels for agent mode selection
#[derive(Debug, Clone, PartialEq)]
pub enum TaskComplexity {
    /// Simple tasks - single file edits, basic questions
    Simple,
    /// Moderate tasks - multiple files, refactoring
    Moderate,
    /// Complex tasks - architecture changes, large refactoring
    Complex,
}

impl TaskComplexity {
    /// Recommend agent mode based on task complexity
    pub fn recommended_agent_mode(&self) -> AgentMode {
        match self {
            TaskComplexity::Simple | TaskComplexity::Moderate => AgentMode::SingleCoder,
            TaskComplexity::Complex => AgentMode::SingleCoder, // Default to SingleCoder as MultiAgent is removed
        }
    }
}
