//! Planning workflow intent detection.
//!
//! This module isolates the logic for detecting user intent related to
//! planning workflow transitions (enter/exit/stay). It provides a clean
//! interface that decouples intent detection from the turn loop mechanics.
//!
//! ## Interface Guard Rails
//!
//! The `PlanningIntent` enum ensures that all possible intent states are
//! explicitly handled, preventing implicit fallthrough bugs. The detection
//! functions are pure (no side effects) and independently testable.

use vtcode_core::llm::provider as uni;

/// Represents the user's intent regarding planning workflow transitions.
///
/// This enum provides an exhaustive set of possible intents, ensuring
/// that all cases are handled explicitly rather than through boolean flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanningIntent {
    /// User wants to exit planning and start implementing.
    ExitAndImplement,
    /// User explicitly wants to stay in planning workflow.
    StayInPlanning,
    /// No planning-related intent detected.
    None,
}

/// Detect planning-related intent from user text.
///
/// This is the single entry point for intent detection. It checks for
/// stay-phrases first (higher priority), then exit-phrases, then
/// confirmation phrases.
///
/// # Arguments
/// * `text` - The user's input text
/// * `assistant_prompted_implementation` - Whether the assistant recently
///   asked the user if they want to implement
///
/// # Returns
/// A `PlanningIntent` indicating the user's intent.
pub(crate) fn detect_planning_intent(
    text: &str,
    assistant_prompted_implementation: bool,
) -> PlanningIntent {
    let normalized = normalize_user_intent_text(text);
    let trimmed = normalized.trim();

    // Priority 1: Explicit stay-in-planning phrases override everything.
    if is_stay_in_planning_phrase(&normalized) {
        return PlanningIntent::StayInPlanning;
    }

    // Priority 2: Direct exit commands (short imperative words).
    if is_direct_exit_command(trimmed) {
        return PlanningIntent::ExitAndImplement;
    }

    // Priority 3: Exit trigger phrases (longer phrases).
    if is_exit_trigger_phrase(&normalized) {
        return PlanningIntent::ExitAndImplement;
    }

    // Priority 4: Short confirmation when assistant recently prompted.
    if assistant_prompted_implementation && is_short_confirmation(trimmed) {
        return PlanningIntent::ExitAndImplement;
    }

    PlanningIntent::None
}

/// Detect intent to enter planning workflow from user text.
pub(crate) fn detect_enter_planning_intent(text: &str) -> bool {
    // Check for /plan command before normalization (slash is stripped by normalization).
    let trimmed_raw = text.trim();
    if trimmed_raw == "/plan" || trimmed_raw.to_ascii_lowercase().starts_with("/plan ") {
        return true;
    }

    let normalized = normalize_user_intent_text(text);

    let explicit_phrases = [
        "make a plan",
        "create a plan",
        "write a plan",
        "come up with a plan",
        "plan this",
        "stay in planning workflow",
        "keep planning",
        "continue planning",
        "before you implement make a plan",
        "before implementing make a plan",
        "outline the implementation plan",
    ];

    explicit_phrases
        .iter()
        .any(|phrase| normalized.contains(phrase))
}

/// Check if the assistant recently prompted for implementation.
///
/// Scans backward through working history to find the last assistant
/// message before the last user message, then checks if it contains
/// implementation-related cues.
pub(crate) fn assistant_recently_prompted_implementation(working_history: &[uni::Message]) -> bool {
    let Some(last_user_index) = working_history
        .iter()
        .rposition(|msg| msg.role == uni::MessageRole::User)
    else {
        return false;
    };

    let Some(last_assistant_msg) = working_history[..last_user_index]
        .iter()
        .rev()
        .find(|msg| msg.role == uni::MessageRole::Assistant)
    else {
        return false;
    };

    let assistant_text = normalize_user_intent_text(&last_assistant_msg.content.as_text());
    let cues = [
        "implement this plan",
        "implement the plan",
        "ready to implement",
        "exit planning workflow",
        "execute the plan",
        "switch out of planning workflow",
        "start implementation",
        "start implementing",
        "start coding",
    ];
    cues.iter().any(|cue| assistant_text.contains(cue))
}

// ============================================================================
// Internal helpers (pure functions, no side effects)
// ============================================================================

/// Normalize user text for intent matching.
///
/// Converts to lowercase and replaces non-alphanumeric chars with spaces
/// for flexible matching.
fn normalize_user_intent_text(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
}

/// Check if text contains explicit stay-in-planning phrases.
fn is_stay_in_planning_phrase(normalized: &str) -> bool {
    let stay_phrases = [
        "stay in planning workflow",
        "keep in planning workflow",
        "continue planning",
        "keep planning",
        "do not implement",
        "don t implement",
        "not ready to implement",
        "don t exit planning workflow",
        "do not exit planning workflow",
    ];
    stay_phrases
        .iter()
        .any(|phrase| normalized.contains(phrase))
}

/// Check if text is a direct exit command (short imperative).
fn is_direct_exit_command(trimmed: &str) -> bool {
    let direct_commands = [
        "implement",
        "yes",
        "go",
        "start",
        "implement now",
        "start implementing",
        "start implementation",
        "execute plan",
        "execute the plan",
        "execute this plan",
        "switch to agent mode",
        "exit planning workflow",
        "exit planning workflow and implement",
    ];
    direct_commands.contains(&trimmed)
}

/// Check if text contains an exit trigger phrase.
fn is_exit_trigger_phrase(normalized: &str) -> bool {
    let trigger_phrases = [
        "start implement",
        "start implementation",
        "start implementing",
        "implement now",
        "implement the plan",
        "implement this plan",
        "begin implement",
        "begin implementation",
        "begin coding",
        "proceed to implement",
        "proceed with implementation",
        "proceed to coding",
        "proceed with coding",
        "execute the plan",
        "execute this plan",
        "let s implement",
        "lets implement",
        "go ahead and implement",
        "go ahead and code",
        "ready to implement",
        "start coding",
        "start building",
        "switch to agent mode",
        "exit planning workflow",
        "exit planning workflow and implement",
    ];
    trigger_phrases
        .iter()
        .any(|phrase| normalized.contains(phrase))
}

/// Check if text is a short confirmation word.
fn is_short_confirmation(trimmed: &str) -> bool {
    let confirmation_tokens = [
        "yes",
        "y",
        "ok",
        "okay",
        "continue",
        "go",
        "go ahead",
        "proceed",
        "start",
        "start now",
        "begin",
        "begin now",
        "let s start",
        "lets start",
        "sounds good",
        "do it",
    ];
    confirmation_tokens.contains(&trimmed)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_implement_as_exit_intent() {
        assert_eq!(
            detect_planning_intent("implement", false),
            PlanningIntent::ExitAndImplement
        );
        assert_eq!(
            detect_planning_intent("Implement the plan.", false),
            PlanningIntent::ExitAndImplement
        );
    }

    #[test]
    fn detects_stay_in_planning_as_higher_priority() {
        assert_eq!(
            detect_planning_intent("Do not implement yet; keep planning.", false),
            PlanningIntent::StayInPlanning
        );
        assert_eq!(
            detect_planning_intent("Stay in planning workflow and don't implement.", false),
            PlanningIntent::StayInPlanning
        );
    }

    #[test]
    fn detects_short_confirmation_with_context() {
        assert_eq!(
            detect_planning_intent("yes", true),
            PlanningIntent::ExitAndImplement
        );
        assert_eq!(
            detect_planning_intent("continue", true),
            PlanningIntent::ExitAndImplement
        );
    }

    #[test]
    fn short_confirmation_without_context_is_none() {
        assert_eq!(
            detect_planning_intent("yes", false),
            PlanningIntent::ExitAndImplement // "yes" is a direct command
        );
        assert_eq!(
            detect_planning_intent("continue", false),
            PlanningIntent::None
        );
    }

    #[test]
    fn detects_enter_planning_intent() {
        assert!(detect_enter_planning_intent("make a plan for this"));
        assert!(detect_enter_planning_intent("/plan"));
        assert!(detect_enter_planning_intent(
            "before implementing, create a plan"
        ));
    }

    #[test]
    fn non_intent_text_returns_none() {
        assert_eq!(
            detect_planning_intent("The implementation details are unclear.", false),
            PlanningIntent::None
        );
    }

    #[test]
    fn assistant_prompted_implementation_detects_cues() {
        let history = vec![
            uni::Message::assistant("Implement this plan?".to_string()),
            uni::Message::user("yes".to_string()),
        ];
        assert!(assistant_recently_prompted_implementation(&history));
    }

    #[test]
    fn assistant_prompted_implementation_requires_cue() {
        let history = vec![
            uni::Message::assistant("Continue planning and expand the risks section.".to_string()),
            uni::Message::user("yes".to_string()),
        ];
        assert!(!assistant_recently_prompted_implementation(&history));
    }
}
