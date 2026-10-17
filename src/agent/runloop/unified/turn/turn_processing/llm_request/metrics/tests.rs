use super::*;

#[test]
fn cache_hit_ratio_counts_uncached_input_for_total_and_exclusive_providers() {
    for (provider, prompt_tokens) in [
        ("openai", 1000),
        ("merge-gateway", 1000),
        ("anthropic", 150),
        ("minimax", 150),
    ] {
        let usage = Usage {
            prompt_tokens,
            cached_prompt_tokens: Some(800),
            cache_creation_tokens: Some(50),
            ..Usage::default()
        };
        // 800 cached + 50 newly cached + 150 uncached input tokens.
        assert!((prompt_cache_hit_ratio(provider, &usage) - 0.8).abs() < 1e-12, "{provider}");
    }
}

#[test]
fn cache_hit_ratio_reports_partial_reuse_without_cache_creation() {
    let usage = Usage {
        prompt_tokens: 12_081,
        cached_prompt_tokens: Some(11_136),
        ..Usage::default()
    };
    // Recorded October 6 request: 945 input tokens were uncached.
    assert!((prompt_cache_hit_ratio("merge-gateway", &usage) - 11_136.0 / 12_081.0).abs() < 1e-12);
    assert!(prompt_cache_hit_ratio("openai", &Usage::default()).abs() < f64::EPSILON);
    assert!(prompt_cache_hit_ratio("openai", &Usage { prompt_tokens: 1000, ..Usage::default() }).abs() < f64::EPSILON);
}

#[test]
fn cache_hit_ratio_includes_all_sampling_iterations_and_prefers_explicit_reads() {
    let usage = Usage {
        prompt_tokens: 10,
        cached_prompt_tokens: Some(999),
        iterations: Some(vec![
            serde_json::json!({"type":"compaction", "input_tokens":100, "cache_read_input_tokens":200}),
            serde_json::json!({"type":"message", "input_tokens":50, "cache_read_input_tokens":600, "cache_creation_input_tokens":50}),
        ]),
        ..Usage::default()
    };
    assert!((prompt_cache_hit_ratio("anthropic", &usage) - 0.8).abs() < 1e-12);
    let usage = Usage {
        prompt_tokens: 1000,
        cached_prompt_tokens: Some(999),
        cache_read_tokens: Some(400),
        ..Usage::default()
    };
    assert!((prompt_cache_hit_ratio("openai", &usage) - 0.4).abs() < 1e-12);
}
