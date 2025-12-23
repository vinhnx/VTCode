//! Centralized reasoning utilities for detecting and transforming agent reasoning.
//!
//! This module provides functions for detecting patterns in agent reasoning.

/// Detects if reasoning text indicates the agent is giving up on a task.
///
/// Looks for patterns like "stop", "probably stop", "can't continue", "too complex", etc.
#[allow(dead_code)]
pub fn is_giving_up_reasoning(text: &str) -> bool {
    let lower = text.to_lowercase();

    // Patterns indicating the agent is giving up
    let giving_up_patterns = [
        "stop",
        "can't continue",
        "cannot continue",
        "too complex",
        "probably stop",
        "should stop",
        "unable to continue",
        "give up",
    ];

    giving_up_patterns
        .iter()
        .any(|pattern| lower.contains(pattern))
}

/// Generates constructive reasoning to replace giving-up reasoning.
///
/// Transforms defeatist reasoning into actionable next steps.
#[allow(dead_code)]
pub fn get_constructive_reasoning(original: &str) -> String {
    let lower = original.to_lowercase();

    // Determine the context from the original reasoning
    let is_pdf = lower.contains("pdf");
    let is_tool = lower.contains("tool") || lower.contains("execut");

    if is_pdf {
        "Let me try a solution that breaks down the PDF generation into smaller steps, focusing on file path validation and incremental output.".to_string()
    } else if is_tool {
        "Let me try a different approach to tool execution, breaking it down into smaller, more manageable steps.".to_string()
    } else {
        "Let me try a solution by breaking this down into smaller, more manageable steps."
            .to_string()
    }
}
