use super::*;

#[tokio::test]
async fn few_shot_context_is_persisted_once_and_replayed_append_only() {
    let mut backing = TestTurnProcessingBacking::new().await;
    write_patch_edit_few_shot_example(backing.workspace_path());
    let mut ctx = backing.turn_processing_context();
    ctx.working_history
        .push(uni::Message::user("please patch and edit the parser".to_string()));

    let mut snapshot = capture_turn_request_snapshot(&mut ctx, "claude-opus-5-5", false);
    snapshot.provider_name = "anthropic".to_string();
    snapshot.capabilities.turn_scoped_system_messages = true;

    let first = build_turn_request(&mut ctx, 1, "claude-opus-5-5", &snapshot, Some(320), None, false)
        .await
        .expect("first request should build");
    let first_messages = non_runtime_request_messages(&first.request);
    assert_eq!(first_messages.len(), 2);
    assert_eq!(first_messages[0], uni::Message::user("please patch and edit the parser".to_string()));
    assert_eq!(first_messages[1].role, uni::MessageRole::System);
    assert_eq!(first_messages[1].clear_at, Some(uni::MessageClearAt::NextUserMessage));
    assert!(is_few_shot_block(&first_messages[1]));
    assert!(first_messages[1].content.as_text().contains("### patch-edit"));
    assert!(!system_prompt_text(&first.request).contains(vtcode_core::prompts::FEW_SHOT_SECTION_HEADER));

    ctx.working_history.push(uni::Message::assistant_with_tools(
        String::new(),
        vec![uni::ToolCall::function(
            "call_1".to_string(),
            "read_file".to_string(),
            "{}".to_string(),
        )],
    ));
    ctx.working_history
        .push(uni::Message::tool_response("call_1".to_string(), "fn parse() {}".to_string()));

    let second = build_turn_request(&mut ctx, 2, "claude-opus-5-5", &snapshot, Some(320), None, false)
        .await
        .expect("second request should build");
    let second_messages = non_runtime_request_messages(&second.request);
    assert_eq!(
        &second_messages[..first_messages.len()],
        first_messages.as_slice(),
        "later requests of the turn must only append to the earlier request"
    );
    assert_eq!(second_messages.iter().filter(|message| is_few_shot_block(message)).count(), 1);

    let mut fold_snapshot = snapshot.clone();
    fold_snapshot.capabilities.turn_scoped_system_messages = false;
    let folded = build_turn_request(&mut ctx, 3, "claude-sonnet-5", &fold_snapshot, Some(320), None, false)
        .await
        .expect("fold-route request should build");
    let folded_messages = non_runtime_request_messages(&folded.request);
    assert_eq!(folded_messages[1].role, uni::MessageRole::User);
    assert!(is_few_shot_block(&folded_messages[1]));
    assert!(folded_messages.iter().all(|message| message.clear_at.is_none()));
    assert!(
        !folded_messages
            .iter()
            .any(|message| message.role == uni::MessageRole::System && is_few_shot_block(message)),
        "fold routes must not receive few-shot context as a system message"
    );
    assert_eq!(ctx.working_history.iter().filter(|message| is_few_shot_block(message)).count(), 1);
}
#[tokio::test]
async fn collapsed_output_notice_is_append_only_across_tool_rounds() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut config = VTCodeConfig::default();
    config.agent.provider = "anthropic".to_string();
    let config = Box::leak(Box::new(config));

    let mut ctx = backing.turn_processing_context();
    ctx.vt_cfg = Some(config);
    ctx.working_history.push(uni::Message::user("run the checks".to_string()));
    exec_round(&mut ctx, "toolu_1");

    let mut snapshot = capture_turn_request_snapshot(&mut ctx, "claude-opus-5-5", false);
    snapshot.provider_name = "anthropic".to_string();
    snapshot.capabilities.turn_scoped_system_messages = true;

    let first = build_turn_request(&mut ctx, 1, "claude-opus-5-5", &snapshot, Some(320), None, false)
        .await
        .expect("first request should build");
    let first_messages = non_runtime_request_messages(&first.request);
    assert_eq!(collapsed_notice_count(&first_messages), 1);
    assert!(first_messages.iter().any(|message| {
        message.clear_at == Some(uni::MessageClearAt::NextUserMessage)
            && message.content.as_text().as_ref() == super::super::COLLAPSED_TOOL_OUTPUT_NOTICE
    }));

    let repeated = build_turn_request(&mut ctx, 2, "claude-opus-5-5", &snapshot, Some(320), None, false)
        .await
        .expect("repeated request should build");
    assert_eq!(non_runtime_request_messages(&repeated.request), first_messages);

    exec_round(&mut ctx, "toolu_2");
    let second = build_turn_request(&mut ctx, 3, "claude-opus-5-5", &snapshot, Some(320), None, false)
        .await
        .expect("next tool round should build");
    let second_messages = non_runtime_request_messages(&second.request);
    assert_eq!(
        &second_messages[..first_messages.len()],
        first_messages.as_slice(),
        "a later tool round must not move or delete the notice already sent"
    );
    assert_eq!(collapsed_notice_count(&second_messages), 1, "one notice per user turn");

    ctx.working_history.push(uni::Message::assistant("checks pass".to_string()));
    ctx.working_history.push(uni::Message::user("run them again".to_string()));
    exec_round(&mut ctx, "toolu_3");
    let next_turn = build_turn_request(&mut ctx, 4, "claude-opus-5-5", &snapshot, Some(320), None, false)
        .await
        .expect("next user turn should build");
    let next_turn_messages = non_runtime_request_messages(&next_turn.request);
    assert!(next_turn_messages.starts_with(&second_messages));
    assert_eq!(collapsed_notice_count(&next_turn_messages), 2, "each user turn gets its own turn-scoped copy");

    let mut non_anthropic_snapshot = snapshot.clone();
    non_anthropic_snapshot.provider_name = "openai".to_string();
    non_anthropic_snapshot.capabilities.turn_scoped_system_messages = false;
    let switched = build_turn_request(&mut ctx, 5, "gpt-5", &non_anthropic_snapshot, Some(320), None, false)
        .await
        .expect("provider-switched request should build");
    assert!(switched.request.messages.iter().all(|message| message.clear_at.is_none()));
    assert_eq!(
        collapsed_notice_count(switched.request.messages.as_slice()),
        1,
        "routes without turn-scoped messages receive only the latest copy"
    );
    let latest_notice = switched
        .request
        .messages
        .iter()
        .rposition(|message| message.content.as_text().as_ref() == super::super::COLLAPSED_TOOL_OUTPUT_NOTICE)
        .expect("notice");
    assert_eq!(switched.request.messages[latest_notice - 1].tool_call_id.as_deref(), Some("toolu_3"));
    assert_eq!(collapsed_notice_count(switched.continuation_messages.as_slice()), 2);
}
