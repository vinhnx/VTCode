use vtcode_core::llm::provider as uni;

use crate::agent::runloop::unified::turn::context::TurnProcessingResult;
use crate::agent::runloop::unified::turn::turn_processing::extract_interview_questions;

const MIN_PLAN_MODE_TURNS_BEFORE_INTERVIEW: usize = 1;
const PLAN_MODE_REMINDER: &str = vtcode_core::prompts::system::PLAN_MODE_IMPLEMENT_REMINDER;

fn has_discovery_tool(session_stats: &crate::agent::runloop::unified::state::SessionStats) -> bool {
    use vtcode_core::config::constants::tools;

    [
        tools::READ_FILE,
        tools::LIST_FILES,
        tools::GREP_FILE,
        tools::UNIFIED_SEARCH,
        tools::CODE_INTELLIGENCE,
        tools::SPAWN_SUBAGENT,
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

fn strip_assistant_text(processing_result: TurnProcessingResult) -> TurnProcessingResult {
    match processing_result {
        TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text: _,
            reasoning,
        } => TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text: String::new(),
            reasoning,
        },
        TurnProcessingResult::TextResponse { .. } => TurnProcessingResult::Empty,
        TurnProcessingResult::Empty
        | TurnProcessingResult::Completed
        | TurnProcessingResult::Cancelled
        | TurnProcessingResult::Aborted => processing_result,
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
        } => TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text: append_plan_mode_reminder_text(&assistant_text),
            reasoning,
        },
        TurnProcessingResult::TextResponse {
            text,
            reasoning,
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
                proposed_plan,
            }
        }
        TurnProcessingResult::Empty
        | TurnProcessingResult::Completed
        | TurnProcessingResult::Cancelled
        | TurnProcessingResult::Aborted => processing_result,
    }
}

pub(crate) fn maybe_force_plan_mode_interview(
    processing_result: TurnProcessingResult,
    response_text: Option<&str>,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    conversation_len: usize,
) -> TurnProcessingResult {
    let allow_interview = plan_mode_interview_ready(session_stats);

    let response_has_plan = response_text
        .map(|text| text.contains("<proposed_plan>"))
        .unwrap_or(false);

    if response_has_plan {
        if !session_stats.plan_mode_interview_shown() && allow_interview {
            let stripped = strip_assistant_text(processing_result);
            return inject_plan_mode_interview(stripped, session_stats, conversation_len);
        }

        return maybe_append_plan_mode_reminder(processing_result);
    }

    let filter_outcome = filter_interview_tool_calls(
        processing_result,
        session_stats,
        allow_interview,
        response_has_plan,
    );
    let processing_result = filter_outcome.processing_result;
    let has_interview_tool_calls = filter_outcome.had_interview_tool_calls;
    let has_non_interview_tool_calls = filter_outcome.had_non_interview_tool_calls;

    if session_stats.plan_mode_interview_shown() {
        if has_interview_tool_calls {
            session_stats.mark_plan_mode_interview_shown();
        }
        return processing_result;
    }

    if session_stats.plan_mode_interview_pending() {
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

        return inject_plan_mode_interview(processing_result, session_stats, conversation_len);
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
        session_stats.mark_plan_mode_interview_pending();
        return processing_result;
    }

    if !allow_interview {
        return processing_result;
    }

    inject_plan_mode_interview(processing_result, session_stats, conversation_len)
}

fn default_plan_mode_interview_args() -> serde_json::Value {
    serde_json::json!({
        "questions": [
            {
                "id": "goal",
                "header": "Goal",
                "question": "What user-visible outcome should this change deliver, and what constraints or non-goals must be respected?",
                "options": [
                    {
                        "label": "Single outcome metric (Recommended)",
                        "description": "Pick one user-visible result and optimize scope to ship that outcome quickly."
                    },
                    {
                        "label": "Outcome plus hard constraints",
                        "description": "Define the target result and explicitly lock constraints/non-goals up front."
                    },
                    {
                        "label": "MVP scope boundary",
                        "description": "Choose the smallest useful deliverable that still demonstrates user impact."
                    }
                ]
            },
            {
                "id": "constraints",
                "header": "Plan",
                "question": "Break the work into 3-7 composable steps. For each step include target file(s) and a concrete expected outcome.",
                "options": [
                    {
                        "label": "Dependency-first slices (Recommended)",
                        "description": "Order steps by dependency so each slice can be built and validated independently."
                    },
                    {
                        "label": "User-flow slices",
                        "description": "Split by user journey milestones so each step improves one visible workflow."
                    },
                    {
                        "label": "Risk-isolated slices",
                        "description": "Separate high-risk changes into dedicated steps to simplify debugging and rollback."
                    }
                ]
            },
            {
                "id": "verification",
                "header": "Verification",
                "question": "For each step, what exact command or manual check proves it is complete?",
                "options": [
                    {
                        "label": "Command proof per step (Recommended)",
                        "description": "Attach an explicit command/check for each step to make completion objective."
                    },
                    {
                        "label": "Manual behavior proof",
                        "description": "Use concrete user-visible manual checks when automated coverage is not available."
                    },
                    {
                        "label": "Hybrid proof strategy",
                        "description": "Combine automated commands with targeted manual checks for stronger confidence."
                    }
                ]
            }
        ]
    })
}

fn inject_plan_mode_interview(
    processing_result: TurnProcessingResult,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    conversation_len: usize,
) -> TurnProcessingResult {
    use vtcode_core::config::constants::tools;

    let args = default_plan_mode_interview_args();
    let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
    let call_id = format!("call_plan_interview_{}", conversation_len);
    let call = uni::ToolCall::function(call_id, tools::ASK_QUESTIONS.to_string(), args_json);

    session_stats.mark_plan_mode_interview_shown();

    match processing_result {
        TurnProcessingResult::ToolCalls {
            mut tool_calls,
            assistant_text,
            reasoning,
        } => {
            tool_calls.push(call);
            TurnProcessingResult::ToolCalls {
                tool_calls,
                assistant_text,
                reasoning,
            }
        }
        TurnProcessingResult::TextResponse {
            text,
            reasoning,
            proposed_plan: _,
        } => TurnProcessingResult::ToolCalls {
            tool_calls: vec![call],
            assistant_text: text,
            reasoning,
        },
        TurnProcessingResult::Empty | TurnProcessingResult::Completed => {
            TurnProcessingResult::ToolCalls {
                tool_calls: vec![call],
                assistant_text: String::new(),
                reasoning: Vec::new(),
            }
        }
        TurnProcessingResult::Cancelled | TurnProcessingResult::Aborted => processing_result,
    }
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
) -> InterviewToolCallFilter {
    use vtcode_core::config::constants::tools;

    let TurnProcessingResult::ToolCalls {
        tool_calls,
        assistant_text,
        reasoning,
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
        let is_interview = call
            .function
            .as_ref()
            .map(|func| {
                matches!(
                    func.name.as_str(),
                    tools::ASK_QUESTIONS | tools::REQUEST_USER_INPUT
                )
            })
            .unwrap_or(false);

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

    if had_interview && (had_non_interview || !allow_interview) && !response_has_plan {
        session_stats.mark_plan_mode_interview_pending();
    }

    let processing_result = if filtered.is_empty() {
        if assistant_text.trim().is_empty() {
            TurnProcessingResult::ToolCalls {
                tool_calls: Vec::new(),
                assistant_text,
                reasoning,
            }
        } else {
            TurnProcessingResult::TextResponse {
                text: assistant_text,
                reasoning,
                proposed_plan: None,
            }
        }
    } else {
        TurnProcessingResult::ToolCalls {
            tool_calls: filtered,
            assistant_text,
            reasoning,
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
