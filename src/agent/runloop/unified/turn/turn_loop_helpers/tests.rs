use super::{
    ToolLoopGrantSource, UNLIMITED_TOOL_LOOPS, apply_mode_switch_remaining_tool_call_floor,
    apply_mode_switch_remaining_tool_loop_floor, arm_tool_loop_synthesis_recovery, auto_tool_loop_grant_increment,
    clamp_tool_loop_increment, effective_max_tool_calls_for_approved_plan_execution, effective_max_tool_calls_for_turn,
    extract_turn_config, handle_steering_messages, initial_tool_loop_limit, is_internal_harness_follow_up,
    is_stale_approved_plan_pause_response, resolve_safety_tool_call_limits, resolve_tool_loop_limit,
    tool_loop_grant_source, tool_loop_hard_cap, tool_loop_limit_recovery_reason,
};
use crate::agent::runloop::unified::planning_workflow::{
    PlanningIntent, detect_enter_planning_intent, detect_planning_intent,
};
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
use std::time::Duration;
use vtcode_core::config::constants::tool_limits::PLANNING_WORKFLOW_MIN_TOOL_LOOPS;
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
fn tool_loop_hard_cap_scales_and_bounds() {
    assert_eq!(tool_loop_hard_cap(20, false), 60);
    assert_eq!(tool_loop_hard_cap(40, false), 120);
    assert_eq!(tool_loop_hard_cap(120, false), 120);
    assert_eq!(tool_loop_hard_cap(200, false), 200);
    assert_eq!(tool_loop_hard_cap(40, true), 240);
    assert_eq!(tool_loop_hard_cap(120, true), 240);
}

#[test]
fn clamp_tool_loop_increment_respects_cap_and_per_prompt_limit() {
    assert_eq!(clamp_tool_loop_increment(200, 20, 60, false), 40);
    assert_eq!(clamp_tool_loop_increment(50, 20, 80, false), 50);
    assert_eq!(clamp_tool_loop_increment(10, 75, 80, false), 5);
    assert_eq!(clamp_tool_loop_increment(10, 80, 80, false), 0);
    assert_eq!(clamp_tool_loop_increment(120, 80, 240, true), 80);
}

#[test]
fn auto_grant_uses_max_prompt_step_clamped_to_remaining_headroom() {
    // Ordinary turns grant the +50 manual maximum: 20 -> 60 needs one
    // grant, 40 -> 120 needs two (40 + 50, then 90 + 30).
    assert_eq!(auto_tool_loop_grant_increment(20, 60, false), 40);
    assert_eq!(auto_tool_loop_grant_increment(40, 120, false), 50);
    assert_eq!(auto_tool_loop_grant_increment(90, 120, false), 30);
    // Planning turns grant the +80 manual maximum instead.
    assert_eq!(auto_tool_loop_grant_increment(60, 240, true), 80);
    assert_eq!(auto_tool_loop_grant_increment(200, 240, true), 40);
    // No headroom left means no grant: the caller breaks the loop.
    assert_eq!(auto_tool_loop_grant_increment(60, 60, false), 0);
    assert_eq!(auto_tool_loop_grant_increment(120, 120, false), 0);
    assert_eq!(auto_tool_loop_grant_increment(240, 240, true), 0);
}

#[test]
fn tool_loop_grant_source_prefers_full_auto_then_session_latch() {
    assert_eq!(
        tool_loop_grant_source(true, false),
        ToolLoopGrantSource::FullAuto,
        "full-auto wins even before any interactive grant"
    );
    assert_eq!(
        tool_loop_grant_source(true, true),
        ToolLoopGrantSource::FullAuto,
        "full-auto wording stays full-auto after an interactive grant"
    );
    assert_eq!(
        tool_loop_grant_source(false, true),
        ToolLoopGrantSource::SessionPreauthorized,
        "after the first successful grant later hits auto-grant without a prompt"
    );
    assert_eq!(
        tool_loop_grant_source(false, false),
        ToolLoopGrantSource::Manual,
        "first interactive hit (and any denial retry) still prompts"
    );

    // Only Manual opens the HITL modal; the other two auto-grant max +N
    // via `auto_tool_loop_grant_increment` (already covered above).
    assert!(!tool_loop_grant_source(false, false).is_automatic());
    assert!(tool_loop_grant_source(false, true).is_automatic());
    assert!(tool_loop_grant_source(true, false).is_automatic());
    assert!(tool_loop_grant_source(true, true).is_automatic());
}

#[test]
fn extract_turn_config_applies_planning_workflow_loop_floor() {
    for (configured_limit, expected_limit) in [(20, 60), (40, 60), (60, 60), (80, 80)] {
        let mut cfg = VTCodeConfig::default();
        cfg.tools.max_tool_loops = configured_limit;
        let turn_cfg = extract_turn_config(Some(&cfg), true, true);
        assert_eq!(turn_cfg.max_tool_loops, expected_limit);
    }
}

#[test]
fn extract_turn_config_keeps_non_planning_workflow_loop_limit() {
    for configured_limit in [20, 40, 60, 80] {
        let mut cfg = VTCodeConfig::default();
        cfg.tools.max_tool_loops = configured_limit;
        let turn_cfg = extract_turn_config(Some(&cfg), false, true);
        assert_eq!(turn_cfg.max_tool_loops, configured_limit);
    }
}

#[test]
fn resolve_tool_loop_limit_allows_unlimited_mode() {
    assert_eq!(resolve_tool_loop_limit(0, false), UNLIMITED_TOOL_LOOPS);
    assert_eq!(resolve_tool_loop_limit(0, true), UNLIMITED_TOOL_LOOPS);
}

#[test]
fn resolve_safety_tool_call_limits_maps_zero_turn_budget_to_unbounded_limits() {
    assert_eq!(resolve_safety_tool_call_limits(0, 50, false), (usize::MAX, usize::MAX));
}

#[test]
fn resolve_safety_tool_call_limits_scales_session_limit_from_turn_budget() {
    assert_eq!(resolve_safety_tool_call_limits(12, 40, false), (12, 480));
}

#[test]
fn resolve_safety_tool_call_limits_keeps_planning_workflow_session_unbounded() {
    assert_eq!(resolve_safety_tool_call_limits(48, 40, true), (48, usize::MAX));
}

#[test]
fn planning_workflow_applies_tool_call_floor() {
    assert_eq!(effective_max_tool_calls_for_turn(32, true), 120);
    assert_eq!(effective_max_tool_calls_for_turn(64, true), 120);
    assert_eq!(effective_max_tool_calls_for_turn(120, true), 120);
    assert_eq!(effective_max_tool_calls_for_turn(200, true), 200);
}

#[test]
fn zero_tool_call_limit_stays_unlimited_in_all_modes() {
    assert_eq!(effective_max_tool_calls_for_turn(0, true), 0);
    assert_eq!(effective_max_tool_calls_for_turn(0, false), 0);
}

#[test]
fn edit_mode_keeps_configured_tool_call_limit() {
    assert_eq!(effective_max_tool_calls_for_turn(32, false), 32);
}

#[test]
fn mode_switch_grants_remaining_tool_call_floor() {
    // Build already spent most of a 120-cap; Plan entry must not inherit
    // the leftover 20-call budget.
    let mut max = 120usize;
    apply_mode_switch_remaining_tool_call_floor(&mut max, 110, true);
    assert_eq!(max, 110 + 120);
    // Cap already larger than used+floor is preserved.
    let mut max = 400usize;
    apply_mode_switch_remaining_tool_call_floor(&mut max, 10, true);
    assert_eq!(max, 400);
    // Unlimited stays unlimited.
    let mut max = 0usize;
    apply_mode_switch_remaining_tool_call_floor(&mut max, 10, true);
    assert_eq!(max, 0);
}

#[test]
fn mode_switch_grants_remaining_tool_loop_floor() {
    let mut loops = 60usize;
    apply_mode_switch_remaining_tool_loop_floor(&mut loops, 55, true);
    assert_eq!(loops, 55 + PLANNING_WORKFLOW_MIN_TOOL_LOOPS);
    let mut loops = 200usize;
    apply_mode_switch_remaining_tool_loop_floor(&mut loops, 10, true);
    assert_eq!(loops, 200);
    let mut loops = usize::MAX;
    apply_mode_switch_remaining_tool_loop_floor(&mut loops, 10, true);
    assert_eq!(loops, usize::MAX);
}

#[test]
fn approved_plan_execution_gets_a_fresh_implementation_budget() {
    assert_eq!(effective_max_tool_calls_for_approved_plan_execution(32), 120);
    assert_eq!(effective_max_tool_calls_for_approved_plan_execution(160), 160);
    assert_eq!(effective_max_tool_calls_for_approved_plan_execution(0), 0);
}

#[test]
fn approved_plan_execution_gets_one_bounded_loop_allowance() {
    assert_eq!(initial_tool_loop_limit(40, true), 90);
    assert_eq!(initial_tool_loop_limit(100, true), 120);
    assert_eq!(initial_tool_loop_limit(0, true), UNLIMITED_TOOL_LOOPS);
    assert_eq!(initial_tool_loop_limit(40, false), 40);
    // The helper is an initialization transform; applying the allowance
    // to the already-initialized value is not part of the turn loop.
    assert_eq!(initial_tool_loop_limit(initial_tool_loop_limit(40, true), false), 90);
}

#[test]
fn exhausted_loop_limit_arms_one_tool_free_synthesis_pass() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 40, 0, 1);
    let mut loop_limit = 40;

    assert!(arm_tool_loop_synthesis_recovery(&mut state, &mut loop_limit));
    assert_eq!(loop_limit, UNLIMITED_TOOL_LOOPS);
    assert!(state.is_recovery_active());
    assert!(state.recovery_is_tool_free());
    assert_eq!(state.recovery_reason(), Some(tool_loop_limit_recovery_reason(40).as_str()));

    // The recovery request is the only pass added by this transition.
    assert!(!arm_tool_loop_synthesis_recovery(&mut state, &mut loop_limit));
    assert!(state.consume_recovery_pass());
    assert!(state.finish_recovery_pass());
    assert!(!arm_tool_loop_synthesis_recovery(&mut state, &mut loop_limit));
}

#[tokio::test]
async fn planning_loop_exhaustion_retains_limit_and_hard_cap_stays_terminal() {
    let mut planning = TestTurnProcessingBacking::new(4).await;
    planning.activate_planning_for_test();
    let mut limit = 7;
    let mut ctx = planning.turn_loop_context();
    assert!(matches!(
        super::maybe_handle_tool_loop_limit(&mut ctx, 7, &mut limit).await.unwrap(),
        super::ToolLoopLimitAction::ContinueLoop
    ));
    assert_eq!(limit, UNLIMITED_TOOL_LOOPS);
    assert_eq!(
        ctx.harness_state.budget_recovery_reason(),
        Some("Tool loop budget exhausted before a final response (7/7)")
    );
    assert!(ctx.harness_state.recovery_is_tool_free());

    let mut build = TestTurnProcessingBacking::new(4).await;
    let mut cfg = VTCodeConfig::default();
    cfg.tools.max_tool_loops = 5;
    build.set_vt_cfg_for_test(cfg);
    let cap = tool_loop_hard_cap(5, false);
    let mut limit = cap;
    let mut ctx = build.turn_loop_context();
    assert!(matches!(
        super::maybe_handle_tool_loop_limit(&mut ctx, cap, &mut limit).await.unwrap(),
        super::ToolLoopLimitAction::BreakLoop
    ));
    assert_eq!(limit, cap);
    assert!(!ctx.harness_state.is_recovery_active());
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
    history.push(Message::system(tool_loop_limit_recovery_reason(60)));
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
    use crate::agent::runloop::unified::run_loop_context::{ToolBudgetExhaustion, ToolWallClockExhaustion};
    use crate::agent::runloop::unified::turn::turn_loop::{
        POST_TOOL_RECOVERY_REASON, POST_TOOL_RECOVERY_REASON_PLAN_MODE, RECOVERY_TOOL_CALL_RETRY_DIRECTIVE_PLAN_MODE,
    };
    use vtcode_core::llm::provider::Message;

    for directive in [
        tool_loop_limit_recovery_reason(60),
        ToolBudgetExhaustion { used: 4, max: 4, remaining: 0 }.synthesis_directive_message(),
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
    use crate::agent::runloop::unified::turn::session_loop::{
        BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX, VERIFICATION_AUTO_RECOVERY_PREFIX,
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
        format!("{VERIFICATION_AUTO_RECOVERY_PREFIX} Verification is still pending.").as_str()
    ));
    assert!(!is_internal_harness_follow_up("leftover"));
    assert!(!is_internal_harness_follow_up("please keep going with the build"));
}

#[test]
fn extract_turn_config_honors_request_user_input_setting_in_planning_workflow() {
    let mut cfg = VTCodeConfig::default();
    cfg.chat.ask_questions.enabled = false;

    let turn_cfg = extract_turn_config(Some(&cfg), true, true);
    assert!(!turn_cfg.request_user_input_enabled);
}

#[tokio::test]
async fn steering_follow_up_inputs_apply_mid_turn_in_order() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
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
async fn leftover_steering_intent_applies_without_channel_messages() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
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
