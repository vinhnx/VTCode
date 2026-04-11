use super::*;

#[tokio::test]
async fn denied_tool_call_emits_one_failed_output_for_runtime_invocation() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = make_runner(&temp, VTCodeConfig::default(), "thread-denied-tool-output").await;
    runner
        .enable_full_auto(&[tools::UNIFIED_FILE.to_string()])
        .await;

    let response = tool_call_response(
        tools::UNIFIED_EXEC,
        json!({
            "action": "run",
            "command": "echo vtcode",
        }),
    );
    let provider = QueuedProvider::new(vec![response]);
    let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-denied".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let request = LLMRequest {
        model: "gpt-5.3-codex".to_string(),
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
        .execute_sequential_tool_calls(tool_calls, &mut runtime, &mut recorder, "[single]", false)
        .await
        .expect("tool execution should finish");

    let events = recorder.into_events();
    let call_item_id =
        completed_tool_invocation_item_id(&events, &tool_call_id).expect("completed invocation");
    assert_eq!(
        completed_tool_output_count(
            &events,
            &tool_call_id,
            ToolCallStatus::Failed,
            &call_item_id
        ),
        1
    );
}

#[tokio::test]
async fn denied_parallel_tool_halt_returns_promptly() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = make_runner(&temp, VTCodeConfig::default(), "thread-denied-parallel").await;
    runner
        .enable_full_auto(&[tools::UNIFIED_FILE.to_string()])
        .await;

    let tool_calls = tool_call_response(
        tools::UNIFIED_EXEC,
        json!({
            "action": "run",
            "command": "echo vtcode",
        }),
    )
    .tool_calls
    .expect("tool call response");

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-denied-parallel".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let mut recorder = ExecEventRecorder::new("thread-denied-parallel", None, None);

    let start = Instant::now();
    runner
        .execute_parallel_tool_calls(tool_calls, &mut runtime, &mut recorder, "[parallel]", false)
        .await
        .expect("tool execution should finish");

    assert!(start.elapsed() < Duration::from_millis(200));
}

#[tokio::test]
async fn duplicate_parallel_tool_names_are_split_into_safe_batches() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(temp.path().join("note-a.txt"), "hello\n").expect("workspace file");
    fs::write(temp.path().join("note-b.txt"), "world\n").expect("workspace file");

    let mut runner = make_runner(&temp, VTCodeConfig::default(), "thread-duplicate-parallel").await;
    runner
        .enable_full_auto(&[tools::READ_FILE.to_string()])
        .await;

    let tool_calls = vec![
        ToolCall::function(
            "call-read-a".to_string(),
            tools::READ_FILE.to_string(),
            json!({
                "path": "note-a.txt",
            })
            .to_string(),
        ),
        ToolCall::function(
            "call-read-b".to_string(),
            tools::READ_FILE.to_string(),
            json!({
                "path": "note-b.txt",
            })
            .to_string(),
        ),
    ];

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-duplicate-parallel".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let mut recorder = ExecEventRecorder::new("thread-duplicate-parallel", None, None);

    runner
        .execute_tool_call_batches(
            tool_calls,
            &mut runtime,
            &mut recorder,
            "[batch]",
            false,
            false,
        )
        .await
        .expect("tool execution should finish");

    let tool_outputs = runtime
        .state
        .messages
        .iter()
        .filter_map(|message| {
            let id = message.tool_call_id.as_ref()?;
            let output =
                serde_json::from_str::<serde_json::Value>(&message.content.as_text()).ok()?;
            Some((id.as_str(), output))
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_outputs.len(), 2);
    assert!(tool_outputs.iter().any(|(id, output)| {
        *id == "call-read-a"
            && output["success"].as_bool() == Some(true)
            && output["path"].as_str() == Some("note-a.txt")
    }));
    assert!(tool_outputs.iter().any(|(id, output)| {
        *id == "call-read-b"
            && output["success"].as_bool() == Some(true)
            && output["path"].as_str() == Some("note-b.txt")
    }));
}

#[tokio::test]
async fn list_files_and_unified_search_parallel_batch_avoids_reentrancy() {
    let temp = TempDir::new().expect("tempdir");
    fs::create_dir_all(temp.path().join("notes")).expect("notes directory");
    fs::write(temp.path().join("notes/a.txt"), "hello from vt code\n").expect("workspace file");
    fs::write(temp.path().join("notes/b.txt"), "another line\n").expect("workspace file");

    let mut runner = make_runner(
        &temp,
        VTCodeConfig::default(),
        "thread-list-search-parallel",
    )
    .await;
    runner
        .enable_full_auto(&[
            tools::LIST_FILES.to_string(),
            tools::UNIFIED_SEARCH.to_string(),
        ])
        .await;

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
        AgentSessionState::new("session-list-search-parallel".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let mut recorder = ExecEventRecorder::new("thread-list-search-parallel", None, None);

    runner
        .execute_tool_call_batches(
            tool_calls,
            &mut runtime,
            &mut recorder,
            "[batch]",
            false,
            false,
        )
        .await
        .expect("tool execution should finish");

    let tool_outputs = runtime
        .state
        .messages
        .iter()
        .filter_map(|message| {
            let id = message.tool_call_id.as_ref()?;
            let output =
                serde_json::from_str::<serde_json::Value>(&message.content.as_text()).ok()?;
            Some((id.as_str(), output))
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_outputs.len(), 2);

    let list_output = tool_outputs
        .iter()
        .find_map(|(id, output)| (*id == "call-list").then_some(output))
        .expect("list_files output");
    assert_ne!(
        list_output
            .pointer("/error/error_type")
            .and_then(serde_json::Value::as_str),
        Some("PolicyViolation")
    );
    assert_eq!(
        list_output
            .get("reentrant_call_blocked")
            .and_then(serde_json::Value::as_bool),
        None
    );
    assert!(
        list_output["items"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );

    let search_output = tool_outputs
        .iter()
        .find_map(|(id, output)| (*id == "call-search").then_some(output))
        .expect("unified_search output");
    assert_ne!(
        search_output
            .pointer("/error/error_type")
            .and_then(serde_json::Value::as_str),
        Some("PolicyViolation")
    );
    assert_eq!(
        search_output
            .get("reentrant_call_blocked")
            .and_then(serde_json::Value::as_bool),
        None
    );
    assert!(
        search_output["matches"]
            .as_array()
            .is_some_and(|matches| !matches.is_empty())
    );
}

#[tokio::test]
async fn denied_sequential_tool_halt_returns_promptly() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = make_runner(&temp, VTCodeConfig::default(), "thread-denied-sequential").await;
    runner
        .enable_full_auto(&[tools::UNIFIED_FILE.to_string()])
        .await;

    let tool_calls = tool_call_response(
        tools::UNIFIED_EXEC,
        json!({
            "action": "run",
            "command": "echo vtcode",
        }),
    )
    .tool_calls
    .expect("tool call response");

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-denied-sequential".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let mut recorder = ExecEventRecorder::new("thread-denied-sequential", None, None);

    let start = Instant::now();
    runner
        .execute_sequential_tool_calls(
            tool_calls,
            &mut runtime,
            &mut recorder,
            "[sequential]",
            false,
        )
        .await
        .expect("tool execution should finish");

    assert!(start.elapsed() < Duration::from_millis(200));
}

#[tokio::test]
async fn execute_tool_internal_retries_open_circuit_breaker() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(temp.path().join("note.txt"), "hello\n").expect("workspace file");
    let runner = make_runner(&temp, VTCodeConfig::default(), "thread-open-circuit").await;
    let breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 1,
        min_backoff: Duration::from_millis(10),
        max_backoff: Duration::from_millis(10),
        reset_timeout: Duration::from_millis(10),
        ..CircuitBreakerConfig::default()
    }));
    runner
        .tool_registry
        .set_shared_circuit_breaker(breaker.clone());
    breaker.record_failure_category_for_tool(
        tools::READ_FILE,
        vtcode_commons::ErrorCategory::ExecutionError,
    );

    let start = Instant::now();
    let result = runner
        .execute_tool_internal(tools::READ_FILE, &json!({"path": "note.txt"}))
        .await
        .expect("circuit-open retry should recover");

    assert!(start.elapsed() >= Duration::from_millis(10));
    assert_eq!(
        result.get("content").and_then(|value| value.as_str()),
        Some("hello")
    );
}

#[tokio::test]
async fn sequential_policy_failure_halts_following_tool_calls() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(temp.path().join("note.txt"), "hello\n").expect("workspace file");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.commands.deny_regex = vec!["^blocked-cmd$".to_string()];
    let mut runner = make_runner(&temp, vt_cfg, "thread-policy-halt").await;
    runner
        .enable_full_auto(&[
            tools::UNIFIED_EXEC.to_string(),
            tools::READ_FILE.to_string(),
        ])
        .await;
    assert!(runner.is_valid_tool(tools::UNIFIED_EXEC).await);
    assert!(runner.is_valid_tool(tools::READ_FILE).await);

    let tool_calls = vec![
        ToolCall::function(
            "call-blocked".to_string(),
            tools::UNIFIED_EXEC.to_string(),
            json!({
                "action": "run",
                "command": "blocked-cmd",
            })
            .to_string(),
        ),
        ToolCall::function(
            "call-read".to_string(),
            tools::READ_FILE.to_string(),
            json!({
                "path": "note.txt",
            })
            .to_string(),
        ),
    ];

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-policy-halt".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let mut recorder = ExecEventRecorder::new("thread-policy-halt", None, None);

    runner
        .execute_sequential_tool_calls(tool_calls, &mut runtime, &mut recorder, "[single]", false)
        .await
        .expect("tool execution should finish");

    assert!(
        runtime.state.warnings.iter().any(
            |warning| warning == "Tool denied by policy; halting further tool calls this turn."
        ),
        "warnings: {:?}",
        runtime.state.warnings
    );
    assert!(
        !runtime
            .state
            .executed_commands
            .iter()
            .any(|tool| tool == tools::READ_FILE)
    );

    let events = recorder.into_events();
    assert!(completed_tool_invocation_item_id(&events, "call-blocked").is_some());
    assert!(completed_tool_invocation_item_id(&events, "call-read").is_none());
}

#[tokio::test]
async fn sequential_tool_failures_record_categorized_user_message() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.commands.deny_regex = vec!["^blocked-cmd$".to_string()];
    let mut runner = make_runner(&temp, vt_cfg, "thread-policy-message").await;
    runner
        .enable_full_auto(&[tools::UNIFIED_EXEC.to_string()])
        .await;
    assert!(runner.is_valid_tool(tools::UNIFIED_EXEC).await);

    let tool_calls = vec![ToolCall::function(
        "call-blocked".to_string(),
        tools::UNIFIED_EXEC.to_string(),
        json!({
            "action": "run",
            "command": "blocked-cmd",
        })
        .to_string(),
    )];

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-policy-message".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let mut recorder = ExecEventRecorder::new("thread-policy-message", None, None);

    runner
        .execute_sequential_tool_calls(tool_calls, &mut runtime, &mut recorder, "[single]", false)
        .await
        .expect("tool execution should finish");

    let tool_error = runtime
        .state
        .messages
        .last()
        .map(|message| message.content.as_text().into_owned())
        .expect("tool error recorded");
    let tool_error: serde_json::Value =
        serde_json::from_str(&tool_error).expect("structured tool error");
    assert_eq!(
        tool_error["error"]["category"].as_str(),
        Some("PolicyViolation"),
        "{tool_error}"
    );
    assert!(
        tool_error["error"]["recovery_suggestions"]
            .as_array()
            .is_some_and(|suggestions| suggestions.iter().any(|value| {
                value.as_str() == Some("Review workspace policies and restrictions")
            })),
        "{tool_error}"
    );
    assert_eq!(
        tool_error["error"]["partial_state_possible"].as_bool(),
        Some(false),
        "{tool_error}"
    );
}

#[tokio::test]
async fn sequential_tool_failures_do_not_record_interruption_guards() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.commands.deny_regex = vec!["^blocked-cmd$".to_string()];
    let mut runner = make_runner(&temp, vt_cfg, "thread-policy-guard").await;
    runner
        .enable_full_auto(&[tools::UNIFIED_EXEC.to_string()])
        .await;

    let tool_calls = vec![ToolCall::function(
        "call-blocked".to_string(),
        tools::UNIFIED_EXEC.to_string(),
        json!({
            "action": "run",
            "command": "blocked-cmd",
        })
        .to_string(),
    )];

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-policy-guard".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let mut recorder = ExecEventRecorder::new("thread-policy-guard", None, None);

    runner
        .execute_sequential_tool_calls(tool_calls, &mut runtime, &mut recorder, "[single]", false)
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
    let mut runner = make_runner(&temp, VTCodeConfig::default(), "thread-stop-tool-output").await;

    fs::write(temp.path().join("note.txt"), "hello\n").expect("workspace file");

    let response = tool_call_response(
        tools::READ_FILE,
        json!({
            "path": "note.txt",
        }),
    );
    let provider = QueuedProvider::new(vec![response]);
    let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);

    let (steering_tx, steering_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-stop".to_string(), 16, 4, 128_000),
        None,
        Some(steering_rx),
    );
    let request = LLMRequest {
        model: "gpt-5.3-codex".to_string(),
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
    steering_tx
        .send(SteeringMessage::SteerStop)
        .expect("steer stop should queue");

    runner
        .execute_sequential_tool_calls(tool_calls, &mut runtime, &mut recorder, "[single]", false)
        .await
        .expect("tool execution should finish");

    let events = recorder.into_events();
    let call_item_id =
        completed_tool_invocation_item_id(&events, &tool_call_id).expect("completed invocation");
    assert_eq!(
        completed_tool_output_count(
            &events,
            &tool_call_id,
            ToolCallStatus::Failed,
            &call_item_id
        ),
        1
    );
}
