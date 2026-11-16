use super::{
    extract_turn_config, handle_steering_messages, is_internal_harness_follow_up, is_stale_approved_plan_pause_response,
};
use crate::agent::runloop::unified::planning_workflow::{
    PlanningIntent, detect_enter_planning_intent, detect_planning_intent,
};
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
use std::time::Duration;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::steering::SteeringMessage;
use vtcode_core::llm::provider::MessageRole;

#[test]
fn detects_implement_the_plan_trigger() {
    assert_eq!(detect_planning_intent("Implement the plan.", false), PlanningIntent::ExitAndImplement);
    assert_eq!(
        detect_planning_intent("Please execute this plan and start coding.", false),
        PlanningIntent::ExitAndImplement
    );
}

#[test]
fn detects_existing_exit_intents() {
    assert_eq!(
        detect_planning_intent("Exit planning workflow and implement.", false),
        PlanningIntent::ExitAndImplement
    );
    assert_eq!(
        detect_planning_intent("Exit planning workflow and proceed.", false),
        PlanningIntent::ExitAndImplement
    );
}

#[test]
fn does_not_exit_when_user_wants_to_keep_planning() {
    assert_eq!(
        detect_planning_intent("Don't implement yet, stay in planning workflow and refine the plan.", false),
        PlanningIntent::StayInPlanning
    );
    assert_eq!(detect_planning_intent("Continue planning for now.", false), PlanningIntent::StayInPlanning);
    assert_eq!(detect_planning_intent("Do not approve yet", false), PlanningIntent::None);
    assert_eq!(detect_planning_intent("Don't accept this plan", false), PlanningIntent::None);
    assert_eq!(detect_planning_intent("Do not yet approve the plan", false), PlanningIntent::None);
    assert_eq!(detect_planning_intent("Please do not immediately accept this plan", false), PlanningIntent::None);
    assert_eq!(
        detect_planning_intent("Do not under any circumstances approve the plan", false),
        PlanningIntent::None
    );
    assert_eq!(
        detect_planning_intent("No schema changes. I approve the plan", false),
        PlanningIntent::ExitAndImplement
    );
    assert_eq!(
        detect_planning_intent("Yes. Please explain the tradeoffs before making changes", false),
        PlanningIntent::None
    );
    assert_eq!(detect_planning_intent("Please approve this plan", false), PlanningIntent::ExitAndImplement);
    assert_eq!(
        detect_planning_intent("Do not use plan mode but approve the plan", false),
        PlanningIntent::ExitAndImplement
    );
}

#[test]
fn detects_bare_implement_trigger() {
    assert_eq!(detect_planning_intent("implement", false), PlanningIntent::ExitAndImplement);
    assert_eq!(detect_planning_intent("/implement", false), PlanningIntent::ExitAndImplement);
    assert_eq!(detect_planning_intent("implement.", false), PlanningIntent::ExitAndImplement);
}

#[test]
fn detects_short_implement_variants() {
    assert_eq!(detect_planning_intent("Implement now", false), PlanningIntent::ExitAndImplement);
    assert_eq!(detect_planning_intent("Start implementing", false), PlanningIntent::ExitAndImplement);
}

#[test]
fn detects_direct_confirmation_aliases_as_execute_intent() {
    assert_eq!(detect_planning_intent("yes", false), PlanningIntent::ExitAndImplement);
    // "continue" is NOT a direct exit trigger — it is ambiguous.
    // It only works as a short confirmation when the assistant
    // recently prompted for implementation.
    assert_eq!(detect_planning_intent("continue", false), PlanningIntent::None);
    assert_eq!(detect_planning_intent("go", false), PlanningIntent::ExitAndImplement);
    assert_eq!(detect_planning_intent("start", false), PlanningIntent::ExitAndImplement);
    assert_eq!(detect_planning_intent("yes!", false), PlanningIntent::ExitAndImplement);
}

#[test]
fn stay_mode_has_priority_over_implement_keyword() {
    assert_eq!(
        detect_planning_intent("Do not implement yet; keep planning.", false),
        PlanningIntent::StayInPlanning
    );
    assert_eq!(
        detect_planning_intent("Stay in planning workflow and don't implement.", false),
        PlanningIntent::StayInPlanning
    );
}

#[test]
fn does_not_false_trigger_on_non_intent_implementation_text() {
    assert_eq!(detect_planning_intent("The implementation details are unclear.", false), PlanningIntent::None);
}

#[test]
fn detects_explicit_planning_requests() {
    assert!(detect_enter_planning_intent("make a plan for this"));
    assert!(detect_enter_planning_intent("before implementing, create a plan"));
    assert!(detect_enter_planning_intent("outline the implementation plan"));
    assert!(detect_enter_planning_intent("use plan mode, refine @README.md"));
    assert!(detect_enter_planning_intent("Do not use plan mode; create a plan instead"));
    assert!(!detect_enter_planning_intent("Do not use plan mode, implement directly"));
    assert!(!detect_enter_planning_intent("Don't switch to plan mode"));
    assert!(!detect_enter_planning_intent("Never enter plan mode"));
    assert!(!detect_enter_planning_intent("Do not under any circumstances use plan mode; implement the fix"));
}

#[test]
fn does_not_start_planning_for_generic_research_requests() {
    assert!(!detect_enter_planning_intent("explore and tell me about the core agent loop"));
    assert!(!detect_enter_planning_intent("review the runloop and summarize the behavior"));
}

#[test]
fn confirmation_words_trigger_with_implementation_prompt_context() {
    assert_eq!(detect_planning_intent("yes", true), PlanningIntent::ExitAndImplement);
    assert_eq!(detect_planning_intent("continue", true), PlanningIntent::ExitAndImplement);
    assert_eq!(detect_planning_intent("go", true), PlanningIntent::ExitAndImplement);
    assert_eq!(detect_planning_intent("start", true), PlanningIntent::ExitAndImplement);
    assert_eq!(detect_planning_intent("begin", true), PlanningIntent::ExitAndImplement);
}

#[test]
fn confirmation_words_do_not_trigger_without_implementation_prompt_context() {
    assert_eq!(
        detect_planning_intent("yes", false),
        PlanningIntent::ExitAndImplement // "yes" is a direct command
    );
    assert_eq!(detect_planning_intent("continue", false), PlanningIntent::None);
}

#[test]
fn confirmation_words_do_not_trigger_when_stay_in_planning_workflow_is_prompted() {
    // When the assistant asks about staying in planning, "yes" should
    // not trigger exit - but "yes" is still a direct command, so it
    // will trigger ExitAndImplement. This is expected behavior.
    assert_eq!(detect_planning_intent("yes", false), PlanningIntent::ExitAndImplement);
}

#[test]
fn stale_approved_plan_pause_response_requires_both_pause_and_unavailable_markers() {
    assert!(is_stale_approved_plan_pause_response(
        "Implementation is paused because tool use is disabled. Wait for the next turn."
    ));
    assert!(!is_stale_approved_plan_pause_response(
        "The implementation is blocked by a missing Docker daemon; no source edits are safe yet."
    ));
    assert!(!is_stale_approved_plan_pause_response(
        "Implementation is paused while I wait for the user to clarify the API contract."
    ));
}

#[test]
fn fresh_turn_supersedes_expired_recovery_without_changing_history() {
    use vtcode_core::llm::provider::Message;

    let restriction = "Recovery: two assistant batches attempted inspection after the tool preview budget was exhausted. Tools are disabled for this pass.";
    let mut history = vec![
        Message::system(restriction.to_owned()),
        Message::assistant("All six steps remain blocked".to_owned()),
        Message::user("Continue the approved plan".to_owned()),
    ];
    super::restore_fresh_turn_tool_guidance(&mut history, false);
    assert_eq!(history.len(), 4);
    assert_eq!(history[0].content.as_text(), restriction);
    assert_eq!(history[3].role, MessageRole::System);
    let fresh = history[3].content.as_text();
    assert!(fresh.contains("restrictions have expired"));
    assert!(fresh.contains("permission checks"));
    assert!(fresh.contains("targeted read or small spool range"));

    history.push(Message::user("Continue the next step".to_owned()));
    let restored_len = history.len();
    super::restore_fresh_turn_tool_guidance(&mut history, false);
    super::restore_fresh_turn_tool_guidance(&mut history, false);
    assert_eq!(history.len(), restored_len, "already superseded restrictions must not duplicate guidance");
    history.push(Message::system(
        "Tool wall-clock budget exhausted for this turn (600s). Tools are disabled for one bounded synthesis pass."
            .to_owned(),
    ));
    history.push(Message::user("Retry after the new recovery".to_owned()));
    super::restore_fresh_turn_tool_guidance(&mut history, true);
    assert_eq!(history.len(), restored_len + 2, "active recovery still takes precedence");
    super::restore_fresh_turn_tool_guidance(&mut history, false);
    assert_eq!(history.len(), restored_len + 3, "a newer restriction needs one new restoration");
    super::restore_fresh_turn_tool_guidance(&mut history, false);
    assert_eq!(history.len(), restored_len + 3);

    let mut active = vec![Message::system(restriction.to_owned())];
    super::restore_fresh_turn_tool_guidance(&mut active, true);
    assert_eq!(active.len(), 1, "current recovery must still disable tools");

    let mut policy = vec![Message::system(
        "exec_command is denied by permission policy".to_owned(),
    )];
    super::restore_fresh_turn_tool_guidance(&mut policy, false);
    assert_eq!(policy.len(), 1, "permission restrictions must not be superseded");

    let mut user_text = vec![Message::user(restriction.to_owned())];
    super::restore_fresh_turn_tool_guidance(&mut user_text, false);
    assert_eq!(user_text.len(), 1, "user text is not a harness recovery event");

    let mut quoted_evidence = vec![Message::system(
        "Planning recovery synthesis: gather evidence.\n<bounded_recovery_evidence>tools are disabled</bounded_recovery_evidence>".to_owned(),
    )];
    super::restore_fresh_turn_tool_guidance(&mut quoted_evidence, false);
    assert_eq!(quoted_evidence.len(), 1, "quoted evidence must not become a runtime restriction");
}

#[test]
fn fresh_turn_restores_generated_budget_and_post_tool_recovery_directives() {
    use crate::agent::runloop::unified::run_loop_context::ToolWallClockExhaustion;
    use crate::agent::runloop::unified::turn::turn_loop::{
        POST_TOOL_RECOVERY_REASON, POST_TOOL_RECOVERY_REASON_PLAN_MODE, RECOVERY_TOOL_CALL_RETRY_DIRECTIVE_PLAN_MODE,
    };
    use vtcode_core::llm::provider::Message;

    for directive in [
        ToolWallClockExhaustion { max_secs: 600 }.synthesis_directive_message(),
        POST_TOOL_RECOVERY_REASON.to_owned(),
        POST_TOOL_RECOVERY_REASON_PLAN_MODE.to_owned(),
        RECOVERY_TOOL_CALL_RETRY_DIRECTIVE_PLAN_MODE.to_owned(),
        crate::agent::runloop::unified::planning_workflow::build_plan_repair_directive("Invalid plan"),
    ] {
        let mut history = vec![Message::system(directive.clone())];
        super::restore_fresh_turn_tool_guidance(&mut history, false);
        assert_eq!(history.len(), 2, "must restore production directive: {directive}");
        assert_eq!(history[0].content.as_text(), directive);
        super::restore_fresh_turn_tool_guidance(&mut history, false);
        assert_eq!(history.len(), 2);
    }
}

#[test]
fn internal_harness_follow_ups_stay_quiet_while_user_steering_echoes() {
    use crate::agent::runloop::unified::turn::session::slash_commands::GOAL_AUTO_CONTINUE_PREFIX;
    use crate::agent::runloop::unified::turn::session_loop::{
        BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX, MONITOR_CONTINUATION_PROMPT_PREFIX,
        VERIFICATION_AUTO_RECOVERY_PREFIX,
    };
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
        PLAN_MODE_AUTO_CONTINUE_MARKER, RECOVERABLE_BLOCKED_CONTINUE_FOLLOW_UP_PREFIX,
        TRACKER_CONTINUE_FOLLOW_UP_PREFIX,
    };

    assert!(is_internal_harness_follow_up(
        format!("{TRACKER_CONTINUE_FOLLOW_UP_PREFIX} #1 do X. This follow-up is the harness resuming the work.")
            .as_str()
    ));
    assert!(is_internal_harness_follow_up(
        format!("{PLAN_MODE_AUTO_CONTINUE_MARKER} no validated persisted plan is ready for approval yet.").as_str()
    ));
    assert!(is_internal_harness_follow_up(
        format!("{RECOVERABLE_BLOCKED_CONTINUE_FOLLOW_UP_PREFIX} Turn ended with a recovery fallback.").as_str()
    ));
    assert!(is_internal_harness_follow_up(
        format!("{BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX} and continue.").as_str()
    ));
    assert!(is_internal_harness_follow_up(
        format!("{MONITOR_CONTINUATION_PROMPT_PREFIX} and continue.").as_str()
    ));
    assert!(is_internal_harness_follow_up(
        format!("{VERIFICATION_AUTO_RECOVERY_PREFIX} Verification is still pending.").as_str()
    ));
    assert!(is_internal_harness_follow_up(
        format!("{GOAL_AUTO_CONTINUE_PREFIX}\"Ship it\". Build on prior turns.").as_str()
    ));
    assert!(!is_internal_harness_follow_up("leftover"));
    assert!(!is_internal_harness_follow_up("please keep going with the build"));
    assert!(
        !is_internal_harness_follow_up("Continue working toward the goal: \"Ship it\"."),
        "user-initiated Resume stays a user boundary"
    );
}

#[test]
fn extract_turn_config_honors_request_user_input_setting_in_planning_workflow() {
    let mut cfg = VTCodeConfig::default();
    cfg.chat.ask_questions.enabled = false;

    let turn_cfg = extract_turn_config(Some(&cfg), true, true);
    assert!(!turn_cfg.request_user_input_enabled);
}

#[tokio::test]
async fn planning_steering_resets_draft_rank_only_for_user_input() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    let events_path = backing.enable_harness_emitter();
    {
        let ctx = backing.turn_loop_context();
        ctx.plan_session.recovery.best_draft = Some("previous draft".to_owned());
        ctx.plan_session.recovery.best_draft_defects = Some(0);
    }
    let mut history = Vec::new();
    let mut result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
    backing.queue_synthetic_follow_up_input_for_test("synthetic continuation");
    handle_steering_messages(&mut backing.turn_loop_context(), &mut history, &mut result)
        .await
        .expect("synthetic steering");
    assert_eq!(backing.turn_loop_context().plan_session.recovery.best_draft_defects, Some(0));

    backing.queue_follow_up_input_for_test("Plan a different task");
    handle_steering_messages(&mut backing.turn_loop_context(), &mut history, &mut result)
        .await
        .expect("user steering");
    let recovery = &backing.turn_loop_context().plan_session.recovery;
    assert!(recovery.best_draft_defects.is_none());
    assert_eq!(recovery.best_draft.as_deref(), Some("previous draft"));
    let events = tokio::fs::read_to_string(events_path).await.expect("steering checkpoint");
    assert_eq!(events.matches("planning-recovery").count(), 1);
}

#[tokio::test]
async fn steering_follow_up_inputs_apply_mid_turn_in_order() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender
        .send(SteeringMessage::FollowUpInput("first".to_string()))
        .expect("first follow-up");
    sender.send(SteeringMessage::Resume).expect("stray resume");
    sender
        .send(SteeringMessage::FollowUpInput("second".to_string()))
        .expect("second follow-up");
    backing.set_steering_receiver(receiver);

    let mut working_history = Vec::new();
    let mut result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
    let handled = {
        let mut ctx = backing.turn_loop_context();
        handle_steering_messages(&mut ctx, &mut working_history, &mut result)
            .await
            .expect("handle steering")
    };

    assert!(!handled);
    assert!(matches!(result, TurnLoopResult::Completed { .. }));
    let steered: Vec<String> = working_history
        .iter()
        .filter(|message| message.role == MessageRole::User)
        .map(|message| message.content.as_text().to_string())
        .collect();
    assert_eq!(steered, vec!["first".to_string(), "second".to_string()]);
    assert!(
        working_history
            .iter()
            .filter(|message| message.role == MessageRole::User)
            .all(|message| { message.metadata.as_ref().and_then(|metadata| metadata.source()) == Some("user_input") })
    );
    // Every steered message is tagged with its intent id so restart
    // recovery can dedupe it.
    assert!(working_history.iter().any(|message| {
        message.role == MessageRole::User
            && message
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.intent_id())
                .is_some_and(|intent_id| !intent_id.is_empty())
    }));
    // Applied intents move to the in-flight queue and stay in the pending
    // snapshot until the post-turn checkpoint acknowledges them.
    let snapshot: Vec<String> = backing
        .pending_follow_up_intents_snapshot()
        .iter()
        .map(|intent| intent.text().to_string())
        .collect();
    assert_eq!(snapshot, vec!["first".to_string(), "second".to_string()]);
    assert_eq!(backing.deferred_follow_up_inputs(), Vec::<String>::new());
}

#[tokio::test]
async fn mid_turn_follow_up_provenance_comes_from_the_queued_intent() {
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::TRACKER_CONTINUE_FOLLOW_UP_PREFIX;

    let mut backing = TestTurnProcessingBacking::new().await;
    let (_sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    backing.set_steering_receiver(receiver);
    backing.queue_synthetic_follow_up_input_for_test("continue the plan");
    let user_text_with_harness_prefix = format!("{TRACKER_CONTINUE_FOLLOW_UP_PREFIX} this is user-authored");
    backing.queue_follow_up_input_for_test(&user_text_with_harness_prefix);

    let mut working_history = Vec::new();
    let mut result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
    {
        let mut ctx = backing.turn_loop_context();
        handle_steering_messages(&mut ctx, &mut working_history, &mut result)
            .await
            .expect("handle queued follow-ups");
    }

    let user_messages: Vec<_> = working_history
        .iter()
        .filter(|message| message.role == MessageRole::User)
        .collect();
    assert_eq!(user_messages.len(), 2);
    assert_eq!(user_messages[0].content.as_text(), "continue the plan");
    assert_eq!(user_messages[0].metadata.as_ref().and_then(|metadata| metadata.source()), Some("synthetic"));
    assert_eq!(user_messages[1].content.as_text(), user_text_with_harness_prefix.as_str());
    assert_eq!(user_messages[1].metadata.as_ref().and_then(|metadata| metadata.source()), Some("user_input"));
}

#[tokio::test]
async fn leftover_steering_intent_applies_without_channel_messages() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let (_sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    backing.set_steering_receiver(receiver);
    backing.queue_follow_up_input_for_test("leftover");

    let mut working_history = Vec::new();
    let mut result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
    let handled = {
        let mut ctx = backing.turn_loop_context();
        handle_steering_messages(&mut ctx, &mut working_history, &mut result)
            .await
            .expect("handle steering")
    };

    assert!(!handled);
    let steered: Vec<String> = working_history
        .iter()
        .filter(|message| message.role == MessageRole::User)
        .map(|message| message.content.as_text().to_string())
        .collect();
    assert_eq!(steered, vec!["leftover".to_string()]);
    assert_eq!(backing.deferred_follow_up_inputs(), Vec::<String>::new());
}

#[tokio::test]
async fn paused_steering_accepts_follow_up_before_resume() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender.send(SteeringMessage::Pause).expect("pause");
    sender
        .send(SteeringMessage::FollowUpInput("refine search".to_string()))
        .expect("follow-up");
    let resume_sender = sender.clone();
    let resume_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        resume_sender.send(SteeringMessage::Resume).expect("resume");
    });
    backing.set_steering_receiver(receiver);

    let mut working_history = Vec::new();
    let mut result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
    let handled = {
        let mut ctx = backing.turn_loop_context();
        handle_steering_messages(&mut ctx, &mut working_history, &mut result)
            .await
            .expect("handle paused steering")
    };
    resume_task.await.expect("resume task");

    assert!(!handled);
    assert!(matches!(result, TurnLoopResult::Completed { .. }));
    let steered: Vec<String> = working_history
        .iter()
        .filter(|message| message.role == MessageRole::User)
        .map(|message| message.content.as_text().to_string())
        .collect();
    assert_eq!(steered, vec!["refine search".to_string()]);
}

#[tokio::test]
async fn paused_steering_keeps_follow_up_after_resume_in_same_batch() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender.send(SteeringMessage::Pause).expect("pause");
    sender.send(SteeringMessage::Resume).expect("resume");
    sender
        .send(SteeringMessage::FollowUpInput("use the queued note".to_string()))
        .expect("follow-up");
    backing.set_steering_receiver(receiver);

    let mut working_history = Vec::new();
    let mut result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
    let handled = {
        let mut ctx = backing.turn_loop_context();
        handle_steering_messages(&mut ctx, &mut working_history, &mut result)
            .await
            .expect("handle paused steering batch")
    };

    assert!(!handled);
    assert!(matches!(result, TurnLoopResult::Completed { .. }));
    let steered: Vec<String> = working_history
        .iter()
        .filter(|message| message.role == MessageRole::User)
        .map(|message| message.content.as_text().to_string())
        .collect();
    assert_eq!(steered, vec!["use the queued note".to_string()]);
}

#[tokio::test]
async fn steering_stop_beats_queued_follow_up() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender
        .send(SteeringMessage::FollowUpInput("ignore me".to_string()))
        .expect("follow-up");
    sender.send(SteeringMessage::SteerStop).expect("stop");
    backing.set_steering_receiver(receiver);

    let mut working_history = Vec::new();
    let mut result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
    let handled = {
        let mut ctx = backing.turn_loop_context();
        handle_steering_messages(&mut ctx, &mut working_history, &mut result)
            .await
            .expect("handle stop steering")
    };

    assert!(handled);
    assert!(matches!(result, TurnLoopResult::Cancelled));
    assert!(working_history.is_empty());
    assert!(backing.deferred_follow_up_inputs().is_empty());
}
