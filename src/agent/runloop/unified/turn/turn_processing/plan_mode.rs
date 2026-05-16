//! Agent Legibility:
//! - Entrypoint: `InterviewResearchContext` and the plan-mode helpers in this root drive interview synthesis, draft validation, and tracker shaping.
//! - Common changes:
//!   - Interview and research orchestration starts here.
//!   - Plan draft extraction, tracker snippets, and question synthesis flow through this root and its `plan_mode/` support directory.
//! - Constraints: This file remains an active TD-005 hotspot; keep new helpers in support modules when possible.
//! - Verify: `cargo check -p vtcode && cargo test -p vtcode --bin vtcode inline_events::tests`

use std::time::Duration;

use serde_json::{Value, json};
use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::handlers::plan_mode::{PlanModeState, validate_plan_content};

#[path = "plan_mode/interview_context.rs"]
mod interview_context;
#[path = "plan_mode/interview_payload.rs"]
mod interview_payload;

use super::response_processing::prepare_tool_calls;
use crate::agent::runloop::unified::plan_blocks::extract_any_plan;
use crate::agent::runloop::unified::turn::context::TurnProcessingResult;
use crate::agent::runloop::unified::turn::turn_processing::extract_interview_questions;
use interview_context::{
    InterviewResearchContext, collect_interview_research_context, has_open_decision_markers,
    select_best_plan_validation,
};
use interview_context::load_plan_draft_context;
use interview_payload::{build_adaptive_fallback_interview_args, single_line};
use interview_payload::{
    build_fallback_question, parse_interview_payload_from_text,
    sanitize_generated_interview_payload,
};

const MIN_PLAN_MODE_TURNS_BEFORE_INTERVIEW: usize = 1;
const PLAN_MODE_REMINDER: &str = vtcode_core::prompts::system::PLAN_MODE_IMPLEMENT_REMINDER;
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

pub(crate) fn plan_mode_interview_ready(
    session_stats: &crate::agent::runloop::unified::state::SessionStats,
) -> bool {
    has_discovery_tool(session_stats)
        && session_stats.plan_mode_turns() >= MIN_PLAN_MODE_TURNS_BEFORE_INTERVIEW
}

pub(crate) fn should_attempt_dynamic_interview_generation(
    processing_result: &TurnProcessingResult,
    response_text: Option<&str>,
    session_stats: &crate::agent::runloop::unified::state::SessionStats,
) -> bool {
    let response_has_plan = response_text
        .map(|text| text.contains("<proposed_plan>"))
        .unwrap_or(false);
    if !plan_mode_interview_ready(session_stats) && !response_has_plan {
        return false;
    }

    if turn_result_has_interview_tool_call(processing_result) {
        return false;
    }

    let need_state = interview_need_state(response_text, session_stats);

    if need_state.response_has_plan {
        return need_state.needs_interview;
    }

    if session_stats.plan_mode_interview_pending() {
        return need_state.needs_interview;
    }

    need_state.needs_interview
}

fn interview_need_state(
    response_text: Option<&str>,
    session_stats: &crate::agent::runloop::unified::state::SessionStats,
) -> InterviewNeedState {
    let response_has_plan = response_text
        .map(|text| text.contains("<proposed_plan>"))
        .unwrap_or(false);
    let has_open_decisions = response_text
        .map(has_open_decision_markers)
        .unwrap_or(false);
    let has_completed_interview = session_stats.plan_mode_interview_cycles_completed() > 0;
    let interview_cancelled = session_stats.plan_mode_last_interview_cancelled();

    InterviewNeedState {
        response_has_plan,
        needs_interview: !has_completed_interview || interview_cancelled || has_open_decisions,
    }
}

pub(crate) async fn synthesize_plan_mode_interview_args(
    provider_client: &mut Box<dyn uni::LLMProvider>,
    active_model: &str,
    working_history: &[uni::Message],
    response_text: Option<&str>,
    session_stats: &crate::agent::runloop::unified::state::SessionStats,
    plan_state: Option<PlanModeState>,
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
        "You generate Plan Mode interview payloads for request_user_input.\n\
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

fn strip_assistant_text(processing_result: TurnProcessingResult) -> TurnProcessingResult {
    match processing_result {
        TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text: _,
            reasoning,
            reasoning_details,
        } => TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text: String::new(),
            reasoning,
            reasoning_details,
        },
        TurnProcessingResult::TextResponse { .. } => TurnProcessingResult::Empty,
        TurnProcessingResult::Empty => processing_result,
    }
}

fn append_plan_mode_reminder_text(text: &str) -> String {
    if text.contains(PLAN_MODE_REMINDER) || text.trim().is_empty() {
        return text.to_string();
    }

    let separator = if text.ends_with('\n') { "\n" } else { "\n\n" };
    format!("{text}{separator}{PLAN_MODE_REMINDER}")
}

fn maybe_append_plan_mode_reminder(
    processing_result: TurnProcessingResult,
) -> TurnProcessingResult {
    match processing_result {
        TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text,
            reasoning,
            reasoning_details,
        } => TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text: append_plan_mode_reminder_text(&assistant_text),
            reasoning,
            reasoning_details,
        },
        TurnProcessingResult::TextResponse {
            text,
            reasoning,
            reasoning_details,
            proposed_plan,
        } => {
            let reminder_text = if text.trim().is_empty() && proposed_plan.is_some() {
                PLAN_MODE_REMINDER.to_string()
            } else {
                append_plan_mode_reminder_text(&text)
            };
            TurnProcessingResult::TextResponse {
                text: reminder_text,
                reasoning,
                reasoning_details,
                proposed_plan,
            }
        }
        TurnProcessingResult::Empty => processing_result,
    }
}

pub(crate) fn maybe_force_plan_mode_interview(
    processing_result: TurnProcessingResult,
    response_text: Option<&str>,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    conversation_len: usize,
    synthesized_interview_args: Option<Value>,
) -> TurnProcessingResult {
    let allow_interview = plan_mode_interview_ready(session_stats);
    let need_state = interview_need_state(response_text, session_stats);
    let response_has_plan = need_state.response_has_plan;

    if response_has_plan {
        let processing_result = filter_interview_tool_calls(
            processing_result,
            session_stats,
            allow_interview,
            response_has_plan,
            need_state.needs_interview,
        )
        .processing_result;

        if allow_interview && need_state.needs_interview {
            let stripped = strip_assistant_text(processing_result);
            return inject_plan_mode_interview(
                stripped,
                session_stats,
                conversation_len,
                response_text,
                synthesized_interview_args,
            );
        }

        return maybe_append_plan_mode_reminder(processing_result);
    }

    let filter_outcome = filter_interview_tool_calls(
        processing_result,
        session_stats,
        allow_interview,
        response_has_plan,
        need_state.needs_interview,
    );
    let processing_result = filter_outcome.processing_result;
    let has_interview_tool_calls = filter_outcome.had_interview_tool_calls;
    let has_non_interview_tool_calls = filter_outcome.had_non_interview_tool_calls;

    if session_stats.plan_mode_interview_pending() {
        if !need_state.needs_interview {
            session_stats.clear_plan_mode_interview_pending();
            return processing_result;
        }

        if has_interview_tool_calls && allow_interview {
            session_stats.mark_plan_mode_interview_shown();
            return processing_result;
        }

        if has_non_interview_tool_calls {
            return processing_result;
        }

        if !allow_interview {
            return processing_result;
        }

        return inject_plan_mode_interview(
            processing_result,
            session_stats,
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
            session_stats.mark_plan_mode_interview_shown();
        }
        return processing_result;
    }

    if has_interview_tool_calls {
        if allow_interview {
            session_stats.mark_plan_mode_interview_shown();
        } else {
            session_stats.mark_plan_mode_interview_pending();
        }
        return processing_result;
    }

    if has_non_interview_tool_calls {
        if need_state.needs_interview {
            session_stats.mark_plan_mode_interview_pending();
        }
        return processing_result;
    }

    if !allow_interview || !need_state.needs_interview {
        return processing_result;
    }

    inject_plan_mode_interview(
        processing_result,
        session_stats,
        conversation_len,
        response_text,
        synthesized_interview_args,
    )
}

fn inject_plan_mode_interview(
    processing_result: TurnProcessingResult,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    conversation_len: usize,
    _response_text: Option<&str>,
    synthesized_interview_args: Option<Value>,
) -> TurnProcessingResult {
    use vtcode_core::config::constants::tools;

    let args = synthesized_interview_args.unwrap_or_else(|| {
        json!({
            "questions": [
                build_fallback_question(
                    "scope",
                    "Scope",
                    "What is the highest-impact planning decision still missing before implementation can start?",
                    &InterviewResearchContext {
                        discovery_tools_used: Vec::new(),
                        recent_targets: Vec::new(),
                        risk_hints: Vec::new(),
                        open_decision_hints: Vec::new(),
                        goal_hints: Vec::new(),
                        verification_hints: Vec::new(),
                        custom_note_policy: CUSTOM_NOTE_POLICY.to_string(),
                        plan_draft_excerpt: None,
                        plan_draft_path: None,
                        plan_validation: None,
                        task_tracker_excerpt: None,
                        task_tracker_path: None,
                        task_tracker_summary: None,
                    },
                )
            ]
        })
    });
    let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
    let call_id = format!("call_plan_interview_{}", conversation_len);
    let call = uni::ToolCall::function(call_id, tools::REQUEST_USER_INPUT.to_string(), args_json);

    session_stats.mark_plan_mode_interview_shown();

    match processing_result {
        TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text,
            reasoning,
            reasoning_details,
        } => {
            let mut raw_tool_calls = tool_calls
                .into_iter()
                .map(|tool_call| tool_call.into_raw_call())
                .collect::<Vec<_>>();
            raw_tool_calls.push(call);
            TurnProcessingResult::ToolCalls {
                tool_calls: prepare_tool_calls(raw_tool_calls),
                assistant_text,
                reasoning,
                reasoning_details,
            }
        }
        TurnProcessingResult::TextResponse {
            text,
            reasoning,
            reasoning_details,
            proposed_plan: _,
        } => TurnProcessingResult::ToolCalls {
            tool_calls: prepare_tool_calls(vec![call]),
            assistant_text: text,
            reasoning,
            reasoning_details,
        },
        TurnProcessingResult::Empty => TurnProcessingResult::ToolCalls {
            tool_calls: prepare_tool_calls(vec![call]),
            assistant_text: String::new(),
            reasoning: Vec::new(),
            reasoning_details: None,
        },
    }
}

fn turn_result_has_interview_tool_call(processing_result: &TurnProcessingResult) -> bool {
    use vtcode_core::config::constants::tools;

    let TurnProcessingResult::ToolCalls { tool_calls, .. } = processing_result else {
        return false;
    };
    tool_calls
        .iter()
        .any(|call| call.tool_name() == tools::REQUEST_USER_INPUT)
}

struct InterviewToolCallFilter {
    processing_result: TurnProcessingResult,
    had_interview_tool_calls: bool,
    had_non_interview_tool_calls: bool,
}

fn filter_interview_tool_calls(
    processing_result: TurnProcessingResult,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    allow_interview: bool,
    response_has_plan: bool,
    needs_interview: bool,
) -> InterviewToolCallFilter {
    use vtcode_core::config::constants::tools;

    let TurnProcessingResult::ToolCalls {
        tool_calls,
        assistant_text,
        reasoning,
        reasoning_details,
    } = processing_result
    else {
        return InterviewToolCallFilter {
            processing_result,
            had_interview_tool_calls: false,
            had_non_interview_tool_calls: false,
        };
    };

    let mut had_interview = false;
    let mut had_non_interview = false;
    let mut filtered = Vec::with_capacity(tool_calls.len());

    for call in tool_calls {
        let is_interview = call.tool_name() == tools::REQUEST_USER_INPUT;

        if is_interview {
            had_interview = true;
            if allow_interview && !response_has_plan {
                filtered.push(call);
            }
        } else {
            had_non_interview = true;
            filtered.push(call);
        }
    }

    if needs_interview
        && had_interview
        && (had_non_interview || !allow_interview)
        && !response_has_plan
    {
        session_stats.mark_plan_mode_interview_pending();
    }

    let processing_result = if filtered.is_empty() {
        if assistant_text.trim().is_empty() {
            TurnProcessingResult::ToolCalls {
                tool_calls: Vec::new(),
                assistant_text,
                reasoning,
                reasoning_details,
            }
        } else {
            TurnProcessingResult::TextResponse {
                text: assistant_text,
                reasoning,
                reasoning_details,
                proposed_plan: None,
            }
        }
    } else {
        TurnProcessingResult::ToolCalls {
            tool_calls: filtered,
            assistant_text,
            reasoning,
            reasoning_details,
        }
    };

    InterviewToolCallFilter {
        processing_result,
        had_interview_tool_calls: had_interview,
        had_non_interview_tool_calls: had_non_interview,
    }
}

#[cfg(test)]
mod tests;
