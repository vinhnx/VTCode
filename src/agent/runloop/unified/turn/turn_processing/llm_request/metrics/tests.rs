use super::*;

#[test]
fn reported_cache_usage_matches_sampling_totals_and_preserves_unknown_zero() {
    assert_eq!(
        ReportedCacheUsage::from_usage(&Usage::default()),
        ReportedCacheUsage { read_tokens: None, write_tokens: None }
    );
    let mut usage = Usage {
        cached_prompt_tokens: Some(0),
        cache_creation_tokens: Some(0),
        ..Usage::default()
    };
    assert_eq!(
        ReportedCacheUsage::from_usage(&usage),
        ReportedCacheUsage { read_tokens: Some(0), write_tokens: Some(0) }
    );
    usage.cache_read_tokens = Some(2);
    usage.cache_creation_tokens = Some(3);
    usage.iterations = Some(vec![
        serde_json::json!({"type": "message", "input_tokens": 50, "cache_read_input_tokens": 80,
            "cache_creation_input_tokens": 11}),
        serde_json::json!({"type": "message", "input_tokens": 20, "cache_read_input_tokens": 7,
            "cache_creation_input_tokens": 5}),
    ]);
    assert_eq!(
        ReportedCacheUsage::from_usage(&usage),
        ReportedCacheUsage { read_tokens: Some(87), write_tokens: Some(16) }
    );
}

#[test]
fn history_projection_metrics_locate_changes_without_mutating_history() {
    let before = vec![
        Message::user("alpha".into()),
        Message::user("a longer second message".into()),
    ];
    let mut after = before.clone();
    let unchanged = HistoryProjectionMetrics::between(&before, &after);
    assert_eq!(unchanged.changed_message_count, 0);
    assert_eq!(unchanged.first_changed_message_index, None);
    after[1] = Message::user("b".into());
    let changed = HistoryProjectionMetrics::between(&before, &after);
    assert_eq!(changed.changed_message_count, 1);
    assert_eq!(changed.first_changed_message_index, Some(1));
    assert!(changed.projected_estimated_tokens < changed.original_estimated_tokens);
    assert_eq!(before[1].content.as_text(), "a longer second message");
    after.push(Message::user("new".into()));
    assert_eq!(HistoryProjectionMetrics::between(&before, &after).changed_message_count, 2);
    for (left, right) in [(&before[..], &before[..1]), (&before[..1], &before[..])] {
        let changed = HistoryProjectionMetrics::between(left, right);
        assert_eq!(changed.changed_message_count, 1);
        assert_eq!(changed.first_changed_message_index, Some(1));
    }
}

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
