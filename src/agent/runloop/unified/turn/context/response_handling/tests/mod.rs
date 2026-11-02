pub(super) use super::*;
pub(super) use crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_auto_continue_is_recoverable_block;
pub(super) use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
pub(super) use vtcode_core::tools::handlers::planning_workflow::CANONICAL_STEP_FORMAT;

fn last_final_answer_text(ctx: &TurnProcessingContext<'_>) -> String {
    ctx.working_history
        .iter()
        .rev()
        .find(|message| {
            message.role == uni::MessageRole::Assistant && message.phase == Some(uni::AssistantPhase::FinalAnswer)
        })
        .map(|message| message.content.as_text().into_owned())
        .expect("terminal rejection must publish a final assistant message")
}

fn assert_history_carries_validator_feedback(final_text: &str) {
    assert!(
        final_text.contains("Plan validation issues:") && final_text.contains(CANONICAL_STEP_FORMAT),
        "history must carry validator feedback and canonical step format, got: {final_text}"
    );
}

fn assert_terminal_plan_rejection_is_blocked(outcome: &TurnHandlerOutcome) {
    let TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }) = outcome else {
        panic!("a rejected plan must end blocked instead of reporting completion");
    };
    assert_eq!(reason, "plan artifact rejected; revise the plan before continuing");
    assert!(!tracker_auto_continue_is_recoverable_block(Some(reason)));
}

/// Unparseable pseudo-tool-call markup: the `<tools:call>` name is empty,
/// so every textual parser rejects it, but the pseudo-marker scan still
/// sees `<tool_call`. Mirrors the raw-XML leak from turn_887/turn_888.
const BROKEN_MARKUP_RESPONSE: &str =
    "I need to inspect the workspace.\n<tool_call>\n<tools:call name=\"\">\n</tools:call>\n</tool_call>";

const RECOVERY_VALID_PLAN: &str = r#"# Recovery plan

## Summary
Fix the read-cap recovery path from gathered evidence.

## Implementation Steps
1. Reorder the guard -> files: [src/agent/runloop/unified/turn/tool_outcomes/handlers/guards/read_guard.rs] -> verify: [cargo check --locked]

## Test Cases and Validation
1. Run cargo check --locked.

## Assumptions and Defaults
1. Keep execution-mode caps strict.
"#;

const RECOVERY_INVALID_PLAN: &str = r#"# Recovery plan

## Summary
Fix the read-cap recovery path from gathered evidence.

## Implementation Steps
1. Reorder the guard

## Test Cases and Validation
1. Run the focused test.

## Assumptions and Defaults
1. Keep existing behavior.
"#;

/// Regression fixture from session-vtcode-20260916T033857Z_346951-33962:
/// the plan has otherwise concrete steps, but more than one verification
/// item is rejected by the validator. The repair must see the complete
/// bounded draft and validator-owned feedback before it gets a corrected
/// plan.
const RECOVERY_INVALID_VERIFICATION_PLAN: &str = r#"# Preview recovery plan

## Summary
Repair planning recovery from the evidence already gathered.

## Implementation Steps
1. Update the preview guard -> files: [src/agent/runloop/unified/turn/guards.rs] -> verify: [run checks]
2. Strengthen synthesis guidance -> files: [src/agent/runloop/unified/turn/turn_loop.rs] -> verify: [cargo check --locked]
3. Preserve validator feedback -> files: [src/agent/runloop/unified/turn/context/response_handling.rs] -> verify: [rg -n verify src/agent/runloop/unified/turn]
4. Keep tools disabled during repair -> files: [src/agent/runloop/unified/turn/turn_processing/result_handler.rs] -> verify: [cargo nextest run -p vtcode]
5. Add the transition regression -> files: [src/agent/runloop/unified/turn/turn_loop/tests.rs] -> verify: [git diff --check]

## Test Cases and Validation
1. Run the focused planning recovery test.

## Assumptions and Defaults
1. Keep strict plan validation and bounded recovery.
"#;

const RECOVERY_CORRECTED_VERIFICATION_PLAN: &str = r#"# Preview recovery plan

## Summary
Repair planning recovery from the evidence already gathered with concrete verification.

## Implementation Steps
1. Update the preview guard -> files: [src/agent/runloop/unified/turn/guards.rs] -> verify: [cargo nextest run -p vtcode]
2. Strengthen synthesis guidance -> files: [src/agent/runloop/unified/turn/turn_loop.rs] -> verify: [cargo check --locked]
3. Preserve validator feedback -> files: [src/agent/runloop/unified/turn/context/response_handling.rs] -> verify: [rg -n verify src/agent/runloop/unified/turn]
4. Keep tools disabled during repair -> files: [src/agent/runloop/unified/turn/turn_processing/result_handler.rs] -> verify: [cargo nextest run -p vtcode]
5. Add the transition regression -> files: [src/agent/runloop/unified/turn/turn_loop/tests.rs] -> verify: [cargo nextest run -p vtcode]

## Test Cases and Validation
1. Run cargo nextest run -p vtcode.

## Assumptions and Defaults
1. Keep strict plan validation and bounded recovery.
"#;

/// Tagless markdown plan in the turn_1075 shape: numbered steps but no
/// `<proposed_plan>` tags, no `files:` targets, and no `verify:` checks.
const UNTAGGED_PLANLIKE_TEXT: &str = "## Simple plan to improve startup\n\n1. Measure the current startup path\n2. Identify the slowest steps\n3. Defer non-critical work\n";

/// A valid revision of an already-approved plan, in the canonical section
/// shape the artifact validator requires.
const EXECUTION_REVISION_PLAN: &str = r#"# Revised plan

## Summary
Repairs the approved plan after the referenced paths moved.

## Implementation Steps
1. Update the module path -> files: [src/lib.rs] -> verify: [cargo check]

## Test Cases and Validation
1. Run cargo check --locked.

## Assumptions and Defaults
1. Keep the approved scope.
"#;

// Captured final draft from session 20261015T161453Z: the interview fallback
// consumed this invalid tagless plan before validation could repair it.
const SESSION_161453_PLAN: &str = include_str!("session_161453_untagged_plan.txt");

fn process_planning_candidate(ctx: &mut TurnProcessingContext<'_>, draft: &str) -> TurnProcessingResult {
    use crate::agent::runloop::unified::turn::turn_processing::{
        maybe_force_planning_workflow_interview, process_llm_response,
    };
    let response = uni::LLMResponse::new("test", draft);
    let processed = process_llm_response(&response, ctx.renderer, 37, true, true, true, true, None, None)
        .expect("process planning response");
    maybe_force_planning_workflow_interview(processed, Some(draft), ctx.session_stats, ctx.plan_session, 37)
}

mod denied_detection;
mod execution_replan;
mod plan_repair;
mod pseudo_tool_call;
mod session_cases;
