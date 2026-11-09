use super::*;

#[tokio::test]
async fn retained_evidence_coaching_reaches_next_request_with_tools() {
    use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{LoopTracker, update_repetition_tracker};
    use vtcode_core::config::constants::tools;

    let mut tracker = LoopTracker::new();
    let args = json!({"cmd":"sed -n '2,3p' README.md"});
    let result = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"output":"second\nthird\n", "exit_code":0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for _ in 0..2 {
        update_repetition_tracker(&mut tracker, &result, tools::EXEC_COMMAND, &args);
    }
    let notice = tracker.take_redundant_navigation_guidance().expect("redundancy notice");
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.add_tool_definition(named_tool("apply_patch")).await;
    let mut ctx = backing.turn_processing_context();
    ctx.working_history.push(uni::Message::user("update README".to_string()));
    exec_round(&mut ctx, "read-1");
    ctx.push_system_message(notice.clone());
    let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    let built = build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
        .await
        .expect("request should build");
    assert!(
        built
            .request
            .messages
            .iter()
            .any(|message| message.content.as_text().contains(&notice))
    );
    assert!(request_tool_names(&built.request).contains(&"apply_patch".to_string()));
}

#[tokio::test]
async fn parallel_tool_replies_retain_interleaved_coaching() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.add_tool_definition(named_tool("apply_patch")).await;
    let mut ctx = backing.turn_processing_context();
    ctx.working_history.push(uni::Message::user("update README".to_string()));
    let calls = ["read-1", "read-2"]
        .into_iter()
        .map(|id| uni::ToolCall::function(id.to_string(), "exec_command".to_string(), "{}".to_string()))
        .collect();
    ctx.working_history
        .push(uni::Message::assistant_with_tools(String::new(), calls));
    ctx.working_history
        .push(uni::Message::tool_response("read-1".to_string(), "first result".to_string()));
    let notice = "This inspection returned only text evidence already gathered this turn.";
    ctx.push_system_message(notice);
    ctx.working_history
        .push(uni::Message::tool_response("read-2".to_string(), "second result".to_string()));
    let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    let built = build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
        .await
        .expect("request should build");
    assert!(
        built
            .request
            .messages
            .iter()
            .any(|message| message.content.as_text().contains(notice))
    );
    assert!(
        built
            .request
            .messages
            .iter()
            .any(|message| message.tool_call_id.as_deref() == Some("read-2"))
    );
    assert!(
        built
            .request
            .messages
            .iter()
            .any(|message| message.tool_call_id.as_deref() == Some("read-1"))
    );
    assert!(request_tool_names(&built.request).contains(&"apply_patch".to_string()));
}

#[tokio::test]
async fn openai_responses_replays_full_structured_history_without_suffixing() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let prior_messages = vec![
        uni::Message::user("hello".to_string()),
        uni::Message::assistant("hi".to_string()),
    ];
    let mut ctx = backing.turn_processing_context();
    ctx.working_history.extend(prior_messages.clone());
    ctx.working_history.push(uni::Message::user("continue".to_string()));
    ctx.session_stats
        .set_previous_response_chain("openai", "noop-model", Some("resp_123"), &prior_messages);

    let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    let built = build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
        .await
        .expect("openai request should build");

    assert_eq!(built.request.previous_response_id, None);
    assert_eq!(
        non_runtime_request_messages(&built.request),
        vec![
            uni::Message::user("hello".to_string()),
            uni::Message::assistant("hi".to_string()),
            uni::Message::user("continue".to_string())
        ]
    );
}
#[tokio::test]
async fn compatible_provider_responses_keeps_full_history_without_previous_response_id() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let prior_messages = vec![uni::Message::user("hello".to_string())];
    let mut ctx = backing.turn_processing_context();
    ctx.working_history.extend(prior_messages.clone());
    ctx.working_history.push(uni::Message::user("continue".to_string()));
    ctx.session_stats
        .set_previous_response_chain("mycorp", "noop-model", Some("resp_123"), &prior_messages);

    let mut snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    snapshot.provider_name = "mycorp".to_string();
    snapshot.capabilities.responses_compaction = true;
    let built = build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
        .await
        .expect("compatible provider request should build");

    assert_eq!(built.request.previous_response_id, None);
    assert_eq!(
        non_runtime_request_messages(&built.request),
        vec![
            uni::Message::user("hello".to_string()),
            uni::Message::user("continue".to_string())
        ]
    );
}
#[tokio::test]
async fn clean_request_shares_messages_with_continuation_history() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    ctx.working_history.push(uni::Message::user("hello".to_string()));

    let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    let built = build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
        .await
        .expect("clean request should build");

    assert!(Arc::ptr_eq(&built.request.messages, &built.continuation_messages));
}
#[tokio::test]
async fn first_call_composition_captures_on_first_build_only() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    ctx.working_history.push(uni::Message::user("hello".to_string()));

    assert!(ctx.session_stats.first_call_composition().is_none());
    let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
        .await
        .expect("first request should build");

    let first = ctx
        .session_stats
        .first_call_composition()
        .expect("first build must capture composition");
    assert!(first.fixed_overhead_tokens() > 0, "first build must record a non-zero harness tax");
    // `first_call` is defined as "composition not yet captured", so the
    // first build is the only one that can observe it as true.
    let first_call_flag = ctx.session_stats.first_call_composition().is_none();
    assert!(!first_call_flag, "composition is already captured after the first build");

    ctx.working_history.push(uni::Message::user("again".to_string()));
    let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
        .await
        .expect("second request should build");
    assert_eq!(
        ctx.session_stats.first_call_composition(),
        Some(first),
        "later builds must not overwrite the first-call snapshot"
    );
}
#[tokio::test]
async fn non_openai_responses_chain_keeps_full_history() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let prior_messages = vec![uni::Message::user("hello".to_string())];
    let mut ctx = backing.turn_processing_context();
    ctx.working_history.extend(prior_messages.clone());
    ctx.working_history.push(uni::Message::user("continue".to_string()));
    ctx.session_stats
        .set_previous_response_chain("gemini", "noop-model", Some("resp_123"), &prior_messages);

    let mut snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    snapshot.provider_name = "gemini".to_string();
    let built = build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
        .await
        .expect("gemini request should build");

    assert_eq!(built.request.previous_response_id.as_deref(), Some("resp_123"));
    assert_eq!(
        non_runtime_request_messages(&built.request),
        vec![
            uni::Message::user("hello".to_string()),
            uni::Message::user("continue".to_string())
        ]
    );
}
