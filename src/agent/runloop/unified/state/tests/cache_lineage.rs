use super::*;

#[test]
fn prompt_cache_fingerprint_reports_expected_change_reasons() {
    let mut stats = SessionStats::default();

    assert_eq!(stats.record_prompt_cache_fingerprint("gpt-5", 11, Some(22)), "model");
    assert_eq!(stats.record_prompt_cache_fingerprint("gpt-5", 11, Some(22)), "unchanged");
    assert_eq!(stats.record_prompt_cache_fingerprint("gpt-5", 33, Some(22)), "stable_prefix");
    assert_eq!(stats.record_prompt_cache_fingerprint("gpt-5", 33, Some(44)), "tool_catalog");
    assert_eq!(stats.record_prompt_cache_fingerprint("gpt-5", 55, Some(66)), "stable_prefix+tool_catalog");
    assert_eq!(stats.record_prompt_cache_fingerprint("gpt-5-mini", 55, Some(66)), "model");

    assert_eq!(
        stats.prompt_cache_diagnostics(),
        PromptCacheDiagnostics {
            observations: 6,
            model_changes: 2,
            unchanged: 1,
            stable_prefix_changes: 1,
            tool_catalog_changes: 1,
            combined_changes: 1,
            last_change_reason: Some("model".to_string()),
            last_stable_prefix_hash: Some(55),
            last_tool_catalog_hash: Some(66),
        }
    );
}
#[test]
fn prompt_cache_fingerprint_attributes_tools_omitted_and_recovery_reason() {
    let mut stats = SessionStats::default();

    assert_eq!(stats.record_prompt_cache_fingerprint_with_context("gpt-6", 1, Some(2), Some(10), None), "model");
    assert_eq!(stats.record_prompt_cache_fingerprint_with_context("gpt-6", 1, Some(2), Some(10), None), "unchanged");
    // Dropping every tool from the wire is its own miss cause.
    assert_eq!(
        stats.record_prompt_cache_fingerprint_with_context("gpt-6", 1, Some(2), Some(0), None),
        "tools_omitted"
    );
    // Restoring tools then rotating the frozen recovery reason is next.
    assert_eq!(
        stats.record_prompt_cache_fingerprint_with_context("gpt-6", 1, Some(2), Some(10), Some("loop detector")),
        "unchanged"
    );
    assert_eq!(
        stats.record_prompt_cache_fingerprint_with_context(
            "gpt-6",
            1,
            Some(2),
            Some(10),
            Some("blocked tool-call fuse tripped")
        ),
        "recovery_reason"
    );
}
#[test]
fn fresh_execution_reset_clears_context_lineage_and_diagnostics() {
    let mut stats = SessionStats::default();
    stats.set_prompt_cache_lineage_id(Some("lineage-1".to_string()));
    stats.record_prompt_cache_fingerprint("gpt-5", 11, Some(22));
    stats.request_envelope("model", "provider", "build", "fixed".to_string(), vec![], 7);
    stats.note_request_sent();
    stats.set_stop_reason(Some("length".to_string()));

    stats.reset_for_fresh_execution();

    assert_eq!(stats.prompt_cache_lineage_id(), None);
    assert_eq!(stats.prompt_cache_diagnostics(), PromptCacheDiagnostics::default());
    assert!(!stats.has_sent_request());
    assert_eq!(stats.stop_reason(), None);
    assert!(stats.request_envelope.is_none());
    assert!(stats.request_envelope_identity.is_none());
    assert!(stats.pending_request_segment_id.is_none());
}
#[test]
fn previous_response_chain_clears_only_matching_scope() {
    let mut stats = SessionStats::default();
    let openai_messages = vec![vtcode_core::llm::provider::Message::user("hello".to_string())];
    let gemini_messages = vec![vtcode_core::llm::provider::Message::user("hi".to_string())];
    stats.set_previous_response_chain("openai", "gpt-5.6-sol", Some("resp_openai"), &openai_messages);
    stats.set_previous_response_chain("gemini", "gemini-2.5-pro", Some("resp_gemini"), &gemini_messages);

    stats.clear_previous_response_chain_for("openai", "gpt-5.6-sol");

    assert_eq!(stats.previous_response_id_for("openai", "gpt-5.6-sol"), None);
    assert_eq!(stats.previous_response_chain_for("openai", "gpt-5.6-sol"), None);
    assert_eq!(stats.previous_response_id_for("gemini", "gemini-2.5-pro"), Some("resp_gemini".to_string()));
    assert_eq!(
        stats
            .previous_response_chain_for("gemini", "gemini-2.5-pro")
            .map(|chain| chain.messages.as_slice()),
        Some(gemini_messages.as_slice())
    );
}
#[test]
fn shared_previous_response_chain_setter_preserves_message_arc() {
    let mut stats = SessionStats::default();
    let messages = Arc::new(vec![vtcode_core::llm::provider::Message::user("hello".to_string())]);

    stats.set_previous_response_chain_shared("gemini", "gemini-2.5-pro", Some("resp_gemini"), Arc::clone(&messages));

    let stored_messages = &stats
        .previous_response_chain_for("gemini", "gemini-2.5-pro")
        .expect("shared response chain should be recorded")
        .messages;
    assert!(Arc::ptr_eq(&messages, stored_messages));
}
#[test]
fn safe_mode_prompts_follow_workspace_trust_for_edit_mode() {
    assert!(should_enforce_safe_mode_prompts(false, false, Some(WorkspaceTrustLevel::ToolsPolicy),));
    assert!(!should_enforce_safe_mode_prompts(false, false, Some(WorkspaceTrustLevel::FullAuto),));
    assert!(should_enforce_safe_mode_prompts(false, false, None));
}
