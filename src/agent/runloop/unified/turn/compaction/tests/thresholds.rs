use super::*;

#[test]
fn grounded_fact_extraction_dedupes_caps_and_skips_errors() {
    let history = vec![
        Message::tool_response_with_origin(
            "call_1".to_string(),
            "{\"result\":\"Cargo.toml declares vtcode-core\"}".to_string(),
            "read_file".to_string(),
        ),
        Message::tool_response_with_origin(
            "call_2".to_string(),
            "{\"result\":\"cargo.toml declares vtcode-core\"}".to_string(),
            "read_file".to_string(),
        ),
        Message::tool_response_with_origin(
            "call_3".to_string(),
            "{\"error\":\"denied\"}".to_string(),
            "read_file".to_string(),
        ),
        Message::user("I prefer concise answers.".to_string()),
    ];

    let facts = dedup_latest_facts(&history, 5);
    assert_eq!(facts.len(), 2);
    assert!(facts.iter().any(|fact| fact.source == "tool:read_file"));
    assert!(facts.iter().any(|fact| fact.source == "user_assertion"));
}

#[test]
fn resolve_compaction_threshold_prefers_configured_value() {
    assert_eq!(resolve_compaction_threshold(Some(42), 200_000), Some(42));
}

#[test]
fn resolve_compaction_threshold_reserves_output_room_when_unset() {
    let reserve = DEFAULT_OUTPUT_RESERVE_TOKENS as u64;
    // Unset values trigger at `DEFAULT_COMPACTION_TRIGGER_RATIO` of the prompt
    // budget (capacity minus the output reserve), not the full budget.
    let prompt_budget = 200_000 - reserve;
    let expected = prompt_budget * 3 / 4;
    assert_eq!(resolve_compaction_threshold(None, 200_000), Some(expected));
}

#[test]
fn resolve_compaction_threshold_clamps_to_context_size() {
    let reserve = DEFAULT_OUTPUT_RESERVE_TOKENS as u64;
    // An explicit threshold is clamped to the full prompt budget (no ratio).
    assert_eq!(resolve_compaction_threshold(Some(300_000), 200_000), Some(200_000 - reserve));
}

#[test]
fn resolve_compaction_threshold_requires_context_or_override() {
    assert_eq!(resolve_compaction_threshold(None, 0), None);
}

#[test]
fn effective_compaction_threshold_follows_provider_capacity_when_session_budget_unset() {
    // Arrange: the default session budget is 0 (unset), so the provider
    // capacity drives the threshold. The output reserve is subtracted and the
    // default trigger ratio then applies to the remaining prompt budget.
    let provider = ContextSizedProvider { context_size: 500_000, provider_name: "openai" };
    let config = VTCodeConfig::default();

    // Act
    let threshold = effective_compaction_threshold(Some(&config), &provider, "stub-model");

    // Assert
    let prompt_budget = 500_000 - DEFAULT_OUTPUT_RESERVE_TOKENS;
    assert_eq!(threshold, Some(prompt_budget * 3 / 4));
}

#[test]
fn effective_compaction_threshold_clamps_session_budget_to_provider_capacity() {
    // Arrange
    let provider = ContextSizedProvider { context_size: 100_000, provider_name: "openai" };
    let mut config = VTCodeConfig::default();
    config.context.max_context_tokens = 160_000;

    // Act
    let threshold = effective_compaction_threshold(Some(&config), &provider, "stub-model");

    // Assert: the 160k session budget is clamped to the 100k provider capacity,
    // the output reserve is subtracted, and the default trigger ratio applies.
    let prompt_budget = 100_000 - DEFAULT_OUTPUT_RESERVE_TOKENS;
    assert_eq!(threshold, Some(prompt_budget * 3 / 4));
}

#[test]
fn effective_compaction_threshold_uses_default_session_budget_without_config() {
    // A zero (unset) session budget defers to the provider capacity. The
    // unset threshold then triggers at the ratio of the prompt budget
    // (capacity minus the output reserve).
    let threshold = effective_compaction_threshold(
        None,
        &ContextSizedProvider { context_size: 500_000, provider_name: "openai" },
        "stub-model",
    );

    let prompt_budget = 500_000 - DEFAULT_OUTPUT_RESERVE_TOKENS;
    assert_eq!(threshold, Some(prompt_budget * 3 / 4));
}

#[test]
fn explicit_compaction_threshold_overrides_session_budget_but_not_provider_capacity() {
    // Arrange
    let mut config = VTCodeConfig::default();
    config.context.max_context_tokens = 300_000;
    config.agent.harness.auto_compaction_threshold_tokens = Some(200_000);

    // Act
    let session_override = effective_compaction_threshold(
        Some(&config),
        &ContextSizedProvider { context_size: 500_000, provider_name: "openai" },
        "stub-model",
    );
    let provider_cap = effective_compaction_threshold(
        Some(&config),
        &ContextSizedProvider { context_size: 150_000, provider_name: "openai" },
        "stub-model",
    );

    // Assert: the explicit 200k threshold wins over the 300k session budget, and
    // is clamped to the prompt budget (capacity minus the output reserve) when
    // the provider capacity is lower. An explicit threshold bypasses the
    // default trigger ratio — only unset values are ratio-scaled.
    assert_eq!(session_override, Some(200_000));
    assert_eq!(provider_cap, Some(150_000 - DEFAULT_OUTPUT_RESERVE_TOKENS));
}

#[test]
fn zero_session_budget_preserves_provider_derived_threshold() {
    // Arrange
    let mut config = VTCodeConfig::default();
    config.context.max_context_tokens = 0;

    // Act
    let threshold = effective_compaction_threshold(
        Some(&config),
        &ContextSizedProvider { context_size: 200_000, provider_name: "openai" },
        "stub-model",
    );

    // Assert
    let prompt_budget = 200_000 - DEFAULT_OUTPUT_RESERVE_TOKENS;
    assert_eq!(threshold, Some(prompt_budget * 3 / 4));
    assert_eq!(resolve_compaction_threshold(Some(0), 200_000), Some((prompt_budget * 3 / 4) as u64));
}

#[test]
fn effective_session_context_budget_preserves_known_limit_when_other_side_is_zero() {
    assert_eq!(effective_session_context_budget(500_000, 160_000), 160_000);
    assert_eq!(effective_session_context_budget(100_000, 160_000), 100_000);
    assert_eq!(effective_session_context_budget(500_000, 0), 500_000);
    assert_eq!(effective_session_context_budget(0, 160_000), 160_000);
    assert_eq!(effective_session_context_budget(0, 0), 0);
}

#[test]
fn build_server_compaction_context_management_creates_openai_payload() {
    assert_eq!(
        build_server_compaction_context_management(Some(512), 2_000_000, 160_000),
        Some(json!([{
            "type": "compaction",
            "compact_threshold": 512,
        }]))
    );
}

#[tokio::test]
async fn capability_driven_compaction_triggers_for_three_families_at_small_and_large_limits() {
    for provider_name in ["openai", "anthropic", "gemini"] {
        for context_size in [32_000, 1_000_000] {
            let provider = ContextSizedProvider { context_size, provider_name };
            let config = VTCodeConfig::default();
            let prompt_budget = context_size - DEFAULT_OUTPUT_RESERVE_TOKENS;
            let threshold = prompt_budget * 3 / 4;
            assert_eq!(
                vtcode_core::compaction::effective_context_budget(Some(&config), &provider, "dynamic-model"),
                context_size
            );
            assert_eq!(effective_compaction_threshold(Some(&config), &provider, "dynamic-model"), Some(threshold));
            for (prompt_tokens, should_compact) in [(threshold - 1, false), (threshold, true)] {
                let temp = tempdir().expect("temporary workspace");
                // The history must exceed the continuity tail at every tested
                // capacity (up to a 20k-token tail at 1M) so a triggered
                // compaction genuinely shrinks instead of reporting
                // already-compact.
                let mut history = tool_heavy_history(ROOMY_HISTORY_CYCLES);
                let mut stats = SessionStats::default();
                let mut manager = test_context_manager();
                manager.update_token_usage(Some(&Usage {
                    prompt_tokens: prompt_tokens as u32,
                    total_tokens: prompt_tokens as u32,
                    ..Usage::default()
                }));
                let outcome = maybe_auto_compact_history(
                    CompactionContext::new(
                        &provider,
                        "dynamic-model",
                        "budget-session",
                        "budget-thread",
                        temp.path(),
                        Some(&config),
                        None,
                        None,
                    ),
                    CompactionState::new(&mut history, &mut stats, &mut manager),
                )
                .await
                .expect("offline compaction");
                assert_eq!(
                    outcome.is_some(),
                    should_compact,
                    "{provider_name}, capacity={context_size}, pressure={prompt_tokens}"
                );
            }
        }
    }
}

#[test]
fn explicit_threshold_cannot_bypass_session_safety_or_request_output_reserve() {
    use vtcode_core::compaction::memory_envelope::effective_compaction_threshold_with_reserve;
    let provider = ContextSizedProvider { context_size: 1_000_000, provider_name: "openai" };
    let mut config = VTCodeConfig::default();
    config.context.max_context_tokens = 32_000;
    config.agent.harness.auto_compaction_threshold_tokens = Some(900_000);
    assert_eq!(
        effective_compaction_threshold_with_reserve(Some(&config), &provider, "dynamic-model", 800),
        Some(31_200)
    );
    assert_eq!(
        effective_compaction_threshold_with_reserve(Some(&config), &provider, "dynamic-model", 8_000),
        Some(24_000)
    );
}

#[test]
fn discovered_resolved_context_reaches_runtime_budget_without_changing_other_models() {
    use vtcode_core::llm::model_resolver::{DynamicModelMeta, ModelResolver};
    use vtcode_core::llm::provider::ContextWindowProvider;
    for provider_name in ["openai", "anthropic", "gemini"] {
        let resolved = ModelResolver::resolve(
            Some(provider_name),
            "dynamic-32k",
            &[],
            Some(DynamicModelMeta {
                display_name: "Discovered model".into(),
                description: None,
                context_window: Some(32_000),
            }),
        )
        .expect("resolved dynamic model");
        let provider = ContextWindowProvider::wrap(
            Box::new(ContextSizedProvider { context_size: 1_000_000, provider_name }),
            &resolved.model_id,
            resolved.context_window(),
        );
        assert_eq!(vtcode_core::compaction::effective_context_budget(None, provider.as_ref(), "dynamic-32k"), 32_000);
        let prompt_budget = 32_000 - DEFAULT_OUTPUT_RESERVE_TOKENS;
        assert_eq!(effective_compaction_threshold(None, provider.as_ref(), "dynamic-32k"), Some(prompt_budget * 3 / 4));
        assert_eq!(provider.effective_context_size("other-model"), 1_000_000);
        assert_eq!(provider.effective_context_size(""), 32_000);
    }
}
