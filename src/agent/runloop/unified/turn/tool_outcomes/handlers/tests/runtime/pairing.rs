use super::*;

fn tool_response_count(history: &[uni::Message], call_id: &str) -> usize {
    history
        .iter()
        .filter(|message| message.role == uni::MessageRole::Tool && message.tool_call_id.as_deref() == Some(call_id))
        .count()
}

fn denied_search_call(call_id: &str) -> PreparedAssistantToolCall {
    PreparedAssistantToolCall::new(uni::ToolCall::function(
        call_id.to_string(),
        tool_names::CODE_SEARCH.to_string(),
        serde_json::json!({"query": "pairing probe"}).to_string(),
    ))
}

fn sibling_read_call(backing: &TestContextBacking, call_id: &str) -> (PreparedAssistantToolCall, serde_json::Value) {
    let args = serde_json::json!({"path": backing.sample_file});
    let call = PreparedAssistantToolCall::new(uni::ToolCall::function(
        call_id.to_string(),
        tool_names::READ_FILE.to_string(),
        args.to_string(),
    ));
    (call, args)
}

fn record_batch(history: &mut Vec<uni::Message>, calls: &[PreparedAssistantToolCall]) {
    let raw_calls = calls.iter().map(|call| call.raw_call().clone()).collect::<Vec<_>>();
    history.push(uni::Message::assistant_with_tools(String::new(), raw_calls));
}

async fn deny_search(backing: &mut TestContextBacking) {
    backing
        .tool_registry
        .set_tool_policy(tool_names::CODE_SEARCH, ToolPolicy::Deny)
        .await
        .expect("deny search tool");
}

#[tokio::test]
async fn blocked_call_under_active_recovery_still_answers_abandoned_sibling() {
    let mut backing = TestContextBacking::new(4).await;
    deny_search(&mut backing).await;
    let (sibling, sibling_args) = sibling_read_call(&backing, "sibling_1");
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &sibling_args, PermissionGrant::Permanent).await;

    let mut ctx = backing.turn_processing_context();
    assert!(ctx.activate_recovery("pairing probe"), "recovery must arm");
    assert!(!ctx.recovery_pass_used(), "recovery pass must stay unused");

    let calls = vec![denied_search_call("denied_1"), sibling];
    record_batch(ctx.working_history, &calls);
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let outcome = handle_tool_calls(&mut outcome_ctx, &calls)
        .await
        .expect("dispatch must not error");
    assert!(
        matches!(outcome, Some(TurnHandlerOutcome::Continue)),
        "recovery-active guard must defer with Continue"
    );

    let history = &outcome_ctx.ctx.working_history;
    assert_eq!(tool_response_count(history, "denied_1"), 1, "denied call must keep its policy response");
    assert_eq!(
        tool_response_count(history, "sibling_1"),
        1,
        "abandoned sibling must get exactly one backstop response"
    );
    let sibling_response = history
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some("sibling_1"))
        .expect("sibling response");
    assert!(
        sibling_response.content.as_text().contains("dispatch_incomplete"),
        "sibling response must come from the dispatch backstop, got: {}",
        sibling_response.content.as_text()
    );
}

#[tokio::test]
async fn denied_call_without_recovery_keeps_batch_paired() {
    let mut backing = TestContextBacking::new(4).await;
    deny_search(&mut backing).await;
    let (sibling, sibling_args) = sibling_read_call(&backing, "sibling_1");
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &sibling_args, PermissionGrant::Permanent).await;

    let mut ctx = backing.turn_processing_context();
    let calls = vec![denied_search_call("denied_1"), sibling];
    record_batch(ctx.working_history, &calls);
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    handle_tool_calls(&mut outcome_ctx, &calls)
        .await
        .expect("dispatch must not error");

    let history = &outcome_ctx.ctx.working_history;
    assert_eq!(tool_response_count(history, "denied_1"), 1, "denied call must keep its policy response");
    assert_eq!(tool_response_count(history, "sibling_1"), 1, "executed sibling must keep its real result");
    let sibling_response = history
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some("sibling_1"))
        .expect("sibling response");
    assert!(
        !sibling_response.content.as_text().contains("dispatch_incomplete"),
        "executed sibling must not be overwritten by the backstop, got: {}",
        sibling_response.content.as_text()
    );
}
