//! Safety checks for VT Code operations
//!
//! This module provides safety validations for potentially expensive
//! or resource-intensive operations to ensure user control and efficiency.

use crate::config::models::ModelId;
use crate::ui::user_confirmation::{AgentMode, ProModelConfirmationResult, UserConfirmation};
use crate::utils::colors::style;
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};

static PRO_MODEL_AUTO_ACCEPT: AtomicBool = AtomicBool::new(false);

/// Safety validation utilities for VT Code operations
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
            s if s == models::google::GEMINI_3_1_PRO_PREVIEW => Some(ModelId::Gemini31ProPreview),
            s if s == models::google::GEMINI_3_FLASH_PREVIEW => Some(ModelId::Gemini3FlashPreview),
            _ => None,
        };

        // Check if this is the most capable (and expensive) model
        if let Some(ModelId::Gemini31ProPreview) = model_id {
            let current_default = ModelId::default();

            if skip_confirmations {
                println!(
                    "{}",
                    style("Using Gemini 3.1 Pro model (confirmations skipped)").cyan()
                );
                return Ok(requested_model.to_string());
            }

            if PRO_MODEL_AUTO_ACCEPT.load(Ordering::Relaxed) {
                println!(
                    "{}",
                    style("Using Gemini 3.1 Pro model (auto-accept enabled)").cyan()
                );
                return Ok(requested_model.to_string());
            }

            if let Some(task) = task_description {
                println!("{}", style("Model Selection Review").cyan().bold());
                println!("Task: {}", style(task).cyan());
                println!();
            }

            // Ask for explicit confirmation before using the most capable model
            match UserConfirmation::confirm_pro_model_usage(current_default.as_str())? {
                ProModelConfirmationResult::Yes => {}
                ProModelConfirmationResult::YesAutoAccept => {
                    PRO_MODEL_AUTO_ACCEPT.store(true, Ordering::Relaxed);
                }
                ProModelConfirmationResult::No => {
                    println!(
                        "Falling back to default model: {}",
                        current_default.display_name()
                    );
                    return Ok(current_default.to_string());
                }
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
                !matches!(to, ModelId::Gemini31ProPreview)
                    || matches!(from, ModelId::Gemini31ProPreview)
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
            s if s == models::google::GEMINI_3_FLASH_PREVIEW => {
                println!("{}", style("[FAST] Using balanced model:").green());
                println!("• Good quality responses");
                println!("• Reasonable cost");
                println!("• Fast response times");
            }
            s if s == models::google::GEMINI_3_PRO_PREVIEW => {
                println!("{}", style("Using most capable model:").cyan());
                println!("• Highest quality responses");
                println!("• Higher cost per token");
                println!("• Slower response times");
            }
            _ => {}
        }

        // Agent mode recommendations
        match agent_mode {
            AgentMode::SingleCoder => {
                println!("{}", style("Single-Agent System:").cyan());
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
        if model == models::google::GEMINI_3_PRO_PREVIEW {
            warnings.push("Using most expensive model (Gemini 3 Pro)");
        }

        // Single-agent mode uses standard resource usage

        // Check for high token usage
        if let Some(tokens) = estimated_tokens
            && tokens > 10000
        {
            warnings.push("High token usage estimated (>10k tokens)");
        }

        if !warnings.is_empty() {
            println!("{}", style(" Resource Usage Warning").red().bold());
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
