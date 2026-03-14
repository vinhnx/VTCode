use std::time::Duration;

use serde_json::{Value, json};
use vtcode_core::llm::provider as uni;

use super::response_processing::prepare_tool_calls;
use crate::agent::runloop::unified::turn::context::TurnProcessingResult;
use crate::agent::runloop::unified::turn::turn_processing::extract_interview_questions;

const MIN_PLAN_MODE_TURNS_BEFORE_INTERVIEW: usize = 1;
const PLAN_MODE_REMINDER: &str = vtcode_core::prompts::system::PLAN_MODE_IMPLEMENT_REMINDER;
const INTERVIEW_SYNTHESIS_TIMEOUT_SECS: u64 = 20;
const MAX_RESEARCH_SNIPPETS_PER_BUCKET: usize = 6;
const CUSTOM_NOTE_POLICY: &str =
    "Users can always type custom notes/free-form responses for every question.";

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct InterviewResearchContext {
    discovery_tools_used: Vec<String>,
    recent_targets: Vec<String>,
    risk_hints: Vec<String>,
    open_decision_hints: Vec<String>,
    goal_hints: Vec<String>,
    verification_hints: Vec<String>,
    custom_note_policy: String,
}

#[derive(Debug, Clone, Copy)]
struct InterviewNeedState {
    response_has_plan: bool,
    needs_interview: bool,
}

fn has_discovery_tool(session_stats: &crate::agent::runloop::unified::state::SessionStats) -> bool {
    use vtcode_core::config::constants::tools;

    [
        tools::READ_FILE,
        "list_files",
        "grep_file",
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
    if !plan_mode_interview_ready(session_stats) {
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
) -> Option<serde_json::Value> {
    let context = collect_interview_research_context(working_history, response_text, session_stats);
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
    .await
    .ok()?
    .ok()?;
    let content = response.content?;
    let payload = parse_interview_payload_from_text(&content)?;
    sanitize_generated_interview_payload(payload, &context)
}

fn collect_interview_research_context(
    working_history: &[uni::Message],
    response_text: Option<&str>,
    session_stats: &crate::agent::runloop::unified::state::SessionStats,
) -> InterviewResearchContext {
    let discovery_tools_used = session_stats
        .sorted_tools()
        .into_iter()
        .filter(|tool| {
            matches!(
                tool.as_str(),
                "read_file" | "list_files" | "grep_file" | "unified_search"
            )
        })
        .collect::<Vec<_>>();

    let mut recent_targets = Vec::new();
    let mut risk_hints = Vec::new();
    let mut verification_hints = Vec::new();
    let mut goal_hints = Vec::new();

    for message in working_history.iter().rev().take(16) {
        let text = message.content.as_text();
        for line in text.lines().take(20) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if is_path_or_symbol_hint(trimmed)
                && recent_targets.len() < MAX_RESEARCH_SNIPPETS_PER_BUCKET
            {
                push_unique_case_insensitive(&mut recent_targets, trimmed.to_string());
            }
            let lower = trimmed.to_ascii_lowercase();
            if contains_any(
                &lower,
                &[
                    "risk",
                    "constraint",
                    "non-goal",
                    "tradeoff",
                    "rollback",
                    "incompatible",
                    "blocked",
                ],
            ) && risk_hints.len() < MAX_RESEARCH_SNIPPETS_PER_BUCKET
            {
                push_unique_case_insensitive(&mut risk_hints, trimmed.to_string());
            }
            if contains_any(
                &lower,
                &[
                    "verify",
                    "validation",
                    "test",
                    "assert",
                    "cargo check",
                    "cargo nextest",
                    "manual check",
                    "command",
                ],
            ) && verification_hints.len() < MAX_RESEARCH_SNIPPETS_PER_BUCKET
            {
                push_unique_case_insensitive(&mut verification_hints, trimmed.to_string());
            }
            if contains_any(
                &lower,
                &[
                    "outcome",
                    "goal",
                    "user-visible",
                    "success criteria",
                    "deliver",
                ],
            ) && goal_hints.len() < MAX_RESEARCH_SNIPPETS_PER_BUCKET
            {
                push_unique_case_insensitive(&mut goal_hints, trimmed.to_string());
            }
        }
    }

    let open_decision_hints = response_text
        .map(extract_open_decision_hints)
        .unwrap_or_default();

    InterviewResearchContext {
        discovery_tools_used,
        recent_targets,
        risk_hints,
        open_decision_hints,
        goal_hints,
        verification_hints,
        custom_note_policy: CUSTOM_NOTE_POLICY.to_string(),
    }
}

fn is_path_or_symbol_hint(text: &str) -> bool {
    text.contains('/')
        || text.contains("::")
        || text.contains(".rs")
        || text.contains(".toml")
        || text.contains(".md")
}

fn push_unique_case_insensitive(target: &mut Vec<String>, value: String) {
    if target
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(value.as_str()))
    {
        return;
    }
    target.push(value);
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
    synthesized_interview_args: Option<serde_json::Value>,
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

fn has_open_decision_markers(text: &str) -> bool {
    text.lines().any(line_has_open_decision_marker)
}

fn line_has_open_decision_marker(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    if !lower.contains("next open decision") {
        return false;
    }

    !contains_any(
        &lower,
        &[
            "none",
            "no remaining",
            "no further",
            "resolved",
            "closed",
            "locked",
            "n/a",
            "not applicable",
        ],
    )
}

fn extract_open_decision_hints(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && line_has_open_decision_marker(line))
        .take(MAX_RESEARCH_SNIPPETS_PER_BUCKET)
        .map(ToString::to_string)
        .collect()
}

fn parse_interview_payload_from_text(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
        return Some(parsed);
    }

    if let Some(json_start) = trimmed.find("```json")
        && let Some(end) = trimmed[json_start + 7..].find("```")
    {
        let inner = trimmed[json_start + 7..json_start + 7 + end].trim();
        if let Ok(parsed) = serde_json::from_str::<Value>(inner) {
            return Some(parsed);
        }
    }

    let object_start = trimmed.find('{')?;
    let object_end = trimmed.rfind('}')?;
    if object_end <= object_start {
        return None;
    }
    serde_json::from_str::<Value>(&trimmed[object_start..=object_end]).ok()
}

fn sanitize_generated_interview_payload(
    payload: Value,
    context: &InterviewResearchContext,
) -> Option<Value> {
    let questions_raw = payload
        .get("questions")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| payload.as_array().cloned())?;

    let analysis_hints = build_analysis_hints(context);
    let mut questions = Vec::new();
    for (index, question) in questions_raw.into_iter().enumerate() {
        if questions.len() == 3 {
            break;
        }
        let Some(obj) = question.as_object() else {
            continue;
        };
        let Some(question_text) = obj
            .get("question")
            .and_then(Value::as_str)
            .map(single_line)
            .filter(|text| !text.is_empty())
        else {
            continue;
        };
        let id = normalize_question_id(
            obj.get("id").and_then(Value::as_str).unwrap_or("question"),
            index,
        );
        let header = normalize_question_header(
            obj.get("header")
                .and_then(Value::as_str)
                .unwrap_or("Question"),
            index,
        );
        let options = sanitize_generated_options(obj.get("options"), &question_text, context);
        if options.len() < 2 {
            continue;
        }
        let mut question_payload = serde_json::Map::new();
        question_payload.insert("id".to_string(), Value::String(id));
        question_payload.insert("header".to_string(), Value::String(header));
        question_payload.insert("question".to_string(), Value::String(question_text.clone()));
        question_payload.insert("options".to_string(), Value::Array(options));

        if let Some(focus_area) = infer_focus_area_hint(&question_text, context) {
            question_payload.insert("focus_area".to_string(), Value::String(focus_area));
        }
        if !analysis_hints.is_empty() {
            question_payload.insert(
                "analysis_hints".to_string(),
                Value::Array(analysis_hints.iter().cloned().map(Value::String).collect()),
            );
        }

        questions.push(Value::Object(question_payload));
    }

    if questions.is_empty() {
        return None;
    }
    Some(json!({ "questions": questions }))
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_question_id(raw_id: &str, index: usize) -> String {
    let mut normalized = raw_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    while normalized.contains("__") {
        normalized = normalized.replace("__", "_");
    }
    normalized = normalized.trim_matches('_').to_string();
    let starts_with_letter = normalized
        .chars()
        .next()
        .map(|ch| ch.is_ascii_lowercase())
        .unwrap_or(false);
    if !starts_with_letter {
        normalized = format!("question_{}", index + 1);
    }
    normalized
}

fn normalize_question_header(raw_header: &str, index: usize) -> String {
    let header = single_line(raw_header);
    if header.is_empty() {
        return format!("Q{}", index + 1);
    }
    header.chars().take(12).collect()
}

fn sanitize_generated_options(
    options_val: Option<&Value>,
    question: &str,
    context: &InterviewResearchContext,
) -> Vec<Value> {
    let mut options = Vec::new();
    if let Some(Value::Array(arr)) = options_val {
        for item in arr.iter().take(3) {
            let Some(obj) = item.as_object() else {
                continue;
            };
            let label = obj
                .get("label")
                .and_then(Value::as_str)
                .map(single_line)
                .unwrap_or_default();
            if label.is_empty() || label.eq_ignore_ascii_case("other") {
                continue;
            }
            let description = obj
                .get("description")
                .and_then(Value::as_str)
                .map(single_line)
                .unwrap_or_else(|| "Choose this option to keep planning momentum.".to_string());
            if options.iter().any(|existing: &Value| {
                existing["label"]
                    .as_str()
                    .map(|value| value.eq_ignore_ascii_case(&label))
                    .unwrap_or(false)
            }) {
                continue;
            }
            options.push(json!({
                "label": label,
                "description": description,
            }));
        }
    }

    if options.len() < 2 {
        options = fallback_options_for_question(question, context);
    }

    if let Some(first) = options.first_mut()
        && let Some(label) = first.get("label").and_then(Value::as_str)
        && !label.contains("(Recommended)")
    {
        let label = label.to_string();
        let description = first
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        *first = json!({
            "label": format!("{label} (Recommended)"),
            "description": description
        });
    }

    options.truncate(3);
    options
}

fn fallback_options_for_question(question: &str, context: &InterviewResearchContext) -> Vec<Value> {
    let lower = question.to_ascii_lowercase();
    let target_hint = context
        .recent_targets
        .first()
        .cloned()
        .unwrap_or_else(|| "the current implementation slice".to_string());

    if contains_any(&lower, &["outcome", "goal", "constraints", "non-goal"]) {
        return vec![
            json!({
                "label": "Single outcome with hard constraints (Recommended)",
                "description": format!("Optimize for one user-visible result and lock non-goals around {target_hint}."),
            }),
            json!({
                "label": "Outcome plus risk-boundaries",
                "description": "Define the target and include explicit risk and compatibility boundaries.",
            }),
            json!({
                "label": "Minimal viable scope",
                "description": "Choose the smallest delivery slice that still proves user impact.",
            }),
        ];
    }

    if contains_any(&lower, &["step", "plan", "decompose", "composable"]) {
        return vec![
            json!({
                "label": "Dependency-first slices (Recommended)",
                "description": format!("Order 3-7 steps around dependency boundaries touching {target_hint}."),
            }),
            json!({
                "label": "User-flow slices",
                "description": "Split work by visible user journey milestones to reduce ambiguity.",
            }),
            json!({
                "label": "Risk-isolated slices",
                "description": "Isolate high-risk steps first so rollback and debugging stay simple.",
            }),
        ];
    }

    if contains_any(
        &lower,
        &[
            "verify",
            "validation",
            "prove",
            "complete",
            "test",
            "command",
        ],
    ) {
        return vec![
            json!({
                "label": "Command proof per step (Recommended)",
                "description": "Attach one explicit command or check that objectively proves each step.",
            }),
            json!({
                "label": "Manual behavior proof",
                "description": "Define concrete user-visible manual checks where automation is not available.",
            }),
            json!({
                "label": "Hybrid proof strategy",
                "description": "Combine fast automated checks with one targeted manual verification.",
            }),
        ];
    }

    vec![
        json!({
            "label": "Resolve highest-impact decision first (Recommended)",
            "description": "Choose the option that unblocks implementation with the least additional assumptions.",
        }),
        json!({
            "label": "Balance risk and delivery",
            "description": "Prefer moderate scope with explicit constraints and measured validation.",
        }),
        json!({
            "label": "Comprehensive decision lock",
            "description": "Capture all major constraints now to minimize follow-up clarification loops.",
        }),
    ]
}

fn infer_focus_area_hint(question: &str, context: &InterviewResearchContext) -> Option<String> {
    let lower = question.to_ascii_lowercase();
    if contains_any(&lower, &["verify", "validation", "test", "prove"]) {
        return Some("verification".to_string());
    }
    if contains_any(&lower, &["plan", "step", "decompose"]) {
        return Some("planning".to_string());
    }
    if contains_any(&lower, &["goal", "outcome", "constraint", "non-goal"]) {
        return Some("scope".to_string());
    }
    if !context.open_decision_hints.is_empty() {
        return Some("open_decision".to_string());
    }
    None
}

fn build_analysis_hints(context: &InterviewResearchContext) -> Vec<String> {
    let mut hints = Vec::new();
    for bucket in [
        &context.open_decision_hints,
        &context.risk_hints,
        &context.verification_hints,
        &context.goal_hints,
    ] {
        for hint in bucket.iter().take(2) {
            push_unique_case_insensitive(&mut hints, hint.clone());
            if hints.len() == MAX_RESEARCH_SNIPPETS_PER_BUCKET {
                return hints;
            }
        }
    }
    hints
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
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
    _response_text: Option<&str>,
    synthesized_interview_args: Option<serde_json::Value>,
) -> TurnProcessingResult {
    use vtcode_core::config::constants::tools;

    let args = synthesized_interview_args.unwrap_or_else(default_plan_mode_interview_args);
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
