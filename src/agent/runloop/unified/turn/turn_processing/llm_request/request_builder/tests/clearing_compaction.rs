use super::*;

#[tokio::test]
async fn interactive_cli_reuses_overlapping_inspections_then_applies_authorized_patch() {
    struct ScriptedProvider(Arc<std::sync::atomic::AtomicUsize>);
    #[async_trait::async_trait]
    impl uni::LLMProvider for ScriptedProvider {
        fn name(&self) -> &str {
            "openai"
        }
        fn supports_streaming(&self) -> bool {
            false
        }
        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".into()]
        }
        fn validate_request(&self, _: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let step = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut response = uni::LLMResponse::new(request.model.clone(), "Updated the document.");
            let call = match step {
                0 => Some(("read-first", "exec_command", json!({"cmd":"sed -n '1,2p' sample.md"}))),
                1 => Some(("read-second", "exec_command", json!({"cmd":"sed -n '3,4p' sample.md"}))),
                2 => Some(("read-overlap", "exec_command", json!({"cmd":"sed -n '2,3p' sample.md"}))),
                3 => {
                    assert!(
                        request
                            .messages
                            .iter()
                            .any(|m| m.content.as_text().contains("only text evidence already gathered")),
                        "next CLI request must include the notice"
                    );
                    assert!(request_tool_names(&request).contains(&"apply_patch".to_string()));
                    Some((
                        "edit",
                        "apply_patch",
                        json!({"input":"*** Begin Patch\n*** Update File: sample.md\n@@\n-second\n+updated\n*** End Patch\n"}),
                    ))
                }
                4 => None,
                _ => panic!("unexpected additional provider request"),
            };
            if let Some((id, name, args)) = call {
                response.content = None;
                response.tool_calls = Some(vec![uni::ToolCall::function(id.into(), name.into(), args.to_string())]);
            }
            Ok(response)
        }
    }

    let mut backing = TestTurnProcessingBacking::new(12).await;
    let path = backing.workspace_path().join("sample.md");
    std::fs::write(&path, "first\nsecond\nthird\nfourth\n").unwrap();
    backing.add_tool_definition(named_tool("exec_command")).await;
    backing.add_tool_definition(named_tool("apply_patch")).await;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    backing.set_provider(Box::new(ScriptedProvider(calls.clone())));
    let mut history = vec![uni::Message::user("Update the document".into())];
    let outcome =
        crate::agent::runloop::unified::turn::turn_loop::run_turn_loop(&mut history, backing.turn_loop_context())
            .await
            .unwrap();
    assert!(matches!(
        outcome.result,
        crate::agent::runloop::unified::turn::context::TurnLoopResult::Completed { .. }
    ));
    assert!(!outcome.final_response_was_fallback);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 5);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "first\nupdated\nthird\nfourth\n");
}

#[tokio::test]
async fn session_232419_read_evidence_survives_request_clearing() {
    struct EvidencePatchProvider(Arc<std::sync::atomic::AtomicUsize>);
    #[async_trait::async_trait]
    impl uni::LLMProvider for EvidencePatchProvider {
        fn name(&self) -> &str {
            "openai"
        }
        fn supports_streaming(&self) -> bool {
            false
        }
        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".into()]
        }
        fn validate_request(&self, _: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let count = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert!(count < 2, "edit and completion must not enter another recovery pass");
            let mut response = uni::LLMResponse::new(request.model, "Updated the Quick start wording.");
            if count == 0 {
                let source = request
                    .messages
                    .iter()
                    .find(|message| message.tool_call_id.as_deref() == Some("read_1"))
                    .unwrap();
                assert!(source.content.as_text().contains("default. To manage installation yourself, use Homebrew or Cargo; Cargo installs require Rust 1.98.1+:"));
                response.content = None;
                response.tool_calls = Some(vec![uni::ToolCall::function("edit".into(), "apply_patch".into(), json!({
                    "input": "*** Begin Patch\n*** Update File: README.md\n@@\n-default. To manage installation yourself, use Homebrew or Cargo; Cargo installs require Rust 1.98.1+:\n+default. To manage installation yourself, use Homebrew or Cargo:\n*** End Patch\n"
                }).to_string())]);
            }
            Ok(response)
        }
    }
    // Exact successful read outputs from the failed implementation turn,
    // excluding the transport's structured metadata. Six README pages were
    // followed by anchor searches and alignment reads before the intended edit.
    let reads: Vec<serde_json::Value> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/crates/codegen/vtcode-core/src/core/agent/state/fixtures/session_232419_reads.json"
    )))
    .unwrap();
    assert_eq!(reads.len(), 15);
    for parallel in [false, true] {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        std::fs::write(backing.workspace_path().join("README.md"), reads[1]["output"].as_str().unwrap()).unwrap();
        let cfg = VTCodeConfig::default();
        backing.set_vt_cfg_for_test(cfg);
        let mut history = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.push(uni::Message::user("Earlier inspection".into()));
            let mut old = uni::Message::assistant(String::new());
            old.tool_calls = Some(vec![uni::ToolCall::function(
                "old".into(),
                "exec_command".into(),
                json!({"cmd": "git status --short"}).to_string(),
            )]);
            ctx.working_history.push(old);
            ctx.working_history
                .push(uni::Message::tool_response("old".into(), "obsolete output ".repeat(20000)));
            ctx.working_history
                .push(uni::Message::user("Implement the approved README refinement".into()));
            // Replay both the actual parallel batches and one-call batches.
            let mut batch_start = 0;
            while batch_start < reads.len() {
                let batch_end = if parallel {
                    reads[batch_start..]
                        .iter()
                        .take_while(|read| read["batch"] == reads[batch_start]["batch"])
                        .count()
                        + batch_start
                } else {
                    batch_start + 1
                };
                let mut assistant = uni::Message::assistant(String::new());
                assistant.tool_calls = Some(
                    (batch_start..batch_end)
                        .map(|index| {
                            uni::ToolCall::function(
                                format!("read_{index}"),
                                "exec_command".into(),
                                json!({"cmd": reads[index]["cmd"]}).to_string(),
                            )
                        })
                        .collect(),
                );
                ctx.working_history.push(assistant);
                for (index, read) in reads.iter().enumerate().take(batch_end).skip(batch_start) {
                    ctx.working_history.push(uni::Message::tool_response(
                        format!("read_{index}"),
                        read["output"].as_str().unwrap().into(),
                    ));
                }
                batch_start = batch_end;
            }
            let original = ctx.working_history.clone();
            assert!(original.iter().map(uni::Message::estimate_tokens).sum::<usize>() > 40_000);
            let snapshot = capture_turn_request_snapshot(&mut ctx, "test-model", false);
            let built = build_turn_request(&mut ctx, 1, "test-model", &snapshot, Some(2000), None, false)
                .await
                .unwrap();
            let evidence = built
                .continuation_messages
                .iter()
                .find(|message| message.tool_call_id.as_deref() == Some("read_1"))
                .unwrap();
            assert_eq!(evidence.content.as_text(), reads[1]["output"].as_str().unwrap());
            assert!(evidence.content.as_text().contains("Rust 1.98.1+"));
            assert!(
                built
                    .continuation_messages
                    .iter()
                    .find(|message| message.tool_call_id.as_deref() == Some("old"))
                    .unwrap()
                    .content
                    .as_text()
                    .contains("tool_result_clearing")
            );
            for message in built.continuation_messages.iter() {
                for call in message.tool_calls.iter().flatten() {
                    let args: serde_json::Value =
                        serde_json::from_str(&call.function.as_ref().unwrap().arguments).unwrap();
                    assert!(args["cmd"].is_string(), "small executable arguments survive: {args}");
                }
            }
            assert!(vtcode_core::core::agent::state::validate_history_invariants(&built.request.messages).is_valid());
            assert!(ctx.working_history.starts_with(&original));
            original
        };
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        backing.set_provider(Box::new(EvidencePatchProvider(calls.clone())));
        let outcome =
            crate::agent::runloop::unified::turn::turn_loop::run_turn_loop(&mut history, backing.turn_loop_context())
                .await
                .unwrap();
        assert!(matches!(
            outcome.result,
            crate::agent::runloop::unified::turn::context::TurnLoopResult::Completed {
                plan_approved_execution_pending: false
            }
        ));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(!outcome.final_response_was_fallback);
        let edited = std::fs::read_to_string(backing.workspace_path().join("README.md")).unwrap();
        assert!(edited.contains("default. To manage installation yourself, use Homebrew or Cargo:\n"));
        assert!(!edited.contains("Cargo; Cargo installs require"));
    }
}
#[tokio::test]
async fn local_clearing_uses_session_budget_and_preserves_recent_batch() {
    for (enabled, recent_batches, native) in [
        (true, 0, false),
        (true, 1, false),
        (true, 2, false),
        (false, 2, false),
        (true, 0, true),
        (true, 2, true),
    ] {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.inspection.mode = vtcode_core::config::core::agent::InspectionMode::Enforce;
        cfg.context.max_context_tokens = 16_000;
        cfg.agent.harness.tool_result_clearing.trigger_tokens = 40_000;
        cfg.agent.harness.tool_result_clearing.enabled = enabled;
        cfg.agent.harness.tool_result_clearing.keep_recent_tool_batches = recent_batches;
        backing.set_vt_cfg_for_test(cfg);
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.push(uni::Message::user("Inspect evidence".into()));
        for (batch, count) in [1, 4, 3].into_iter().enumerate() {
            let mut assistant = uni::Message::assistant("Inspect batch".into());
            assistant.tool_calls = Some(
                (0..count)
                    .map(|call| {
                        uni::ToolCall::function(
                            format!("call_{batch}_{call}"),
                            "read_file".into(),
                            "{\"path\":\"source.rs\"}".into(),
                        )
                    })
                    .collect(),
            );
            ctx.working_history.push(assistant);
            for call in (0..count).rev() {
                ctx.working_history.push(uni::Message::tool_response(
                    format!("call_{batch}_{call}"),
                    format!("evidence_{batch}_{call} ").repeat(600),
                ));
            }
        }
        let original = ctx.working_history.clone();
        let tokens = original.iter().map(uni::Message::estimate_tokens).sum::<usize>();
        assert!(tokens < 40_000 && tokens > 9_600, "fixture lies between capped and configured triggers");
        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "test-model", false);
        if native {
            snapshot.provider_name = "anthropic".into();
            snapshot.capabilities.context_edits = true;
        }
        let built = build_turn_request(&mut ctx, 1, "test-model", &snapshot, Some(2_000), None, false)
            .await
            .unwrap();
        let results: Vec<_> = built
            .continuation_messages
            .iter()
            .filter(|message| message.role == uni::MessageRole::Tool)
            .collect();
        assert_eq!(results.len(), 8);
        assert_eq!(results[0].content.as_text().contains("\"cleared\":\"tool_result\""), enabled);
        for result in &results[1..5] {
            assert_eq!(result.content.as_text().contains("\"cleared\":\"tool_result\""), enabled && recent_batches < 2);
        }
        for result in &results[5..] {
            assert!(result.content.as_text().contains("evidence_2_"));
        }
        assert!(vtcode_core::core::agent::state::validate_history_invariants(&built.request.messages).is_valid());
        assert!(ctx.working_history.starts_with(&original), "durable history retains full evidence");
    }
}
#[tokio::test]
async fn anthropic_request_build_combines_clearing_and_compaction_when_enabled() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut cfg = VTCodeConfig::default();
    cfg.agent.provider = "anthropic".to_string();
    cfg.agent.harness.auto_compaction_enabled = true;
    cfg.agent.harness.auto_compaction_threshold_tokens = Some(100_000);
    cfg.agent.harness.tool_result_clearing.enabled = true;
    // The default advisory policy preserves supported native clearing.
    assert_eq!(cfg.agent.harness.inspection.mode, vtcode_core::config::core::agent::InspectionMode::Warn);
    cfg.agent.harness.tool_result_clearing.trigger_tokens = 120_000;
    cfg.agent.harness.tool_result_clearing.keep_tool_uses = 5;
    cfg.agent.harness.tool_result_clearing.clear_at_least_tokens = 40_000;
    cfg.provider.anthropic.memory.enabled = true;
    let cfg = Box::leak(Box::new(cfg));

    let mut ctx = backing.turn_processing_context();
    ctx.vt_cfg = Some(cfg);
    let mut snapshot = capture_turn_request_snapshot(&mut ctx, "claude-sonnet-5", false);
    snapshot.provider_name = "anthropic".to_string();
    snapshot.capabilities.context_edits = true;

    let built = build_turn_request(&mut ctx, 1, "claude-sonnet-5", &snapshot, Some(320), None, false)
        .await
        .expect("anthropic request should build");

    assert_eq!(
        built.request.context_management,
        Some(json!({
            "edits": [{
                "type": "clear_tool_uses_20250919",
                "trigger": { "type": "input_tokens", "value": 120000 },
                "keep": { "type": "tool_uses", "value": 5 },
                "clear_at_least": { "type": "input_tokens", "value": 40000 },
                "clear_tool_inputs": true,
                "exclude_tools": ["memory"],
            }, {
                "type": "compact_20260112",
                "trigger": { "type": "input_tokens", "value": 100000 },
            }]
        }))
    );

    let mut compaction_only_cfg = VTCodeConfig::default();
    compaction_only_cfg.agent.provider = "anthropic".to_string();
    compaction_only_cfg.agent.harness.auto_compaction_enabled = true;
    compaction_only_cfg.agent.harness.auto_compaction_threshold_tokens = Some(90_000);
    // `tool_result_clearing` defaults to enabled; disable it here so this
    // scenario exercises the "compaction only" path (clearing off).
    compaction_only_cfg.agent.harness.tool_result_clearing.enabled = false;
    ctx.vt_cfg = Some(Box::leak(Box::new(compaction_only_cfg)));
    let built = build_turn_request(&mut ctx, 1, "claude-sonnet-5", &snapshot, Some(320), None, false)
        .await
        .expect("compaction-only anthropic request should build");
    assert_eq!(
        built.request.context_management,
        Some(json!({
            "edits": [{
                "type": "compact_20260112",
                "trigger": { "type": "input_tokens", "value": 90000 },
            }]
        }))
    );

    // Default now enables auto-compaction, so explicitly disable it here to
    // assert the "no context management payload" (disabled) path.
    let mut disabled_cfg = VTCodeConfig::default();
    disabled_cfg.agent.harness.auto_compaction_enabled = false;
    // `tool_result_clearing` defaults to enabled; disable it here so the
    // "no context management payload" (fully disabled) path is exercised.
    disabled_cfg.agent.harness.tool_result_clearing.enabled = false;
    ctx.vt_cfg = Some(Box::leak(Box::new(disabled_cfg)));
    let built = build_turn_request(&mut ctx, 1, "claude-sonnet-5", &snapshot, Some(320), None, false)
        .await
        .expect("disabled anthropic request should build");
    assert!(built.request.context_management.is_none());
}
#[tokio::test]
async fn openai_request_build_keeps_existing_compaction_payload() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut cfg = VTCodeConfig::default();
    cfg.agent.harness.auto_compaction_enabled = true;
    cfg.agent.harness.auto_compaction_threshold_tokens = Some(512);
    let cfg = Box::leak(Box::new(cfg));

    let mut ctx = backing.turn_processing_context();
    ctx.vt_cfg = Some(cfg);
    let mut snapshot = capture_turn_request_snapshot(&mut ctx, "gpt-5", false);
    snapshot.capabilities.responses_compaction = true;

    let built = build_turn_request(&mut ctx, 1, "gpt-5", &snapshot, Some(320), None, false)
        .await
        .expect("openai request should build");

    assert_eq!(
        built.request.context_management,
        Some(json!([{
            "type": "compaction",
            "compact_threshold": 512,
        }]))
    );
}
