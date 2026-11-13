pub(super) use std::collections::BTreeSet;

pub(super) use super::{
    HandleTurnProcessingResultParams, handle_turn_processing_result, record_assistant_tool_calls,
    should_suppress_pre_tool_result_claim,
};
pub(super) use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
pub(super) use crate::agent::runloop::unified::turn::context::{
    NoOutputFacts, PreparedAssistantToolCall, TurnHandlerOutcome, TurnLoopResult, TurnProcessingResult,
};
pub(super) use crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker;
pub(super) use crate::agent::runloop::unified::turn::turn_loop::RECOVERY_CONTRACT_VIOLATION_REASON;
pub(super) use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
pub(super) use vtcode_core::llm::provider as uni;

fn prepared_command_tool_call() -> PreparedAssistantToolCall {
    PreparedAssistantToolCall::new(uni::ToolCall::function(
        "call_1".to_string(),
        "exec_command".to_string(),
        r#"{"action":"run","command":"cargo clippy"}"#.to_string(),
    ))
}

async fn drive_tracker_poll(
    backing: &mut TestTurnProcessingBacking,
    tracker: &mut LoopTracker,
    modified: &mut BTreeSet<std::path::PathBuf>,
    step: usize,
) -> TurnHandlerOutcome {
    let mut ctx = backing.turn_processing_context();
    let calls = vec![PreparedAssistantToolCall::new(uni::ToolCall::function(
        format!("poll-{step}"),
        "task_tracker".into(),
        serde_json::json!({"action":"list"}).to_string(),
    ))];
    handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::ToolCalls {
            tool_calls: calls,
            assistant_text: "I will finish the last step.".into(),
            reasoning: vec![],
            reasoning_details: None,
        },
        response_streamed: false,
        step_count: step,
        repeated_tool_attempts: tracker,
        turn_modified_files: modified,
        max_tool_loops: 20,
        tool_repeat_limit: 3,
    })
    .await
    .unwrap()
}

/// Drive `step` text responses through the handler, returning the last outcome.
async fn drive_pending_texts(
    backing: &mut TestTurnProcessingBacking,
    repeated_tool_attempts: &mut LoopTracker,
    turn_modified_files: &mut BTreeSet<std::path::PathBuf>,
    steps: u32,
) -> TurnHandlerOutcome {
    let mut outcome = TurnHandlerOutcome::Continue;
    for step in 1..=steps {
        let mut ctx = backing.turn_processing_context();
        outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::TextResponse {
                text: "Still working.".to_string(),
                reasoning: Vec::new(),
                reasoning_details: None,
                proposed_plan: None,
            },
            response_streamed: false,
            step_count: step as usize,
            repeated_tool_attempts: &mut *repeated_tool_attempts,
            turn_modified_files: &mut *turn_modified_files,
            max_tool_loops: 8,
            tool_repeat_limit: 4,
        })
        .await
        .expect("pending-verification text should be handled");
        if !matches!(outcome, TurnHandlerOutcome::Continue)
            || repeated_tool_attempts.auto_verification_executed
            || !repeated_tool_attempts.verification_is_pending()
        {
            break;
        }
    }
    outcome
}

/// Drive one model-issued tool call through the normal dispatch pipeline,
/// returning nothing; assertions read the tracker and history afterwards.
async fn drive_model_tool_call(
    backing: &mut TestTurnProcessingBacking,
    repeated_tool_attempts: &mut LoopTracker,
    turn_modified_files: &mut BTreeSet<std::path::PathBuf>,
    tool_name: &str,
    args_json: &str,
) {
    use crate::agent::runloop::unified::turn::tool_outcomes::ToolOutcomeContext;
    use crate::agent::runloop::unified::turn::tool_outcomes::handle_tool_calls;

    let mut ctx = backing.turn_processing_context();
    let call = PreparedAssistantToolCall::new(uni::ToolCall::function(
        "call-rewrite-e2e".to_string(),
        tool_name.to_string(),
        args_json.to_string(),
    ));
    let mut t_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut *repeated_tool_attempts,
        turn_modified_files: &mut *turn_modified_files,
    };
    handle_tool_calls(&mut t_ctx, std::slice::from_ref(&call))
        .await
        .expect("model tool call should dispatch");
}

/// A complete, parseable tool call takes the contract-violation path even
/// when the surrounding prose honestly reports the disabled state: the
/// disclosure is preserved via the recorded salvage (which feeds the
/// labeled fallback), but the turn must not complete with an action
/// attempt pending. No disclosure exception exists because the retry
/// directive itself contains "tools are disabled", so any exception would
/// be echoable from history (see
/// `recovery_complete_tool_call_with_appended_disclosure_still_breaks`).
#[tokio::test]
async fn recovery_complete_tool_call_with_disclosure_breaks_as_violation() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.activate_recovery("loop detector");
    assert!(ctx.consume_recovery_pass());

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::TextResponse {
            text: r#"The requested change was not applied because tools were disabled.
<minimax:tool_call>
<invoke name="apply_patch">
<parameter name="action">write</parameter>
<parameter name="path">README.md</parameter>
</invoke>
</minimax:tool_call>
Please re-run with tools enabled."#
                .to_string(),
            reasoning: Vec::new(),
            reasoning_details: None,
            proposed_plan: None,
        },
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
        max_tool_loops: 4,
        tool_repeat_limit: 4,
    })
    .await
    .expect("complete tool-call markup should take the violation path");

    assert!(
        matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) })
            if reason == RECOVERY_CONTRACT_VIOLATION_REASON
        ),
        "a complete textual tool call must break with a contract violation even with honest disclosure prose"
    );
    let salvaged = backing
        .take_recovery_rejected_synthesis_for_test()
        .expect("violation should record salvaged prose");
    assert!(
        salvaged.contains("The requested change was not applied because tools were disabled."),
        "honest disclosure prose must be preserved for the fallback, got: {salvaged}"
    );
    assert!(
        salvaged.contains("Please re-run with tools enabled."),
        "trailing guidance must be preserved for the fallback, got: {salvaged}"
    );
    assert!(!backing.last_history_message_contains("<invoke"));
}

/// Drive the deterministic planning ladder to the tool-free synthesis pass:
/// two empty responses, with the first recovery pass consumed. The next
/// response helper admits and consumes the first synthesis request.
async fn advance_planning_recovery_to_tool_free_pass(
    backing: &mut TestTurnProcessingBacking,
    repeated_tool_attempts: &mut LoopTracker,
    turn_modified_files: &mut BTreeSet<std::path::PathBuf>,
) {
    for step in 1..=2usize {
        let outcome = {
            let mut ctx = backing.turn_processing_context();
            handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::Empty { facts: NoOutputFacts::default() },
                response_streamed: false,
                step_count: step,
                repeated_tool_attempts,
                turn_modified_files,
                max_tool_loops: 4,
                tool_repeat_limit: 4,
            })
            .await
            .expect("empty response should arm the deterministic recovery ladder")
        };
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        if step == 1 {
            let mut ctx = backing.turn_processing_context();
            assert!(ctx.consume_recovery_pass());
        }
    }
}

async fn handle_planning_empty_with(
    backing: &mut TestTurnProcessingBacking,
    facts: NoOutputFacts,
    step_count: usize,
    repeated_tool_attempts: &mut LoopTracker,
    turn_modified_files: &mut BTreeSet<std::path::PathBuf>,
) -> TurnHandlerOutcome {
    let mut ctx = backing.turn_processing_context();
    let model = ctx.config.model.clone();
    assert!(
        ctx.admit_planning_recovery_request(&model)
            .await
            .expect("admit actual synthesis request")
    );
    assert!(ctx.consume_recovery_pass());
    let response = uni::LLMResponse {
        content: None,
        model,
        tool_calls: facts.dropped_tool_calls.then(|| {
            vec![uni::ToolCall::function(
                "forbidden-research".into(),
                "read_file".into(),
                "{}".into(),
            )]
        }),
        usage: Some(uni::Usage {
            completion_tokens: facts.completion_tokens,
            ..Default::default()
        }),
        finish_reason: uni::FinishReason::Stop,
        reasoning: facts.reasoning_only.then(|| "private planning reasoning".into()),
        reasoning_details: None,
        organization_id: None,
        request_id: None,
        tool_references: Vec::new(),
        compaction: None,
    };
    ctx.record_planning_response(&response)
        .await
        .expect("record observed response shape");
    handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::Empty { facts },
        response_streamed: false,
        step_count,
        repeated_tool_attempts,
        turn_modified_files,
        max_tool_loops: 4,
        tool_repeat_limit: 4,
    })
    .await
    .expect("empty synthesis response should be handled by the result handler")
}

fn capture_planning_recovery_diagnostics(
    backing: &mut TestTurnProcessingBacking,
) -> std::sync::Arc<std::sync::Mutex<Vec<vtcode_commons::diagnostics::Diagnostic>>> {
    let diagnostics = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = diagnostics.clone();
    backing
        .turn_processing_context()
        .renderer
        .set_diagnostic_sink(std::sync::Arc::new(move |diagnostic| {
            captured.lock().expect("captured diagnostics").push(diagnostic.clone());
            Ok(())
        }));
    diagnostics
}

mod assertions_piped;
mod completion_navigation;
mod harness_verification;
mod incident_records;
mod planning_recovery;
mod recovery_empty;
mod recovery_markers;
mod retained_draft;
