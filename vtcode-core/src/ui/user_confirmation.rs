//! User confirmation utilities for safety-critical operations
//!
//! This module provides utilities for asking user confirmation before
//! performing operations that may be expensive or require explicit consent.

use crate::utils::colors::style;
use anyhow::Result;
use dialoguer::{Confirm, Select};
// use std::io::Write;

/// User confirmation utilities for safety-critical operations
pub struct UserConfirmation;

impl UserConfirmation {
    /// Ask for confirmation before switching to the most capable model (Gemini 2.5 Pro)
    /// This is critical for ensuring user control over potentially expensive operations
    pub fn confirm_pro_model_usage(current_model: &str) -> Result<bool> {
        use crate::config::constants::models;
        println!("{}", style("Model Upgrade Required").yellow().bold());
        println!("Current model: {}", style(current_model).cyan());
        println!(
            "Requested model: {}",
            style(models::GEMINI_2_5_PRO).cyan().bold()
        );
        println!();
        println!("The Gemini 2.5 Pro model is the most capable but also:");
        println!("• More expensive per token");
        println!("• Slower response times");
        println!("• Higher resource usage");
        println!();

        let confirmed = Confirm::new()
            .with_prompt("Do you want to proceed with the more capable (and expensive) Gemini 2.5 Pro model?")
            .default(false)
            .interact()?;

        if confirmed {
            println!("{}", style("Confirmed: Using Gemini 2.5 Pro model").green());
        } else {
            println!("{}", style("Cancelled: Keeping current model").yellow());
        }

        Ok(confirmed)
    }

    /// Present agent mode selection options to the user
    pub fn select_agent_mode() -> Result<AgentMode> {
        println!("{}", style("Agent Mode Selection").cyan().bold());
        println!(
            "VTCode now uses single-agent mode with Decision Ledger for reliable task execution."
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
                    style("Moderate task - Single agent usually sufficient").yellow()
                );
            }
            TaskComplexity::Complex => {
                println!(
                    "{}",
                    style("Complex task detected - proceeding with single-agent mode").blue()
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
        println!("{}", style(" Warning").yellow().bold());
        println!("{}", message);
        println!();

        Confirm::new()
            .with_prompt("Press Enter to continue or Ctrl+C to cancel")
            .default(true)
            .interact()?;

        Ok(())
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
