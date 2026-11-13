use super::*;

#[test]
fn system_directive_is_not_duplicated_after_intervening_messages() {
    let directive = "Synthesize a final response now.";
    let mut history = vec![uni::Message::system(directive.to_string())];
    for index in 0..4 {
        history.push(uni::Message::assistant(format!("intervening message {index}")));
    }

    push_system_directive_once(&mut history, directive);

    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == uni::MessageRole::System && message.content.as_text() == directive)
            .count(),
        1
    );
}
#[test]
fn system_directive_is_reissued_for_a_new_user_turn() {
    let directive = "Synthesize a final response now.";
    let mut history = vec![
        uni::Message::system(directive.to_string()),
        uni::Message::user("first request".to_string()),
    ];

    push_system_directive_once(&mut history, directive);

    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == uni::MessageRole::System && message.content.as_text() == directive)
            .count(),
        2
    );
}
#[test]
fn follow_up_prompt_detection_accepts_continue_variants() {
    assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("continue"));
    assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("continue."));
    assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("go on"));
    assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("please continue"));
    assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like(
        "Continue autonomously from the last stalled turn. Stall reason: x."
    ));
    assert!(!crate::agent::runloop::unified::state::is_follow_up_prompt_like("run cargo clippy and fix"));
}
#[test]
fn tracker_adoption_requires_tracker_use_or_explicit_continuation() {
    // Fresh informational question with no tool activity must not adopt
    // unrelated workspace tracker work (session-vtcode-20261008T094713Z:
    // `what is vtcode` redirected into README edits).
    let fresh_info = vec![uni::Message::user("what is vtcode".to_string())];
    assert!(!tracker_continuation_adoption_allowed(&fresh_info));
    // A work request alone does not identify the existing checklist.
    let progressive = vec![uni::Message::user("please fix the README table".to_string())];
    assert!(!tracker_continuation_adoption_allowed(&progressive));
    // Ordinary tool activity cannot adopt unrelated tracker work.
    let with_tools = vec![
        uni::Message::user("what is vtcode".to_string()),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![vtcode_core::llm::provider::ToolCall::function(
                "call_1".to_string(),
                "exec_command".to_string(),
                "{}".to_string(),
            )],
        ),
    ];
    assert!(!tracker_continuation_adoption_allowed(&with_tools));
    // Explicit follow-up adopts.
    let follow_up = vec![uni::Message::user("continue".to_string())];
    assert!(tracker_continuation_adoption_allowed(&follow_up));
    let mut approved_plan = vec![uni::Message::user(
        vtcode_core::prompts::system::PLANNING_WORKFLOW_IMPLEMENTATION_PROMPT.to_string(),
    )];
    assert!(tracker_continuation_adoption_allowed(&approved_plan));
    approved_plan.push(uni::Message::user(
        crate::agent::runloop::unified::turn::session_loop::BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX
            .to_string(),
    ));
    assert!(tracker_continuation_adoption_allowed(&approved_plan));
    assert!(!tracker_continuation_adoption_allowed(&[]));
}
#[test]
fn tracker_adoption_requires_successful_matching_current_request_result() {
    let request = uni::Message::user("fix the README table".to_string());
    for (action, status, expected) in [
        ("create", "created", true),
        ("create", "replaced", true),
        ("create", "unchanged", true),
        ("update", "updated", true),
        ("add", "added", true),
        ("list", "ok", false),
        ("create", "error", false),
    ] {
        let call = uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "tracker_1".to_string(),
                "task_tracker".to_string(),
                serde_json::json!({"action": action}).to_string(),
            )],
        );
        let mut history = vec![request.clone(), call];
        assert!(!tracker_continuation_adoption_allowed(&history), "unanswered {action}");
        history.push(uni::Message::tool_response(
            "different_id".to_string(),
            serde_json::json!({"status": status}).to_string(),
        ));
        assert!(!tracker_continuation_adoption_allowed(&history), "unmatched {action}");
        history.push(uni::Message::tool_response(
            "tracker_1".to_string(),
            serde_json::json!({"status": status}).to_string(),
        ));
        assert_eq!(tracker_continuation_adoption_allowed(&history), expected, "{action}/{status}");
        history.push(uni::Message::user(
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_continue_follow_up(&[
                "#1 table (pending)".to_string(),
            ]),
        ));
        assert_eq!(tracker_continuation_adoption_allowed(&history), expected, "internal {action}/{status}");
        history.push(uni::Message::user("what is vtcode".to_string()));
        assert!(!tracker_continuation_adoption_allowed(&history), "new request {action}/{status}");
    }
}
#[test]
fn tracker_adoption_does_not_match_reused_ids_from_another_batch() {
    let history = vec![
        uni::Message::user("fix README".to_string()),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "reused".to_string(),
                "task_tracker".to_string(),
                r#"{"action":"create"}"#.to_string(),
            )],
        ),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "reused".to_string(),
                "exec_command".to_string(),
                "{}".to_string(),
            )],
        ),
        uni::Message::tool_response("reused".to_string(), r#"{"status":"created"}"#.to_string()),
    ];
    assert!(!tracker_continuation_adoption_allowed(&history));
}
#[tokio::test]
async fn informational_answer_with_tool_activity_does_not_resume_workspace_tracker() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.set_vt_cfg_for_test(vtcode_core::config::loader::VTCodeConfig::default());
    let mut ctx = backing.turn_processing_context();
    let tracker = ctx.tool_registry.get_tool("task_tracker").expect("tracker");
    tracker
        .execute(serde_json::json!({
            "action": "create", "title": "Unrelated README work", "items": ["Rewrite README"]
        }))
        .await
        .expect("existing workspace tracker");
    ctx.working_history.extend([
        uni::Message::user("what is vtcode".to_string()),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "read_1".to_string(),
                "read_file".to_string(),
                r#"{"path":"README.md"}"#.to_string(),
            )],
        ),
        uni::Message::tool_response("read_1".to_string(), "VT Code is a coding agent.".to_string()),
    ]);
    let outcome = ctx
        .handle_text_response("VT Code is a Rust terminal coding agent.".to_string(), Vec::new(), None, None, false)
        .await
        .expect("informational answer");
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    assert!(!ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().contains(AUTONOMOUS_CONTINUE_DIRECTIVE)
    }));
    let output = tracker
        .execute(serde_json::json!({"action": "list"}))
        .await
        .expect("tracker unchanged");
    assert_eq!(output["checklist"]["title"], "Unrelated README work");
    assert_eq!(output["checklist"]["pending"], 1);
}
#[tokio::test]
async fn explicit_continuation_resumes_incomplete_workspace_tracker() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.set_vt_cfg_for_test(vtcode_core::config::loader::VTCodeConfig::default());
    let mut ctx = backing.turn_processing_context();
    ctx.tool_registry
        .get_tool("task_tracker")
        .expect("tracker")
        .execute(serde_json::json!({"action": "create", "title": "README", "items": ["Repair table"]}))
        .await
        .expect("existing tracker");
    ctx.working_history.push(uni::Message::user("continue".to_string()));
    ctx.tool_registry
        .begin_tracker_request(tracker_request_explicitly_adopts("continue"));
    let outcome = ctx
        .handle_text_response("The README table still needs repair.".to_string(), Vec::new(), None, None, false)
        .await
        .expect("continuation response");
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().contains(AUTONOMOUS_CONTINUE_DIRECTIVE)
    }));
}
#[tokio::test]
async fn interactive_tracker_adoption_survives_compaction_and_resets_for_a_fresh_request() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.set_vt_cfg_for_test(vtcode_core::config::loader::VTCodeConfig::default());
    let mut ctx = backing.turn_processing_context();
    ctx.tool_registry.begin_tracker_request(false);
    ctx.working_history
        .push(uni::Message::user("repair the README table".to_string()));
    ctx.tool_registry
        .execute_tool(
            "task_tracker",
            serde_json::json!({
                "action":"create", "title":"README", "items":["Repair table"]
            }),
        )
        .await
        .expect("current request adopts tracker");
    // No matching call/result pair remains when completion is assessed.
    ctx.working_history.clear();
    ctx.working_history
        .push(uni::Message::user("Compacted current task progress".to_string()));
    assert!(!tracker_continuation_adoption_allowed(ctx.working_history));
    let outcome = ctx
        .handle_text_response("The README table still needs repair.".to_string(), Vec::new(), None, None, false)
        .await
        .expect("compacted response");
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));

    ctx.tool_registry.begin_tracker_request(false);
    ctx.working_history.clear();
    ctx.working_history.push(uni::Message::user("what is vtcode".to_string()));
    let outcome = ctx
        .handle_text_response("VT Code is a Rust terminal coding agent.".to_string(), Vec::new(), None, None, false)
        .await
        .expect("fresh response");
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
}
