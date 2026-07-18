use crate::core::agent::session::AgentSessionState;
use crate::llm::provider::MessageRole;

/// Checks if the agent's response is a candidate for completion handling.
pub fn check_completion_candidate(response_text: &str) -> bool {
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

    const UNRESOLVED_PHRASES: &[&str] = &[
        "still need to",
        "remaining step",
        "remaining work",
        "verification pending",
        "verification still pending",
        "tests not run",
        "haven't run",
        "have not run",
        "blocked on",
        "open questions remain",
        "question remains",
        "todo:",
        "not complete yet",
        "once verification",
        "after verification",
    ];

    let response_lower = response_text.to_lowercase();

    if UNRESOLVED_PHRASES.iter().any(|phrase| response_lower.contains(phrase))
        || structured_contract_has_unresolved_sections(response_text)
    {
        return false;
    }

    // Strategy 1: Explicit terminal sentences
    if COMPLETION_SENTENCES.iter().any(|&s| response_lower.contains(s)) {
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

fn structured_contract_has_unresolved_sections(response_text: &str) -> bool {
    let mut in_open_questions = false;
    let mut in_verification = false;

    for line in response_text.lines() {
        let line_lower = line.trim().to_lowercase();
        if line_lower.starts_with('#') {
            in_open_questions = line_lower == "## open questions" || line_lower == "# open questions";
            in_verification = line_lower == "## verification" || line_lower == "# verification";
            continue;
        }

        if line_lower.is_empty() {
            continue;
        }

        if in_open_questions && !section_entry_is_none(&line_lower) {
            return true;
        }

        if in_verification && section_entry_is_unresolved(&line_lower) {
            return true;
        }
    }

    false
}

fn section_entry_is_none(line: &str) -> bool {
    let normalized = normalized_section_entry(line).trim_end_matches('.');
    matches!(normalized, "none" | "n/a")
}

fn section_entry_is_unresolved(line: &str) -> bool {
    let normalized = normalized_section_entry(line);
    normalized.contains("pending")
        || normalized.contains("not run")
        || normalized.contains("failed")
        || normalized.contains("blocked")
}

fn normalized_section_entry(line: &str) -> &str {
    line.trim_start_matches(['-', '*']).trim()
}

/// Check for repetitive text in assistant responses to catch non-tool-calling loops.
/// Returns true if a loop is detected.
pub fn check_for_response_loop(response_text: &str, session_state: &mut AgentSessionState) -> bool {
    if response_text.len() < 10 {
        return false;
    }

    // Simplistic check: is this response identical to the last one (ignoring whitespace)?
    let normalized_current = response_text.split_whitespace().collect::<Vec<_>>().join(" ");

    let repeated = session_state
        .messages
        .iter()
        .rev()
        .filter(|m| m.role == MessageRole::Assistant)
        .skip(1)
        .take(2)
        .any(|m| {
            let normalized_prev = m.content.as_text().split_whitespace().collect::<Vec<_>>().join(" ");
            normalized_prev == normalized_current
        });

    if repeated {
        let warning = "Repetitive assistant response detected. Breaking potential loop.".to_string();
        session_state.warnings.push(warning);
        session_state.consecutive_idle_turns = session_state.consecutive_idle_turns.saturating_add(1);
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::Message;

    #[test]
    fn test_completion_candidates() {
        assert!(check_completion_candidate("The task is complete"));
        assert!(check_completion_candidate("Revision 1: task is complete."));
        assert!(check_completion_candidate("I have successfully completed the task."));
        assert!(check_completion_candidate("Task done"));
        assert!(check_completion_candidate("All done"));

        // Negative cases
        assert!(!check_completion_candidate("I will have the task done soon"));
        assert!(!check_completion_candidate("Is the task done?"));
        assert!(!check_completion_candidate("random text"));
        assert!(!check_completion_candidate("The task is complete once verification finishes."));
        assert!(!check_completion_candidate("All done. Verification pending."));
        assert!(!check_completion_candidate("All done, but open questions remain."));
    }

    #[test]
    fn subagent_markdown_contract_detected_as_complete() {
        let contract = "## Summary\n- Background subprocess launched; PID 86065.\n\n## Facts\n- Script started at 2026-04-25T08:39:10Z.\n\n## Touched Files\n- None\n\n## Verification\n- Process confirmed.\n\n## Open Questions\n- None";
        assert!(check_completion_candidate(contract));
    }

    #[test]
    fn subagent_markdown_contract_with_crlf_detected_as_complete() {
        let contract = "## Summary\r\n- Done.\r\n\r\n## Facts\r\n- Fact 1.\r\n";
        assert!(check_completion_candidate(contract));
    }

    #[test]
    fn subagent_markdown_contract_with_leading_whitespace_detected() {
        let contract = "\n\n## Summary\n- item\n\n## Facts\n- fact\n";
        assert!(check_completion_candidate(contract));
    }

    #[test]
    fn document_with_only_summary_header_not_detected() {
        let doc = "## Summary\n- This is a doc without a Facts section.\n";
        assert!(!check_completion_candidate(doc));
    }

    #[test]
    fn document_with_only_facts_header_not_detected() {
        let doc = "## Facts\n- Fact without summary.\n";
        assert!(!check_completion_candidate(doc));
    }

    #[test]
    fn structured_contract_with_open_questions_is_not_complete() {
        let doc = "## Summary\n- Work applied.\n\n## Facts\n- Fact.\n\n## Verification\n- Process confirmed.\n\n## Open Questions\n- Need to rerun the end-to-end flow.";
        assert!(!check_completion_candidate(doc));
    }

    #[test]
    fn structured_contract_with_unresolved_verification_is_not_complete() {
        let doc = "## Summary\n- Work applied.\n\n## Facts\n- Fact.\n\n## Verification\n- Verification pending.\n\n## Open Questions\n- None";
        assert!(!check_completion_candidate(doc));
    }

    #[test]
    fn structured_contract_with_none_punctuation_is_complete() {
        let doc = "## Summary\n- Work applied.\n\n## Facts\n- Fact.\n\n## Verification\n- Process confirmed.\n\n## Open Questions\n- None.";
        assert!(check_completion_candidate(doc));
    }

    #[test]
    fn response_loop_ignores_current_assistant_message() {
        let repeated_response = "The task is complete";
        let mut state = AgentSessionState::new("session".to_string(), 8, 4, 128_000);
        state.messages_mut().push(Message::assistant(repeated_response.to_string()));

        assert!(!check_for_response_loop(repeated_response, &mut state));
    }

    #[test]
    fn response_loop_still_detects_prior_duplicate_assistant_message() {
        let repeated_response = "The task is complete";
        let mut state = AgentSessionState::new("session".to_string(), 8, 4, 128_000);
        state.messages_mut().push(Message::assistant(repeated_response.to_string()));
        state.messages_mut().push(Message::assistant(repeated_response.to_string()));

        assert!(check_for_response_loop(repeated_response, &mut state));
    }
}
