use super::*;

#[tokio::test]
async fn interactive_compaction_forks_matching_parent_prompt_and_ordered_tools() {
    use vtcode_core::exec::events::CompactionTrigger;
    use vtcode_core::llm::provider::{ToolChoice, ToolDefinition};
    struct RecordingParentProvider(Arc<RwLock<Vec<LLMRequest>>>);
    #[async_trait]
    impl LLMProvider for RecordingParentProvider {
        fn name(&self) -> &str {
            "stub-roomy"
        }
        fn effective_context_size(&self, _: &str) -> usize {
            128000
        }
        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }
        fn validate_request(&self, _: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }
        async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
            self.0.write().await.push(request);
            Ok(LLMResponse::new("stub-model", "summary of completed checks"))
        }
    }
    for trigger in [
        CompactionTrigger::Manual,
        CompactionTrigger::Recovery,
        CompactionTrigger::Auto,
    ] {
        let temp = tempdir().unwrap();
        let requests = Arc::new(RwLock::new(Vec::new()));
        let provider = RecordingParentProvider(Arc::clone(&requests));
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.auto_compaction_enabled = true;
        cfg.agent.harness.auto_compaction_threshold_tokens = Some(700);
        let mut history = tool_heavy_history(ROOMY_HISTORY_CYCLES);
        let mut stats = SessionStats::default();
        let prompt = "stable system\n\n[Runtime Context]\n- turn: 4";
        let alpha = ToolDefinition::function(
            "alpha".into(),
            "Inspect a file".into(),
            json!({"type":"object", "properties":{"path":{"type":"string"}}}),
        );
        let zeta = ToolDefinition::function(
            "zeta".into(),
            "Compute a bound".into(),
            json!({"type":"object", "properties":{"limit":{"type":"integer"}}}),
        );
        let envelope = stats.request_envelope(
            "stub-model",
            "stub-roomy",
            "build",
            prompt.into(),
            vec![zeta.clone(), alpha.clone()],
            7,
        );
        let mut manager = test_context_manager();
        manager.update_token_usage(Some(&Usage {
            prompt_tokens: 900,
            completion_tokens: 10,
            total_tokens: 910,
            ..Default::default()
        }));
        let context = CompactionContext::new(
            &provider,
            "stub-model",
            "parent-session",
            "parent-thread",
            temp.path(),
            Some(&cfg),
            None,
            None,
        );
        let state = CompactionState::new(&mut history, &mut stats, &mut manager);
        let outcome = if trigger == CompactionTrigger::Auto {
            maybe_auto_compact_history(context, state).await
        } else {
            compact_history_in_place_with_events(context, state, trigger).await
        };
        outcome.unwrap().expect("compaction commits");
        let requests = requests.read().await;
        assert!(!requests.is_empty());
        for request in requests.iter() {
            assert_eq!(request.system_prompt.as_deref(), Some(prompt), "{trigger:?}");
            assert_eq!(serde_json::to_value(request.tools.as_deref().unwrap()).unwrap(), json!([alpha, zeta]));
            assert!(matches!(request.tool_choice, Some(ToolChoice::None)));
        }
        assert_eq!(envelope.system_prompt().as_ref(), prompt, "shared parent remains immutable");
        assert!(stats.compaction_parent_context("stub-roomy", "stub-model").is_none());
    }
}

#[tokio::test]
async fn provider_compaction_error_preserves_existing_history() {
    let temp = tempdir().expect("tempdir");
    let provider = FailingProviderCompactionProvider;
    let mut history = test_history_with_memory_envelope();
    let original_history = history.clone();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let err = compact_history_in_place_with_events(
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
    .expect_err("failing provider should fail");

    assert!(!err.to_string().is_empty());
    assert_eq!(history, original_history);
}

#[tokio::test]
async fn successful_manual_recovery_and_model_switch_compaction_reset_cache_streak() {
    use vtcode_core::exec::events::CompactionTrigger;
    for trigger in [
        CompactionTrigger::Manual,
        CompactionTrigger::Recovery,
        CompactionTrigger::ModelSwitch,
    ] {
        let temp = tempdir().unwrap();
        let mut history = test_history_with_memory_envelope();
        let mut stats = SessionStats::default();
        let usage = Some(Usage {
            prompt_tokens: 12740,
            completion_tokens: 147,
            total_tokens: 12887,
            cached_prompt_tokens: Some(0),
            cache_creation_tokens: Some(5212),
            ..Default::default()
        });
        assert!(stats.record_cache_turn_health("openai", usage.as_ref()).is_none());
        assert!(stats.record_cache_turn_health("openai", usage.as_ref()).is_none());
        let mut context = test_context_manager();
        compact_history_in_place_with_events(
            CompactionContext::new(
                &ProviderCompactionProvider,
                "stub-model",
                "cache-boundary",
                "cache-thread",
                temp.path(),
                Some(&VTCodeConfig::default()),
                None,
                None,
            ),
            CompactionState::new(&mut history, &mut stats, &mut context),
            trigger,
        )
        .await
        .unwrap()
        .expect("successful compaction");
        assert!(stats.record_cache_turn_health("openai", usage.as_ref()).is_none(), "{trigger:?}");
        assert!(stats.record_cache_turn_health("openai", usage.as_ref()).is_none());
        assert!(stats.record_cache_turn_health("openai", usage.as_ref()).is_some());
    }
}

#[tokio::test]
async fn recovery_compaction_falls_back_to_local_when_inline_request_errors() {
    let temp = tempdir().expect("tempdir");
    let provider = InlineRejectingRecoveryProvider;
    let mut history = tool_heavy_history(ROOMY_HISTORY_CYCLES);
    let original_history = history.clone();
    let original_len = history.len();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    // A near-exhausted-budget recovery on a NativeInline-dispatched provider that
    // rejects the inline compact edit must not abort; it falls back to Local
    // summarization and compacts the earlier history.
    let outcome = compact_history_for_recovery_in_place(
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
        original_len,
    )
    .await
    .expect("recovery compaction should fall back to local")
    .expect("history should compact");

    assert_eq!(outcome.mode, vtcode_core::exec::events::CompactionMode::Local);
    assert!(
        outcome.compacted_len < outcome.original_len,
        "expected shrink, got {} -> {}",
        outcome.original_len,
        outcome.compacted_len
    );
    assert_ne!(history, original_history);
}

#[tokio::test]
async fn auto_compaction_replaces_history_and_clears_response_chain() {
    let temp = tempdir().expect("tempdir");
    let provider = RoomyLocalCompactionProvider;
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.auto_compaction_enabled = true;
    vt_cfg.agent.harness.auto_compaction_threshold_tokens = Some(700);

    let mut history = tool_heavy_history(ROOMY_HISTORY_CYCLES);
    let original_len = history.len();
    let estimated_tokens_before: usize = history.iter().map(Message::estimate_tokens).sum();
    let mut session_stats = SessionStats::default();
    session_stats.set_previous_response_chain("stub-roomy", "stub-model", Some("resp_123"), &[]);
    let mut context_manager = test_context_manager();
    context_manager.update_token_usage(Some(&Usage {
        prompt_tokens: 900,
        completion_tokens: 10,
        total_tokens: 910,
        ..Usage::default()
    }));

    let outcome = maybe_auto_compact_history(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&vt_cfg),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
    )
    .await
    .expect("auto compaction succeeds")
    .expect("history should compact");

    assert_eq!(outcome.original_len, original_len);
    let estimated_tokens_after: usize = history.iter().map(Message::estimate_tokens).sum();
    assert_eq!(outcome.estimated_tokens_before, Some(estimated_tokens_before));
    assert_eq!(outcome.estimated_tokens_after, Some(estimated_tokens_after));
    // The history exceeds the continuity tail, so auto-compaction must strictly
    // shrink the message count (never report growth as success).
    assert!(
        outcome.compacted_len < outcome.original_len,
        "expected shrink, got {} -> {}",
        outcome.original_len,
        outcome.compacted_len
    );
    assert_local_compaction_history(&history, 4);
    assert!(history[0].content.as_text().contains("Previous conversation summary"));
    assert_eq!(session_stats.previous_response_id_for("stub-roomy", "stub-model"), None);
    assert!(context_manager.current_token_usage() <= 700);
    assert!(latest_memory_envelope_path_for_session(temp.path(), "session-alpha").is_some());
}

#[tokio::test]
async fn auto_compaction_uses_the_effective_session_safety_ceiling() {
    let temp = tempdir().expect("tempdir");
    let provider = ContextSizedProvider { context_size: 500_000, provider_name: "openai" };
    let vt_cfg = VTCodeConfig::default();
    let mut history = tool_heavy_history(ROOMY_HISTORY_CYCLES);
    let original_len = history.len();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();
    context_manager.update_token_usage(Some(&Usage {
        prompt_tokens: 496_000,
        completion_tokens: 10,
        total_tokens: 496_010,
        ..Usage::default()
    }));

    let outcome = maybe_auto_compact_history(
        CompactionContext::new(
            &provider,
            "stub-model",
            "session-alpha",
            "thread-alpha",
            temp.path(),
            Some(&vt_cfg),
            None,
            None,
        ),
        CompactionState::new(&mut history, &mut session_stats, &mut context_manager),
    )
    .await
    .expect("auto compaction succeeds")
    .expect("496k prompt pressure must cross the 495.9k provider boundary");

    assert_eq!(outcome.original_len, original_len);
    assert!(
        outcome.compacted_len < outcome.original_len,
        "expected shrink, got {} -> {}",
        outcome.original_len,
        outcome.compacted_len
    );
    assert!(context_manager.current_token_usage() <= 495_904);
}

#[tokio::test]
async fn targeted_compaction_preserves_prefix_and_replaces_suffix() {
    let temp = tempdir().expect("tempdir");
    let provider = RoomyLocalCompactionProvider;
    let mut history = vec![Message::user("keep this prefix".to_string())];
    history.extend(tool_heavy_history(ROOMY_HISTORY_CYCLES));
    let original_len = history.len();
    let estimated_tokens_before: usize = history.iter().map(Message::estimate_tokens).sum();
    let preserved_prefix = history[..1].to_vec();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();
    context_manager.update_token_usage(Some(&Usage {
        prompt_tokens: 900,
        completion_tokens: 10,
        total_tokens: 910,
        ..Usage::default()
    }));

    let outcome = compact_history_from_index_in_place(
        &provider,
        "stub-model",
        "session-alpha",
        temp.path(),
        Some(&VTCodeConfig::default()),
        &mut history,
        1,
        &mut session_stats,
        &mut context_manager,
    )
    .await
    .expect("targeted compaction succeeds")
    .expect("history should compact");

    assert_eq!(&history[..1], preserved_prefix.as_slice());
    assert_eq!(outcome.original_len, original_len);
    let estimated_tokens_after: usize = history.iter().map(Message::estimate_tokens).sum();
    assert_eq!(outcome.estimated_tokens_before, Some(estimated_tokens_before));
    assert_eq!(outcome.estimated_tokens_after, Some(estimated_tokens_after));
    // The suffix exceeds the continuity tail, so it compacts to a summary
    // plus tail instead of being kept verbatim.
    assert!(
        outcome.compacted_len < outcome.original_len,
        "expected shrink, got {} -> {}",
        outcome.original_len,
        outcome.compacted_len
    );
    // The suffix's oldest turns are summarized away while its newest turn
    // remains verbatim.
    assert!(
        history
            .iter()
            .any(|m| m.content.as_text() == "Summarize the investigation so far")
    );
    assert!(!history.iter().any(|m| m.content.as_text().contains("Investigate failure 0")));
    assert!(
        history
            .iter()
            .any(|m| m.content.as_text().contains("[Session Memory Envelope]"))
    );
    assert!(
        history
            .iter()
            .any(|m| { m.content.as_text().contains("Previous conversation summary") })
    );
    assert!(latest_memory_envelope_path_for_session(temp.path(), "session-alpha").is_none());
}

#[tokio::test]
async fn recovery_compaction_preserves_current_turn_suffix_and_emits_event() {
    let temp = tempdir().expect("tempdir");
    let provider = RoomyLocalCompactionProvider;
    let harness_path = temp.path().join("recovery-harness.jsonl");
    let harness_emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");
    let mut history = tool_heavy_history(ROOMY_HISTORY_CYCLES);
    let prefix_len = history.len();
    history.push(Message::system("Previous turn already completed tool execution. Reuse the latest tool outputs in history instead of rerunning the same exploration. If those tool outputs include `critical_note`, `hint`, `next_action`, `fallback_tool`, `fallback_tool_args`, or `rerun_hint`, follow that guidance first.".to_string()));
    history.push(Message::system("Model follow-up failed after tool activity. Tools are disabled on the next pass; provide a direct textual response from the current context and reuse the latest tool outputs already in history.".to_string()));
    history.push(Message::user("current-turn".to_string()));
    history.push(Message::assistant("".to_string()));
    history.push(Message::tool_response("call-current".to_string(), "{\"ok\":true}".to_string()));
    let estimated_tokens_before: usize = history.iter().map(Message::estimate_tokens).sum();
    let preserved_suffix = history[prefix_len..].to_vec();
    let mut session_stats = SessionStats::default();
    session_stats.set_previous_response_chain("stub-roomy", "stub-model", Some("resp-recovery"), &[]);
    let mut context_manager = test_context_manager();
    context_manager.update_token_usage(Some(&Usage {
        prompt_tokens: 950,
        completion_tokens: 10,
        total_tokens: 960,
        ..Usage::default()
    }));

    let outcome = compact_history_for_recovery_in_place(
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
        prefix_len,
    )
    .await
    .expect("recovery compaction succeeds")
    .expect("history should compact");

    assert_eq!(history[history.len() - preserved_suffix.len()..], preserved_suffix);
    let estimated_tokens_after: usize = history.iter().map(Message::estimate_tokens).sum();
    assert_eq!(outcome.estimated_tokens_before, Some(estimated_tokens_before));
    assert_eq!(outcome.estimated_tokens_after, Some(estimated_tokens_after));
    assert!(
        outcome.compacted_len < outcome.original_len,
        "expected shrink, got {} -> {}",
        outcome.original_len,
        outcome.compacted_len
    );
    assert_eq!(session_stats.previous_response_id_for("stub-roomy", "stub-model"), None);
    assert!(context_manager.current_token_usage() <= 900);

    let content = fs::read_to_string(harness_path).expect("read harness log");
    assert_eq!(content.matches("\"type\":\"thread.compact_boundary\"").count(), 1);
    assert!(content.contains("\"trigger\":\"recovery\""));
    assert!(content.contains("\"mode\":\"local\""));
    assert!(history.iter().any(|message| message.content.as_text() == "current-turn"));
}

#[tokio::test]
async fn recovery_compaction_uses_provider_mode_when_supported() {
    let temp = tempdir().expect("tempdir");
    let provider = ProviderCompactionProvider;
    let harness_path = temp.path().join("provider-recovery-harness.jsonl");
    let harness_emitter = HarnessEventEmitter::new(harness_path.clone()).expect("emitter");
    let mut history = test_history();
    history.push(Message::user("current-turn".to_string()));
    history.push(Message::assistant("".to_string()));
    history.push(Message::tool_response("call-current".to_string(), "{\"ok\":true}".to_string()));
    let preserved_suffix = history[12..].to_vec();
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = compact_history_for_recovery_in_place(
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
        12,
    )
    .await
    .expect("provider recovery compaction succeeds")
    .expect("history should compact");

    assert_eq!(outcome.mode, vtcode_core::exec::events::CompactionMode::Provider);
    assert_eq!(history[history.len() - preserved_suffix.len()..], preserved_suffix);

    let content = fs::read_to_string(harness_path).expect("read harness log");
    assert!(content.contains("\"trigger\":\"recovery\""));
    assert!(content.contains("\"mode\":\"provider\""));
}
