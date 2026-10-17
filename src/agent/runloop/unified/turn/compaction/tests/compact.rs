use super::*;

#[tokio::test]
async fn manual_compaction_succeeds_without_server_side_support() {
    let temp = tempdir().expect("tempdir");
    let provider = RoomyLocalCompactionProvider;
    let mut history = tool_heavy_history(ROOMY_HISTORY_CYCLES);
    let original_history = history.clone();
    let original_len = history.len();
    let mut session_stats = SessionStats::default();
    session_stats.set_previous_response_chain("stub-roomy", "stub-model", Some("resp_123"), &[]);
    let mut context_manager = test_context_manager();
    context_manager.update_token_usage(&Some(Usage {
        prompt_tokens: 900,
        completion_tokens: 10,
        total_tokens: 910,
        ..Usage::default()
    }));

    let outcome = compact_history_in_place(
        &provider,
        "stub-model",
        "session-alpha",
        temp.path(),
        Some(&VTCodeConfig::default()),
        &mut history,
        &mut session_stats,
        &mut context_manager,
    )
    .await
    .expect("manual compaction succeeds")
    .expect("history should compact");

    assert_eq!(outcome.original_len, original_len);
    // The history exceeds the continuity tail, so compaction must strictly
    // shrink the message count (never report growth as success).
    assert!(
        outcome.compacted_len < outcome.original_len,
        "expected shrink, got {} -> {}",
        outcome.original_len,
        outcome.compacted_len
    );
    assert_local_compaction_history(&history, 0);
    // Continuity is preserved: the newest turn survives verbatim while the
    // oldest summarized-away turns are gone.
    assert!(
        history
            .iter()
            .any(|message| message.content.as_text() == "Summarize the investigation so far"),
        "continuity tail should retain the latest user turn"
    );
    assert!(
        !history
            .iter()
            .any(|message| message.content.as_text().contains("Investigate failure 0")),
        "oldest turns should be summarized away, got {} messages",
        history.len()
    );
    assert_ne!(history, original_history);
    assert_eq!(session_stats.previous_response_id_for("stub-roomy", "stub-model"), None);
    assert!(context_manager.current_token_usage() <= 900);
    assert!(latest_memory_envelope_path_for_session(temp.path(), "session-alpha").is_some());
}

#[tokio::test]
async fn manual_compaction_reports_already_compact_when_tail_covers_history() {
    // Regression test for the reported `86 -> 88` growth: a tool-heavy history
    // that fits entirely within the continuity tail must not be "compacted"
    // into a longer history. Both entry points return `None` and leave the
    // history untouched.
    let temp = tempdir().expect("tempdir");
    let provider = LocalCompactionProvider;
    for entry in [
        "manual_compact_history_in_place",
        "compact_history_on_model_switch_in_place",
    ] {
        let mut history = tail_fitting_tool_history();
        let original_history = history.clone();
        let mut session_stats = SessionStats::default();
        let mut context_manager = test_context_manager();

        let outcome = if entry == "manual_compact_history_in_place" {
            manual_compact_history_in_place(
                CompactionContext::new(
                    &provider,
                    "stub-model",
                    "session-alpha",
                    "thread-alpha",
                    temp.path(),
                    Some(&VTCodeConfig::default()),
                    None,
                    None,
                ),
                CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
                &ManualCompactionOptions::default(),
                false,
            )
            .await
            .expect("compaction check succeeds")
        } else {
            compact_history_on_model_switch_in_place(
                CompactionContext::new(
                    &provider,
                    "stub-model",
                    "session-alpha",
                    "thread-alpha",
                    temp.path(),
                    Some(&VTCodeConfig::default()),
                    None,
                    None,
                ),
                CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
            )
            .await
            .expect("compaction check succeeds")
        };

        assert!(outcome.is_none(), "{entry} must not report growth as compaction");
        assert_eq!(history, original_history, "{entry} must leave history untouched");
    }
}

#[tokio::test]
async fn manual_compaction_emits_local_compaction_boundary_event() {
    let temp = tempdir().expect("tempdir");
    let provider = RoomyLocalCompactionProvider;
    let harness_path = temp.path().join("harness.jsonl");
    let harness_emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");
    let mut history = tool_heavy_history(ROOMY_HISTORY_CYCLES);
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = compact_history_in_place_with_events(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            Some(&harness_emitter),
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        vtcode_core::exec::events::CompactionTrigger::Manual,
    )
    .await
    .expect("compaction succeeds")
    .expect("history should compact");

    assert_eq!(outcome.mode, vtcode_core::exec::events::CompactionMode::Local);
    assert!(
        outcome.compacted_len < outcome.original_len,
        "expected shrink, got {} -> {}",
        outcome.original_len,
        outcome.compacted_len
    );
    let content = fs::read_to_string(harness_path).expect("read harness log");
    assert!(content.contains("\"type\":\"thread.compact_boundary\""));
    assert!(content.contains("\"mode\":\"local\""));
    assert!(content.contains("\"new_segment_id\":\"segment-00000001\""));
}

#[tokio::test]
async fn provider_compaction_emits_provider_boundary_event() {
    let temp = tempdir().expect("tempdir");
    let provider = ProviderCompactionProvider;
    let harness_path = temp.path().join("provider-harness.jsonl");
    let harness_emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");
    let mut history = test_history();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = compact_history_in_place_with_events(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            Some(&harness_emitter),
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        vtcode_core::exec::events::CompactionTrigger::Manual,
    )
    .await
    .expect("compaction succeeds")
    .expect("history should compact");

    assert_eq!(outcome.mode, vtcode_core::exec::events::CompactionMode::Provider);
    let content = fs::read_to_string(harness_path).expect("read harness log");
    assert!(content.contains("\"type\":\"thread.compact_boundary\""));
    assert!(content.contains("\"mode\":\"provider\""));
}

#[tokio::test]
async fn manual_compaction_clears_previous_response_chain() {
    let temp = tempdir().expect("tempdir");
    let provider = ProviderCompactionProvider;
    let mut history = test_history();
    let mut session_stats = SessionStats::default();
    session_stats.set_previous_response_chain("provider-stub", "stub-model", Some("resp_123"), &[]);
    let mut context_manager = test_context_manager();

    let outcome = manual_compact_history_in_place(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        &ManualCompactionOptions::default(),
        false,
    )
    .await
    .expect("manual OpenAI compaction succeeds")
    .expect("history should compact");

    assert_eq!(outcome.mode, vtcode_core::exec::events::CompactionMode::Provider);
    assert_eq!(session_stats.previous_response_id_for("provider-stub", "stub-model"), None);
}

#[tokio::test]
async fn model_switch_compaction_tags_boundary_event_with_model_switch_trigger() {
    let temp = tempdir().expect("tempdir");
    let harness_path = temp.path().join("harness.log");
    let emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");
    let provider = ProviderCompactionProvider;
    let mut history = test_history();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = compact_history_on_model_switch_in_place(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            Some(&emitter),
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
    )
    .await
    .expect("model-switch compaction succeeds")
    .expect("history should compact");

    assert_eq!(outcome.mode, vtcode_core::exec::events::CompactionMode::Provider);

    let log = fs::read_to_string(&harness_path).expect("harness log readable");
    assert!(
        log.contains("\"trigger\":\"model_switch\""),
        "compact boundary event should carry the model_switch trigger, got: {log}"
    );
}

#[tokio::test]
async fn manual_compaction_native_only_rejects_provider_without_standalone_compaction() {
    let temp = tempdir().expect("tempdir");
    let provider = LocalCompactionProvider;
    let mut history = test_history();
    let original_history = history.clone();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let err = manual_compact_history_in_place(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        &ManualCompactionOptions::default(),
        true,
    )
    .await
    .expect_err("native-only should reject a non-standalone provider");

    assert!(err.to_string().contains(
        "`--native-only` `/compact` requires a provider that exposes a native server-side compaction endpoint"
    ));
    assert!(err.to_string().contains("Run `/compact` without `--native-only`"));
    assert_eq!(history, original_history);
}

#[tokio::test]
async fn manual_compaction_compacts_locally_for_non_native_provider() {
    let temp = tempdir().expect("tempdir");
    let provider = RoomyLocalCompactionProvider;
    let mut history = tool_heavy_history(ROOMY_HISTORY_CYCLES);
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = manual_compact_history_in_place(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        &ManualCompactionOptions::default(),
        false,
    )
    .await
    .expect("local compaction succeeds")
    .expect("history should compact");

    assert_eq!(outcome.mode, vtcode_core::exec::events::CompactionMode::Local);
    assert!(
        outcome.compacted_len < outcome.original_len,
        "expected shrink, got {} -> {}",
        outcome.original_len,
        outcome.compacted_len
    );
    assert_local_compaction_history(&history, 0);
}

#[tokio::test]
async fn manual_compaction_noop_preserves_existing_history() {
    let temp = tempdir().expect("tempdir");
    let provider = NoOpProviderCompactionProvider;
    let mut history = test_history_with_memory_envelope();
    let original_history = history.clone();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = manual_compact_history_in_place(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        &ManualCompactionOptions::default(),
        false,
    )
    .await
    .expect("noop compaction succeeds");

    assert!(outcome.is_none());
    assert_eq!(history, original_history);
}

#[tokio::test]
async fn provider_compaction_noop_preserves_existing_history() {
    let temp = tempdir().expect("tempdir");
    let provider = NoOpProviderCompactionProvider;
    let mut history = test_history_with_memory_envelope();
    let original_history = history.clone();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = compact_history_in_place_with_events(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        vtcode_core::exec::events::CompactionTrigger::Manual,
    )
    .await
    .expect("noop compaction succeeds");

    assert!(outcome.is_none());
    assert_eq!(history, original_history);
}

#[tokio::test]
async fn provider_compaction_preserves_original_repeated_file_reads() {
    let temp = tempdir().expect("tempdir");
    let seen_history = Arc::new(RwLock::new(Vec::new()));
    let provider = RecordingProviderCompactionProvider { seen_history: Arc::clone(&seen_history) };
    let mut history = vec![
        assistant_with_tool_call(read_file_tool_call("call-1", "src/lib.rs")),
        Message::tool_response_with_origin(
            "call-1".to_string(),
            json!({
                "file_path": "src/lib.rs",
                "start_line": 1,
                "end_line": 40,
                "result": "older contents"
            })
            .to_string(),
            tool_names::READ_FILE.to_string(),
        ),
        assistant_with_tool_call(read_file_tool_call("call-2", "src/lib.rs")),
        Message::tool_response_with_origin(
            "call-2".to_string(),
            json!({
                "file_path": "src/lib.rs",
                "start_line": 1,
                "end_line": 40,
                "result": "newer contents"
            })
            .to_string(),
            tool_names::READ_FILE.to_string(),
        ),
    ];
    history.extend(test_history());
    let original_history = history.clone();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = compact_history_in_place_with_events(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&VTCodeConfig::default()),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
        vtcode_core::exec::events::CompactionTrigger::Manual,
    )
    .await
    .expect("provider compaction succeeds");

    assert!(outcome.is_none());
    assert_eq!(history, original_history);

    let seen = seen_history.read().await.clone();
    assert_eq!(seen.len(), original_history.len());
    assert!(seen[1].content.as_text().contains("older contents"));
    assert!(!seen[1].content.as_text().contains("deduped_read"));
}
