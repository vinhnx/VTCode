#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::*;
use crate::exec::events::ToolOutcome;

/// Last model-facing tool message, parsed from the JSON string the runner
/// pushes for a rejected call (`push_tool_error` with a `String` value).
fn last_tool_message(runtime: &AgentRuntime) -> String {
    let raw = runtime
        .state
        .messages
        .last()
        .map(|message| message.content.as_text().into_owned())
        .expect("a tool message must be recorded");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("structured tool message");
    parsed.as_str().expect("string tool message").to_string()
}

fn tool_invocation_outcome(events: &[ThreadEvent], tool_call_id: &str) -> Option<ToolOutcome> {
    events.iter().find_map(|event| match event {
        ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) => match &item.details {
            ThreadItemDetails::ToolInvocation(details) if details.tool_call_id.as_deref() == Some(tool_call_id) => {
                details.outcome
            }
            _ => None,
        },
        _ => None,
    })
}

/// Register `tool_call_id`'s invocation item the way a streamed turn does, then
/// run admission so the registered rejection branch (which records the outcome)
/// is the one under test rather than the unregistered `tool_rejected` fallback.
async fn admit_one_tool_call(
    runner: &mut AgentRunner,
    runtime: &mut AgentRuntime,
    session_id: &str,
    tool_name: &str,
    args: serde_json::Value,
) -> Vec<ThreadEvent> {
    let provider = QueuedProvider::new(vec![tool_call_response(tool_name, args)]);
    let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);
    let request = LLMRequest {
        model: "gpt-5-codex".to_string(),
        ..Default::default()
    };
    let turn = runtime
        .run_turn_once(&mut provider_box, request, None)
        .await
        .expect("turn should succeed");
    let tool_calls = turn.response.tool_calls.expect("tool call response");

    let mut recorder = ExecEventRecorder::new(session_id, None, None);
    recorder.record_thread_events(runtime.take_emitted_events());
    runner
        .execute_tool_call_batches(tool_calls, runtime, &mut recorder, "[single]", false, false)
        .await
        .expect("tool execution should finish");
    recorder.into_events()
}

#[tokio::test]
async fn planning_denial_reports_policy_denial_not_invalid_arguments() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "thread-planning-denied")).await;
    runner.enable_full_auto(&[tools::EXEC_COMMAND.to_string()]).await;
    // Mirror the audited anomaly: a mutating command denied by the planning gate.
    runner.tool_registry.enable_planning();
    assert!(
        !runner
            .tool_registry
            .is_planning_active_allowed(tools::EXEC_COMMAND, &json!({"cmd": "cargo build"}))
    );

    let mut runtime =
        AgentRuntime::new(AgentSessionState::new("session-planning-denied".to_string(), 16, 4, 128_000), None, None);

    let events = admit_one_tool_call(
        &mut runner,
        &mut runtime,
        "thread-planning-denied",
        tools::EXEC_COMMAND,
        json!({"cmd": "cargo build"}),
    )
    .await;

    let tool_error = last_tool_message(&runtime);
    assert!(tool_error.contains("tool denied by planning workflow"), "{tool_error}");
    // The gate's own remediation guidance must survive to the model.
    assert!(tool_error.contains("blocked during planning"), "{tool_error}");
    assert!(!tool_error.contains("Invalid arguments"), "{tool_error}");

    assert_eq!(
        tool_invocation_outcome(&events, "call-1"),
        Some(ToolOutcome::HookDenied),
        "a policy denial must not be recorded as a malformed-call outcome"
    );

    let denial = events
        .iter()
        .find_map(|event| {
            let ThreadEvent::ItemUpdated(event) = event else {
                return None;
            };
            let ThreadItemDetails::Reasoning(reasoning) = &event.item.details else {
                return None;
            };
            let diagnostic = reasoning.diagnostic.as_deref()?;
            if diagnostic.summary.starts_with("Tool 'exec_command' denied by policy:") {
                assert_eq!(reasoning.stage.as_deref(), Some("diagnosis"));
                Some(diagnostic)
            } else {
                None
            }
        })
        .expect("policy denial must retain a canonical diagnostic summary");
    assert_eq!(denial.code, "runtime.warning");
    assert_eq!(denial.severity, crate::exec::events::DiagnosticSeverity::Warning);
    assert!(!denial.correlation_id.is_empty(), "the incident must have a stable presentation identity");
    assert!(!denial.summary.contains('\n'), "the human summary must remain single-line");
    assert!(!denial.summary.contains("Invalid arguments"));
}

#[tokio::test]
async fn malformed_arguments_still_report_invalid_tool() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "thread-invalid-args")).await;
    runner.enable_full_auto(&[tools::APPLY_PATCH.to_string()]).await;
    // Planning is inactive: this is a genuine argument failure, not a policy stop.
    assert!(!runner.tool_registry.is_planning_active());

    let mut runtime =
        AgentRuntime::new(AgentSessionState::new("session-invalid-args".to_string(), 16, 4, 128_000), None, None);

    let events =
        admit_one_tool_call(&mut runner, &mut runtime, "thread-invalid-args", tools::APPLY_PATCH, json!({})).await;

    let tool_error = last_tool_message(&runtime);
    assert!(tool_error.contains("Invalid arguments for tool 'apply_patch'"), "{tool_error}");
    assert!(!tool_error.contains("denied by planning workflow"), "{tool_error}");

    assert_eq!(tool_invocation_outcome(&events, "call-1"), Some(ToolOutcome::InvalidTool));
}

#[tokio::test]
async fn denied_tool_call_emits_one_failed_output_for_runtime_invocation() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "thread-denied-tool-output")).await;
    runner.enable_full_auto(&[tools::UNIFIED_FILE.to_string()]).await;

    let response = tool_call_response(
        tools::UNIFIED_EXEC,
        json!({
            "action": "run",
            "command": "echo vtcode",
        }),
    );
    let provider = QueuedProvider::new(vec![response]);
    let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);

    let mut runtime =
        AgentRuntime::new(AgentSessionState::new("session-denied".to_string(), 16, 4, 128_000), None, None);
    let request = LLMRequest {
        model: "gpt-5-codex".to_string(),
        ..Default::default()
    };
    let turn = runtime
        .run_turn_once(&mut provider_box, request, None)
        .await
        .expect("turn should succeed");

    let tool_calls = turn.response.tool_calls.expect("tool call response");
    let tool_call_id = tool_calls[0].id.clone();
    let mut recorder = ExecEventRecorder::new("thread-denied-tool-output", None, None);
    recorder.record_thread_events(runtime.take_emitted_events());

    runner
        .execute_tool_call_batches(tool_calls, &mut runtime, &mut recorder, "[single]", false, false)
        .await
        .expect("tool execution should finish");

    let events = recorder.into_events();
    let call_item_id = completed_tool_invocation_item_id(&events, &tool_call_id).expect("completed invocation");
    assert_eq!(completed_tool_output_count(&events, &tool_call_id, ToolCallStatus::Failed, &call_item_id), 1);
}

#[tokio::test]
async fn denied_parallel_tool_halt_returns_promptly() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "thread-denied-parallel")).await;
    runner.enable_full_auto(&[tools::UNIFIED_FILE.to_string()]).await;

    let tool_calls = tool_call_response(
        tools::UNIFIED_EXEC,
        json!({
            "action": "run",
            "command": "echo vtcode",
        }),
    )
    .tool_calls
    .expect("tool call response");

    let mut runtime =
        AgentRuntime::new(AgentSessionState::new("session-denied-parallel".to_string(), 16, 4, 128_000), None, None);
    let mut recorder = ExecEventRecorder::new("thread-denied-parallel", None, None);

    let start = Instant::now();
    runner
        .execute_tool_call_batches(tool_calls, &mut runtime, &mut recorder, "[parallel]", false, false)
        .await
        .expect("tool execution should finish");

    assert!(start.elapsed() < Duration::from_millis(200));
}

#[tokio::test]
async fn duplicate_parallel_tool_names_are_split_into_safe_batches() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "thread-duplicate-parallel")).await;
    runner.enable_full_auto(&[tools::EXEC_COMMAND.to_string()]).await;

    let tool_calls = vec![
        ToolCall::function(
            "call-echo-a".to_string(),
            tools::EXEC_COMMAND.to_string(),
            json!({
                "cmd": "printf hello",
            })
            .to_string(),
        ),
        ToolCall::function(
            "call-echo-b".to_string(),
            tools::EXEC_COMMAND.to_string(),
            json!({
                "cmd": "printf world",
            })
            .to_string(),
        ),
    ];

    let mut runtime =
        AgentRuntime::new(AgentSessionState::new("session-duplicate-parallel".to_string(), 16, 4, 128_000), None, None);
    let mut recorder = ExecEventRecorder::new("thread-duplicate-parallel", None, None);

    runner
        .execute_tool_call_batches(tool_calls, &mut runtime, &mut recorder, "[batch]", false, false)
        .await
        .expect("tool execution should finish");

    let tool_outputs = runtime
        .state
        .messages
        .iter()
        .filter_map(|message| {
            let id = message.tool_call_id.as_ref()?;
            let output = serde_json::from_str::<serde_json::Value>(&message.content.as_text()).ok()?;
            Some((id.as_str(), output))
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_outputs.len(), 2);
    assert!(tool_outputs.iter().any(|(id, output)| {
        *id == "call-echo-a" && output["success"].as_bool() == Some(true) && output["output"].as_str() == Some("hello")
    }));
    assert!(
        tool_outputs.iter().any(|(id, output)| {
            *id == "call-echo-b"
                && output["success"].as_bool() == Some(true)
                && output["output"].as_str() == Some("world")
        }),
        "unexpected tool outputs: {tool_outputs:?}"
    );
    let events = recorder.into_events();
    for (call_id, expected_output) in [("call-echo-a", "hello"), ("call-echo-b", "world")] {
        let call_item_id = completed_tool_invocation_item_id(&events, call_id).expect("completed invocation");
        assert_eq!(completed_tool_output_count(&events, call_id, ToolCallStatus::Completed, &call_item_id), 1);
        let output = events
            .iter()
            .find_map(|event| match event {
                ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) => match &item.details {
                    ThreadItemDetails::ToolOutput(output) if output.tool_call_id.as_deref() == Some(call_id) => {
                        Some(output)
                    }
                    _ => None,
                },
                _ => None,
            })
            .expect("canonical output");
        assert_eq!(output.output.lines().next(), Some(expected_output));
        assert!(output.output.contains("Structured output:"));
        assert_eq!(output.exit_code, Some(0));
    }
}

#[tokio::test]
async fn removed_public_tool_names_are_rejected() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "thread-removed-public-tools")).await;
    assert!(!runner.is_valid_tool(tools::LIST_FILES).await);
    assert!(!runner.is_valid_tool(tools::UNIFIED_SEARCH).await);

    let tool_calls = vec![
        ToolCall::function(
            "call-list".to_string(),
            tools::LIST_FILES.to_string(),
            json!({
                "path": "notes",
            })
            .to_string(),
        ),
        ToolCall::function(
            "call-search".to_string(),
            tools::UNIFIED_SEARCH.to_string(),
            json!({
                "action": "grep",
                "path": "notes",
                "pattern": "hello",
            })
            .to_string(),
        ),
    ];

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-removed-public-tools".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let mut recorder = ExecEventRecorder::new("thread-removed-public-tools", None, None);

    runner
        .execute_tool_call_batches(tool_calls, &mut runtime, &mut recorder, "[batch]", false, false)
        .await
        .expect("tool execution should finish");

    let tool_outputs = runtime
        .state
        .messages
        .iter()
        .filter_map(|message| {
            let id = message.tool_call_id.as_ref()?;
            let output = serde_json::from_str::<serde_json::Value>(&message.content.as_text()).ok()?;
            Some((id.as_str(), output))
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_outputs.len(), 2);

    let list_output = tool_outputs
        .iter()
        .find_map(|(id, output)| (*id == "call-list").then_some(output))
        .expect("list_files rejection output");
    assert_eq!(list_output.as_str(), Some("Tool execution denied: list_files"));

    let search_output = tool_outputs
        .iter()
        .find_map(|(id, output)| (*id == "call-search").then_some(output))
        .expect("search_dispatch rejection output");
    let expected_search_denial = format!("Tool execution denied: {}", tools::UNIFIED_SEARCH);
    assert_eq!(search_output.as_str(), Some(expected_search_denial.as_str()));

    let events = recorder.into_events();
    let list_item_id = completed_tool_invocation_item_id(&events, "call-list").expect("list invocation");
    let search_item_id = completed_tool_invocation_item_id(&events, "call-search").expect("search invocation");
    assert_eq!(completed_tool_output_count(&events, "call-list", ToolCallStatus::Failed, &list_item_id), 1);
    assert_eq!(completed_tool_output_count(&events, "call-search", ToolCallStatus::Failed, &search_item_id), 1);
}

#[tokio::test]
async fn denied_sequential_tool_halt_returns_promptly() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "thread-denied-sequential")).await;
    runner.enable_full_auto(&[tools::UNIFIED_FILE.to_string()]).await;

    let tool_calls = tool_call_response(
        tools::UNIFIED_EXEC,
        json!({
            "action": "run",
            "command": "echo vtcode",
        }),
    )
    .tool_calls
    .expect("tool call response");

    let mut runtime =
        AgentRuntime::new(AgentSessionState::new("session-denied-sequential".to_string(), 16, 4, 128_000), None, None);
    let mut recorder = ExecEventRecorder::new("thread-denied-sequential", None, None);

    let start = Instant::now();
    runner
        .execute_tool_call_batches(tool_calls, &mut runtime, &mut recorder, "[sequential]", false, false)
        .await
        .expect("tool execution should finish");

    assert!(start.elapsed() < Duration::from_millis(200));
}

#[tokio::test]
async fn execute_tool_internal_retries_open_circuit_breaker() {
    let temp = TempDir::new().expect("tempdir");
    let runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "thread-open-circuit")).await;
    let breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 1,
        min_backoff: Duration::from_millis(10),
        max_backoff: Duration::from_millis(10),
        reset_timeout: Duration::from_millis(10),
        ..CircuitBreakerConfig::default()
    }));
    runner.tool_registry.set_shared_circuit_breaker(breaker.clone());
    breaker.record_failure_category_for_tool(tools::EXEC_COMMAND, vtcode_commons::ErrorCategory::ExecutionError);

    let start = Instant::now();
    let result = runner
        .execute_tool_internal(tools::EXEC_COMMAND, &json!({"cmd": "printf hello"}))
        .await
        .expect("circuit-open retry should recover");

    assert!(start.elapsed() >= Duration::from_millis(10));
    assert_eq!(result.get("output").and_then(|value| value.as_str()), Some("hello"));
}

#[tokio::test]
async fn sequential_policy_failure_halts_following_tool_calls() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(temp.path().join("note.txt"), "hello\n").expect("workspace file");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.commands.deny_regex = vec!["^blocked-cmd$".to_string()];
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-policy-halt")).await;
    runner
        .enable_full_auto(&[tools::EXEC_COMMAND.to_string(), tools::APPLY_PATCH.to_string()])
        .await;
    assert!(runner.is_valid_tool(tools::EXEC_COMMAND).await);
    assert!(runner.is_valid_tool(tools::APPLY_PATCH).await);
    assert!(!runner.is_valid_tool(tools::UNIFIED_EXEC).await);
    assert!(!runner.is_valid_tool(tools::READ_FILE).await);

    let tool_calls = vec![
        ToolCall::function(
            "call-blocked".to_string(),
            tools::EXEC_COMMAND.to_string(),
            json!({
                "cmd": "blocked-cmd",
            })
            .to_string(),
        ),
        ToolCall::function(
            "call-patch".to_string(),
            tools::APPLY_PATCH.to_string(),
            json!({
                "patch": "*** Begin Patch\n*** Update File: note.txt\n@@\n-hello\n+changed\n*** End Patch\n",
            })
            .to_string(),
        ),
    ];

    let mut runtime =
        AgentRuntime::new(AgentSessionState::new("session-policy-halt".to_string(), 16, 4, 128_000), None, None);
    let mut recorder = ExecEventRecorder::new("thread-policy-halt", None, None);

    runner
        .execute_tool_call_batches(tool_calls, &mut runtime, &mut recorder, "[single]", false, false)
        .await
        .expect("tool execution should finish");

    assert!(
        runtime
            .state
            .warnings
            .iter()
            .any(|warning| warning == "Tool denied by policy; halting further tool calls this turn."),
        "warnings: {:?}",
        runtime.state.warnings
    );
    assert!(!runtime.state.executed_commands.iter().any(|tool| tool == tools::APPLY_PATCH));

    let events = recorder.into_events();
    assert!(completed_tool_invocation_item_id(&events, "call-blocked").is_some());
    assert!(completed_tool_invocation_item_id(&events, "call-patch").is_none());
}

#[tokio::test]
async fn sequential_tool_failures_record_categorized_user_message() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.commands.deny_regex = vec!["^blocked-cmd$".to_string()];
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-policy-message")).await;
    runner.enable_full_auto(&[tools::EXEC_COMMAND.to_string()]).await;
    assert!(runner.is_valid_tool(tools::EXEC_COMMAND).await);
    assert!(!runner.is_valid_tool(tools::UNIFIED_EXEC).await);

    let tool_calls = vec![ToolCall::function(
        "call-blocked".to_string(),
        tools::EXEC_COMMAND.to_string(),
        json!({
            "cmd": "blocked-cmd",
        })
        .to_string(),
    )];

    let mut runtime =
        AgentRuntime::new(AgentSessionState::new("session-policy-message".to_string(), 16, 4, 128_000), None, None);
    let mut recorder = ExecEventRecorder::new("thread-policy-message", None, None);

    runner
        .execute_tool_call_batches(tool_calls, &mut runtime, &mut recorder, "[single]", false, false)
        .await
        .expect("tool execution should finish");

    let tool_error = runtime
        .state
        .messages
        .last()
        .map(|message| message.content.as_text().into_owned())
        .expect("tool error recorded");
    let tool_error: serde_json::Value = serde_json::from_str(&tool_error).expect("structured tool error");
    assert_eq!(tool_error["error"]["category"].as_str(), Some("PolicyViolation"), "{tool_error}");
    assert!(
        tool_error["error"]["recovery_suggestions"]
            .as_array()
            .is_some_and(|suggestions| suggestions
                .iter()
                .any(|value| { value.as_str() == Some("Review workspace policies and restrictions") })),
        "{tool_error}"
    );
    assert_eq!(tool_error["error"]["partial_state_possible"].as_bool(), Some(false), "{tool_error}");
}

#[tokio::test]
async fn sequential_tool_failures_do_not_record_interruption_guards() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.commands.deny_regex = vec!["^blocked-cmd$".to_string()];
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-policy-guard")).await;
    runner.enable_full_auto(&[tools::EXEC_COMMAND.to_string()]).await;
    assert!(!runner.is_valid_tool(tools::UNIFIED_EXEC).await);

    let tool_calls = vec![ToolCall::function(
        "call-blocked".to_string(),
        tools::EXEC_COMMAND.to_string(),
        json!({
            "cmd": "blocked-cmd",
        })
        .to_string(),
    )];

    let mut runtime =
        AgentRuntime::new(AgentSessionState::new("session-policy-guard".to_string(), 16, 4, 128_000), None, None);
    let mut recorder = ExecEventRecorder::new("thread-policy-guard", None, None);

    runner
        .execute_tool_call_batches(tool_calls, &mut runtime, &mut recorder, "[single]", false, false)
        .await
        .expect("tool execution should finish");

    assert!(
        runtime.state.error_recovery.lock().recent_errors.is_empty(),
        "handled tool failures should not be recorded as interrupted executions"
    );
}

#[tokio::test]
async fn steer_stop_closes_open_tool_calls_with_failed_output_items() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "thread-stop-tool-output")).await;

    let response = tool_call_response(
        tools::EXEC_COMMAND,
        json!({
            "cmd": "printf hello",
        }),
    );
    let provider = QueuedProvider::new(vec![response]);
    let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);

    let (steering_tx, steering_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime =
        AgentRuntime::new(AgentSessionState::new("session-stop".to_string(), 16, 4, 128_000), None, Some(steering_rx));
    let request = LLMRequest {
        model: "gpt-5-codex".to_string(),
        ..Default::default()
    };
    let turn = runtime
        .run_turn_once(&mut provider_box, request, None)
        .await
        .expect("turn should succeed");

    let tool_calls = turn.response.tool_calls.expect("tool call response");
    let tool_call_id = tool_calls[0].id.clone();
    let mut recorder = ExecEventRecorder::new("thread-stop-tool-output", None, None);
    recorder.record_thread_events(runtime.take_emitted_events());
    steering_tx.send(SteeringMessage::SteerStop).expect("steer stop should queue");

    runner
        .execute_tool_call_batches(tool_calls, &mut runtime, &mut recorder, "[single]", false, false)
        .await
        .expect("tool execution should finish");

    let events = recorder.into_events();
    let call_item_id = completed_tool_invocation_item_id(&events, &tool_call_id).expect("completed invocation");
    assert_eq!(completed_tool_output_count(&events, &tool_call_id, ToolCallStatus::Failed, &call_item_id), 1);
}

#[tokio::test]
async fn headless_fetch_retries_use_the_remaining_tool_deadline() {
    use crate::tools::registry::{ToolExecutionError, ToolRegistration};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TransientFetch(Arc<AtomicUsize>);
    #[async_trait]
    impl Tool for TransientFetch {
        async fn execute(&self, args: serde_json::Value) -> anyhow::Result<serde_json::Value> {
            let budget = crate::tools::web_fetch::request_budget::RequestBudget::current();
            let _transport = match &budget {
                Some(budget) => Some(budget.lock_transport().await?),
                None => None,
            };
            let url = url::Url::parse(args["url"].as_str().unwrap())?;
            if let Some(budget) = &budget
                && let Err(error) = budget.admit(&url)
            {
                return Ok(error.to_json_value());
            }
            let call = self.0.fetch_add(1, Ordering::SeqCst) + 1;
            if call == 1 {
                let output = json!({"error":"HTTP request failed with status: 429", "http_status":429,
                    "retry_after_ms":1100, "next_action":"Disclose unavailable external evidence."});
                Ok(budget.as_ref().map_or_else(
                    || output.clone(),
                    |budget| {
                        budget
                            .observe(&url, ToolExecutionError::from_tool_output(&output).unwrap())
                            .to_json_value()
                    },
                ))
            } else {
                Ok(json!({"preview":"retrieved", "call":call}))
            }
        }
        fn name(&self) -> &str {
            tools::WEB_FETCH
        }
        fn description(&self) -> &str {
            "Controlled transient fetch without network access"
        }
    }

    for wall_clock_secs in [1, 3, 0, u64::MAX] {
        let temp = TempDir::new().unwrap();
        let mut config = VTCodeConfig::default();
        config.agent.harness.max_tool_wall_clock_secs = wall_clock_secs;
        config.agent.harness.max_tool_retries = 1;
        let runner = make_runner(&temp, config, "headless-fetch-deadline").await;
        let calls = Arc::new(AtomicUsize::new(0));
        runner
            .tool_registry
            .register_tool(ToolRegistration::from_tool_instance(
                tools::WEB_FETCH,
                CapabilityLevel::CodeSearch,
                TransientFetch(calls.clone()),
            ))
            .await
            .unwrap();
        runner.tool_registry.allow_all_tools().await.unwrap();
        let result = runner
            .execute_tool_internal(tools::WEB_FETCH, &json!({"url":"https://example.com/page"}))
            .await;
        if wall_clock_secs == u64::MAX {
            let error = result.expect_err("an unrepresentable budget must fail before dispatch, without panicking");
            assert_eq!(error.error_type, crate::tools::registry::ToolErrorType::InvalidParameters);
            assert!(error.message.contains("deadline range"));
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        } else if wall_clock_secs == 1 {
            let error = result.expect_err("the HTTP limitation must survive the short deadline");
            assert_eq!(error.category, vtcode_commons::ErrorCategory::RateLimit);
            assert_eq!(error.retry_after_ms, Some(1100));
            assert_eq!(error.to_json_value()["error"]["http_status"], 429);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        } else {
            assert_eq!(result.unwrap()["preview"], "retrieved");
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        }
    }
}
