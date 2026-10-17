use super::*;
use crate::core::agent::state::validate_history_invariants;
use crate::llm::provider::{Message, MessageRole};

#[tokio::test]
async fn headless_noop_compaction_preserves_shared_history() {
    struct NoOpCase {
        enabled: bool,
        capacity: usize,
        projected_tokens: usize,
        suppressed: u8,
    }
    let cases = [
        NoOpCase {
            enabled: true,
            capacity: 32_000,
            projected_tokens: 1_000,
            suppressed: crate::compaction::SUPPRESS_NONE,
        },
        NoOpCase {
            enabled: false,
            capacity: 32_000,
            projected_tokens: 30_000,
            suppressed: crate::compaction::SUPPRESS_NONE,
        },
        NoOpCase {
            enabled: true,
            capacity: 32_000,
            projected_tokens: 30_000,
            suppressed: crate::compaction::SUPPRESS_STICKY,
        },
        NoOpCase {
            enabled: true,
            capacity: 0,
            projected_tokens: 1_000,
            suppressed: crate::compaction::SUPPRESS_NONE,
        },
    ];
    for case in cases {
        let temp = TempDir::new().unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.tool_result_clearing.enabled = false;
        cfg.agent.harness.auto_compaction_enabled = case.enabled;
        cfg.context.max_context_tokens = case.capacity;
        let mut runner = Box::pin(make_runner(&temp, cfg, "noop-sharing")).await;
        let provider = ClearingProvider {
            name: "queued-test-provider",
            capacity: case.capacity,
            requests: Arc::new(Mutex::new(Vec::new())),
        };
        runner.provider_client = Box::new(provider.clone());
        let mut bundle = runner.build_validated_runtime_prompt_bundle(false).await.unwrap();
        let mut state = AgentSessionState::new("noop-sharing".into(), 1, 1, case.capacity);
        state.messages = Arc::new(vec![Message::user("retained source ".repeat(1_000))]);
        state.auto_compact_suppressed = case.suppressed;
        let miss = crate::exec::events::Usage {
            input_tokens: 12740,
            cached_input_tokens: 0,
            cache_creation_tokens: 5212,
            output_tokens: 147,
        };
        assert!(state.stats.prompt_cache_health.record_turn(&miss).is_none());
        assert!(state.stats.prompt_cache_health.record_turn(&miss).is_none());
        let projection = runner.project_request_history(&state, "unknown-clearing-model", 0, 2_000);
        assert!(Arc::ptr_eq(&projection, &state.messages));
        let mut recorder = ExecEventRecorder::new("noop-sharing", None, None);
        let compacted = runner
            .maybe_auto_compact(
                &mut state,
                &mut recorder,
                "unknown-clearing-model",
                2,
                case.projected_tokens,
                2_000,
                &mut bundle.request_envelope,
            )
            .await;
        assert!(!compacted);
        assert!(state.stats.prompt_cache_health.record_turn(&miss).is_some(), "no-op must preserve the streak");
        assert!(Arc::ptr_eq(&projection, &state.messages), "no-op compaction must not copy shared history");
        assert_eq!(state.auto_compact_suppressed, case.suppressed);
        assert!(provider.requests.lock().is_empty());
        let events = recorder.into_events();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ThreadEvent::ThreadStarted(_)));
    }
}

#[tokio::test]
async fn headless_successful_compaction_starts_new_cache_health_streak() {
    let temp = TempDir::new().unwrap();
    let mut cfg = VTCodeConfig::default();
    cfg.agent.harness.auto_compaction_enabled = true;
    cfg.context.max_context_tokens = 32000;
    let mut runner = Box::pin(make_runner(&temp, cfg, "cache-compaction")).await;
    runner.provider_client = Box::new(ClearingProvider {
        name: "queued-test-provider",
        capacity: 32000,
        requests: Arc::new(Mutex::new(Vec::new())),
    });
    let mut bundle = runner.build_validated_runtime_prompt_bundle(false).await.unwrap();
    let mut state = AgentSessionState::new("cache-compaction".into(), 1, 1, 32000);
    state.messages = Arc::new(evidence_history(32000, false));
    let miss = crate::exec::events::Usage {
        input_tokens: 12740,
        cached_input_tokens: 0,
        cache_creation_tokens: 5212,
        output_tokens: 147,
    };
    assert!(state.stats.prompt_cache_health.record_turn(&miss).is_none());
    assert!(state.stats.prompt_cache_health.record_turn(&miss).is_none());
    assert!(
        runner
            .maybe_auto_compact(
                &mut state,
                &mut ExecEventRecorder::new("cache-compaction", None, None),
                "unknown-clearing-model",
                2,
                30000,
                2000,
                &mut bundle.request_envelope
            )
            .await
    );
    assert!(state.stats.prompt_cache_health.record_turn(&miss).is_none());
    assert!(state.stats.prompt_cache_health.record_turn(&miss).is_none());
    assert!(state.stats.prompt_cache_health.record_turn(&miss).is_some());
}

#[derive(Clone)]
struct ClearingProvider {
    name: &'static str,
    capacity: usize,
    requests: Arc<Mutex<Vec<LLMRequest>>>,
}

fn is_summary_request(request: &LLMRequest) -> bool {
    request.messages.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("Summarize the conversation so far using this exact structure")
    })
}

#[async_trait]
impl LLMProvider for ClearingProvider {
    fn name(&self) -> &str {
        self.name
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        self.capacity
    }

    fn supports_context_edits(&self, _model: &str) -> bool {
        self.name == "anthropic"
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let summary = is_summary_request(&request);
        self.requests.lock().push(request);
        Ok(text_response(if summary {
            "## Goal\nInspect evidence.\n## Progress\nOld evidence inspected.\n## Next Steps\nFinish the inspection."
        } else {
            "Inspection complete."
        }))
    }

    fn supported_models(&self) -> Vec<String> {
        vec![ModelId::default().as_str().into_owned()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }
}

fn evidence_body(label: &str, tokens: usize) -> String {
    let line = format!("{label}: measured evidence with a distinct observation.\n");
    let mut lower = 0;
    let mut upper = tokens.max(1);
    while lower < upper {
        let repetitions = lower + (upper - lower) / 2;
        let body = line.repeat(repetitions);
        if Message::tool_response("probe".into(), body).estimate_tokens() < tokens {
            lower = repetitions + 1;
        } else {
            upper = repetitions;
        }
    }
    line.repeat(lower)
}

fn evidence_history(capacity: usize, protected_large: bool) -> Vec<Message> {
    let mut messages = vec![Message::user("Inspect recorded evidence".into())];
    for (batch, count) in [1, 4, 3].into_iter().enumerate() {
        if batch > 0 {
            messages.push(Message::user(format!("Continue inspection for batch {batch}")));
        }
        let mut assistant = Message::assistant(format!("Inspect batch {batch}"));
        assistant.tool_calls = Some(
            (0..count)
                .map(|call| {
                    ToolCall::function(
                        format!("evidence_{batch}_{call}"),
                        "read_file".into(),
                        format!("{{\"path\":\"batch_{batch}_{call}.txt\"}}"),
                    )
                })
                .collect(),
        );
        messages.push(assistant);
        for call in (0..count).rev() {
            let tokens = match batch {
                0 => capacity / 2,
                1 => capacity / 10,
                _ if protected_large => capacity * 4 / 15,
                _ => 200 + call * 100,
            };
            messages.push(Message::tool_response(
                format!("evidence_{batch}_{call}"),
                evidence_body(&format!("raw_evidence_{batch}_{call}"), tokens),
            ));
        }
    }
    messages
}

#[tokio::test]
async fn headless_clearing_stages_before_compaction_capacity_sweep() {
    for capacity in [32_000, 64_000, 96_000, 128_000] {
        for (recent_batches, protected_large) in [(0, false), (0, true), (2, false), (2, true)] {
            let temp = TempDir::new().unwrap();
            let mut cfg = VTCodeConfig::default();
            cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
            cfg.agent.harness.auto_compaction_enabled = true;
            cfg.agent.harness.tool_result_clearing.keep_recent_tool_batches = recent_batches;
            cfg.context.max_context_tokens = capacity;
            cfg.context.preserve_recent_turns = 2;
            cfg.automation.full_auto.max_turns = 1;
            let mut runner = Box::pin(make_runner(&temp, cfg, "clearing-sweep")).await;
            let provider = ClearingProvider {
                name: "queued-test-provider",
                capacity,
                requests: Arc::new(Mutex::new(Vec::new())),
            };
            runner.provider_client = Box::new(provider.clone());
            let full_history = evidence_history(capacity, protected_large);
            runner.bootstrap_messages = full_history.clone();

            let bundle = runner.build_validated_runtime_prompt_bundle(false).await.unwrap();
            let overhead = bundle.system_prompt_report.token_estimate as usize
                + crate::llm::usage_cost::estimate_tool_definition_tokens(&bundle.request_envelope.ordered_tools())
                    as usize;
            let mut state = AgentSessionState::new("projection".into(), 1, 1, capacity);
            state.messages = Arc::new(full_history.clone());
            let projected = runner.project_request_history(&state, &ModelId::default().as_str(), overhead, 2_000);
            let projected_tokens = projected.iter().map(Message::estimate_tokens).sum::<usize>();
            let raw_tokens = full_history.iter().map(Message::estimate_tokens).sum::<usize>();
            let threshold = (capacity - 2_000) * 75 / 100;
            assert!(raw_tokens + overhead >= threshold, "fixture must pressure full history");
            assert_eq!(projected_tokens + overhead >= threshold, protected_large);
            assert_eq!(state.messages.as_ref(), &full_history, "projection must not alter durable history");
            assert!(validate_history_invariants(&projected).is_valid());
            let retained: Vec<_> = projected
                .iter()
                .filter(|message| {
                    message.role == MessageRole::Tool
                        && !message.content.as_text().contains("\"cleared\":\"tool_result\"")
                })
                .map(|message| message.tool_call_id.as_deref().unwrap())
                .collect();
            let expected = if recent_batches == 0 {
                vec!["evidence_2_2", "evidence_2_1", "evidence_2_0"]
            } else {
                vec![
                    "evidence_1_3",
                    "evidence_1_2",
                    "evidence_1_1",
                    "evidence_1_0",
                    "evidence_2_2",
                    "evidence_2_1",
                    "evidence_2_0",
                ]
            };
            assert_eq!(retained, expected);

            let mut inspection = task("Inspect retained evidence", "clearing-sweep");
            inspection.description = "Inspect the retained file evidence and reconcile the observations across the three historical tool batches. ".repeat(4);
            assert!(!AgentRunner::is_simple_task(&inspection, &[]));
            let result = Box::pin(runner.execute_task(&inspection, &[])).await.unwrap();
            let requests = provider.requests.lock();
            let summaries = requests.iter().filter(|request| is_summary_request(request)).count();
            assert_eq!(summaries, usize::from(protected_large), "capacity={capacity}");
            assert_eq!(requests.len(), 1 + summaries);
            let final_request = requests.last().unwrap();
            assert_eq!(final_request.max_tokens, Some(2_000));
            assert!(validate_history_invariants(&final_request.messages).is_valid());
            assert!(!result.warnings.iter().any(|warning| warning.starts_with("Pre-flight check:")));
            if !protected_large {
                let tool_results: Vec<_> = final_request
                    .messages
                    .iter()
                    .filter(|message| message.role == MessageRole::Tool)
                    .collect();
                assert_eq!(tool_results.len(), 8);
                assert_eq!(
                    tool_results
                        .iter()
                        .filter(|message| message.content.as_text().contains("\"cleared\":\"tool_result\""))
                        .count(),
                    if recent_batches == 0 { 5 } else { 1 }
                );
                assert!(
                    final_request
                        .messages
                        .iter()
                        .any(|message| message.content.as_text().contains("raw_evidence_2_0"))
                );
                assert!(
                    !result
                        .thread_events
                        .iter()
                        .any(|event| matches!(event, ThreadEvent::ThreadCompactBoundary(_)))
                );
            } else {
                let boundary = result
                    .thread_events
                    .iter()
                    .find_map(|event| match event {
                        ThreadEvent::ThreadCompactBoundary(boundary) => Some(boundary),
                        _ => None,
                    })
                    .expect("compaction boundary");
                let artifact = boundary.history_artifact_path.as_ref().expect("full-history artifact");
                let original = fs::read_to_string(workspace_root(&temp).join(artifact)).unwrap();
                assert!(original.contains("raw_evidence_0_0"), "compaction receives full original evidence");
                assert!(!original.contains("\"cleared\":\"tool_result\""));
            }
            eprintln!(
                "clearing fixture: capacity={capacity} protected_large={protected_large} raw_tokens={raw_tokens} projected_tokens={projected_tokens} overhead={overhead} retained_results={} summary_calls={summaries}",
                retained.len()
            );
        }
    }
}

#[tokio::test]
async fn headless_clearing_preserves_opt_out_and_anthropic_local_fallback() {
    for (name, enabled) in [("queued-test-provider", false), ("anthropic", true)] {
        let temp = TempDir::new().unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
        cfg.agent.harness.tool_result_clearing.enabled = enabled;
        cfg.context.max_context_tokens = 32_000;
        cfg.context.preserve_recent_turns = 2;
        cfg.automation.full_auto.max_turns = 1;
        let mut runner = Box::pin(make_runner(&temp, cfg, "clearing-gate")).await;
        let provider = ClearingProvider {
            name,
            capacity: 32_000,
            requests: Arc::new(Mutex::new(Vec::new())),
        };
        runner.provider_client = Box::new(provider.clone());
        runner.bootstrap_messages = evidence_history(32_000, false);
        Box::pin(runner.execute_task(&task("Inspect retained evidence", "clearing-gate"), &[]))
            .await
            .unwrap();
        let requests = provider.requests.lock();
        assert_eq!(requests.iter().filter(|request| is_summary_request(request)).count(), usize::from(!enabled));
        assert!(requests.last().unwrap().context_management.is_none());
        assert!(validate_history_invariants(&requests.last().unwrap().messages).is_valid());
    }
}
