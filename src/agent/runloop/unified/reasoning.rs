//! Centralized reasoning utilities for detecting and transforming agent reasoning.
//!
//! This module provides functions for detecting patterns in agent reasoning
//! and implementing Chain-of-Thought monitoring patterns from The Agentic AI Handbook.
//!
//! ## Chain-of-Thought Monitoring Pattern
//!
//! Monitor agent reasoning in real-time to enable early intervention:
//! - First tool call reveals understanding—monitor closely
//! - Interrupt immediately if wrong approach detected
//! - "Have your finger on the trigger to escape and interrupt any bad behavior"

use once_cell::sync::Lazy;
use regex::Regex;

static RUSHING_PATTERNS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(wrapping up|running out of time|quick summary|to conclude|finishing up|just to be safe|that.?s it|time to stop)").unwrap()
});

static UNCERTAIN_PATTERNS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(not sure|i.?m not certain|might be wrong|could be|sorry|apologies)").unwrap()
});

static COMPLEXITY_AVOIDANCE_PATTERNS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(too complex|too many|let.?s skip|for now|we can worry about|later)").unwrap()
});

/// Detects if reasoning text indicates the agent is giving up on a task.
///
/// Looks for patterns like "stop", "probably stop", "can't continue", "too complex", etc.
pub fn is_giving_up_reasoning(text: &str) -> bool {
    let lower = text.to_lowercase();

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

/// Detects rushing behavior indicating context anxiety.
pub fn is_rushing_to_conclude(text: &str) -> bool {
    RUSHING_PATTERNS.is_match(text)
}

/// Detects uncertainty in agent reasoning.
pub fn has_high_uncertainty(text: &str) -> bool {
    UNCERTAIN_PATTERNS.is_match(text)
}

/// Detects complexity avoidance patterns.
pub fn is_avoiding_complexity(text: &str) -> bool {
    COMPLEXITY_AVOIDANCE_PATTERNS.is_match(text)
}

/// Analyzes reasoning text for multiple concern patterns.
pub fn analyze_reasoning(text: &str) -> ReasoningAnalysis {
    ReasoningAnalysis {
        is_giving_up: is_giving_up_reasoning(text),
        is_rushing: is_rushing_to_conclude(text),
        has_uncertainty: has_high_uncertainty(text),
        is_avoiding_complexity: is_avoiding_complexity(text),
    }
}

/// Result of reasoning analysis with concern flags.
#[derive(Debug, Clone, Default)]
pub struct ReasoningAnalysis {
    pub is_giving_up: bool,
    pub is_rushing: bool,
    pub has_uncertainty: bool,
    pub is_avoiding_complexity: bool,
}

impl ReasoningAnalysis {
    pub fn has_concerns(&self) -> bool {
        self.is_giving_up || self.is_rushing || self.has_uncertainty || self.is_avoiding_complexity
    }

    pub fn priority_concern(&self) -> Option<ReasoningConcern> {
        if self.is_giving_up {
            Some(ReasoningConcern::GivingUp)
        } else if self.is_rushing {
            Some(ReasoningConcern::Rushing)
        } else if self.has_uncertainty {
            Some(ReasoningConcern::Uncertainty)
        } else if self.is_avoiding_complexity {
            Some(ReasoningConcern::AvoidingComplexity)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReasoningConcern {
    GivingUp,
    Rushing,
    Uncertainty,
    AvoidingComplexity,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_giving_up_detection() {
        assert!(is_giving_up_reasoning("I think we should stop here"));
        assert!(is_giving_up_reasoning("I can't continue with this task"));
        assert!(is_giving_up_reasoning("This is too complex"));
        assert!(!is_giving_up_reasoning("Let me try another approach"));
    }

    #[test]
    fn test_rushing_detection() {
        assert!(is_rushing_to_conclude("Wrapping up now"));
        assert!(is_rushing_to_conclude("Just to be safe, let's conclude"));
        assert!(!is_rushing_to_conclude("Let me continue investigating"));
    }

    #[test]
    fn test_uncertainty_detection() {
        assert!(has_high_uncertainty("I'm not sure about this"));
        assert!(has_high_uncertainty("This could be wrong"));
        assert!(!has_high_uncertainty("This is the correct approach"));
    }

    #[test]
    fn test_complexity_avoidance() {
        assert!(is_avoiding_complexity("This is too complex"));
        assert!(is_avoiding_complexity("Let's skip that for now"));
        assert!(!is_avoiding_complexity("Let me tackle this step by step"));
    }

    #[test]
    fn test_analysis() {
        let analysis = analyze_reasoning("I think we should stop here, it's too complex");
        assert!(analysis.has_concerns());
        assert_eq!(
            analysis.priority_concern(),
            Some(ReasoningConcern::GivingUp)
        );
    }

    #[test]
    fn test_analysis_no_concerns() {
        let analysis = analyze_reasoning("Let me continue with the next step");
        assert!(!analysis.has_concerns());
        assert_eq!(analysis.priority_concern(), None);
    }
}
