//! Safety checks for VTCode operations
//!
//! This module provides safety validations for potentially expensive
//! or resource-intensive operations to ensure user control and efficiency.

use crate::config::models::ModelId;
use crate::ui::user_confirmation::{AgentMode, UserConfirmation};
use anyhow::Result;
use console::style;

/// Safety validation utilities for VTCode operations
pub struct SafetyValidator;

impl SafetyValidator {
    /// Validate and potentially request confirmation for model usage
    /// Returns the approved model to use, which may be different from the requested model
    pub fn validate_model_usage(
        requested_model: &str,
        task_description: Option<&str>,
        skip_confirmations: bool,
    ) -> Result<String> {
        use crate::config::constants::models;
        // Parse the requested model
        let model_id = match requested_model {
            s if s == models::GEMINI_2_5_PRO => Some(ModelId::Gemini25Pro),
            s if s == models::GEMINI_2_5_FLASH_PREVIEW => Some(ModelId::Gemini25FlashPreview),
            s if s == models::GEMINI_2_5_PRO => Some(ModelId::Gemini25Pro),
            _ => None,
        };

        // Check if this is the most capable (and expensive) model
        if let Some(ModelId::Gemini25Pro) = model_id {
            let current_default = ModelId::default();

            if skip_confirmations {
                println!(
                    "{}",
                    style("Using Gemini 2.5 Pro model (confirmations skipped)").yellow()
                );
                return Ok(requested_model.to_string());
            }

            if let Some(task) = task_description {
                println!("{}", style("Model Selection Review").cyan().bold());
                println!("Task: {}", style(task).cyan());
                println!();
            }

            // Ask for explicit confirmation before using the most capable model
            let confirmed = UserConfirmation::confirm_pro_model_usage(current_default.as_str())?;
            if !confirmed {
                println!(
                    "Falling back to default model: {}",
                    current_default.display_name()
                );
                return Ok(current_default.as_str().to_string());
            }
        }

        Ok(requested_model.to_string())
    }

    /// Validate agent mode selection based on task complexity and user preferences
    /// Returns the recommended agent mode with user confirmation if needed
    pub fn validate_agent_mode(
        _task_description: &str,
        _skip_confirmations: bool,
    ) -> Result<AgentMode> {
        // Always use single-agent mode
        println!(
            "{}",
            style("Using single-agent mode with Decision Ledger").green()
        );
        Ok(AgentMode::SingleCoder)
    }

    /// Check if a model switch is safe and cost-effective
    pub fn is_model_switch_safe(from_model: &str, to_model: &str) -> bool {
        use std::str::FromStr;
        let from_id = ModelId::from_str(from_model).ok();
        let to_id = ModelId::from_str(to_model).ok();

        match (from_id, to_id) {
            (Some(from), Some(to)) => {
                // Switching to Pro model requires confirmation
                !matches!(to, ModelId::Gemini25Pro) || matches!(from, ModelId::Gemini25Pro)
            }
            _ => true, // Unknown models are allowed
        }
    }

    /// Display safety recommendations for the current configuration
    pub fn display_safety_recommendations(
        model: &str,
        agent_mode: &AgentMode,
        task_description: Option<&str>,
    ) {
        println!("{}", style(" Safety Configuration Summary").cyan().bold());
        println!("Model: {}", style(model).green());
        println!("Agent Mode: {}", style(format!("{:?}", agent_mode)).green());

        if let Some(task) = task_description {
            println!("Task: {}", style(task).cyan());
        }

        println!();

        // Model-specific recommendations
        use crate::config::constants::models;
        match model {
            s if s == models::GEMINI_2_5_FLASH_PREVIEW => {
                println!("{}", style("[FAST] Using balanced model:").green());
                println!("• Good quality responses");
                println!("• Reasonable cost");
                println!("• Fast response times");
            }
            s if s == models::GEMINI_2_5_PRO => {
                println!("{}", style("Using most capable model:").yellow());
                println!("• Highest quality responses");
                println!("• Higher cost per token");
                println!("• Slower response times");
            }
            _ => {}
        }

        // Agent mode recommendations
        match agent_mode {
            AgentMode::SingleCoder => {
                println!("{}", style("Single-Agent System:").blue());
                println!("• Streamlined execution");
                println!("• Decision Ledger tracking");
                println!("• Lower API costs");
                println!("• Faster task completion");
                println!("• Best for most development tasks");
            }
        }

        println!();
    }

    /// Validate resource usage and warn about potential costs
    pub fn validate_resource_usage(
        model: &str,
        _agent_mode: &AgentMode,
        estimated_tokens: Option<usize>,
    ) -> Result<bool> {
        use crate::config::constants::models;
        let mut warnings = Vec::new();

        // Check for expensive model usage
        if model == models::GEMINI_2_5_PRO {
            warnings.push("Using most expensive model (Gemini 2.5 Pro)");
        }

        // Single-agent mode uses standard resource usage

        // Check for high token usage
        if let Some(tokens) = estimated_tokens
            && tokens > 10000
        {
            warnings.push("High token usage estimated (>10k tokens)");
        }

        if !warnings.is_empty() {
            println!("{}", style(" Resource Usage Warning").yellow().bold());
            for warning in &warnings {
                println!("• {}", warning);
            }
            println!();

            let confirmed = UserConfirmation::confirm_action(
                "Do you want to proceed with these resource usage implications?",
                false,
            )?;

            return Ok(confirmed);
        }

        Ok(true)
    }
}
