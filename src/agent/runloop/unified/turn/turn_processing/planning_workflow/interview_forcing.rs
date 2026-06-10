use serde_json::{Value, json};
use vtcode_core::llm::provider as uni;

use super::super::response_processing::prepare_tool_calls;
use super::interview_context::InterviewResearchContext;
use super::interview_payload::build_fallback_question;
use super::{CUSTOM_NOTE_POLICY, PLANNING_WORKFLOW_REMINDER};
use crate::agent::runloop::unified::turn::context::TurnProcessingResult;

pub(super) fn strip_assistant_text(
    processing_result: TurnProcessingResult,
) -> TurnProcessingResult {
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

fn append_planning_workflow_reminder_text(text: &str) -> String {
    if text.contains(PLANNING_WORKFLOW_REMINDER) || text.trim().is_empty() {
        return text.to_string();
    }

    let separator = if text.ends_with('\n') { "\n" } else { "\n\n" };
    format!("{text}{separator}{PLANNING_WORKFLOW_REMINDER}")
}

pub(super) fn maybe_append_planning_workflow_reminder(
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
            assistant_text: append_planning_workflow_reminder_text(&assistant_text),
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
                PLANNING_WORKFLOW_REMINDER.to_string()
            } else {
                append_planning_workflow_reminder_text(&text)
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

pub(super) fn inject_planning_workflow_interview(
    processing_result: TurnProcessingResult,
    plan_session: &mut crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState,
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

    plan_session.mark_interview_shown();

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

pub(super) fn turn_result_has_interview_tool_call(
    processing_result: &TurnProcessingResult,
) -> bool {
    use vtcode_core::config::constants::tools;

    let TurnProcessingResult::ToolCalls { tool_calls, .. } = processing_result else {
        return false;
    };
    tool_calls
        .iter()
        .any(|call| call.tool_name() == tools::REQUEST_USER_INPUT)
}

pub(super) struct InterviewToolCallFilter {
    pub(super) processing_result: TurnProcessingResult,
    pub(super) had_interview_tool_calls: bool,
    pub(super) had_non_interview_tool_calls: bool,
}

pub(super) fn filter_interview_tool_calls(
    processing_result: TurnProcessingResult,
    plan_session: &mut crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState,
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
        plan_session.mark_interview_pending();
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
