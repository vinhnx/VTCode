use crate::core::agent::session::AgentSessionState;
use crate::llm::provider::MessageRole;

/// Checks if the agent's response indicates that the task has been completed.
pub fn check_completion_indicators(response_text: &str) -> bool {
    // High-confidence terminal markers that strongly indicate intent to stop.
    const COMPLETION_SENTENCES: &[&str] = &[
        "the task is complete",
        "task has been completed",
        "i have successfully completed the task",
        "work is finished",
        "operation successful",
        "i am done",
        "no more actions needed",
        "successfully accomplished",
        "task is now complete",
        "everything is finished",
        "i've finished the task",
        "all requested changes have been applied",
        "i have finished all the work",
    ];

    // Lower-confidence markers that need to be at the core of the message.
    const SOFT_INDICATORS: &[&str] = &[
        "task completed",
        "task done",
        "all done",
        "finished.",
        "complete.",
        "done.",
    ];

    let response_lower = response_text.to_lowercase();

    // Strategy 1: Explicit terminal sentences
    if COMPLETION_SENTENCES
        .iter()
        .any(|&s| response_lower.contains(s))
    {
        return true;
    }

    // Strategy 2: Soft indicators that appear at the very end or are the entire message
    let trimmed = response_lower.trim();
    for &indicator in SOFT_INDICATORS {
        if trimmed.ends_with(indicator) || trimmed == indicator {
            // Heuristic: Ensure it's not "I will soon have the task completed"
            // Check if preceded by future-tense markers within the same sentence
            let sentences: Vec<_> = trimmed.split(['.', '!', '?']).collect();
            if let Some(last_sentence) = sentences.last() {
                let ls = last_sentence.trim();
                if ls.contains(indicator)
                    && !ls.contains("will")
                    && !ls.contains("going to")
                    && !ls.contains("about to")
                    && !ls.contains("once")
                    && !ls.contains("after")
                {
                    return true;
                }
            }
        }
    }

    false
}

/// Check for repetitive text in assistant responses to catch non-tool-calling loops.
/// Returns true if a loop is detected.
pub fn check_for_response_loop(response_text: &str, session_state: &mut AgentSessionState) -> bool {
    if response_text.len() < 10 {
        return false;
    }

    // Simplistic check: is this response identical to the last one (ignoring whitespace)?
    let normalized_current = response_text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let repeated = session_state
        .messages
        .iter()
        .rev()
        .filter(|m| m.role == MessageRole::Assistant)
        .take(2)
        .any(|m| {
            let normalized_prev = m
                .content
                .as_text()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            normalized_prev == normalized_current
        });

    if repeated {
        let warning =
            "Repetitive assistant response detected. Breaking potential loop.".to_string();
        session_state.warnings.push(warning);
        session_state.consecutive_idle_turns =
            session_state.consecutive_idle_turns.saturating_add(1);
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_indicators() {
        assert!(check_completion_indicators("The task is complete"));
        assert!(check_completion_indicators(
            "I have successfully completed the task."
        ));
        assert!(check_completion_indicators("Task done"));
        assert!(check_completion_indicators("All done"));

        // Negative cases
        assert!(!check_completion_indicators(
            "I will have the task done soon"
        ));
        assert!(!check_completion_indicators("Is the task done?"));
        assert!(!check_completion_indicators("random text"));
    }
}
