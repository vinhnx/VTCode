use super::post_tool_recovery::complete_turn_after_failed_tool_free_recovery;
use super::post_tool_recovery::prepare_post_tool_tool_free_recovery;
use super::post_tool_recovery::{ensure_post_tool_resume_directive, has_tool_response_since};
use super::responses::{
    GENERIC_BLOCKED_FINAL_RESPONSE, PLANNING_RECOVERY_SYNTHESIS_FALLBACK, RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER,
    completed_turn_requires_final_response, ensure_blocked_turn_response, ensure_completed_turn_response,
};
use super::{
    COMPLETED_TURN_FALLBACK_RESPONSE, HarnessUsage, PENDING_VERIFICATION_BLOCK_REASON,
    PLANNING_COMPLETED_FALLBACK_RESPONSE, POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON, POST_TOOL_RECOVERY_REASON,
    POST_TOOL_RECOVERY_REASON_PLAN_MODE, POST_TOOL_RESUME_DIRECTIVE, POST_TOOL_TOOL_ENABLED_RETRY_DIRECTIVE,
    PostToolFailureRecovery, RECOVERY_CONTRACT_VIOLATION_REASON, accumulate_turn_usage, blocked_turn_final_response,
    current_turn_preserve_index, finalize_turn, has_turn_usage, maybe_recover_after_post_tool_llm_failure,
    normalize_tool_free_recovery_break_outcome, run_turn_loop,
};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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

// Re-exported for `tests/<cluster>.rs` (pure-move split of this file): grandchildren keep
// addressing these as `super::X`, mirroring the original flat layout.
pub(super) use super::responses::{
    completed_recovery_fallback_reason, is_primary_agent_handoff, provider_stop_outcome,
};
pub(super) use super::{
    ASSISTANT_TEXT_RESPONSE_CAP_REASON, MAX_RECOVERY_RETRIES, RECOVERY_TOOL_CALL_RETRY_DIRECTIVE_PLAN_MODE,
    completed_fallback_reason, is_plan_entry_handoff,
};

const PENDING_VERIFICATION_RESPONSE_MARKER: &str = "Completion remains unverified";
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

pub mod blocked;
pub mod completion;
pub mod handoff;
pub mod mid_turn_model_switch;
pub mod plan;
pub mod preview;
pub mod recovery;
pub mod response_cap;
pub mod streamed_plan;
pub mod verification;
