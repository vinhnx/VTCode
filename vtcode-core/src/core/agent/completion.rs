use crate::core::agent::session::AgentSessionState;
use crate::llm::provider::MessageRole;

/// Checks if the agent's response indicates that the task has been completed.
pub fn check_completion_indicators(response_text: &str) -> bool {
    // High-confidence terminal markers that strongly indicate intent to stop.
    const COMPLETION_SENTENCES: &[&str] = &[
        "the task is complete",
        "task is complete",
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

    // Strategy 3: Structured subagent markdown contract output.
    // When the model produces the canonical "## Summary / ## Facts / ..." contract, it has
    // finished its task even without an explicit done phrase.  Detect this by checking that
    // the response opens with a "## Summary" heading (after stripping leading whitespace) and
    // also contains a "## Facts" section.  Headers are matched line-by-line after trimming so
    // CRLF, extra spaces, and capitalisation variations are handled uniformly.
    {
        let mut has_summary_header = false;
        let mut has_facts_header = false;
        for line in response_text.lines() {
            let line_lower = line.trim().to_lowercase();
            if line_lower == "## summary" || line_lower == "# summary" {
                has_summary_header = true;
            }
            if line_lower == "## facts" || line_lower == "# facts" {
                has_facts_header = true;
            }
            if has_summary_header && has_facts_header {
                return true;
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
        .skip(1)
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
    use crate::llm::provider::Message;

    #[test]
    fn test_completion_indicators() {
        assert!(check_completion_indicators("The task is complete"));
        assert!(check_completion_indicators("Revision 1: task is complete."));
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

    #[test]
    fn subagent_markdown_contract_detected_as_complete() {
        let contract = "## Summary\n- Background subprocess launched; PID 86065.\n\n## Facts\n- Script started at 2026-04-25T08:39:10Z.\n\n## Touched Files\n- None\n\n## Verification\n- Process confirmed.\n\n## Open Questions\n- None";
        assert!(check_completion_indicators(contract));
    }

    #[test]
    fn subagent_markdown_contract_with_crlf_detected_as_complete() {
        let contract = "## Summary\r\n- Done.\r\n\r\n## Facts\r\n- Fact 1.\r\n";
        assert!(check_completion_indicators(contract));
    }

    #[test]
    fn subagent_markdown_contract_with_leading_whitespace_detected() {
        let contract = "\n\n## Summary\n- item\n\n## Facts\n- fact\n";
        assert!(check_completion_indicators(contract));
    }

    #[test]
    fn document_with_only_summary_header_not_detected() {
        let doc = "## Summary\n- This is a doc without a Facts section.\n";
        assert!(!check_completion_indicators(doc));
    }

    #[test]
    fn document_with_only_facts_header_not_detected() {
        let doc = "## Facts\n- Fact without summary.\n";
        assert!(!check_completion_indicators(doc));
    }

    #[test]
    fn response_loop_ignores_current_assistant_message() {
        let repeated_response = "The task is complete";
        let mut state = AgentSessionState::new("session".to_string(), 8, 4, 128_000);
        state
            .messages
            .push(Message::assistant(repeated_response.to_string()));

        assert!(!check_for_response_loop(repeated_response, &mut state));
    }

    #[test]
    fn response_loop_still_detects_prior_duplicate_assistant_message() {
        let repeated_response = "The task is complete";
        let mut state = AgentSessionState::new("session".to_string(), 8, 4, 128_000);
        state
            .messages
            .push(Message::assistant(repeated_response.to_string()));
        state
            .messages
            .push(Message::assistant(repeated_response.to_string()));

        assert!(check_for_response_loop(repeated_response, &mut state));
    }
}
