//! Agent Legibility:
//! - Entrypoint: `InterviewResearchContext` and the planning helpers in this root drive interview synthesis, draft validation, and tracker shaping.
//! - Common changes:
//!   - Interview and research orchestration starts here.
//!   - Plan draft extraction, tracker snippets, and question synthesis flow through this root and its `planning_workflow/` support directory.
//! - Constraints: This file remains an active TD-005 hotspot; keep new helpers in support modules when possible.
//! - Verify: `cargo check -p vtcode && cargo test -p vtcode --bin vtcode inline_events::tests`

use std::time::Duration;

use serde_json::Value;
use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::handlers::planning_workflow::{
    PlanningWorkflowState, validate_plan_content,
};

use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;

#[path = "planning_workflow/interview_context.rs"]
mod interview_context;
#[path = "planning_workflow/interview_forcing.rs"]
mod interview_forcing;
#[path = "planning_workflow/interview_payload.rs"]
mod interview_payload;

use crate::agent::runloop::unified::plan_blocks::extract_any_plan;
use crate::agent::runloop::unified::turn::context::TurnProcessingResult;
use crate::agent::runloop::unified::turn::turn_processing::extract_interview_questions;
use interview_context::load_plan_draft_context;
use interview_context::{
    collect_interview_research_context, has_open_decision_markers, select_best_plan_validation,
};
use interview_forcing::{
    filter_interview_tool_calls, inject_planning_workflow_interview,
    maybe_append_planning_workflow_reminder, strip_assistant_text,
    turn_result_has_interview_tool_call,
};
use interview_payload::{build_adaptive_fallback_interview_args, single_line};
use interview_payload::{parse_interview_payload_from_text, sanitize_generated_interview_payload};

#[cfg(test)]
use interview_context::InterviewResearchContext;

#[cfg(test)]
use super::response_processing::prepare_tool_calls;

const MIN_PLANNING_WORKFLOW_TURNS_BEFORE_INTERVIEW: usize = 1;
const PLANNING_WORKFLOW_REMINDER: &str =
    vtcode_core::prompts::system::PLANNING_WORKFLOW_IMPLEMENT_REMINDER;
const INTERVIEW_SYNTHESIS_TIMEOUT_SECS: u64 = 20;
const MAX_RESEARCH_SNIPPETS_PER_BUCKET: usize = 6;
const CUSTOM_NOTE_POLICY: &str =
    "Users can always type custom notes/free-form responses for every question.";
const MAX_PLAN_DRAFT_CHARS: usize = 2400;
const MAX_TASK_TRACKER_CHARS: usize = 1400;
const PLAN_TRACKER_START: &str = "<!-- vtcode:plan-tracker:start -->";
const PLAN_TRACKER_END: &str = "<!-- vtcode:plan-tracker:end -->";

#[derive(Debug, Clone, Copy)]
struct InterviewNeedState {
    response_has_plan: bool,
    needs_interview: bool,
}

fn has_discovery_tool(session_stats: &crate::agent::runloop::unified::state::SessionStats) -> bool {
    [
        tools::READ_FILE,
        tools::LIST_FILES,
        tools::GREP_FILE,
        tools::UNIFIED_SEARCH,
    ]
    .iter()
    .any(|tool| session_stats.has_tool(tool))
}

pub(crate) fn planning_workflow_interview_ready(
    session_stats: &crate::agent::runloop::unified::state::SessionStats,
    plan_session: &PlanningWorkflowSessionState,
) -> bool {
    has_discovery_tool(session_stats)
        && plan_session.turns() >= MIN_PLANNING_WORKFLOW_TURNS_BEFORE_INTERVIEW
}

pub(crate) fn should_attempt_dynamic_interview_generation(
    processing_result: &TurnProcessingResult,
    response_text: Option<&str>,
    session_stats: &crate::agent::runloop::unified::state::SessionStats,
    plan_session: &PlanningWorkflowSessionState,
) -> bool {
    let response_has_plan = response_text
        .map(|text| text.contains("<proposed_plan>"))
        .unwrap_or(false);
    if !planning_workflow_interview_ready(session_stats, plan_session) && !response_has_plan {
        return false;
    }

    if turn_result_has_interview_tool_call(processing_result) {
        return false;
    }

    let need_state = interview_need_state(response_text, plan_session);

    if need_state.response_has_plan {
        return need_state.needs_interview;
    }

    if plan_session.interview_pending() {
        return need_state.needs_interview;
    }

    need_state.needs_interview
}

fn interview_need_state(
    response_text: Option<&str>,
    plan_session: &PlanningWorkflowSessionState,
) -> InterviewNeedState {
    let response_has_plan = response_text
        .map(|text| text.contains("<proposed_plan>"))
        .unwrap_or(false);
    let has_open_decisions = response_text
        .map(has_open_decision_markers)
        .unwrap_or(false);
    let has_completed_interview = plan_session.interview_cycles_completed() > 0;
    let interview_cancelled = plan_session.last_interview_cancelled();

    InterviewNeedState {
        response_has_plan,
        needs_interview: !has_completed_interview || interview_cancelled || has_open_decisions,
    }
}

pub(crate) async fn synthesize_planning_workflow_interview_args(
    provider_client: &mut Box<dyn uni::LLMProvider>,
    active_model: &str,
    working_history: &[uni::Message],
    response_text: Option<&str>,
    session_stats: &crate::agent::runloop::unified::state::SessionStats,
    _plan_session: &PlanningWorkflowSessionState,
    plan_state: Option<PlanningWorkflowState>,
) -> Option<Value> {
    let plan_context = load_plan_draft_context(plan_state).await;
    let context = collect_interview_research_context(
        working_history,
        response_text,
        session_stats,
        plan_context.as_ref(),
    );
    let latest_user_request = working_history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::User)
        .map(|message| single_line(message.content.as_text().as_ref()))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "(none)".to_string());
    let system_prompt = format!(
        "You generate Planning workflow interview payloads for request_user_input.\n\
Return strict JSON only (no markdown/prose): {{\"questions\": [...]}}\n\
Constraints:\n\
- 1 to 3 questions\n\
- each question: id snake_case, header <= 12 chars, question is one line\n\
- each question options: 2 or 3 mutually-exclusive options\n\
- recommended option first and include '(Recommended)' in its label\n\
- {CUSTOM_NOTE_POLICY}\n\
Use repository research context to ask questions that close planning decisions for scope, decomposition, and verification."
    );
    let user_prompt = format!(
        "Build context-aware interview questions for this planning state.\n\
Current user request:\n{}\n\
Research context JSON:\n{}\n\
Assistant response snapshot:\n{}\n\
Return JSON only.",
        latest_user_request,
        serde_json::to_string_pretty(&context).ok()?,
        response_text.unwrap_or("(none)")
    );

    let request = uni::LLMRequest {
        messages: vec![uni::Message::user(user_prompt)],
        system_prompt: Some(std::sync::Arc::new(system_prompt)),
        tools: None,
        model: active_model.to_string(),
        temperature: Some(0.2),
        stream: false,
        max_tokens: Some(700),
        ..Default::default()
    };

    let response = tokio::time::timeout(
        Duration::from_secs(INTERVIEW_SYNTHESIS_TIMEOUT_SECS),
        provider_client.generate(request),
    )
    .await;

    let generated = response
        .ok()
        .and_then(Result::ok)
        .and_then(|response| response.content)
        .and_then(|content| parse_interview_payload_from_text(&content))
        .and_then(|payload| sanitize_generated_interview_payload(payload, &context));

    let response_plan_validation = response_text
        .and_then(|text| extract_any_plan(text).plan_text)
        .as_deref()
        .map(validate_plan_content);
    let plan_validation = select_best_plan_validation(
        plan_context
            .as_ref()
            .and_then(|ctx| ctx.plan_validation.as_ref()),
        response_plan_validation.as_ref(),
    );
    let tracker_summary = plan_context
        .as_ref()
        .and_then(|ctx| ctx.tracker_summary.clone());

    generated.or_else(|| {
        build_adaptive_fallback_interview_args(
            &context,
            response_text,
            plan_validation,
            tracker_summary,
        )
    })
}

pub(crate) fn maybe_force_planning_workflow_interview(
    processing_result: TurnProcessingResult,
    response_text: Option<&str>,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    plan_session: &mut PlanningWorkflowSessionState,
    conversation_len: usize,
    synthesized_interview_args: Option<Value>,
) -> TurnProcessingResult {
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
            return inject_planning_workflow_interview(
                stripped,
                plan_session,
                conversation_len,
                response_text,
                synthesized_interview_args,
            );
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
            response_text,
            synthesized_interview_args,
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

    inject_planning_workflow_interview(
        processing_result,
        plan_session,
        conversation_len,
        response_text,
        synthesized_interview_args,
    )
}

#[cfg(test)]
mod tests;
