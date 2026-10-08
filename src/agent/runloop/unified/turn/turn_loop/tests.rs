use super::post_tool_recovery::complete_turn_after_failed_tool_free_recovery;
use super::post_tool_recovery::prepare_post_tool_tool_free_recovery;
use super::post_tool_recovery::{ensure_post_tool_resume_directive, has_tool_response_since};
use super::{
    COMPLETED_TURN_FALLBACK_RESPONSE, GENERIC_BLOCKED_FINAL_RESPONSE, HarnessUsage, PENDING_VERIFICATION_BLOCK_REASON,
    PLANNING_COMPLETED_FALLBACK_RESPONSE, PLANNING_RECOVERY_SYNTHESIS_FALLBACK,
    POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON, POST_TOOL_RECOVERY_REASON, POST_TOOL_RECOVERY_REASON_PLAN_MODE,
    POST_TOOL_RESUME_DIRECTIVE, POST_TOOL_TOOL_ENABLED_RETRY_DIRECTIVE, PostToolFailureRecovery,
    RECOVERY_CONTRACT_VIOLATION_REASON, RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER, accumulate_turn_usage,
    blocked_turn_final_response, completed_turn_requires_final_response, current_turn_preserve_index,
    ensure_blocked_turn_response, ensure_completed_turn_response, finalize_turn, has_turn_usage,
    maybe_recover_after_post_tool_llm_failure, normalize_tool_free_recovery_break_outcome, run_turn_loop,
};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn interrupted_provider_follow_up_stops_after_completed_tool_without_recovery() {
    struct InterruptedProvider {
        requests: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl uni::LLMProvider for InterruptedProvider {
        fn name(&self) -> &str {
            "openai"
        }
        fn supports_streaming(&self) -> bool {
            false
        }
        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let call = self.requests.fetch_add(1, Ordering::SeqCst);
            if call == 1 {
                return Err(uni::LLMError::Provider {
                    message: "Provider error: Interrupted by user".to_owned(),
                    metadata: None,
                });
            }
            let path = if call == 0 {
                "completed.txt"
            } else {
                "late-mutation.txt"
            };
            Ok(uni::LLMResponse {
                content: None,
                model: request.model,
                tool_calls: Some(vec![uni::ToolCall::function(
                    "write".to_owned(),
                    tool_names::APPLY_PATCH.to_owned(),
                    json!({"input": format!("*** Begin Patch\n*** Add File: {path}\n+retained\n*** End Patch\n")})
                        .to_string(),
                )]),
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
            vec!["noop-model".to_owned()]
        }
        fn validate_request(&self, _: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }
    let requests = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(8).await;
    let harness_path = backing.enable_harness_emitter();
    backing.set_provider(Box::new(InterruptedProvider { requests: requests.clone() }));
    let mut history = vec![uni::Message::user("write the file".to_owned())];
    let context = backing.turn_loop_context();
    let emitter = context.harness_emitter.cloned().unwrap();
    let outcome = run_turn_loop(&mut history, context).await.expect("cancelled turn");
    emitter.finish().await.unwrap();
    assert!(matches!(outcome.result, TurnLoopResult::Cancelled));
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert_eq!(fs::read_to_string(backing.workspace_path().join("completed.txt")).unwrap(), "retained\n");
    assert!(!backing.workspace_path().join("late-mutation.txt").exists());
    assert!(
        !history
            .iter()
            .any(|m| m.role == uni::MessageRole::System && m.content.as_text().contains(POST_TOOL_RESUME_DIRECTIVE))
    );
    assert!(!outcome.final_response_was_fallback);
    let events = fs::read_to_string(harness_path).unwrap();
    assert_eq!(events.matches("turn.failed").count(), 1, "{events}");
    assert!(!events.contains("turn.blocked") && !events.contains("full_auto_continuation"));
}

#[test]
fn provider_cancellation_and_shared_exit_precede_other_errors() {
    let state = crate::agent::runloop::unified::state::CtrlCState::new();
    let error = anyhow::anyhow!("Interrupted by user");
    assert!(matches!(super::provider_stop_outcome(&state, &error), Some(TurnLoopResult::Cancelled)));
    assert!(super::provider_stop_outcome(&state, &anyhow::anyhow!("invalid schema")).is_none());
    state.request_local_cancel();
    assert!(matches!(
        super::provider_stop_outcome(&state, &anyhow::anyhow!("invalid schema")),
        Some(TurnLoopResult::Cancelled)
    ));
    state.request_exit();
    assert!(matches!(super::provider_stop_outcome(&state, &error), Some(TurnLoopResult::Exit)));
}

use crate::agent::runloop::unified::planning_workflow::recovery::{
    PLANNING_SYNTHESIS_TRUNCATED_CONDENSE_DIRECTIVE, plan_synthesis_was_truncated,
};
use crate::agent::runloop::unified::planning_workflow::{
    PlanApprovalRequestContext, PlanApprovalTelemetryContext, PlanExecutionContext, execute_plan_approval,
    load_plan_text_for_approval, persist_plan_draft,
};
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::turn::context::TurnHandlerOutcome;
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
use anyhow::anyhow;
use serde_json::json;
use vtcode_config::{builtin_primary_auto_agent, builtin_primary_build_agent};
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::exec::events::{ThreadEvent, ThreadItemDetails, ToolCallStatus, VersionedThreadEvent};
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::{
    InlineEvent, InlineHandle, InlineListSelection, InlineSession, TransientEvent, TransientSubmission,
};

const PENDING_VERIFICATION_RESPONSE_MARKER: &str = "Inspection-only checks do not clear the verification gate";
const CONTEXT_CAPACITY_RESPONSE_MARKER: &str = "context capacity or compaction failed";

const STREAMED_VALID_PLAN: &str = r#"# Streamed plan

## Summary
Preserve the streamed planning handoff.

## Implementation Steps
1. Keep the semantic plan -> files: [src/agent/runloop/unified/ui_interaction_stream.rs] -> verify: [target/release/vtcode --version, cargo nextest run -p vtcode --bin vtcode]

## Test Cases and Validation
1. Run the focused streamed-plan regression tests.

## Assumptions and Defaults
1. Preserve the existing response-processing and approval flow.
"#;

const STREAMED_INVALID_PLAN: &str = r#"# Invalid streamed plan

## Summary
This draft is missing concrete step evidence.

## Implementation Steps
1. Do the thing

## Test Cases and Validation
1. Run the focused planning test.

## Assumptions and Defaults
1. Preserve the existing behavior.
"#;

#[derive(Clone, Copy)]
enum StreamedPlanScript {
    Valid,
    InvalidThenProse,
    InvalidThenValid,
    InvalidThenInvalidThenProse,
    ProseThenInvalidThenValid,
    ThreeInvalid,
}

struct StreamedPlanProvider {
    script: StreamedPlanScript,
    calls: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl uni::LLMProvider for StreamedPlanProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn generate(&self, _request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
        Err(uni::LLMError::Provider {
            message: "streamed plan test provider should not use generate".to_string(),
            metadata: None,
        })
    }

    async fn stream(&self, request: uni::LLMRequest) -> Result<uni::LLMStream, uni::LLMError> {
        let call_number = self.calls.fetch_add(1, Ordering::SeqCst);
        let plan = match self.script {
            StreamedPlanScript::Valid => Some(STREAMED_VALID_PLAN),
            StreamedPlanScript::InvalidThenProse if call_number == 0 => Some(STREAMED_INVALID_PLAN),
            StreamedPlanScript::InvalidThenProse => None,
            StreamedPlanScript::InvalidThenValid if call_number == 0 => Some(STREAMED_INVALID_PLAN),
            StreamedPlanScript::InvalidThenValid => Some(STREAMED_VALID_PLAN),
            StreamedPlanScript::InvalidThenInvalidThenProse if call_number < 2 => Some(STREAMED_INVALID_PLAN),
            StreamedPlanScript::InvalidThenInvalidThenProse => None,
            StreamedPlanScript::ProseThenInvalidThenValid if call_number == 0 => None,
            StreamedPlanScript::ProseThenInvalidThenValid if call_number == 1 => Some(STREAMED_INVALID_PLAN),
            StreamedPlanScript::ProseThenInvalidThenValid => Some(STREAMED_VALID_PLAN),
            StreamedPlanScript::ThreeInvalid => Some(STREAMED_INVALID_PLAN),
        };
        let completion_content = plan
            .is_none()
            .then(|| "The repair response did not contain a plan.".to_string());
        let completion = uni::LLMResponse {
            content: completion_content,
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
        };
        let stream = async_stream::stream! {
            if let Some(plan) = plan {
                yield Ok(uni::LLMStreamEvent::Token { delta: "Research complete.\n<propo".to_string() });
                yield Ok(uni::LLMStreamEvent::Token {
                    delta: format!("sed_plan>\n{plan}\n</proposed_plan>\n"),
                });
            }
            yield Ok(uni::LLMStreamEvent::Completed { response: Box::new(completion) });
        };
        Ok(Box::pin(stream))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["noop-model".to_string()]
    }

    fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

fn final_answer_text(history: &[uni::Message]) -> String {
    history
        .iter()
        .filter(|message| message.phase == Some(uni::AssistantPhase::FinalAnswer))
        .map(|message| message.content.as_text())
        .next_back()
        .expect("blocked turn should retain a final assistant response")
        .to_string()
}

fn assert_blocked_response_surfaces(
    backing: &mut TestTurnProcessingBacking,
    history: &[uni::Message],
    harness_path: &Path,
    response_marker: &str,
) {
    let response = final_answer_text(history);
    assert!(response.contains(response_marker), "unexpected blocked response: {response}");
    assert_eq!(
        history
            .iter()
            .filter(|message| message.phase == Some(uni::AssistantPhase::FinalAnswer))
            .count(),
        1,
        "blocked recovery must append exactly one final assistant message"
    );

    let rendered = backing.rendered_inline_output();
    assert_eq!(
        rendered.matches(response_marker).count(),
        1,
        "blocked recovery must render the final response exactly once: {rendered}"
    );

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("blocked recovery harness output should use the versioned event contract")
                .into_event()
        })
        .collect::<Vec<_>>();
    let agent_messages = events
        .iter()
        .filter(|event| {
            let ThreadEvent::ItemCompleted(item) = event else {
                return false;
            };
            let ThreadItemDetails::AgentMessage(message) = &item.item.details else {
                return false;
            };
            message.text.contains(response_marker)
        })
        .count();
    assert_eq!(agent_messages, 1, "blocked recovery must emit one agent_message item: {harness}");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ThreadEvent::TurnFailed(_)))
            .count(),
        1,
        "blocked recovery must emit one turn.failed event: {harness}"
    );
    assert!(
        !events.iter().any(|event| matches!(event, ThreadEvent::TurnCompleted(_))),
        "blocked recovery must not emit turn.completed: {harness}"
    );
}

#[test]
fn current_turn_preserve_index_keeps_user_request_before_transient_notes() {
    let history = vec![
        uni::Message::assistant("older response".to_string()),
        uni::Message::user("apply the requested fix".to_string()),
        uni::Message::system("transient recovery note".to_string()),
    ];

    assert_eq!(current_turn_preserve_index(&history, history.len()), 1);
}

#[test]
fn recovery_synthesis_fallback_says_no_tool_call_was_applied() {
    assert!(RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER.contains("no tool call applied"));
}

#[test]
fn approved_plan_handoff_bypasses_final_response_guard() {
    let approved_handoff = TurnLoopResult::Completed { plan_approved_execution_pending: true };
    let ordinary_completion = TurnLoopResult::Completed { plan_approved_execution_pending: false };

    assert!(!completed_turn_requires_final_response(&approved_handoff, true));
    assert!(!completed_turn_requires_final_response(&approved_handoff, false));
    assert!(completed_turn_requires_final_response(&ordinary_completion, false));
    // Plan-entry handoff completes with `plan_approved_execution_pending: false`
    // but is still a control-flow turn: the tool result is its terminal output.
    assert!(!completed_turn_requires_final_response(&ordinary_completion, true));
}

#[test]
fn plan_entry_handoff_is_not_approved_plan_execution() {
    use super::is_plan_entry_handoff;

    assert!(is_plan_entry_handoff("plan"));
    assert!(is_plan_entry_handoff("Plan"));
    assert!(!is_plan_entry_handoff("build"));
    assert!(!is_plan_entry_handoff("auto"));
}

#[test]
fn deferred_plan_entry_switch_counts_as_primary_agent_handoff() {
    use super::is_primary_agent_handoff;
    use crate::agent::runloop::unified::planning_workflow::{PlanExecutionContext, PlanExecutionTarget};

    let none: Option<String> = None;
    let plan = Some("plan".to_string());
    let target = PlanExecutionTarget::build(PlanExecutionContext::Current, false);
    assert!(is_primary_agent_handoff(&plan, &None, false));
    assert!(
        is_primary_agent_handoff(&none, &None, true),
        "deferred plan entry must exempt the final-response guard"
    );
    assert!(!is_primary_agent_handoff(&none, &None, false));
    // A stronger handoff already owns the boundary even without the switch.
    assert!(is_primary_agent_handoff(&none, &Some(target), false));
}

#[test]
fn completed_fallback_reason_preserves_planning_specificity() {
    use super::completed_fallback_reason;

    let planning = completed_fallback_reason(true);
    assert!(
        planning.contains("planning remains active"),
        "planning fallback must name planning state: {planning}"
    );
    assert!(
        planning.contains("approval-ready plan"),
        "planning fallback must name approval readiness: {planning}"
    );

    let generic = completed_fallback_reason(false);
    assert!(generic.contains("recovery fallback"), "non-planning fallback must keep generic reason: {generic}");
    assert_ne!(planning, generic);
}

#[test]
fn planning_recovery_prompt_constants_keep_inspection_verify_examples() {
    use super::{
        PLANNING_COMPLETED_FALLBACK_RESPONSE, POST_TOOL_RECOVERY_REASON_PLAN_MODE,
        RECOVERY_TOOL_CALL_RETRY_DIRECTIVE_PLAN_MODE,
    };

    for (name, text) in [
        ("completed-fallback", PLANNING_COMPLETED_FALLBACK_RESPONSE),
        ("post-tool-plan-mode", POST_TOOL_RECOVERY_REASON_PLAN_MODE),
        ("tool-call-retry-plan-mode", RECOVERY_TOOL_CALL_RETRY_DIRECTIVE_PLAN_MODE),
    ] {
        assert!(text.contains("sed -n") && text.contains("grep -n"), "{name} missing inspection examples: {text}");
        assert!(text.contains("git diff --check"), "{name} missing invalid VCS example: {text}");
    }
}

#[tokio::test]
async fn approved_plan_handoff_without_assistant_response_stays_completed() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.persist_plan_for_test(STREAMED_VALID_PLAN).await;
    let harness_path = backing.enable_harness_emitter();

    let mut history = vec![uni::Message::user("yes".to_string())];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("approved plan should hand off without a synthesis response");

    assert!(outcome.plan_approved_execution_pending);
    assert!(matches!(outcome.result, TurnLoopResult::Completed { plan_approved_execution_pending: true }));
    assert!(!outcome.final_response_was_fallback);

    let harness = fs::read_to_string(harness_path).expect("read approved-plan handoff events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("approved-plan handoff harness output should be versioned")
                .into_event()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ThreadEvent::TurnCompleted(_)))
            .count(),
        1,
        "approved-plan handoff must emit exactly one control-flow completion"
    );
    assert!(
        !events.iter().any(|event| matches!(event, ThreadEvent::TurnFailed(_))),
        "approved-plan handoff must not be reported as blocked"
    );
    assert!(
        !events.iter().any(|event| {
            matches!(event, ThreadEvent::ItemCompleted(item) if matches!(item.item.details, ThreadItemDetails::AgentMessage(_)))
        }),
        "approved-plan handoff must not synthesize a final assistant item"
    );
}

#[test]
fn has_tool_response_since_detects_new_tool_message() {
    let messages = vec![
        uni::Message::user("run script".to_string()),
        uni::Message::assistant("".to_string()),
        uni::Message::tool_response("call_1".to_string(), "ok".to_string()),
    ];

    assert!(has_tool_response_since(&messages, 1));
}

#[test]
fn has_tool_response_since_ignores_non_tool_messages() {
    let messages = vec![
        uni::Message::user("hello".to_string()),
        uni::Message::assistant("done".to_string()),
    ];

    assert!(!has_tool_response_since(&messages, 0));
}

#[test]
fn has_tool_response_since_handles_baseline_past_end() {
    let messages = vec![uni::Message::tool_response("call_1".to_string(), "ok".to_string())];

    assert!(!has_tool_response_since(&messages, 10));
}

#[test]
fn ensure_post_tool_resume_directive_is_idempotent_near_history_tail() {
    let mut history = vec![
        uni::Message::user("run cargo nextest".to_string()),
        uni::Message::tool_response("call_1".to_string(), "{\"success\":false}".to_string()),
    ];

    ensure_post_tool_resume_directive(&mut history);
    ensure_post_tool_resume_directive(&mut history);

    let directive_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RESUME_DIRECTIVE
        })
        .count();
    assert_eq!(directive_count, 1);
}

#[test]
fn prepare_post_tool_tool_free_recovery_is_idempotent_near_history_tail() {
    let mut history = vec![
        uni::Message::user("summarize the existing tool outputs".to_string()),
        uni::Message::tool_response("call_1".to_string(), "{\"ok\":true}".to_string()),
    ];

    prepare_post_tool_tool_free_recovery(&mut history, POST_TOOL_RECOVERY_REASON);
    prepare_post_tool_tool_free_recovery(&mut history, POST_TOOL_RECOVERY_REASON);

    // The resume directive must NOT be injected for tool-free recovery: it
    // instructs the model to follow tool-output guidance, contradicting the
    // tools-disabled synthesis contract.
    let resume_directive_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RESUME_DIRECTIVE
        })
        .count();
    assert_eq!(resume_directive_count, 0);

    let recovery_reason_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RECOVERY_REASON
        })
        .count();
    assert_eq!(recovery_reason_count, 1);
}

#[test]
fn retryable_post_tool_follow_up_failure_schedules_one_tool_enabled_recovery() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    let mut history = vec![
        uni::Message::user("run cargo nextest".to_string()),
        uni::Message::assistant("".to_string()),
        uni::Message::tool_response("call_1".to_string(), "{\"critical_note\":\"reuse output\"}".to_string()),
    ];

    let action = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut history,
        &anyhow!("Network error"),
        2,
        1,
        "streaming",
        true,
        true,
        false,
    )
    .expect("recovery should succeed");
    assert_eq!(action, PostToolFailureRecovery::RetryToolEnabled);

    let action_again = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut history,
        &anyhow!("Network error"),
        3,
        1,
        "streaming",
        false,
        false,
        false,
    )
    .expect("repeat recovery should succeed");
    assert_eq!(action_again, PostToolFailureRecovery::StopAfterDirective);

    let enabled_retry_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text() == POST_TOOL_TOOL_ENABLED_RETRY_DIRECTIVE
        })
        .count();
    assert_eq!(enabled_retry_count, 1);

    let directive_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RESUME_DIRECTIVE
        })
        .count();
    assert_eq!(directive_count, 1);

    let recovery_reason_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RECOVERY_REASON
        })
        .count();
    assert_eq!(recovery_reason_count, 0);
}

#[test]
fn plan_mode_recovery_uses_plan_aware_directive() {
    // In plan mode the tool-free recovery pass must inject the plan-aware
    // reason (which demands a `<proposed_plan>` from gathered research) instead
    // of the generic "respond with text" reason — otherwise the model treats
    // the pass as another research step and emits `<invoke>` tool-call markup
    // instead of finalizing the plan (checkpoints turn_648 / turn_650).
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    let mut history = vec![
        uni::Message::user("plan launch-time optimization".to_string()),
        uni::Message::assistant("".to_string()),
        uni::Message::tool_response("call_1".to_string(), "{\"ok\":true}".to_string()),
    ];

    let action = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut history,
        &anyhow!("Network error"),
        2,
        1,
        "streaming",
        true,
        false,
        true,
    )
    .expect("recovery should succeed");
    assert_eq!(action, PostToolFailureRecovery::RetryToolFree);

    let plan_directive_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RECOVERY_REASON_PLAN_MODE
        })
        .count();
    assert_eq!(plan_directive_count, 1);

    // The generic reason must NOT be injected in plan mode.
    let generic_directive_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RECOVERY_REASON
        })
        .count();
    assert_eq!(generic_directive_count, 0);
}

#[test]
fn retryable_post_tool_follow_up_failure_stops_after_recovery_pass_is_spent() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    let mut history = vec![
        uni::Message::user("summarize the tool output".to_string()),
        uni::Message::assistant("".to_string()),
        uni::Message::tool_response("call_1".to_string(), "{\"ok\":true}".to_string()),
    ];

    let action = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut history,
        &anyhow!("Network error"),
        2,
        1,
        "streaming",
        false,
        false,
        false,
    )
    .expect("recovery classification should succeed");

    assert_eq!(action, PostToolFailureRecovery::StopAfterDirective);
    assert!(!history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RECOVERY_REASON
    }));
    // Turn-ending path keeps the resume directive for the next turn.
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RESUME_DIRECTIVE
    }));
}

#[test]
fn post_tool_follow_up_failure_chain_consumes_tool_free_recovery_pass() {
    // End-to-end regression guard for the infinite loop: starting from a fresh
    // (non-recovery) turn state (phase == Inactive), a retryable post-tool
    // follow-up failure must schedule a tool-free recovery pass that is
    // actually consumable. Before the fix, `switch_to_tool_free_recovery`
    // left the phase as Inactive, so `consume_recovery_pass()` returned false,
    // `tool_free_recovery` evaluated to false, and tools were never disabled
    // at the API level — the model kept emitting tool calls and the follow-up
    // kept failing, looping until the wall-clock timeout.
    use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    let mut history = vec![
        uni::Message::user("run cargo nextest".to_string()),
        uni::Message::assistant("".to_string()),
        uni::Message::tool_response("call_1".to_string(), "{\"critical_note\":\"reuse output\"}".to_string()),
    ];

    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);
    // Fresh turn: recovery is inactive.
    assert!(!state.is_recovery_active());

    // Simulate the turn-loop error path: the follow-up LLM phase failed after
    // tool execution. `allow_tool_free_retry = !tool_free_recovery = true`
    // because this is a non-recovery turn.
    let action = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut history,
        &anyhow!("Network error"),
        2,
        1,
        "execute_llm_request",
        true,
        false,
        false,
    )
    .expect("recovery classification should succeed");
    assert_eq!(action, PostToolFailureRecovery::RetryToolFree);

    // The caller (turn_loop.rs) then switches to tool-free recovery. Before
    // the fix this was a no-op on the phase because it was Inactive.
    assert!(state.switch_to_tool_free_recovery());

    // The next loop iteration consumes the pass — this is the gate that
    // decides `tool_free_recovery = true` and disables tools at the API level.
    assert!(state.consume_recovery_pass(), "consume_recovery_pass must succeed after switch from Inactive");
    assert!(state.recovery_is_tool_free());
}

#[tokio::test]
async fn empty_model_response_after_recovery_is_visible_and_blocked() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_tool_free_recovery_for_test("post-tool follow-up failure");
    let mut history = vec![uni::Message::user("summarize the tool outputs".to_string())];

    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("recovery should produce a visible fallback");

    assert!(matches!(outcome.result, TurnLoopResult::Blocked { .. }));
    assert!(outcome.final_response_was_fallback);
    let final_text = history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::Assistant)
        .map(|message| message.content.as_text().trim().to_string())
        .unwrap_or_default();
    assert!(!final_text.is_empty(), "recovery must not leave an empty final response");
}

#[tokio::test]
async fn exhausted_budget_survives_empty_recovery_and_final_publication() {
    use crate::agent::runloop::unified::run_loop_context::{ToolBudgetExhaustion, ToolWallClockExhaustion};

    for (directive, cause) in [
        ("Tool loop budget exhausted before a final response (60/60). Tools are disabled for one bounded synthesis pass.".to_string(), "Tool loop budget exhausted before a final response (60/60)"),
        (ToolBudgetExhaustion { used: 32, max: 32, remaining: 0 }.synthesis_directive_message(), "Tool-call budget exhausted for this turn (32/32)"),
        (ToolWallClockExhaustion { max_secs: 73 }.synthesis_directive_message(), "Tool wall-clock budget exhausted for this turn (73s)"),
    ] {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let harness_path = backing.enable_harness_emitter();
        backing.activate_tool_free_recovery_for_test(&directive);
        let mut history = vec![uni::Message::user("continue the pending change".to_string())];
        let outcome = run_turn_loop(&mut history, backing.turn_loop_context()).await.unwrap();
        let TurnLoopResult::Blocked { reason: Some(reason) } = outcome.result else { panic!("expected budget handoff") };
        assert!(reason.starts_with(cause), "{reason}");
        assert!(outcome.final_response_was_fallback);
        let text = final_answer_text(&history);
        assert!(text.contains(&cause.replacen("Tool", "tool", 1)), "{text}");
        assert!(text.contains("retained conversation") && text.contains("verification"));
        assert!(!text.contains("Re-state") && !text.contains("please retry"), "{text}");
        assert_blocked_response_surfaces(&mut backing, &history, &harness_path, &cause.replacen("Tool", "tool", 1));
        let harness = fs::read_to_string(harness_path).unwrap();
        assert!(harness.contains(cause), "canonical event must retain budget cause: {harness}");
    }
}

#[tokio::test]
async fn planning_budget_survives_failed_synthesis_on_all_final_surfaces() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    let path = backing.enable_harness_emitter();
    let cause = "Tool loop budget exhausted before a final response (7/7)";
    backing.activate_tool_free_recovery_for_test(&format!("{cause}. Tools are disabled for synthesis."));
    let mut history = vec![uni::Message::user("plan the pending change".to_string())];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context()).await.unwrap();
    let TurnLoopResult::Blocked { reason: Some(reason) } = outcome.result else {
        panic!("expected resumable planning handoff");
    };
    assert!(reason.starts_with(cause), "{reason}");
    assert!(reason.contains("planning remains active"));
    assert!(outcome.final_response_was_fallback);
    assert!(final_answer_text(&history).contains("Planning remains active"));
    assert_blocked_response_surfaces(&mut backing, &history, &path, cause);
}

#[tokio::test]
async fn budget_fallback_keeps_salvage_and_planning_state_without_duplicate_finals() {
    for planning in [false, true] {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        if planning {
            backing.activate_planning_for_test();
        }
        backing.activate_tool_free_recovery_for_test(
            "Tool-call budget exhausted for this turn (32/32). Tools are disabled for the rest of this turn.",
        );
        let mut history = vec![
            uni::Message::user("implement the pending step".to_string()),
            uni::Message::assistant("Parser updated; validation remains pending.".to_string())
                .with_phase(Some(uni::AssistantPhase::FinalAnswer)),
        ];
        let mut ctx = backing.turn_loop_context();
        ctx.harness_state.mark_final_response_fallback();
        assert!(ensure_completed_turn_response(&mut ctx, &mut history, 1).unwrap());
        assert!(ensure_completed_turn_response(&mut ctx, &mut history, 1).unwrap());
        assert_eq!(history.len(), 2);
        let text = history[1].content.as_text();
        assert_eq!(text.matches("tool-call budget exhausted").count(), 1);
        assert!(text.contains("Parser updated; validation remains pending."));
        assert_eq!(text.contains("planning remains active"), planning);
        let reason = super::completed_recovery_fallback_reason(planning, ctx.harness_state);
        assert!(reason.starts_with("Tool-call budget exhausted for this turn (32/32)."));
        assert_eq!(reason.contains("planning remains active"), planning);
    }
}

#[tokio::test]
async fn genuine_synthesis_after_budget_recovery_is_not_rewritten() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_tool_free_recovery_for_test(
        "Tool-call budget exhausted for this turn (32/32). Tools are disabled for the rest of this turn.",
    );
    let answer = "The requested analysis is complete; no edits were needed.";
    let mut history =
        vec![uni::Message::assistant(answer.to_string()).with_phase(Some(uni::AssistantPhase::FinalAnswer))];
    let mut ctx = backing.turn_loop_context();
    assert!(!ensure_completed_turn_response(&mut ctx, &mut history, 0).unwrap());
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content.as_text(), answer);
    assert!(!ctx.harness_state.final_response_was_fallback());
}

#[test]
fn blocked_turn_final_response_explains_pending_verification() {
    let response = blocked_turn_final_response(PENDING_VERIFICATION_BLOCK_REASON);

    assert!(response.contains("Inspection-only checks do not clear the verification gate"));
    assert!(response.contains("cargo check --locked"));
    assert!(response.contains(vtcode_core::tools::tool_intent::GENERIC_VERIFIER_DESCRIPTION));
    assert!(response.contains(vtcode_core::tools::tool_intent::VERIFIER_SHELL_FORM_NOTE));
    assert!(response.contains("autonomous recovery"));
    assert!(response.contains("max_output_tokens"));
    assert!(response.contains("continue"));
    // The block fires whether or not a project verifier was detected, so the
    // notice must not claim the harness already ran one, and must not restate
    // a stricter pipe rule than the classifier applies.
    assert!(!response.contains("already tried"), "{response}");
    assert!(!response.contains("no `|`"), "{response}");
}

#[test]
fn blocked_turn_final_response_explains_context_capacity_failure() {
    let response =
        blocked_turn_final_response(&format!("recovery failed: {POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON}"));

    assert!(response.contains("context capacity"));
    assert!(response.contains("retained"));
    assert!(response.contains("resume"));
    assert!(response.contains("switch models"));
}

#[test]
fn blocked_turn_final_response_is_not_suppressed_by_prior_event_state() {
    // A prior streamed event is not evidence that this blocked outcome has a
    // final assistant item; the blocked handoff must still be selected.
    assert!(
        blocked_turn_final_response(&format!("prior event; {PENDING_VERIFICATION_BLOCK_REASON}"))
            .contains("Inspection-only checks")
    );
}

#[test]
fn blocked_turn_final_response_has_generic_fallback() {
    let response = blocked_turn_final_response("blocked");

    assert_eq!(response, GENERIC_BLOCKED_FINAL_RESPONSE);

    let empty_response = blocked_turn_final_response("");
    assert_eq!(empty_response, GENERIC_BLOCKED_FINAL_RESPONSE);
}

#[test]
fn blocked_turn_final_response_formats_tool_call_limit() {
    let response = blocked_turn_final_response("Blocked tool-call limit reached after 4 calls");
    assert_eq!(
        response,
        "The turn stopped because repeated tool calls were rejected: blocked tool-call limit reached after 4 calls. \
         Resume the request with specific guidance, or adjust permissions or tools to continue."
    );
}

#[test]
fn blocked_turn_final_response_formats_repeated_shell() {
    let response = blocked_turn_final_response("Repeated shell command detected");
    assert!(response.contains("repeated identical shell commands were detected"));
    assert!(!response.contains("retained"));
}

#[test]
fn blocked_turn_final_response_formats_specific_reason() {
    let response = blocked_turn_final_response("specific error happened");
    assert_eq!(
        response,
        "The turn stopped: specific error happened. Resume the request or give updated instructions to continue."
    );
}

#[test]
fn blocked_turn_final_response_reads_cleanly_for_sentence_reasons() {
    // Reasons are full sentences; the notice must not produce "..", keep a
    // capital after the colon, or claim anything about retained history.
    for reason in [
        super::ASSISTANT_TEXT_RESPONSE_CAP_REASON,
        "Provider stream ended unexpectedly.",
        "Blocked tool-call limit reached.",
    ] {
        let response = blocked_turn_final_response(reason);
        assert!(!response.contains(".."), "double period in: {response}");
        assert!(!response.contains(": T") && !response.contains(": P") && !response.contains(": B"), "{response}");
        assert!(!response.contains("before success could be confirmed"), "{response}");
        assert!(!response.contains("retained"), "{response}");
    }
    assert!(!GENERIC_BLOCKED_FINAL_RESPONSE.contains("retained"));
    assert!(!GENERIC_BLOCKED_FINAL_RESPONSE.contains("before success could be confirmed"));
}

#[test]
fn blocked_turn_final_response_keeps_acronym_case_after_colon() {
    let response = blocked_turn_final_response("API quota exhausted");
    assert!(response.starts_with("The turn stopped: API quota exhausted."), "{response}");
}

#[test]
fn blocked_turn_final_response_returns_refusal_notice_verbatim() {
    let reason = vtcode_core::core::agent::refusal::refusal_reason(&uni::LLMResponse {
        finish_reason: uni::FinishReason::Refusal,
        reasoning_details: Some(vec![
            serde_json::json!({
                "type": "stop_details",
                "category": "cyber",
                "explanation": "This looks like malware development."
            })
            .to_string(),
        ]),
        ..uni::LLMResponse::default()
    });

    let response = blocked_turn_final_response(&reason);

    assert_eq!(
        response,
        "The model declined this request (category: cyber): this looks like malware development. \
         The request was not retried; rephrase it or switch models."
    );
}

#[tokio::test]
async fn complete_turn_after_failed_tool_free_recovery_appends_fallback_once() {
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        Some(&anyhow!("Network error")),
        None,
        None,
        None,
    )
    .await;
    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    let fallback_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && message.content.as_text() == RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER
        })
        .count();
    assert_eq!(fallback_count, 1);

    let outcome_again =
        complete_turn_after_failed_tool_free_recovery(&mut history, "test.stage", None, None, None, None).await;
    assert!(matches!(outcome_again, TurnLoopResult::Completed { .. }));
    let fallback_count_again = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && message.content.as_text() == RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER
        })
        .count();
    assert_eq!(fallback_count_again, 1);
}

#[tokio::test]
async fn complete_turn_after_failed_tool_free_recovery_prefers_salvaged_prose() {
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        None,
        Some("Here is the launch-time plan: reduce config IO.".to_string()),
        None,
        None,
    )
    .await;
    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    let last = history.last().unwrap();
    assert_eq!(last.role, uni::MessageRole::Assistant);
    assert_eq!(last.phase, Some(uni::AssistantPhase::FinalAnswer));
    let text = last.content.as_text();
    assert!(text.contains("reduce config IO"));
    assert!(text != RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER);

    // Whitespace-only salvage falls back to the canned answer.
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        None,
        Some("   \n".to_string()),
        None,
        None,
    )
    .await;
    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    assert_eq!(history.last().unwrap().content.as_text(), RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER);
}

#[tokio::test]
async fn normalize_tool_free_recovery_break_outcome_converts_contract_violation_to_completed() {
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = normalize_tool_free_recovery_break_outcome(
        &mut history,
        TurnLoopResult::Blocked {
            reason: Some(RECOVERY_CONTRACT_VIOLATION_REASON.to_string()),
        },
        true,
        None,
        None,
        None,
    )
    .await;

    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message.content.as_text() == RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER
    }));
}

#[tokio::test]
async fn normalize_tool_free_recovery_break_outcome_keeps_non_recovery_blocked_result() {
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = normalize_tool_free_recovery_break_outcome(
        &mut history,
        TurnLoopResult::Blocked {
            reason: Some("Stopped after reaching budget limit.".to_string()),
        },
        true,
        None,
        None,
        None,
    )
    .await;

    assert!(matches!(
        outcome,
        TurnLoopResult::Blocked {
            reason: Some(ref reason)
        } if reason == "Stopped after reaching budget limit."
    ));
    assert!(!history.iter().any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message.content.as_text() == RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER
    }));
}

#[tokio::test]
async fn plan_mode_recovery_fallback_marks_interview_pending_and_preserves_research() {
    use vtcode_core::core::interfaces::session::PlanningEntrySource;

    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.enter(PlanningEntrySource::UserRequest);
    assert!(!plan_session.interview_pending());

    let mut history = vec![uni::Message::user("plan launch-time optimization".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        Some(&anyhow!("Network error")),
        None,
        Some(&mut plan_session),
        None,
    )
    .await;

    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    // Planning session must survive the failed recovery so the next turn
    // re-forces the interview instead of dead-ending.
    assert!(plan_session.interview_pending());
    // The plan-aware fallback must be shown (not the generic dead-end one).
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message.content.as_text().contains(PLANNING_RECOVERY_SYNTHESIS_FALLBACK)
    }));
    assert!(!history.iter().any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message.content.as_text() == RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER
    }));
}

#[tokio::test]
async fn plan_mode_recovery_exhausted_finalizes_instead_of_reforcing_interview() {
    use vtcode_core::core::interfaces::session::PlanningEntrySource;

    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.enter(PlanningEntrySource::UserRequest);
    // Mirrors the cross-turn loop bug: the post-tool recovery cycle cap was
    // reached because the planning context is saturated. Re-forcing the
    // interview on the next turn would re-research the still-huge context and
    // loop forever.
    plan_session.mark_recovery_exhausted();
    assert!(!plan_session.interview_pending());

    let mut history = vec![uni::Message::user("plan launch-time optimization".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        Some(&anyhow!("context length exceeded")),
        None,
        Some(&mut plan_session),
        None,
    )
    .await;

    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    // Must NOT re-force the interview — that is what caused the infinite loop.
    assert!(!plan_session.interview_pending());
    // Must conclude with the USER-facing recovery-exhausted notice (not the
    // model-addressed `*_FINALIZE` directive). With no validated persisted
    // artifact, the user must be told to keep planning rather than implement.
    let last = history.last().unwrap();
    assert_eq!(last.role, uni::MessageRole::Assistant);
    assert_eq!(last.phase, Some(uni::AssistantPhase::FinalAnswer));
    let text = last.content.as_text();
    assert!(text.contains("Plan synthesis failed after repeated recovery attempts"));
    assert!(
        !text.contains("Do NOT attempt more tool calls"),
        "model directive must not leak into the user-visible final answer"
    );
    assert!(!text.contains("`implement`"), "no approval hint is allowed without a valid draft");
    assert!(text.to_ascii_lowercase().contains("keep planning"));
}

#[tokio::test]
async fn plan_mode_recovery_rejects_non_plan_salvage() {
    use vtcode_core::core::interfaces::session::PlanningEntrySource;

    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.enter(PlanningEntrySource::UserRequest);

    let mut history = vec![uni::Message::user("plan launch-time optimization".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        None,
        // Salvage that is prose, not a real `<proposed_plan>`.
        Some("Partial plan: batch config reads.".to_string()),
        Some(&mut plan_session),
        None,
    )
    .await;

    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    assert!(plan_session.interview_pending());
    let last = history.last().unwrap();
    // The garbled/non-plan salvage must NOT be injected as the plan; the
    // structured plan-mode message is used instead.
    assert!(last.content.as_text().contains("final synthesis failed"));
    assert!(!last.content.as_text().contains("batch config reads"));
}

#[tokio::test]
async fn plan_mode_recovery_fallback_lists_files_read_when_present() {
    use vtcode_core::core::interfaces::session::PlanningEntrySource;

    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.enter(PlanningEntrySource::UserRequest);

    // Simulate the turn_640 shape: a wall-clock-budgeted plan turn that read
    // several files before the tool-free recovery follow-up failed.
    let mut history = vec![
        uni::Message::user("plan launch-time optimization".to_string()),
        uni::Message::tool_response(
            "call_1".to_string(),
            "{\"path\": \"src/main.rs\", \"content\": \"...\"}".to_string(),
        ),
        uni::Message::tool_response(
            "call_2".to_string(),
            "{\"path\": \"src/startup/mod.rs\", \"content\": \"...\"}".to_string(),
        ),
    ];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        Some(&anyhow!("Network error")),
        None,
        Some(&mut plan_session),
        None,
    )
    .await;

    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    assert!(plan_session.interview_pending());
    let last = history.last().unwrap();
    let text = last.content.as_text();
    // Plan mode must stay at least as informative as the generic dead-end:
    // it must still surface the files already read so the next turn can reuse
    // them instead of re-exploring.
    assert!(text.contains("Files already read this turn"));
    assert!(text.contains("src/main.rs"));
    assert!(text.contains("src/startup/mod.rs"));
    // And it must lead with the plan-aware message, not the generic one.
    assert!(text.contains(PLANNING_RECOVERY_SYNTHESIS_FALLBACK));
}

#[test]
fn accumulate_turn_usage_merges_prompt_completion_and_cached_tokens() {
    let mut total = HarnessUsage::default();

    accumulate_turn_usage(
        "openai",
        &mut total,
        &Some(uni::Usage {
            prompt_tokens: 100,
            completion_tokens: 20,
            total_tokens: 120,
            cached_prompt_tokens: Some(15),
            cache_creation_tokens: None,
            cache_read_tokens: Some(15),
            iterations: None,
        }),
    );
    accumulate_turn_usage(
        "openai",
        &mut total,
        &Some(uni::Usage {
            prompt_tokens: 40,
            completion_tokens: 10,
            total_tokens: 50,
            cached_prompt_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            iterations: None,
        }),
    );

    assert_eq!(total.input_tokens, 140);
    assert_eq!(total.cached_input_tokens, 15);
    assert_eq!(total.output_tokens, 30);
    assert!(has_turn_usage(&total));
}

#[test]
fn accumulate_turn_usage_normalizes_anthropic_exclusive_input() {
    let mut total = HarnessUsage::default();

    accumulate_turn_usage(
        "anthropic",
        &mut total,
        &Some(uni::Usage {
            prompt_tokens: 100,
            completion_tokens: 20,
            total_tokens: 120,
            cached_prompt_tokens: None,
            cache_creation_tokens: Some(50),
            cache_read_tokens: Some(400),
            iterations: None,
        }),
    );

    assert_eq!(total.input_tokens, 550);
    assert_eq!(total.cached_input_tokens, 400);
    assert_eq!(total.cache_creation_tokens, 50);
    assert_eq!(total.output_tokens, 20);
}

#[tokio::test]
async fn turn_loop_preserves_legacy_loop_detector_state() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_loop_limit(tool_names::READ_FILE, 2);
    let seeded_args = json!({"path":"sample.txt"});
    assert!(backing.record_tool_call(tool_names::READ_FILE, &seeded_args).is_none());
    let _ = backing.record_tool_call(tool_names::READ_FILE, &seeded_args);
    let warning = backing.record_tool_call(tool_names::READ_FILE, &seeded_args);
    assert!(warning.is_some());
    assert!(backing.is_hard_limit_exceeded(tool_names::READ_FILE));

    let mut history = vec![uni::Message::user("continue".to_string())];
    run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("turn loop should complete");

    assert!(backing.is_hard_limit_exceeded(tool_names::READ_FILE));
}

#[tokio::test]
async fn resumed_turn_cannot_complete_while_verification_is_pending() {
    #[derive(Clone)]
    struct TextOnlyProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for TextOnlyProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            Ok(uni::LLMResponse {
                content: Some("The requested work is complete.".to_string()),
                model: request.model.clone(),
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
    backing.set_provider(Box::new(TextOnlyProvider { requests: requests.clone() }));

    let mut history = vec![uni::Message::user("continue the implementation".to_string())];
    let turn_context = backing.turn_loop_context();
    turn_context.session_stats.set_verification_snapshot((true, 0));
    let outcome = run_turn_loop(&mut history, turn_context)
        .await
        .expect("a resumed unverified turn should produce a blocked handoff");

    assert!(matches!(
        outcome.result,
        TurnLoopResult::Blocked {
            reason: Some(ref reason)
        } if reason == PENDING_VERIFICATION_BLOCK_REASON
    ));
    assert_eq!(
        requests.load(Ordering::SeqCst),
        6,
        "the gate must cap repeated unverified responses after bounded auto-recovery"
    );
    assert!(outcome.final_response_was_fallback);
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::System
            && message
                .content
                .as_text()
                .contains(crate::agent::runloop::unified::turn::tool_outcomes::helpers::ANTI_BLIND_EDITING_DIRECTIVE)
    }));
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().contains("Verification recovery (")
    }));
    assert!(
        !history
            .iter()
            .any(|message| { message.content.as_text().contains("The requested work is complete.") })
    );
}

#[tokio::test]
async fn harness_auto_verification_completes_text_only_turn_without_manual_continue() {
    #[derive(Clone)]
    struct TextOnlyProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for TextOnlyProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            Ok(uni::LLMResponse {
                content: Some("The requested work is complete.".to_string()),
                model: request.model.clone(),
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
    let mut backing = TestTurnProcessingBacking::new(12).await;
    backing.set_provider(Box::new(TextOnlyProvider { requests: requests.clone() }));
    // Hermetic passing verifier: `rustc --version` exits 0 with no files.
    backing.set_verification_override_for_test("rustc --version");

    let mut history = vec![uni::Message::user("continue the implementation".to_string())];
    let turn_context = backing.turn_loop_context();
    turn_context.session_stats.set_verification_snapshot((true, 0));
    let outcome = run_turn_loop(&mut history, turn_context)
        .await
        .expect("harness auto-verification should complete the turn");

    // The provider claims completion on its FIRST text, so the claim fast
    // path fires immediately: the harness runs `rustc --version` itself, the
    // gate clears, and the second response completes normally — two requests,
    // zero directive rounds, no manual `continue`, no blocked handoff.
    assert!(
        matches!(outcome.result, TurnLoopResult::Completed { .. }),
        "expected Completed, got: {:?}",
        outcome.result
    );
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert!(!outcome.final_response_was_fallback);
    assert!(
        !history.iter().any(|message| {
            message.role == uni::MessageRole::System && message.content.as_text().contains("Verification recovery (")
        }),
        "fast path must skip directive rounds entirely"
    );
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().contains("gate is cleared")
    }));
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.content.as_text().contains("The requested work is complete.")
    }));
}

#[tokio::test]
async fn response_cap_preserves_commentary_as_one_completed_final_harness_message() {
    const FIRST_COMMENTARY: &str = "Let me continue analyzing the results from the first inspection.";
    const SECOND_COMMENTARY: &str = "Let me continue analyzing the results from the second inspection.";

    #[derive(Clone)]
    struct RepeatedCommentaryProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for RepeatedCommentaryProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let response_text = match self.requests.fetch_add(1, Ordering::SeqCst) {
                0 => FIRST_COMMENTARY,
                1 => SECOND_COMMENTARY,
                _ => "unexpected third response",
            };
            Ok(uni::LLMResponse {
                content: Some(response_text.to_string()),
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_provider(Box::new(RepeatedCommentaryProvider { requests: requests.clone() }));
    let harness_path = backing.enable_harness_emitter();

    let mut history = vec![
        uni::Message::user("inspect the workspace and summarize the results".to_string()),
        uni::Message::assistant(String::new()).with_tool_calls(vec![uni::ToolCall::function(
            "call_1".to_string(),
            "code_search".to_string(),
            "{}".to_string(),
        )]),
        uni::Message::tool_response("call_1".to_string(), "inspection complete".to_string()),
    ];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("the capped recovery loop should preserve the final commentary");

    // The verification gate is clear and the latest commentary was promoted
    // to the final answer, so the capped turn completes instead of blocking
    // (session-vtcode-20260912T083718Z: a verified recap was continued into
    // the cap and published as Blocked for finished work).
    assert!(matches!(outcome.result, TurnLoopResult::Completed { plan_approved_execution_pending: false }));
    assert_eq!(requests.load(Ordering::SeqCst), 2, "the response cap must stop before a third request");
    assert!(!outcome.final_response_was_fallback);
    assert_eq!(final_answer_text(&history), SECOND_COMMENTARY);
    assert_eq!(
        history
            .iter()
            .filter(|message| message.phase == Some(uni::AssistantPhase::FinalAnswer))
            .count(),
        1,
        "the preserved commentary must become the only final assistant message"
    );
    assert!(
        !history
            .iter()
            .any(|message| message.content.as_text().contains(COMPLETED_TURN_FALLBACK_RESPONSE)),
        "the cap must not replace substantive commentary with the generic fallback"
    );

    let rendered = backing.rendered_inline_output();
    assert_eq!(rendered.matches(FIRST_COMMENTARY).count(), 1);
    assert_eq!(rendered.matches(SECOND_COMMENTARY).count(), 1);

    let harness = fs::read_to_string(harness_path).expect("read capped recovery harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("capped recovery harness output should use the versioned event contract")
                .into_event()
        })
        .collect::<Vec<_>>();
    let agent_messages = events
        .iter()
        .filter_map(|event| {
            let ThreadEvent::ItemCompleted(item) = event else {
                return None;
            };
            let ThreadItemDetails::AgentMessage(message) = &item.item.details else {
                return None;
            };
            Some(message.text.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(agent_messages, vec![SECOND_COMMENTARY]);
    assert!(events.iter().any(|event| matches!(event, ThreadEvent::TurnCompleted(_))));
    assert!(!events.iter().any(|event| matches!(event, ThreadEvent::TurnFailed(_))));
}

#[tokio::test]
async fn successful_tool_round_resets_consecutive_text_response_cap() {
    const FIRST_COMMENTARY: &str = "Let me continue by inspecting the first result.";
    const SECOND_COMMENTARY: &str = "Let me continue by checking the tool result.";
    const FINAL_RESPONSE: &str = "The requested inspection is complete and validation passed";

    #[derive(Clone)]
    struct CommentaryWithToolProgressProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for CommentaryWithToolProgressProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let request_number = self.requests.fetch_add(1, Ordering::SeqCst);
            let (content, tool_calls) = match request_number {
                0 => (Some(FIRST_COMMENTARY.to_string()), None),
                1 => (
                    None,
                    Some(vec![uni::ToolCall::function(
                        "productive-tool-call".to_string(),
                        tool_names::APPLY_PATCH.to_string(),
                        json!({
                            "patch": "*** Begin Patch\n*** Add File: productive-tool-progress.txt\n+tool progress\n*** End Patch\n"
                        })
                        .to_string(),
                    )]),
                ),
                2 => (Some(SECOND_COMMENTARY.to_string()), None),
                3 => (Some(FINAL_RESPONSE.to_string()), None),
                _ => panic!("unexpected request after final response"),
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_provider(Box::new(CommentaryWithToolProgressProvider { requests: requests.clone() }));

    let mut history = vec![uni::Message::user(
        "inspect the workspace and finish the analysis".to_string(),
    )];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("tool progress should break the text-only response streak");

    assert!(matches!(outcome.result, TurnLoopResult::Completed { plan_approved_execution_pending: false }));
    assert_eq!(requests.load(Ordering::SeqCst), 4, "the final request must run after tool progress");
    assert_eq!(final_answer_text(&history), FINAL_RESPONSE);
    assert!(backing.workspace_path().join("productive-tool-progress.txt").exists());
}

#[tokio::test]
async fn blocked_mutation_does_not_reset_consecutive_text_response_cap() {
    #[derive(Clone)]
    struct TextBlockedMutationTextProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for TextBlockedMutationTextProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let request_number = self.requests.fetch_add(1, Ordering::SeqCst);
            // Texts at 0, 2, 3, 5, 6, 7 (six total = 2 * (1 + 2 auto-recovery
            // attempts)); blocked mutations at 1 and 4 must retain — never
            // reset — the text-response streak, so the cap-hits (and recovery
            // grants) land exactly on the even texts.
            let (content, tool_calls) = match request_number {
                0 => (Some("The unverified change is complete.".to_string()), None),
                1 | 4 => (
                    None,
                    Some(vec![uni::ToolCall::function(
                        "blocked-mutation".to_string(),
                        tool_names::APPLY_PATCH.to_string(),
                        json!({
                            "patch": "*** Begin Patch\n*** Add File: must-not-exist.rs\n+blocked\n*** End Patch\n"
                        })
                        .to_string(),
                    )]),
                ),
                2 => (Some("The unverified change is still complete.".to_string()), None),
                3 => (Some("The unverified change is still complete (2).".to_string()), None),
                5 => (Some("The unverified change is still complete (3).".to_string()), None),
                6 => (Some("The unverified change is still complete (4).".to_string()), None),
                7 => (Some("The unverified change is still complete (5).".to_string()), None),
                _ => panic!("blocked mutation incorrectly reset the response streak"),
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
    let mut backing = TestTurnProcessingBacking::new(12).await;
    backing.set_provider(Box::new(TextBlockedMutationTextProvider { requests: requests.clone() }));

    let mut history = vec![uni::Message::user("finish the pending change".to_string())];
    let turn_context = backing.turn_loop_context();
    turn_context.session_stats.set_verification_snapshot((true, 0));
    let outcome = run_turn_loop(&mut history, turn_context)
        .await
        .expect("blocked mutation should retain the text-response streak");

    assert!(matches!(outcome.result, TurnLoopResult::Blocked { .. }));
    assert_eq!(
        requests.load(Ordering::SeqCst),
        8,
        "the sixth text response must reach the cap after auto-recovery exhaustion"
    );
    assert!(!backing.workspace_path().join("must-not-exist.rs").exists());
}

#[tokio::test]
async fn anti_blind_guard_stops_outer_loop_after_bounded_pending_stale_plan_pause_responses() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct RepeatedTextAfterMutationsProvider {
        requests: Arc<AtomicUsize>,
        text_responses: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for RepeatedTextAfterMutationsProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let request_number = self.requests.fetch_add(1, Ordering::SeqCst);
            let response = if request_number < 6 {
                let path = format!("anti-blind-regression-{request_number}.rs");
                let patch =
                    format!("*** Begin Patch\n*** Add File: {path}\n+mutation {request_number}\n*** End Patch\n");
                uni::LLMResponse {
                    content: None,
                    model: request.model.clone(),
                    tool_calls: Some(vec![uni::ToolCall::function(
                        format!("mutation-{request_number}"),
                        tool_names::APPLY_PATCH.to_string(),
                        json!({"patch": patch}).to_string(),
                    )]),
                    usage: None,
                    finish_reason: uni::FinishReason::Stop,
                    reasoning: None,
                    reasoning_details: None,
                    organization_id: None,
                    request_id: None,
                    tool_references: Vec::new(),
                    compaction: None,
                }
            } else {
                self.text_responses.fetch_add(1, Ordering::SeqCst);
                uni::LLMResponse {
                    content: Some(
                        "Implementation is paused because tool use is disabled. Wait for the next turn.".to_string(),
                    ),
                    model: request.model.clone(),
                    tool_calls: None,
                    usage: None,
                    finish_reason: uni::FinishReason::Stop,
                    reasoning: None,
                    reasoning_details: None,
                    organization_id: None,
                    request_id: None,
                    tool_references: Vec::new(),
                    compaction: None,
                }
            };
            Ok(response)
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".to_string()]
        }

        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    let requests = Arc::new(AtomicUsize::new(0));
    let text_responses = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(16).await;
    backing.set_provider(Box::new(RepeatedTextAfterMutationsProvider {
        requests: requests.clone(),
        text_responses: text_responses.clone(),
    }));

    let mut history = vec![uni::Message::user("apply and verify the requested change".to_string())];
    let turn_context = backing.turn_loop_context();
    turn_context.harness_state.set_approved_plan_execution(true);
    let outcome = run_turn_loop(&mut history, turn_context)
        .await
        .expect("turn loop should stop at the pending-verification response cap");

    assert!(matches!(outcome.result, TurnLoopResult::Blocked { .. }));
    assert!(matches!(
        outcome.result,
        TurnLoopResult::Blocked {
            reason: Some(ref reason)
        } if reason == "Turn blocked after repeated unverified assistant responses; verification is still pending."
    ));
    // The per-turn text cap is 2, but each cap-hit consumes one bounded
    // autonomous recovery attempt (fresh streak + project-aware directive):
    // 2 * (1 + MAX_VERIFICATION_AUTO_RECOVERY_ATTEMPTS) texts before block.
    assert_eq!(
        text_responses.load(Ordering::SeqCst),
        2 * (1 + usize::from(
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::MAX_VERIFICATION_AUTO_RECOVERY_ATTEMPTS
        ))
    );
    assert_eq!(
        requests.load(Ordering::SeqCst),
        12,
        "the provider must not receive requests beyond the auto-recovery budget"
    );
    assert!(outcome.final_response_was_fallback);
    assert!(history.iter().any(|message| {
        message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message
                .content
                .as_text()
                .contains("Inspection-only checks do not clear the verification gate")
    }));
    assert_eq!(
        history
            .iter()
            .filter(|message| message.phase == Some(uni::AssistantPhase::FinalAnswer))
            .count(),
        1
    );
    assert!(!history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("Implementation is paused because tool use is disabled.")
    }));
}

#[tokio::test]
async fn blocked_anti_blind_recovery_publishes_one_actionable_handoff() {
    #[derive(Clone)]
    struct VerificationRecoveryProvider {
        requests: Arc<AtomicUsize>,
        steps: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for VerificationRecoveryProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            // Bounded tool-failure diagnosis issues auxiliary requests on the
            // same provider client (main-model route when no lightweight model
            // is configured). Serve them with a valid diagnosis JSON without
            // consuming the scripted main-loop sequence below.
            if request
                .system_prompt
                .as_deref()
                .is_some_and(|prompt| prompt.contains("failure-diagnosis checkpoint"))
            {
                return Ok(uni::LLMResponse {
                    content: Some(
                        r#"{"observed":"the scripted tool call failed","likely_cause":"intentional test failure","next_action":"inspect the tool output and retry with corrected arguments"}"#
                            .to_string(),
                    ),
                    model: request.model.clone(),
                    tool_calls: None,
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
            let request_number = self.requests.fetch_add(1, Ordering::SeqCst);
            let tool_call = |label: &str, tool_name: &str, args: serde_json::Value| {
                self.steps.lock().expect("step trace lock").push(label.to_string());
                uni::LLMResponse {
                    content: None,
                    model: request.model.clone(),
                    tool_calls: Some(vec![uni::ToolCall::function(
                        format!("verification-{request_number}"),
                        tool_name.to_string(),
                        args.to_string(),
                    )]),
                    usage: None,
                    finish_reason: uni::FinishReason::Stop,
                    reasoning: None,
                    reasoning_details: None,
                    organization_id: None,
                    request_id: None,
                    tool_references: Vec::new(),
                    compaction: None,
                }
            };

            let patch = |path: &str, contents: &str| {
                format!("*** Begin Patch\n*** Add File: {path}\n+{contents}\n*** End Patch\n")
            };

            let response = match request_number {
                0..=4 => tool_call(
                    "successful_edit",
                    tool_names::APPLY_PATCH,
                    json!({"patch": patch(&format!("anti-blind-sequence-{request_number}.rs"), "effective edit")}),
                ),
                5 => tool_call(
                    "failed_patch",
                    tool_names::APPLY_PATCH,
                    json!({
                        "patch": "*** Begin Patch\n*** Update File: missing-target.rs\n@@\n-old\n+new\n*** End Patch\n"
                    }),
                ),
                6 => tool_call(
                    "inspection",
                    tool_names::EXEC_COMMAND,
                    json!({"cmd": "rg -n 'effective edit' . || true"}),
                ),
                7 => tool_call(
                    "link_check",
                    tool_names::EXEC_COMMAND,
                    json!({"cmd": "rg -n '\\[[^]]+\\]\\([^)]*\\)' . || true"}),
                ),
                8 => tool_call("diff_check", tool_names::EXEC_COMMAND, json!({"cmd": "git diff --check"})),
                9 => tool_call(
                    "successful_edit",
                    tool_names::APPLY_PATCH,
                    json!({"patch": patch("anti-blind-sequence-final.rs", "last effective edit")}),
                ),
                _ => {
                    self.steps.lock().expect("step trace lock").push("unverified_text".to_string());
                    uni::LLMResponse {
                        content: Some("The edits are complete, but verification was not run.".to_string()),
                        model: request.model.clone(),
                        tool_calls: None,
                        usage: None,
                        finish_reason: uni::FinishReason::Stop,
                        reasoning: None,
                        reasoning_details: None,
                        organization_id: None,
                        request_id: None,
                        tool_references: Vec::new(),
                        compaction: None,
                    }
                }
            };
            Ok(response)
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".to_string()]
        }

        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    let requests = Arc::new(AtomicUsize::new(0));
    let steps = Arc::new(Mutex::new(Vec::new()));
    let mut backing = TestTurnProcessingBacking::new(20).await;
    let harness_path = backing.enable_harness_emitter();
    backing.set_provider(Box::new(VerificationRecoveryProvider { requests: requests.clone(), steps: steps.clone() }));

    let mut history = vec![uni::Message::user("apply the change and verify it".to_string())];
    let turn_context = backing.turn_loop_context();
    turn_context.harness_state.set_approved_plan_execution(true);
    let outcome = run_turn_loop(&mut history, turn_context)
        .await
        .expect("anti-blind recovery should return a blocked outcome");

    assert!(
        matches!(
            outcome.result,
            TurnLoopResult::Blocked {
                reason: Some(ref reason)
            } if reason == PENDING_VERIFICATION_BLOCK_REASON
        ),
        "expected PENDING_VERIFICATION_BLOCK_REASON, got: {:?}, steps: {:?}",
        outcome.result,
        steps.lock().expect("step trace lock").as_slice()
    );
    assert_eq!(requests.load(Ordering::SeqCst), 16, "ten tool steps plus six bounded auto-recovery texts");
    assert_eq!(
        steps.lock().expect("step trace lock").as_slice(),
        [
            "successful_edit",
            "successful_edit",
            "successful_edit",
            "successful_edit",
            "successful_edit",
            "failed_patch",
            "inspection",
            "link_check",
            "diff_check",
            "successful_edit",
            "unverified_text",
            "unverified_text",
            "unverified_text",
            "unverified_text",
            "unverified_text",
            "unverified_text",
        ]
    );
    assert_blocked_response_surfaces(&mut backing, &history, &harness_path, PENDING_VERIFICATION_RESPONSE_MARKER);
}

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

#[tokio::test]
async fn context_capacity_blocked_recovery_publishes_one_actionable_handoff() {
    #[derive(Clone)]
    struct ContextCapacityProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for ContextCapacityProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            if self.requests.fetch_add(1, Ordering::SeqCst) == 0 {
                let patch =
                    "*** Begin Patch\n*** Add File: context-capacity-sequence.txt\n+effective edit\n*** End Patch\n";
                return Ok(uni::LLMResponse {
                    content: None,
                    model: request.model,
                    tool_calls: Some(vec![uni::ToolCall::function(
                        "context-capacity-edit".to_string(),
                        tool_names::APPLY_PATCH.to_string(),
                        json!({"patch": patch}).to_string(),
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

            Err(uni::LLMError::InvalidRequest {
                message: "maximum context length is 114688 tokens".to_string(),
                metadata: None,
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
    backing.set_provider(Box::new(ContextCapacityProvider { requests: requests.clone() }));

    let mut history = vec![uni::Message::user("apply the change".to_string())];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("context-capacity recovery should return a blocked outcome");

    assert!(matches!(
        outcome.result,
        TurnLoopResult::Blocked {
            reason: Some(ref reason)
        } if reason == POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON
    ));
    assert_eq!(requests.load(Ordering::SeqCst), 2, "context failure should stop after the bounded retry path");
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().contains(POST_TOOL_RESUME_DIRECTIVE)
    }));
    assert_blocked_response_surfaces(&mut backing, &history, &harness_path, CONTEXT_CAPACITY_RESPONSE_MARKER);
}

#[tokio::test]
async fn refused_turn_renders_refusal_notice_over_stale_final_answer() {
    let stale_final = "Here is the answer to the earlier part of this turn.";
    let notice = "The model declined this request (category: cyber). \
                  The request was not retried; rephrase it or switch models.";
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let harness_path = backing.enable_harness_emitter();

    let mut history = vec![
        uni::Message::user("refused request".to_string()),
        uni::Message::assistant(stale_final.to_string()).with_phase(Some(uni::AssistantPhase::FinalAnswer)),
    ];
    let history_before = history.clone();
    let blocked = TurnLoopResult::Blocked { reason: Some(notice.to_string()) };
    {
        let mut context = backing.turn_loop_context();
        context.harness_state.mark_turn_refused();
        // The stale final was already shown; the refusal must still render.
        context.harness_state.mark_final_response_rendered();
        ensure_blocked_turn_response(&mut context, &mut history, 0, notice).expect("refusal notice");
        finalize_turn(&mut context, &history, &blocked, &HarnessUsage::default()).await;
    }

    assert_eq!(history, history_before, "the refusal notice must not enter model-visible history");
    let rendered = backing.rendered_inline_output();
    assert_eq!(rendered.matches("The model declined this request").count(), 1, "{rendered}");

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let agent_messages = harness
        .lines()
        .filter_map(|line| {
            let event = serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("versioned harness event")
                .into_event();
            let ThreadEvent::ItemCompleted(item) = event else {
                return None;
            };
            let ThreadItemDetails::AgentMessage(message) = item.item.details else {
                return None;
            };
            Some(message.text)
        })
        .collect::<Vec<_>>();
    assert_eq!(agent_messages, vec![notice.to_string()]);
}

#[tokio::test]
async fn blocked_recovery_does_not_duplicate_prior_harness_agent_message() {
    let prior_text = "A streamed progress message was already published.";
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let harness_path = backing.enable_harness_emitter();
    backing.emit_harness_assistant_message_for_test(prior_text);

    let mut history = vec![uni::Message::user("resume the request".to_string())];
    let blocked = TurnLoopResult::Blocked {
        reason: Some(PENDING_VERIFICATION_BLOCK_REASON.to_string()),
    };
    {
        let mut context = backing.turn_loop_context();
        context.harness_state.mark_final_response_event_emitted();
        ensure_blocked_turn_response(&mut context, &mut history, 1, PENDING_VERIFICATION_BLOCK_REASON)
            .expect("blocked recovery handoff");
        finalize_turn(&mut context, &history, &blocked, &HarnessUsage::default()).await;
    }

    assert_eq!(
        history
            .iter()
            .filter(|message| message.phase == Some(uni::AssistantPhase::FinalAnswer))
            .count(),
        1
    );
    let final_text = final_answer_text(&history);
    assert!(final_text.contains(PENDING_VERIFICATION_RESPONSE_MARKER));
    let rendered = backing.rendered_inline_output();
    assert_eq!(rendered.matches(PENDING_VERIFICATION_RESPONSE_MARKER).count(), 1);

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("versioned harness event")
                .into_event()
        })
        .collect::<Vec<_>>();
    let agent_messages = events
        .iter()
        .filter_map(|event| {
            let ThreadEvent::ItemCompleted(item) = event else {
                return None;
            };
            let ThreadItemDetails::AgentMessage(message) = &item.item.details else {
                return None;
            };
            Some(message.text.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(agent_messages, vec![prior_text]);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ThreadEvent::TurnFailed(_)))
            .count(),
        1
    );
    assert!(!events.iter().any(|event| matches!(event, ThreadEvent::TurnCompleted(_))));
}

#[tokio::test]
async fn finalize_turn_blocked_event_populates_fuse_telemetry() {
    use crate::agent::runloop::unified::run_loop_context::BlockedToolRecoveryTelemetry;

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let harness_path = backing.enable_harness_emitter();

    let history = vec![uni::Message::user("resume the request".to_string())];
    let blocked = TurnLoopResult::Blocked {
        reason: Some("blocked tool-call limit reached".to_string()),
    };
    {
        let mut context = backing.turn_loop_context();
        context.harness_state.arm_blocked_tool_recovery(
            "blocked tool-call limit reached",
            BlockedToolRecoveryTelemetry {
                last_tool: "exec_command 'cargo check'".to_string(),
                consecutive_cap: 3,
                total_cap: 6,
                blocked_streak: 4,
                blocked_total: 5,
            },
        );
        finalize_turn(&mut context, &history, &blocked, &HarnessUsage::default()).await;
    }

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("versioned harness event")
                .into_event()
        })
        .collect::<Vec<_>>();
    let blocked_events = events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::TurnBlocked(blocked) => Some(blocked),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(blocked_events.len(), 1, "exactly one turn.blocked event");
    let blocked_event = blocked_events[0];
    assert_eq!(blocked_event.last_tool.as_deref(), Some("exec_command 'cargo check'"));
    assert_eq!(blocked_event.consecutive_cap, 3);
    assert_eq!(blocked_event.total_cap, 6);
    assert_eq!(blocked_event.blocked_streak, 4);
    assert_eq!(blocked_event.blocked_total, 5);
}

#[tokio::test]
async fn finalize_turn_blocked_event_without_fuse_keeps_legacy_fields() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let harness_path = backing.enable_harness_emitter();

    let history = vec![uni::Message::user("resume the request".to_string())];
    let blocked = TurnLoopResult::Blocked {
        reason: Some(PENDING_VERIFICATION_BLOCK_REASON.to_string()),
    };
    {
        let mut context = backing.turn_loop_context();
        context.harness_state.consecutive_blocked_tool_calls = 2;
        context.harness_state.blocked_tool_calls = 7;
        finalize_turn(&mut context, &history, &blocked, &HarnessUsage::default()).await;
    }

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("versioned harness event")
                .into_event()
        })
        .collect::<Vec<_>>();
    let blocked_events = events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::TurnBlocked(blocked) => Some(blocked),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(blocked_events.len(), 1, "exactly one turn.blocked event");
    let blocked_event = blocked_events[0];
    // Blocks that did not trip the fuse keep the live counters and the
    // previous `None`/zero fields.
    assert_eq!(blocked_event.last_tool, None);
    assert_eq!(blocked_event.blocked_streak, 2);
    assert_eq!(blocked_event.blocked_total, 7);
    assert_eq!(blocked_event.consecutive_cap, 0);
    assert_eq!(blocked_event.total_cap, 0);
}

#[tokio::test]
async fn finalize_turn_closes_streamed_items_that_never_reached_the_pipeline() {
    use crate::agent::runloop::unified::run_loop_context::StreamedToolCallItem;

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let harness_path = backing.enable_harness_emitter();

    let history = vec![uni::Message::user("resume the request".to_string())];
    let blocked = TurnLoopResult::Blocked {
        reason: Some("blocked tool-call limit reached".to_string()),
    };
    {
        let mut context = backing.turn_loop_context();
        context.harness_state.remember_streamed_tool_call_items([(
            "call_dangling".to_string(),
            StreamedToolCallItem {
                item_id: "turn-step-1-assistant-stream-1-tool-call-call_dangling".to_string(),
                tool_name: "exec_command".to_string(),
            },
        )]);
        finalize_turn(&mut context, &history, &blocked, &HarnessUsage::default()).await;
    }

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("versioned harness event")
                .into_event()
        })
        .collect::<Vec<_>>();

    let mut invocation_closed = false;
    let mut output_closed = false;
    for event in &events {
        let ThreadEvent::ItemCompleted(completed) = event else {
            continue;
        };
        match &completed.item.details {
            ThreadItemDetails::ToolInvocation(invocation) => {
                assert_eq!(completed.item.id, "turn-step-1-assistant-stream-1-tool-call-call_dangling");
                assert_eq!(invocation.tool_name, "exec_command");
                assert_eq!(invocation.tool_call_id.as_deref(), Some("call_dangling"));
                assert_eq!(invocation.status, ToolCallStatus::Failed);
                assert_eq!(invocation.outcome, Some(vtcode_core::exec::events::ToolOutcome::Cancelled));
                invocation_closed = true;
            }
            ThreadItemDetails::ToolOutput(output) => {
                assert_eq!(completed.item.id, "turn-step-1-assistant-stream-1-tool-call-call_dangling:output");
                assert_eq!(output.tool_call_id.as_deref(), Some("call_dangling"));
                assert_eq!(output.status, ToolCallStatus::Failed);
                assert!(output.output.contains("ended when the turn finished"));
                output_closed = true;
            }
            _ => {}
        }
    }
    assert!(invocation_closed, "dangling invocation must be closed with Failed status");
    assert!(output_closed, "dangling invocation must get a terminal tool_output");
}

#[tokio::test]
async fn completed_and_interrupted_turns_close_unexecuted_calls_without_error_outcomes() {
    use crate::agent::runloop::unified::run_loop_context::StreamedToolCallItem;
    for result in [
        TurnLoopResult::Completed { plan_approved_execution_pending: false },
        TurnLoopResult::Cancelled,
    ] {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let path = backing.enable_harness_emitter();
        let mut ctx = backing.turn_loop_context();
        ctx.harness_state.remember_streamed_tool_call_items([(
            "pending-call".into(),
            StreamedToolCallItem {
                item_id: "pending-item".into(),
                tool_name: "exec_command".into(),
            },
        )]);
        finalize_turn(&mut ctx, &[], &result, &HarnessUsage::default()).await;
        assert!(ctx.harness_state.take_all_streamed_tool_call_item_ids().is_empty());
        let records = fs::read_to_string(path).unwrap();
        let events = records
            .lines()
            .map(|line| serde_json::from_str::<VersionedThreadEvent>(line).unwrap().into_event())
            .collect::<Vec<_>>();
        let mut invocation_count = 0;
        let mut output_count = 0;
        for event in events {
            if let ThreadEvent::ItemCompleted(item) = event {
                match item.item.details {
                    ThreadItemDetails::ToolInvocation(invocation) => {
                        assert_eq!(invocation.tool_call_id.as_deref(), Some("pending-call"));
                        assert_eq!(invocation.outcome, Some(vtcode_core::exec::events::ToolOutcome::Cancelled));
                        invocation_count += 1;
                    }
                    ThreadItemDetails::ToolOutput(output) => {
                        assert_eq!(output.exit_code, None);
                        assert!(output.output.contains("before it could execute"));
                        output_count += 1;
                    }
                    _ => {}
                }
            }
        }
        assert_eq!((invocation_count, output_count), (1, 1));
    }
}

#[tokio::test]
async fn stale_plan_pause_discards_text_without_consuming_response_budget() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct RepeatedStalePauseProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for RepeatedStalePauseProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            Ok(uni::LLMResponse {
                content: Some(
                    "Implementation is paused because tool use is disabled. Wait for the next turn.".to_string(),
                ),
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
    backing.set_provider(Box::new(RepeatedStalePauseProvider { requests: requests.clone() }));

    let mut history = vec![uni::Message::user("continue the approved implementation".to_string())];
    let turn_context = backing.turn_loop_context();
    turn_context.harness_state.set_approved_plan_execution(true);
    let outcome = run_turn_loop(&mut history, turn_context)
        .await
        .expect("turn loop should stop at the bounded stale-pause retries");

    assert!(matches!(outcome.result, TurnLoopResult::Blocked { .. }));
    assert!(outcome.final_response_was_fallback);
    // Two bounded clearing retries plus the terminal stale response that ends
    // Blocked. Discarded stale text never consumes the generic text-response
    // budget, so the generic cap is not what stops this loop.
    assert_eq!(requests.load(Ordering::SeqCst), 3);
    assert!(!history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("Implementation is paused because tool use is disabled.")
    }));
}

/// End-to-end regression test for the tool-free recovery contract-violation
/// retry (checkpoint turn_621): when the model emits textual tool-call markup
/// during a tool-free synthesis pass instead of prose, the turn loop must
/// retry up to `MAX_RECOVERY_RETRIES` times with a corrective directive rather
/// than immediately concluding with the canned fallback answer. After retries
/// are exhausted, the turn must conclude with the salvaged prose from the
/// rejected synthesis response.
#[tokio::test]
async fn tool_free_recovery_retries_on_contract_violation_then_salvages() {
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct ContractViolationProvider {
        requests: Arc<Mutex<usize>>,
        content: String,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for ContractViolationProvider {
        fn name(&self) -> &str {
            "openai"
        }
        fn supports_streaming(&self) -> bool {
            false
        }
        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            *self.requests.lock().expect("requests lock") += 1;
            Ok(uni::LLMResponse {
                content: Some(self.content.clone()),
                model: request.model.clone(),
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

    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_tool_free_recovery_for_test("post-tool follow-up failure");

    // Markup with surrounding prose so the salvage step has non-trivial text.
    // A dangling `</tool_call>` close tag trips `contains_pseudo_tool_call_markers`
    // (so the recovery guard fires) but has no matching opening `<tool_call>`
    // for `strip_textual_tool_call_regions` to remove, so the response cannot
    // be "cleaned" into a valid final answer. The turn must retry, then salvage.
    let markup = "Here is my plan: the change was not applied because tools were disabled. \
                  </tool_call> Please re-run with tools enabled.";
    let requests = Arc::new(Mutex::new(0usize));
    backing.set_provider(Box::new(ContractViolationProvider {
        requests: requests.clone(),
        content: markup.to_string(),
    }));

    let mut history = vec![uni::Message::user("summarize the tool outputs".to_string())];
    run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("turn loop should complete after recovery retries");

    // Exactly MAX_RECOVERY_RETRIES retries: 1 initial recovery pass + 3 retries.
    assert_eq!(
        *requests.lock().expect("requests lock"),
        super::MAX_RECOVERY_RETRIES as usize + 1,
        "recovery must retry exactly MAX_RECOVERY_RETRIES times before falling back"
    );

    // The turn must conclude with the salvaged prose, not the canned string.
    let final_text = history
        .iter()
        .rev()
        .find(|m| m.role == uni::MessageRole::Assistant)
        .map(|m| m.content.as_text().to_string())
        .unwrap_or_default();
    assert!(final_text.contains("Here is my plan:"), "expected salvaged prose, got: {final_text}");
    assert!(
        !final_text.contains(RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER),
        "must not emit canned fallback when salvage is available"
    );
    assert!(backing.recovery_is_tool_free());
}

/// Regression test for the plan-mode "cut off mid-flight" bug: a planning
/// synthesis truncated at the model's output token limit (unclosed
/// supported plan tag) must be detected so the turn loop can condense and
/// re-emit instead of accepting a partial plan or looping.
#[test]
fn plan_synthesis_truncated_detects_unclosed_proposed_plan() {
    let truncated = uni::LLMResponse {
        content: Some("<proposed_plan>\n# Improve launch time\n## Steps\n1. Fix warmup -> src/main.rs".to_string()),
        model: "noop".to_string(),
        tool_calls: None,
        usage: None,
        finish_reason: uni::FinishReason::Length,
        reasoning: None,
        reasoning_details: None,
        organization_id: None,
        request_id: None,
        tool_references: Vec::new(),
        compaction: None,
    };
    assert!(
        plan_synthesis_was_truncated(&truncated),
        "unclosed <proposed_plan> with Length finish must be detected as truncated"
    );

    let alternate_truncated = uni::LLMResponse {
        content: Some("<plan>\n# Improve launch time\n## Steps\n1. Fix warmup -> src/main.rs".to_string()),
        model: "noop".to_string(),
        tool_calls: None,
        usage: None,
        finish_reason: uni::FinishReason::Length,
        reasoning: None,
        reasoning_details: None,
        organization_id: None,
        request_id: None,
        tool_references: Vec::new(),
        compaction: None,
    };
    assert!(
        plan_synthesis_was_truncated(&alternate_truncated),
        "unclosed <plan> with Length finish must be detected as truncated"
    );

    // A complete plan (closed tag) is not a truncation even with Length.
    let complete = uni::LLMResponse {
        content: Some("<proposed_plan>\n# Title\n## Steps\n1. x\n</proposed_plan>".to_string()),
        model: "noop".to_string(),
        tool_calls: None,
        usage: None,
        finish_reason: uni::FinishReason::Length,
        reasoning: None,
        reasoning_details: None,
        organization_id: None,
        request_id: None,
        tool_references: Vec::new(),
        compaction: None,
    };
    assert!(!plan_synthesis_was_truncated(&complete), "closed <proposed_plan> must not be flagged as truncated");

    let alternate_complete = uni::LLMResponse {
        content: Some("<plan>\n# Title\n## Steps\n1. x\n</plan>".to_string()),
        model: "noop".to_string(),
        tool_calls: None,
        usage: None,
        finish_reason: uni::FinishReason::Length,
        reasoning: None,
        reasoning_details: None,
        organization_id: None,
        request_id: None,
        tool_references: Vec::new(),
        compaction: None,
    };
    assert!(!plan_synthesis_was_truncated(&alternate_complete), "closed <plan> must not be flagged as truncated");

    // A normal (Stop) response that happens to mention the tag is not truncated.
    let normal = uni::LLMResponse {
        content: Some("<proposed_plan>\n# Title\n</proposed_plan>".to_string()),
        model: "noop".to_string(),
        tool_calls: None,
        usage: None,
        finish_reason: uni::FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        organization_id: None,
        request_id: None,
        tool_references: Vec::new(),
        compaction: None,
    };
    assert!(!plan_synthesis_was_truncated(&normal), "Stop-finished plan must not be flagged as truncated");
}

/// End-to-end regression test for the plan-mode "cut off mid-flight" fix (Fix B):
/// when the planning synthesis is truncated at the model's output token limit
/// (unclosed `<proposed_plan>`, `finish_reason == Length`), the turn loop must
/// inject `PLANNING_SYNTHESIS_TRUNCATED_CONDENSE_DIRECTIVE` and re-run the
/// synthesis once to produce a compact completion — NOT loop forever or accept
/// the partial plan. The retry response is a plain completion (no
/// `<proposed_plan>`) so the planning interview is not re-triggered, keeping
/// the test deterministic and focused on the re-prompt control flow.
#[tokio::test]
async fn planning_synthesis_truncated_retries_with_compact_spec() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct TruncateThenCompactProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for TruncateThenCompactProvider {
        fn name(&self) -> &str {
            "openai"
        }
        fn supports_streaming(&self) -> bool {
            false
        }
        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            let (content, finish_reason) = if n == 0 {
                // First pass: plan cut off mid-<proposed_plan> (output token limit).
                (
                    "<proposed_plan>\n# Improve launch time\n## Summary\nMake cold start faster.\n## Steps\n1. Fix warmup -> src/main.rs -> verify: build".to_string(),
                    uni::FinishReason::Length,
                )
            } else {
                // Second pass: compact completion after the condense directive.
                (
                    "Plan condensed: warmup path in src/main.rs fixed; rebuild to verify.".to_string(),
                    uni::FinishReason::Stop,
                )
            };
            Ok(uni::LLMResponse {
                content: Some(content),
                model: request.model.clone(),
                tool_calls: None,
                usage: None,
                finish_reason,
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

    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.set_provider(Box::new(TruncateThenCompactProvider { calls: calls.clone() }));

    let mut history = vec![uni::Message::user("make a plan to improve launch time".to_string())];
    run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("turn loop must complete after condensing the truncated plan");

    // Two generations: the truncated pass + exactly one compact retry (bounded
    // by MAX_PLAN_SYNTHESIS_CONDENSE_ATTEMPTS, so it must NOT loop).
    assert_eq!(calls.load(Ordering::SeqCst), 2, "must re-run synthesis exactly once after truncation, not loop");

    // The condense directive must have been injected into the history.
    assert!(
        history.iter().any(|message| message
            .content
            .as_text()
            .contains(PLANNING_SYNTHESIS_TRUNCATED_CONDENSE_DIRECTIVE)),
        "condense directive must be injected after a truncated plan"
    );

    // The final assistant message must be the compact retry, not the truncated
    // draft (proving the partial plan was discarded and re-emitted).
    let final_text = history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::Assistant)
        .map(|message| message.content.as_text().to_string())
        .unwrap_or_default();
    assert!(final_text.contains("Plan condensed:"), "final answer must be the compact retry, got: {final_text}");
    assert!(
        !final_text.contains("Fix warmup -> src/main.rs -> verify: build"),
        "final answer must not be the truncated draft"
    );
}

#[tokio::test]
async fn streamed_invalid_plan_uses_one_bounded_repair_without_approval_artifacts() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    let harness_path = backing.enable_harness_emitter();
    backing.set_provider(Box::new(StreamedPlanProvider {
        script: StreamedPlanScript::InvalidThenProse,
        calls: calls.clone(),
    }));

    let mut history = vec![uni::Message::user("make a plan for the streamed handoff".to_string())];
    run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("invalid streamed plan should finish after bounded repair");

    assert_eq!(calls.load(Ordering::SeqCst), 2, "invalid plans get exactly one repair response");
    assert!(
        history.iter().any(|message| {
            message.role == uni::MessageRole::System
                && message
                    .content
                    .as_text()
                    .contains("Planning recovery: the proposed plan was rejected")
        }),
        "validator feedback must schedule the bounded repair"
    );

    let plans_dir = backing.workspace_path().join(".vtcode").join("plans");
    assert!(
        !plans_dir.exists() || fs::read_dir(&plans_dir).expect("read plans directory").next().is_none(),
        "invalid streamed plans must not create approval artifacts"
    );

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("harness output should use the versioned event contract")
                .into_event()
        })
        .collect::<Vec<_>>();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ThreadEvent::PlanApprovalRequested(_))),
        "rejected streamed plans must not publish approval requests"
    );
}

#[tokio::test]
async fn streamed_invalid_plan_repairs_to_valid_plan_and_persists_it() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.set_provider(Box::new(StreamedPlanProvider {
        script: StreamedPlanScript::InvalidThenValid,
        calls: calls.clone(),
    }));

    let mut history = vec![uni::Message::user("make a plan for the streamed handoff".to_string())];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("a valid repair should reach the approval handoff");

    assert_eq!(calls.load(Ordering::SeqCst), 2, "one invalid draft should get one repair request");
    assert!(outcome.plan_approved_execution_pending, "the repaired plan should be approval-ready");
    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == uni::MessageRole::System)
            .filter(|message| message.content.as_text().contains("proposed plan was rejected"))
            .count(),
        1,
        "exactly one bounded repair directive should be recorded"
    );
    assert!(
        history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.content.as_text().contains("<proposed_plan>")
                && message.content.as_text().contains("1. Do the thing")
        }),
        "the rejected draft must remain in assistant history for the repair request"
    );
    assert!(
        history.iter().any(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().contains("proposed plan was rejected")
                && !message.content.as_text().contains("1. Do the thing")
        }),
        "repair directives must not elevate rejected draft text into the system role"
    );

    let plans_dir = backing.workspace_path().join(".vtcode").join("plans");
    let plan_text = fs::read_dir(&plans_dir)
        .expect("repaired plan should create the plans directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| {
            path.extension().is_some_and(|extension| extension == "md")
                && !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(".tasks.md"))
        })
        .map(|path| fs::read_to_string(path).expect("read repaired plan"))
        .expect("repaired plan should be persisted");
    assert!(plan_text.contains("Preserve the streamed planning handoff."));
    assert!(
        history
            .iter()
            .all(|message| !message.content.as_text().contains("Rejected plan draft:")),
        "recoverable invalid drafts must not be rendered as user-facing warning text"
    );
}

#[tokio::test]
async fn streamed_invalid_plan_repair_is_admitted_after_prior_plain_response() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    backing.set_provider(Box::new(StreamedPlanProvider {
        script: StreamedPlanScript::ProseThenInvalidThenValid,
        calls: calls.clone(),
    }));

    let mut history = vec![uni::Message::user(
        "make a plan after the interview fallback".to_string(),
    )];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("a repair after an earlier plain response should reach the approval handoff");

    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "the invalid plan must get a repair despite the earlier prose response"
    );
    assert!(outcome.plan_approved_execution_pending, "the repaired plan should be approval-ready");
    let plans_dir = backing.workspace_path().join(".vtcode").join("plans");
    assert!(
        plans_dir.exists()
            && fs::read_dir(&plans_dir)
                .expect("repaired plan should create the plans directory")
                .any(|entry| entry.ok().is_some_and(|entry| {
                    entry.path().extension().is_some_and(|extension| extension == "md")
                        && !entry
                            .path()
                            .file_name()
                            .is_some_and(|name| name.to_string_lossy().ends_with(".tasks.md"))
                })),
        "the repair admitted after prior prose must persist its validated plan"
    );
}

#[tokio::test]
async fn streamed_invalid_plan_gets_a_second_repair_before_terminal_fallback() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.set_provider(Box::new(StreamedPlanProvider {
        script: StreamedPlanScript::InvalidThenInvalidThenProse,
        calls: calls.clone(),
    }));

    let mut history = vec![uni::Message::user("make a plan for the streamed handoff".to_string())];
    run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("the bounded second repair should finish the turn");

    assert_eq!(calls.load(Ordering::SeqCst), 3, "two invalid drafts should receive two repair passes");
    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == uni::MessageRole::System)
            .filter(|message| message.content.as_text().contains("proposed plan was rejected"))
            .count(),
        2,
        "the second invalid draft must schedule the second repair"
    );
    let plans_dir = backing.workspace_path().join(".vtcode").join("plans");
    assert!(
        !plans_dir.exists() || fs::read_dir(&plans_dir).expect("read plans directory").next().is_none(),
        "recoverable invalid drafts must not create approval artifacts"
    );
}

#[tokio::test]
async fn streamed_third_invalid_plan_is_terminal_and_retains_each_rejected_draft_in_assistant_history() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.set_provider(Box::new(StreamedPlanProvider {
        script: StreamedPlanScript::ThreeInvalid,
        calls: calls.clone(),
    }));

    let mut history = vec![uni::Message::user("make a plan for the streamed handoff".to_string())];
    run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("the third invalid draft should terminate the bounded repair loop");

    assert_eq!(calls.load(Ordering::SeqCst), 3, "the third invalid draft must not trigger a fourth request");
    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == uni::MessageRole::System)
            .filter(|message| message.content.as_text().contains("proposed plan was rejected"))
            .count(),
        2,
        "only the first two invalid drafts should schedule repairs"
    );
    let rejected_drafts = history
        .iter()
        .filter(|message| message.role == uni::MessageRole::Assistant)
        .map(|message| message.content.as_text())
        .filter(|text| text.contains("<proposed_plan>"))
        .count();
    assert_eq!(rejected_drafts, 3, "each rejected draft must remain available in assistant history");
    let plans_dir = backing.workspace_path().join(".vtcode").join("plans");
    assert!(
        !plans_dir.exists() || fs::read_dir(&plans_dir).expect("read plans directory").next().is_none(),
        "terminally rejected plans must not create approval artifacts"
    );
}

#[tokio::test]
async fn streamed_valid_plan_is_persisted_and_publishes_approval_ready_events() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    let harness_path = backing.enable_harness_emitter();
    backing.set_provider(Box::new(StreamedPlanProvider {
        script: StreamedPlanScript::Valid,
        calls: calls.clone(),
    }));

    let mut history = vec![uni::Message::user("make a plan for the streamed handoff".to_string())];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("valid streamed plan should reach the approval handoff");

    assert_eq!(calls.load(Ordering::SeqCst), 1, "a valid streamed plan must not re-enter synthesis");
    assert!(outcome.plan_approved_execution_pending, "the existing automatic approval route should be selected");
    assert_eq!(
        outcome.pending_plan_execution_target,
        Some(crate::agent::runloop::unified::planning_workflow::PlanExecutionTarget::build(
            PlanExecutionContext::Current,
            true,
        )),
        "automatic approval must retain the typed current-session execution handoff"
    );

    let plans_dir = backing.workspace_path().join(".vtcode").join("plans");
    let plan_text = fs::read_dir(&plans_dir)
        .expect("valid streamed plan should create the plans directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| {
            path.extension().is_some_and(|extension| extension == "md")
                && !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(".tasks.md"))
        })
        .map(|path| fs::read_to_string(path).expect("read persisted streamed plan"))
        .expect("valid streamed plan should be persisted");
    assert!(plan_text.contains("Preserve the streamed planning handoff."));
    assert!(plan_text.contains("src/agent/runloop/unified/ui_interaction_stream.rs"));
    assert!(plan_text.contains("target/release/vtcode --version"));
    assert!(plan_text.contains("cargo nextest run -p vtcode --bin vtcode"));

    let tracker_text = fs::read_dir(&plans_dir)
        .expect("valid streamed plan should create the plans directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".tasks.md"))
        })
        .map(|path| fs::read_to_string(path).expect("read persisted streamed task tracker"))
        .expect("automatic approval should persist a task tracker before execution handoff");
    assert!(tracker_text.contains("Keep the semantic plan"));

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("harness output should use the versioned event contract")
                .into_event()
        })
        .collect::<Vec<_>>();
    assert!(
        events.iter().any(|event| matches!(event, ThreadEvent::PlanDelta(delta) if delta.delta.contains("Preserve the streamed planning handoff."))),
        "the persisted plan must flow through the canonical plan delta event"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ThreadEvent::PlanApprovalRequested(_))),
        "the existing approval-ready event path must be published"
    );
}

#[tokio::test]
async fn explicit_build_and_auto_approval_selections_handoff_persisted_plan_without_implementation_writes() {
    for (selection, expected_agent, expected_skip_confirmations) in [
        (InlineListSelection::PlanApprovalSwitchBuild, builtin_primary_build_agent().name, false),
        (InlineListSelection::PlanApprovalSwitchAuto, builtin_primary_auto_agent().name, true),
    ] {
        let workspace = tempfile::TempDir::new().expect("create approval selection workspace");
        let mut tool_registry = vtcode_core::tools::ToolRegistry::new(workspace.path().to_path_buf()).await;
        tool_registry.enable_planning();
        let plan_state = tool_registry.planning_workflow_state();
        persist_plan_draft(&plan_state, STREAMED_VALID_PLAN)
            .await
            .expect("persist canonical plan before explicit approval selection");
        let plan = load_plan_text_for_approval(&tool_registry)
            .await
            .expect("load the canonical persisted plan for approval");
        let mut plan_session = PlanningWorkflowSessionState::default();
        plan_session.enter(vtcode_core::core::interfaces::session::PlanningEntrySource::UserRequest);

        assert!(
            fs::read_dir(workspace.path())
                .expect("read workspace before approval")
                .all(|entry| entry.expect("read workspace entry").file_name() == ".vtcode"),
            "persisting a plan must not create implementation files before explicit approval"
        );

        let (command_tx, _command_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_tx);
        let mut session = InlineSession {
            handle: handle.clone(),
            events: event_rx,
            worker: None,
        };
        event_tx
            .send(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(selection))))
            .expect("submit explicit plan approval selection");

        let approval_workspace_root = tool_registry.workspace_root().clone();
        let outcome = execute_plan_approval(
            &mut tool_registry,
            &mut plan_session,
            &handle,
            &mut session,
            &Arc::new(crate::agent::runloop::unified::state::CtrlCState::new()),
            &Arc::new(tokio::sync::Notify::new()),
            PlanApprovalRequestContext {
                plan: &plan,
                skip_confirmations: false,
                full_auto: false,
                context_usage_percent: 0,
                editor: vtcode_core::config::EditorToolConfig::default(),
                workspace_root: approval_workspace_root,
            },
            PlanApprovalTelemetryContext {
                emitter: None,
                thread_id: "thread-test",
                turn_id: "turn-test",
            },
        )
        .await
        .expect("explicit approval selection should hand off the persisted plan");

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy {
                target,
            } if target.agent_name() == expected_agent
                && target.skip_confirmations == expected_skip_confirmations
                && target.execution_context == PlanExecutionContext::Current
        ));
        assert!(
            fs::read_dir(workspace.path())
                .expect("read workspace after approval handoff")
                .all(|entry| entry.expect("read workspace entry").file_name() == ".vtcode"),
            "approval handoff must remain execution-pending and must not write implementation files"
        );
    }
}

#[tokio::test]
async fn approval_input_without_plan_synthesizes_before_approval() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct PlanProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for PlanProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(uni::LLMResponse {
                content: Some(
                    "<proposed_plan>\nSummary: optimize one startup subsystem.\n1. Measure -> src/main.rs -> verify: cargo check --locked\nValidation: run the focused startup test.\nAssumptions: preserve public APIs.\n</proposed_plan>"
                        .to_string(),
                ),
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

    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.set_provider(Box::new(PlanProvider { calls: calls.clone() }));

    let mut history = vec![uni::Message::user("yes".to_string())];
    run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("missing-plan approval should continue to synthesis");

    assert_eq!(calls.load(Ordering::SeqCst), 1, "the missing-plan path must synthesize exactly once");
    assert!(
        history
            .iter()
            .any(|message| { message.content.as_text().contains("no completed plan draft exists yet") })
    );
}

#[tokio::test]
async fn single_tool_free_text_answer_completes_without_fallback() {
    #[derive(Clone)]
    struct LongAnswerProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for LongAnswerProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            let body = "VT Code stands out through its focus on being a secure, extensible, terminal-native coding agent. \
                It splits functionality into focused crates for providers, configuration, UI, indexing, memory, safety, MCP, ACP, plugins, and evaluation. \
                Command execution is treated as an adversarial boundary with workspace checks and fail-closed behavior. \
                The runtime uses a structured ThreadEvent contract for replay and observability. \
                The terminal remains the core experience while supporting Zed, MCP, browser bridges, skills, and plugins. \
                All set. Summary complete.";
            Ok(uni::LLMResponse {
                content: Some(body.to_string()),
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_provider(Box::new(LongAnswerProvider { requests: requests.clone() }));

    let mut history = vec![uni::Message::user("tell me about this project".to_string())];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("single text answer should complete");

    assert!(
        matches!(outcome.result, TurnLoopResult::Completed { plan_approved_execution_pending: false }),
        "simple Q&A must stay Completed, got {:?}",
        outcome.result
    );
    assert!(!outcome.final_response_was_fallback, "valid model answer must not be labeled fallback");
    assert_eq!(requests.load(Ordering::SeqCst), 1, "simple answer needs exactly one request");
    assert!(
        !history
            .iter()
            .any(|message| { message.content.as_text().contains(COMPLETED_TURN_FALLBACK_RESPONSE) }),
        "must not inject the generic fallback for a valid answer"
    );
    assert!(
        history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && message.content.as_text().contains("VT Code stands out")
        }),
        "final answer must be retained as FinalAnswer"
    );
}

#[tokio::test]
async fn ensure_completed_reuses_found_final_without_requiring_prior_render() {
    // Regression for simple requests ending Blocked with
    // COMPLETED_TURN_FALLBACK_REASON: a valid FinalAnswer already in history
    // (e.g. streamed output that missed the render flag) must publish and
    // succeed, not synthesize the generic fallback.
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut history = vec![
        uni::Message::user("tell me about this project".to_string()),
        uni::Message::assistant("VT Code stands out as a secure terminal agent.".to_string())
            .with_phase(Some(uni::AssistantPhase::FinalAnswer)),
    ];
    let history_len_before = history.len();
    let mut ctx = backing.turn_loop_context();
    assert!(!ctx.harness_state.final_response_rendered());
    assert!(!ctx.harness_state.final_response_was_fallback());

    let was_fallback = ensure_completed_turn_response(&mut ctx, &mut history, 0).expect("publish must succeed");

    assert!(!was_fallback, "found model answer must not be labeled fallback");
    assert_eq!(history.len(), history_len_before, "must not inject a second fallback answer");
    assert!(!ctx.harness_state.final_response_was_fallback());
    assert!(ctx.harness_state.final_response_rendered());
    assert!(ctx.harness_state.final_response_event_emitted());
}

#[tokio::test]
async fn ensure_completed_synthesizes_fallback_when_no_final_exists() {
    // Asymmetric counterpart: with no FinalAnswer in the turn slice, the
    // helper must synthesize COMPLETED_TURN_FALLBACK_RESPONSE and report fallback.
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut history = vec![uni::Message::user("do work".to_string())];
    let mut ctx = backing.turn_loop_context();

    let was_fallback = ensure_completed_turn_response(&mut ctx, &mut history, 0).expect("fallback must succeed");

    assert!(was_fallback, "missing final must be reported as fallback");
    assert!(ctx.harness_state.final_response_was_fallback());
    assert!(
        history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && message.content.as_text().contains(COMPLETED_TURN_FALLBACK_RESPONSE)
        }),
        "must push the generic fallback as FinalAnswer"
    );
}

#[tokio::test]
async fn ensure_completed_synthesizes_planning_fallback_when_planning_active() {
    // Planning counterpart: with no FinalAnswer and planning active, the
    // helper must publish the planning resumable handoff (keep planning +
    // `<proposed_plan>` contract), not the generic retry text.
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    let mut history = vec![uni::Message::user("plan the fix".to_string())];
    let mut ctx = backing.turn_loop_context();

    let was_fallback = ensure_completed_turn_response(&mut ctx, &mut history, 0).expect("fallback must succeed");

    assert!(was_fallback, "missing final must be reported as fallback");
    assert!(ctx.harness_state.final_response_was_fallback());
    assert!(
        history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && message.content.as_text().contains(PLANNING_COMPLETED_FALLBACK_RESPONSE)
                && message.content.as_text().contains("keep planning")
                && message.content.as_text().contains("<proposed_plan>")
        }),
        "must push the planning fallback as FinalAnswer"
    );
    assert!(
        history.iter().all(|message| {
            message.role != uni::MessageRole::Assistant
                || !message.content.as_text().contains(COMPLETED_TURN_FALLBACK_RESPONSE)
        }),
        "planning fallback must not contain the generic retry text"
    );
}

#[tokio::test]
async fn ensure_completed_preserves_explicit_recovery_flag() {
    // Recovery fallbacks mark the flag at creation; a found final with the
    // flag set must stay fallback so the outer loop still blocks resumably.
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut history = vec![
        uni::Message::user("summarize".to_string()),
        uni::Message::assistant("recovery synthesis output".to_string())
            .with_phase(Some(uni::AssistantPhase::FinalAnswer)),
    ];
    let mut ctx = backing.turn_loop_context();
    ctx.harness_state.mark_final_response_fallback();

    let was_fallback = ensure_completed_turn_response(&mut ctx, &mut history, 0).expect("publish must succeed");

    assert!(was_fallback, "explicit recovery flag must survive even with a found final");
}

#[tokio::test]
async fn plan_mode_auto_continue_directive_reaches_provider_without_early_exit() {
    // Regression for checkpoint turn_857: the harness-generated plan-mode
    // auto-continue directive contains the phrase "do not implement", which
    // normalizes to a `STAY_PHRASES` entry. When the planning exit trigger
    // consumed that directive as a genuine user stay message, the turn broke
    // before any provider request, published the completed-turn fallback, and
    // the outer loop re-queued the same directive forever.
    #[derive(Clone)]
    struct PlanAnsweringProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for PlanAnsweringProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            Ok(uni::LLMResponse {
                content: Some(format!("<proposed_plan>\n{STREAMED_VALID_PLAN}\n</proposed_plan>")),
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.set_provider(Box::new(PlanAnsweringProvider { requests: requests.clone() }));
    let directive = crate::agent::runloop::unified::turn::tool_outcomes::helpers::plan_mode_continue_follow_up();
    backing.queue_follow_up_input_for_test(&directive);

    // The seeded message must be neutral: a bare `continue` is an exit alias
    // and would end the turn before the directive is ever consumed.
    let mut history = vec![uni::Message::user("please research the current work".to_string())];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("an auto-continue directive must not break the turn before a provider request");

    assert!(
        matches!(outcome.result, TurnLoopResult::Completed { .. }),
        "the directive must run a normal plan turn, got {:?}",
        outcome.result
    );
    assert!(
        history.last().is_some_and(|message| {
            message.role == uni::MessageRole::User
                && crate::agent::runloop::unified::planning_workflow::exit_trigger::is_plan_mode_auto_continue_directive(
                    &message.content.as_text(),
                )
        }),
        "the directive must be steered into history as the last user message"
    );
    assert!(
        requests.load(Ordering::SeqCst) >= 1,
        "the directive must reach the provider instead of breaking the turn early"
    );
    assert!(
        !history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.content.as_text().contains(PLANNING_COMPLETED_FALLBACK_RESPONSE)
        }),
        "an auto-continue directive must not publish the completed-turn fallback"
    );
}
