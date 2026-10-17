use super::*;

#[tokio::test]
async fn exhausted_previews_pending_verification_done_claim_is_blocked() {
    #[derive(Clone)]
    struct DoneProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for DoneProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            Ok(uni::LLMResponse {
                content: Some("Done — the requested change is complete.".to_string()),
                model: request.model,
                tool_calls: None,
                usage: None,
                finish_reason: uni::FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
                compaction: None,
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".to_string()]
        }

        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    let requests = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(8).await;
    let harness_path = backing.enable_harness_emitter();
    backing.set_provider(Box::new(DoneProvider { requests: requests.clone() }));

    // This is the bodyless registry response that crossed the upstream
    // aggregate budget. Keep it in history and seed the state through the
    // same marker observer used by provider-history insertion.
    let marker = json!({
        "tool": tool_names::READ_FILE,
        "preview_budget_exhausted": true,
        "total_output_bytes": 131_072,
        "exit_code": 0
    })
    .to_string();
    let mut history = vec![
        uni::Message::user("apply the change and report when Done".to_string()),
        uni::Message::tool_response("registry-read-0".to_string(), marker.clone()),
    ];
    {
        let context = backing.turn_loop_context();
        let budget = vtcode_config::constants::output_limits::TURN_PREVIEW_BUDGET_BYTES;
        assert!(
            context
                .harness_state
                .observe_upstream_preview_budget_exhaustion("registry-read-0", &marker, budget)
        );
        context.session_stats.set_verification_snapshot((true, 0));
    }

    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("pending verification must block repeated Done claims");

    assert!(matches!(
        outcome.result,
        TurnLoopResult::Blocked {
            reason: Some(ref reason)
        } if reason == PENDING_VERIFICATION_BLOCK_REASON
    ));
    assert_eq!(requests.load(Ordering::SeqCst), 6, "six bounded auto-recovery texts are the terminal cap");
    assert!(outcome.turn_diagnostics.model_visible_tool_preview_budget_exhausted);
    assert!(outcome.turn_diagnostics.suppressed_tool_previews >= 1);
    assert_blocked_response_surfaces(&mut backing, &history, &harness_path, PENDING_VERIFICATION_RESPONSE_MARKER);
}

#[tokio::test]
async fn legacy_preview_marker_keeps_planning_tools_available_for_approval() {
    #[derive(Debug, Clone, Copy)]
    struct RequestObservation {
        has_tools: bool,
        tool_choice_none: bool,
    }

    #[derive(Clone)]
    struct PreviewRecoveryPlanProvider {
        requests: Arc<AtomicUsize>,
        observations: Arc<Mutex<Vec<RequestObservation>>>,
    }

    const PLAN: &str = r#"# Preview recovery plan

## Summary
Synthesize the approval-ready plan from evidence gathered before preview exhaustion.

## Implementation Steps
1. Keep the preview guard bounded -> files: [src/agent/runloop/unified/turn/guards.rs] -> verify: [cargo nextest run -p vtcode]
2. Preserve recovery guidance -> files: [src/agent/runloop/unified/turn/context/response_handling.rs] -> verify: [cargo check --locked]
3. Confirm the tool-free handoff -> files: [src/agent/runloop/unified/turn/turn_loop/tests.rs] -> verify: [rg -n preview_exhaustion src/agent/runloop/unified/turn]

## Test Cases and Validation
1. Run cargo nextest run -p vtcode.

## Assumptions and Defaults
1. Keep strict validation and bounded recovery.
"#;

    #[async_trait::async_trait]
    impl uni::LLMProvider for PreviewRecoveryPlanProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let request_number = self.requests.fetch_add(1, Ordering::SeqCst);
            self.observations
                .lock()
                .expect("request observations lock")
                .push(RequestObservation {
                    has_tools: request.tools.as_ref().is_some_and(|tools| !tools.is_empty()),
                    tool_choice_none: matches!(request.tool_choice, Some(uni::ToolChoice::None)),
                });

            let (content, tool_calls) = if request_number == 0 {
                (
                    None,
                    Some(vec![uni::ToolCall::function(
                        "preview-blind-read".to_string(),
                        tool_names::EXEC_COMMAND.to_string(),
                        json!({"cmd": "printf 'fresh evidence\\n'"}).to_string(),
                    )]),
                )
            } else {
                (Some(format!("<proposed_plan>\n{PLAN}\n</proposed_plan>")), None)
            };
            Ok(uni::LLMResponse {
                content,
                model: request.model,
                tool_calls,
                usage: None,
                finish_reason: uni::FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
                compaction: None,
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".to_string()]
        }

        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    let requests = Arc::new(AtomicUsize::new(0));
    let observations = Arc::new(Mutex::new(Vec::new()));
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    backing
        .add_tool_definition(uni::ToolDefinition::function(
            tool_names::EXEC_COMMAND.to_string(),
            "Run a workspace command".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "cmd": {"type": "string"}
                },
                "required": ["cmd"],
                "additionalProperties": false
            }),
        ))
        .await;
    backing.set_provider(Box::new(PreviewRecoveryPlanProvider {
        requests: requests.clone(),
        observations: observations.clone(),
    }));

    // A legacy marker may survive replay, but must not remove tools from
    // either request or prevent an ordinary plan from reaching approval.
    let prior_marker = json!({
        "tool": tool_names::EXEC_COMMAND,
        "preview_budget_exhausted": true,
        "total_output_bytes": 131_072,
        "exit_code": 0
    })
    .to_string();
    {
        let context = backing.turn_loop_context();
        let budget = vtcode_config::constants::output_limits::TURN_PREVIEW_BUDGET_BYTES_PLANNING;
        assert!(context.harness_state.observe_upstream_preview_budget_exhaustion(
            "prior-preview",
            &prior_marker,
            budget,
        ));
    }
    let mut history = vec![
        uni::Message::user("make a plan from the repository evidence and wait for approval".to_string()),
        uni::Message::tool_response("prior-preview".to_string(), prior_marker),
    ];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("legacy diagnostics should not prevent plan approval");

    assert!(
        matches!(outcome.result, TurnLoopResult::Completed { plan_approved_execution_pending: true }),
        "ordinary planning should reach the approval handoff, got {:?}",
        outcome.result
    );
    assert_eq!(requests.load(Ordering::SeqCst), 2, "exhaustion must not cause another inspection or recovery loop");
    let observations = observations.lock().expect("request observations lock");
    assert_eq!(observations.len(), 2);
    assert!(observations[0].has_tools, "the initial planning request must retain the read tool");
    assert!(!observations[0].tool_choice_none, "the initial request must not be forced tool-free");
    assert!(observations[1].has_tools, "legacy exhaustion must not remove tools: {observations:?}");
    assert!(
        !observations[1].tool_choice_none,
        "legacy exhaustion must not force tool-free synthesis: {observations:?}"
    );
    assert!(outcome.turn_diagnostics.model_visible_tool_preview_budget_exhausted);
    assert!(outcome.turn_diagnostics.suppressed_tool_previews >= 1);
    assert!(!history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().contains("preview budget exhausted")
    }));
    let plans_dir = backing.workspace_path().join(".vtcode").join("plans");
    let persisted_plan = fs::read_dir(&plans_dir)
        .expect("the recovered plan should create the plans directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| {
            path.extension().is_some_and(|extension| extension == "md")
                && !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(".tasks.md"))
        })
        .map(|path| fs::read_to_string(path).expect("read the recovered plan"))
        .expect("the recovered plan should be persisted before approval");
    assert!(persisted_plan.contains("Synthesize the approval-ready plan"));
    assert!(persisted_plan.contains("cargo nextest run -p vtcode"));
}

#[tokio::test]
async fn exhausted_previews_pending_verification_completes_after_standalone_verifier() {
    #[derive(Clone)]
    struct VerifierThenDoneProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for VerifierThenDoneProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let request_number = self.requests.fetch_add(1, Ordering::SeqCst);
            if request_number == 0 {
                return Ok(uni::LLMResponse {
                    content: None,
                    model: request.model,
                    tool_calls: Some(vec![uni::ToolCall::function(
                        "standalone-verifier".to_string(),
                        tool_names::EXEC_COMMAND.to_string(),
                        json!({"cmd": "cargo check --quiet"}).to_string(),
                    )]),
                    usage: None,
                    finish_reason: uni::FinishReason::Stop,
                    reasoning: None,
                    reasoning_details: None,
                    organization_id: None,
                    request_id: None,
                    tool_references: Vec::new(),
                    compaction: None,
                });
            }

            Ok(uni::LLMResponse {
                content: Some("Done — the requested change is complete.".to_string()),
                model: request.model,
                tool_calls: None,
                usage: None,
                finish_reason: uni::FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
                compaction: None,
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".to_string()]
        }

        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    let requests = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(8).await;
    let harness_path = backing.enable_harness_emitter();
    let workspace = backing.workspace_path().to_path_buf();
    fs::create_dir_all(workspace.join("src")).expect("create verifier fixture source directory");
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"preview-budget-verifier\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("write verifier fixture manifest");
    fs::write(workspace.join("src/lib.rs"), "pub fn verified() -> bool { true }\n")
        .expect("write verifier fixture source");
    backing.set_provider(Box::new(VerifierThenDoneProvider { requests: requests.clone() }));

    let marker = json!({
        "tool": tool_names::READ_FILE,
        "preview_budget_exhausted": true,
        "total_output_bytes": 131_072,
        "exit_code": 0
    })
    .to_string();
    let mut history = vec![
        uni::Message::user("apply the change and report when Done".to_string()),
        uni::Message::tool_response("registry-read-0".to_string(), marker.clone()),
    ];
    {
        let context = backing.turn_loop_context();
        let budget = vtcode_config::constants::output_limits::TURN_PREVIEW_BUDGET_BYTES;
        assert!(
            context
                .harness_state
                .observe_upstream_preview_budget_exhaustion("registry-read-0", &marker, budget)
        );
        context.session_stats.set_verification_snapshot((true, 0));
    }

    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("successful standalone verifier should permit the final Done claim");

    assert!(matches!(outcome.result, TurnLoopResult::Completed { plan_approved_execution_pending: false }));
    assert_eq!(
        requests.load(Ordering::SeqCst),
        2,
        "the verifier and final Done response should be the only requests"
    );
    assert!(outcome.turn_diagnostics.model_visible_tool_preview_budget_exhausted);
    assert!(
        history.iter().any(|message| {
            message.role == uni::MessageRole::Tool
                && serde_json::from_str::<serde_json::Value>(&message.content.as_text())
                    .ok()
                    .and_then(|value| value.get("exit_code").and_then(serde_json::Value::as_i64))
                    == Some(0)
        }),
        "standalone verifier exit status must remain visible after preview exhaustion: {history:?}"
    );
    let harness = fs::read_to_string(harness_path).expect("read successful verifier harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("successful verifier harness output should use the versioned event contract")
                .into_event()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ThreadEvent::TurnCompleted(_)))
            .count(),
        1,
        "successful standalone verifier must publish turn.completed: {harness}"
    );
    assert!(
        !events.iter().any(|event| matches!(event, ThreadEvent::TurnFailed(_))),
        "successful standalone verifier must not publish turn.failed: {harness}"
    );
    assert!(final_answer_text(&history).contains("Done"));
}
