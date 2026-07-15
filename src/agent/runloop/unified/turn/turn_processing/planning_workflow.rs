//! Agent Legibility:
//! - Entrypoint: `maybe_force_planning_workflow_interview` is the sole orchestrator; it decides via `gating` whether an interview is ready/needed and, if so, injects a static clarifying-question interview via `interview_forcing`.
//! - Common changes:
//!   - Pure readiness/need-state predicates (no I/O) live in `gating.rs` — extend those for new interview-skip conditions.
//!   - Static fallback question shaping lives in `interview_payload.rs` (`build_fallback_question`); plan draft / open-decision detection helpers live in `interview_context.rs`.
//! - Constraints: This file is intentionally kept to orchestration + shared constants only. Put new logic in `gating.rs` (pure predicates) or `interview_payload.rs` (question shaping) — do not grow this root file's function bodies.
//! - Verify: `cargo check -p vtcode && cargo test -p vtcode --bin vtcode inline_events::tests`

use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;

#[path = "planning_workflow/gating.rs"]
mod gating;
#[path = "planning_workflow/interview_context.rs"]
mod interview_context;
#[path = "planning_workflow/interview_forcing.rs"]
mod interview_forcing;
#[path = "planning_workflow/interview_payload.rs"]
mod interview_payload;

use crate::agent::runloop::unified::turn::context::TurnProcessingResult;
use crate::agent::runloop::unified::turn::turn_processing::extract_interview_questions;
use gating::interview_need_state;
pub(crate) use gating::planning_workflow_interview_ready;
use interview_forcing::{
    filter_interview_tool_calls, inject_planning_workflow_interview,
    maybe_append_planning_workflow_reminder, strip_assistant_text,
};

#[cfg(test)]
use super::response_processing::prepare_tool_calls;

#[cfg(test)]
use vtcode_core::llm::provider as uni;

const MIN_PLANNING_WORKFLOW_TURNS_BEFORE_INTERVIEW: usize = 1;
const PLANNING_WORKFLOW_REMINDER: &str =
    vtcode_core::prompts::system::PLANNING_WORKFLOW_IMPLEMENT_REMINDER;

pub(crate) fn maybe_force_planning_workflow_interview(
    processing_result: TurnProcessingResult,
    response_text: Option<&str>,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    plan_session: &mut PlanningWorkflowSessionState,
    conversation_len: usize,
) -> TurnProcessingResult {
    // Do NOT force the interview when budget is exhausted — no further LLM
    // calls are possible and re-forcing would loop forever. The same applies
    // when post-tool recovery is exhausted (saturated planning context):
    // re-forcing the interview would re-research and loop forever. Likewise,
    // once the interview has been permanently denied by policy, forcing it
    // again just repeats the same denial (checkpoint turn_655/turn_660).
    if plan_session.is_budget_exhausted()
        || plan_session.is_recovery_exhausted()
        || plan_session.is_interview_denied()
    {
        return processing_result;
    }
    let allow_interview = planning_workflow_interview_ready(session_stats, plan_session);
    let need_state = interview_need_state(response_text, plan_session);
    let response_has_plan = need_state.response_has_plan;

    if response_has_plan {
        let processing_result = filter_interview_tool_calls(
            processing_result,
            plan_session,
            allow_interview,
            response_has_plan,
            need_state.needs_interview,
        )
        .processing_result;

        if allow_interview && need_state.needs_interview {
            let stripped = strip_assistant_text(processing_result);
            return inject_planning_workflow_interview(stripped, plan_session, conversation_len);
        }

        return maybe_append_planning_workflow_reminder(processing_result);
    }

    let filter_outcome = filter_interview_tool_calls(
        processing_result,
        plan_session,
        allow_interview,
        response_has_plan,
        need_state.needs_interview,
    );
    let processing_result = filter_outcome.processing_result;
    let has_interview_tool_calls = filter_outcome.had_interview_tool_calls;
    let has_non_interview_tool_calls = filter_outcome.had_non_interview_tool_calls;

    if plan_session.interview_pending() {
        if !need_state.needs_interview {
            plan_session.clear_interview_pending();
            return processing_result;
        }

        if has_interview_tool_calls && allow_interview {
            plan_session.mark_interview_shown();
            return processing_result;
        }

        if has_non_interview_tool_calls {
            return processing_result;
        }

        if !allow_interview {
            return processing_result;
        }

        return inject_planning_workflow_interview(
            processing_result,
            plan_session,
            conversation_len,
        );
    }

    let explicit_questions = response_text
        .map(|text| !extract_interview_questions(text).is_empty())
        .unwrap_or(false);
    if explicit_questions {
        if allow_interview {
            plan_session.mark_interview_shown();
        }
        return processing_result;
    }

    if has_interview_tool_calls {
        if allow_interview {
            plan_session.mark_interview_shown();
        } else {
            plan_session.mark_interview_pending();
        }
        return processing_result;
    }

    if has_non_interview_tool_calls {
        if need_state.needs_interview {
            plan_session.mark_interview_pending();
        }
        return processing_result;
    }

    if !allow_interview || !need_state.needs_interview {
        return processing_result;
    }

    inject_planning_workflow_interview(processing_result, plan_session, conversation_len)
}

#[cfg(test)]
mod tests;
