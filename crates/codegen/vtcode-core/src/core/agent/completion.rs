use crate::core::agent::session::AgentSessionState;
use crate::llm::provider::MessageRole;

/// Successful tracker mutations establish adoption within the caller's current
/// request history. Listing, rejected calls, and unrelated tool results do not.
pub fn tracker_was_adopted(history: &[crate::llm::provider::Message]) -> bool {
    let mut pending_tracker_calls = std::collections::HashMap::new();
    for message in history {
        if message.role == MessageRole::Assistant {
            // Call IDs can be reused between batches; never match an unanswered
            // tracker request to a later batch's result.
            pending_tracker_calls.clear();
            for call in message.tool_calls.iter().flatten() {
                let Some(function) = call
                    .function
                    .as_ref()
                    .filter(|function| function.name == crate::config::constants::tools::TASK_TRACKER)
                else {
                    continue;
                };
                let Ok(args) = serde_json::from_str::<serde_json::Value>(&function.arguments) else {
                    continue;
                };
                if matches!(args.get("action").and_then(serde_json::Value::as_str), Some("create" | "update" | "add")) {
                    pending_tracker_calls.insert(call.id.as_str(), args);
                }
            }
        } else if message.role == MessageRole::Tool
            && let Some(args) = message.tool_call_id.as_deref().and_then(|id| pending_tracker_calls.remove(id))
            && let Ok(output) = serde_json::from_str::<serde_json::Value>(message.content.as_text().as_ref())
            && tracker_adoption_succeeded(crate::config::constants::tools::TASK_TRACKER, &args, &output)
        {
            return true;
        }
    }
    false
}

/// Observe adoption before tool output or conversation history is compacted.
pub fn tracker_adoption_succeeded(tool_name: &str, args: &serde_json::Value, output: &serde_json::Value) -> bool {
    use serde_json::Value;
    tool_name == crate::config::constants::tools::TASK_TRACKER
        && output.get("error").is_none()
        && output.get("success").and_then(Value::as_bool) != Some(false)
        && output.get("blocked").and_then(Value::as_bool) != Some(true)
        && output.get("not_executed").and_then(Value::as_bool) != Some(true)
        && matches!(
            (args.get("action").and_then(Value::as_str), output.get("status").and_then(Value::as_str)),
            (Some("create"), Some("created" | "replaced" | "unchanged"))
                | (Some("update"), Some("updated" | "unchanged"))
                | (Some("add"), Some("added"))
        )
}

/// True when assistant text is a genuine safety/permission handoff that must
/// not be auto-continued, even if `task_tracker` still has incomplete steps.
///
/// Shared by the binary outer-loop Completed queue and AgentRunner status
/// continuation so both surfaces use the same vocabulary. Safety/policy
/// denials are evaluated first so a recap that mentions both a budget and a
/// policy denial still counts as a handoff. Pure budget/recovery recaps are
/// not handoffs (outer/in-turn recoverable classifiers treat those as continue).
pub fn tracker_final_text_is_safety_handoff(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    tracker_final_text_is_policy_handoff(text)
        || lower.contains("verification is still pending")
        || lower.contains("unverified assistant responses")
        || lower.contains("anti-blind")
        || lower.contains("verification gate")
}

/// Permission, policy, safety, and credential handoffs remain terminal even
/// when a pending-verification block can recover with fresh execution budget.
/// Verification wording is handled separately by the turn's recovery gate.
pub fn tracker_final_text_is_policy_handoff(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.trim().is_empty() {
        return false;
    }
    // True handoffs win even when the text also mentions a budget.
    if lower.contains("permission denied")
        || lower.contains("access denied")
        || lower.contains("safety fuse")
        || lower.contains("tool-call safety fuse")
        || lower.contains("policy block")
        || lower.contains("blocked by policy")
        || lower.contains("requires manual intervention")
        || lower.contains("missing credentials")
        || lower.contains("credentials are missing")
        || lower.contains("denied by policy")
        || lower.contains("denied by workspace tool policy")
        || lower.contains("denied by tool policy")
        || lower.contains("execution denied by policy")
        || lower.contains("blocked by tool policy")
    {
        return true;
    }
    // Pure budget/recovery recaps ("blocked by turn budget", "tool loop
    // budget exhausted", …) are not user handoffs.
    false
}

/// Shared recoverable status-recap vocabulary for tracker continuation.
///
/// Used by the binary in-turn override and outer blocked-reason classifier
/// tests so session/production budget phrases cannot drift between surfaces.
/// Lowercase input expected.
pub fn recoverable_status_recap_phrasing(lower: &str) -> bool {
    lower.contains("turn budget")
        || lower.contains("preview budget")
        || lower.contains("tool preview budget")
        || lower.contains("wall clock")
        || lower.contains("safety cap")
        || lower.contains("recovery fallback")
        || lower.contains("recovery could not confirm")
        || lower.contains("recovery exhausted")
        || lower.contains("recovery was exhausted")
        || lower.contains("tool budget")
        || lower.contains("tool loop")
        || lower.contains("tool-call budget")
        || lower.contains("tool follow-up")
        || lower.contains("read cap")
        || lower.contains("work budget")
        || lower.contains("max tool")
        || lower.contains("per-turn tool")
        || lower.contains("budget exhausted")
        || lower.contains("budget ran out")
}

/// True when final assistant text asks the user for a decision/confirmation.
///
/// Shared by outer tracker auto-queue and in-turn continuation so Completed
/// turns that end with a genuine question are not auto-continued past the ask.
pub fn tracker_final_text_requires_user_input(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.ends_with('?') {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    const STRONG: &[&str] = &[
        "please provide",
        "please confirm",
        "please approve",
        "need your approval",
        "need your permission",
        "need your decision",
        "need you to choose",
        "need you to confirm",
        "waiting for your",
        "awaiting your",
        "your choice",
        "your decision",
        "need approval",
        "requires approval",
        "approval is required",
        "need permission",
        "requires permission",
        "permission is required",
        "grant permission",
        "authorize this",
        "need a decision",
        "need clarification",
        "waiting for input",
        "awaiting input",
        "waiting on you",
        "how should i proceed",
        "what should i do",
        "what would you like",
        "how would you like",
        "do you want me to",
    ];
    const CLAUSE_START: &[&str] = &["could you ", "can you ", "shall i ", "should i "];
    if STRONG.iter().any(|p| lower.contains(p)) {
        return true;
    }
    CLAUSE_START
        .iter()
        .any(|p| lower.trim_start().starts_with(p) || lower.contains(&format!("\n{p}")))
}

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
        session_state.push_warning(warning);
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
    fn policy_handoffs_are_distinct_from_pending_verification_recaps() {
        for text in [
            "Verification is still pending after the tool-call budget was exhausted.",
            "Repeated unverified assistant responses reached the anti-blind verification gate.",
        ] {
            assert!(tracker_final_text_is_safety_handoff(text));
            assert!(!tracker_final_text_is_policy_handoff(text));
        }
        for text in [
            "Verification is still pending; permission denied.",
            "Tool budget exhausted; blocked by tool policy.",
            "Missing credentials for the verifier.",
            "Tool-call safety fuse stopped verification.",
        ] {
            assert!(tracker_final_text_is_policy_handoff(text));
            assert!(tracker_final_text_is_safety_handoff(text));
        }
    }

    #[test]
    fn tracker_final_text_is_safety_handoff_vocabulary() {
        assert!(tracker_final_text_is_safety_handoff(
            "Permission denied for exec_command. Next step: retry after access is granted."
        ));
        assert!(tracker_final_text_is_safety_handoff(
            "I hit the tool-call safety fuse mid-verification; policy block."
        ));
        assert!(tracker_final_text_is_safety_handoff(
            "Blocked action: exec_command is denied by workspace tool policy."
        ));
        assert!(tracker_final_text_is_safety_handoff(
            "Turn blocked after repeated unverified assistant responses; verification is still pending."
        ));
        assert!(tracker_final_text_is_safety_handoff(
            "Anti-blind checkpoint remains; verification is still pending."
        ));
        assert!(!tracker_final_text_is_safety_handoff(
            "## Status\nBlocked by turn budget. Next step: read design/diff.rs."
        ));
        assert!(!tracker_final_text_is_safety_handoff(
            "## Status\nBlocked by turn tool budget. Next step: patch the struct."
        ));
        assert!(!tracker_final_text_is_safety_handoff(
            "Tool loop budget exhausted; continuing next turn with remaining tracker steps."
        ));
        assert!(!tracker_final_text_is_safety_handoff(
            "Fix 1 partially applied, needs verification; next step is cargo check."
        ));
        // Bare tool-policy mention is not a handoff; explicit denial still is.
        assert!(!tracker_final_text_is_safety_handoff("Reviewed tool policy docs; next step is the tracker patch."));
        assert!(tracker_final_text_is_safety_handoff(
            "Blocked action: exec_command is denied by workspace tool policy."
        ));
        assert!(!tracker_final_text_is_safety_handoff("Implemented patch apply; verification passed."));
        assert!(!tracker_final_text_is_safety_handoff(""));
        // Policy denial wins even when a budget is also mentioned.
        assert!(tracker_final_text_is_safety_handoff("Tool budget exhausted; denied by policy for exec_command."));
        assert!(tracker_final_text_is_safety_handoff("Turn budget hit, then permission denied for the write."));
    }

    #[test]
    fn recoverable_status_recap_phrasing_covers_session_budget_recaps() {
        for phrase in [
            "blocked by the turn's preview budget",
            "tool budget ran out",
            "hit the per-file read cap",
            "preview budget exhausted",
            "tool loop budget exhausted",
            "recovery fallback",
            "reached the safety cap",
            "work budget exhausted",
        ] {
            assert!(
                recoverable_status_recap_phrasing(&phrase.to_ascii_lowercase()),
                "must treat as recoverable: {phrase}"
            );
        }
        for phrase in [
            "permission denied for exec_command",
            "verification is still pending",
            "needs a user decision",
        ] {
            assert!(
                !recoverable_status_recap_phrasing(&phrase.to_ascii_lowercase()),
                "must not treat as recoverable budget phrasing: {phrase}"
            );
        }
    }

    #[test]
    fn tracker_final_text_requires_user_input_vocabulary() {
        assert!(tracker_final_text_requires_user_input("Step 1 done. Which branch should I use for step 2?"));
        assert!(tracker_final_text_requires_user_input(
            "## Status\nPlease confirm whether to use the stable schema."
        ));
        assert!(tracker_final_text_requires_user_input("Can you confirm the migration path before I edit."));
        assert!(!tracker_final_text_requires_user_input(
            "## Status\nNext we need your workspace path in CONFIG; patching the struct now."
        ));
        assert!(!tracker_final_text_requires_user_input("Blocked by turn budget. Next step: read helpers.rs."));
        assert!(!tracker_final_text_requires_user_input(""));
    }

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
