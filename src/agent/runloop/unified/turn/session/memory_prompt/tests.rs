use super::*;

#[test]
fn detects_remember_intent_from_natural_language() {
    assert_eq!(
        detect_memory_prompt_intent("Please remember that I prefer pnpm"),
        Some(MemoryPromptIntent::Remember { request: "remember that I prefer pnpm".to_string() })
    );
}

#[test]
fn detects_forget_intent_from_natural_language() {
    assert_eq!(
        detect_memory_prompt_intent("forget that I prefer pnpm"),
        Some(MemoryPromptIntent::Forget { request: "forget that I prefer pnpm".to_string() })
    );
}

#[test]
fn detects_show_memory_questions() {
    assert_eq!(detect_memory_prompt_intent("what do you remember about this repo?"), Some(MemoryPromptIntent::Show));
}

#[test]
fn ignores_non_imperative_memory_questions() {
    assert_eq!(detect_memory_prompt_intent("do you remember my name?"), None);
}

#[test]
fn strips_nested_memory_markers() {
    assert_eq!(
        detect_memory_prompt_intent("save to memory, remember my preferred shell is zsh"),
        Some(MemoryPromptIntent::Remember {
            request: "save to memory, remember my preferred shell is zsh".to_string(),
        })
    );
}

#[test]
fn normalize_prompt_clause_does_not_panic_on_short_input() {
    assert_eq!(normalize_prompt_clause("my name"), Some("my name".to_string()));
}

#[test]
fn contextual_memory_requests_use_only_the_latest_non_empty_assistant_reply() {
    let history = vec![
        uni::Message::assistant("Earlier identity answer".to_string()),
        uni::Message::assistant("Immediate identity answer".to_string()),
        uni::Message::tool_response("call-1".to_string(), "Tool output with another identity".to_string()),
        uni::Message::user("remember it".to_string()),
    ];

    assert_eq!(
        prior_assistant_reply_for_memory_request("remember it", &history),
        Some("Immediate identity answer".to_string())
    );
    assert_eq!(
        prior_assistant_reply_for_memory_request("remember my name", &history),
        None,
        "explicit facts must not receive prior-answer context"
    );
}

#[test]
fn contextual_memory_requests_skip_empty_assistant_replies_and_tool_output() {
    let history = vec![
        uni::Message::assistant("Usable assistant answer".to_string()),
        uni::Message::assistant("   ".to_string()),
        uni::Message::tool_response("call-1".to_string(), "Tool output".to_string()),
    ];

    assert_eq!(
        prior_assistant_reply_for_memory_request("remember this", &history),
        Some("Usable assistant answer".to_string())
    );
}

#[test]
fn only_deictic_memory_requests_can_use_prior_answer_context() {
    assert!(is_deictic_remember_request("remember it"));
    assert!(is_deictic_remember_request("remember this"));
    assert!(is_deictic_remember_request("remember that"));
    assert!(is_deictic_remember_request("Remember THIS."));
    assert!(!is_deictic_remember_request("remember that my name is Vinh Nguyen"));
    assert!(!is_deictic_remember_request("remember my name"));
}

#[test]
fn exhausted_missing_details_message_is_actionable() {
    let missing = MemoryMissingField {
        field: "alias".to_string(),
        prompt: "What alias should VT Code remember?".to_string(),
    };

    let message = exhausted_missing_details_message(&missing);
    assert!(message.contains("alias"));
    assert!(message.contains("What alias should VT Code remember?"));
    assert!(message.contains("submit the complete fact directly"));
    assert!(message.contains("remember that"));
    assert!(!message.contains("still needs more information."));
}

#[test]
fn exhausted_message_normalizes_rule_field_and_prompt() {
    let missing = MemoryMissingField {
        field: "  rule  ".to_string(),
        prompt: "  What rule should VT Code remember?  ".to_string(),
    };

    let message = exhausted_missing_details_message(&missing);
    assert!(message.contains("`rule`"));
    assert!(message.contains("What rule should VT Code remember?"));
    assert!(!message.contains("  rule  "));
}

#[test]
fn empty_memory_answers_cancel_instead_of_looping() {
    assert!(is_empty_memory_answer(""));
    assert!(is_empty_memory_answer("   "));
    assert!(is_empty_memory_answer("\n\t "));
    assert!(!is_empty_memory_answer("Use pnpm"));
    assert!(!is_empty_memory_answer("  valid rule  "));
}

#[test]
fn supplemental_labels_missing_field_for_planner_retry() {
    let first = format_supplemental_for_missing(None, "rule", "Always run cargo nextest");
    assert_eq!(first, "For missing field 'rule': Always run cargo nextest");

    let second = format_supplemental_for_missing(Some(first), "alias", "srivera");
    assert!(second.contains("For missing field 'rule': Always run cargo nextest"));
    assert!(second.contains("For missing field 'alias': srivera"));
    assert!(second.find("rule").unwrap() < second.find("alias").unwrap());
}

#[test]
fn supplemental_trims_field_and_answer_whitespace_asymmetrically() {
    let labeled =
        format_supplemental_for_missing(None, "  repository_fact  ", "  Tests live under vtcode-core/tests  ");
    assert_eq!(labeled, "For missing field 'repository_fact': Tests live under vtcode-core/tests");
    assert!(!labeled.contains("  "));
}

#[test]
fn memory_operation_notice_describes_remember_flow() {
    let intent = MemoryPromptIntent::Remember { request: "remember my editor theme".to_string() };
    assert_eq!(memory_operation_notice(&intent), "Memory save requested. VT Code is preparing a normalized note.");
}

#[test]
fn memory_operation_notice_describes_forget_flow() {
    let intent = MemoryPromptIntent::Forget { request: "forget my editor theme".to_string() };
    assert_eq!(memory_operation_notice(&intent), "Memory removal requested. VT Code is matching normalized notes.");
}

#[test]
fn suppresses_repeated_cleanup_confirmation_for_same_fingerprint() {
    let status = vtcode_core::persistent_memory::PersistentMemoryStatus {
        enabled: true,
        auto_write: true,
        directory: std::path::PathBuf::from("/tmp/memory"),
        summary_file: std::path::PathBuf::from("/tmp/memory/memory_summary.md"),
        memory_file: std::path::PathBuf::from("/tmp/memory/MEMORY.md"),
        preferences_file: std::path::PathBuf::from("/tmp/memory/preferences.md"),
        repository_facts_file: std::path::PathBuf::from("/tmp/memory/repository-facts.md"),
        notes_dir: std::path::PathBuf::from("/tmp/memory/notes"),
        rollout_summaries_dir: std::path::PathBuf::from("/tmp/memory/rollout_summaries"),
        summary_exists: true,
        registry_exists: true,
        pending_rollout_summaries: 0,
        cleanup_status: vtcode_core::persistent_memory::MemoryCleanupStatus {
            needed: true,
            suspicious_facts: 2,
            suspicious_summary_lines: 1,
        },
    };

    assert!(should_suppress_cleanup_confirmation(Some((2, 1)), &status));
    assert!(!should_suppress_cleanup_confirmation(Some((1, 1)), &status));
    assert!(!should_suppress_cleanup_confirmation(None, &status));
}
