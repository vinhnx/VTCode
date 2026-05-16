//! Plan Mode interview payload parsing, sanitization, and fallback shaping.

use serde_json::{Value, json};
use vtcode_core::tools::handlers::plan_mode::{PlanValidationReport, validate_plan_content};

use super::MAX_RESEARCH_SNIPPETS_PER_BUCKET;
use super::interview_context::{
    InterviewResearchContext, TaskTrackerSummary, contains_any, push_unique_case_insensitive,
};
use crate::agent::runloop::unified::plan_blocks::extract_any_plan;

pub(super) fn parse_interview_payload_from_text(text: &str) -> Option<Value> {
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

pub(super) fn sanitize_generated_interview_payload(
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

pub(super) fn single_line(text: &str) -> String {
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

pub(super) fn build_adaptive_fallback_interview_args(
    context: &InterviewResearchContext,
    response_text: Option<&str>,
    plan_validation: Option<PlanValidationReport>,
    tracker_summary: Option<TaskTrackerSummary>,
) -> Option<Value> {
    let validation = plan_validation.or_else(|| extract_plan_validation(response_text));
    let mut questions = Vec::new();
    let needs_task_metadata = tracker_summary.as_ref().is_some_and(|summary| {
        summary.item_count > 0 && (!summary.has_outcome || !summary.has_verify)
    });

    let needs_scope = validation.as_ref().is_some_and(|report| {
        report
            .missing_sections
            .iter()
            .any(|section| section == "Summary")
            || !report.open_decisions.is_empty()
    }) || (!context.open_decision_hints.is_empty() && validation.is_none());

    if needs_scope {
        questions.push(build_fallback_question(
            "scope",
            "Scope",
            "What user-visible outcome should this plan optimize for, and which constraints or non-goals must stay fixed?",
            context,
        ));
    }

    let planning_placeholders = validation.as_ref().is_some_and(|report| {
        report.placeholder_tokens.iter().any(|token| {
            token.contains("[step]") || token.contains("[paths]") || token.contains("[check]")
        })
    });
    let needs_planning = validation.as_ref().is_some_and(|report| {
        report
            .missing_sections
            .iter()
            .any(|section| section == "Implementation Steps")
            || report.implementation_step_count == 0
    }) || planning_placeholders
        || (validation.is_none() && !context.recent_targets.is_empty());

    if needs_planning {
        questions.push(build_fallback_question(
            "plan",
            "Plan",
            "How should the work be decomposed into concrete implementation steps, including target files or modules for each slice?",
            context,
        ));
    }

    let needs_verification = validation.as_ref().is_some_and(|report| {
        report
            .missing_sections
            .iter()
            .any(|section| section == "Test Cases and Validation")
            || report.validation_item_count == 0
    }) || validation
        .as_ref()
        .is_some_and(|report| report.assumption_count == 0)
        || (validation.is_none() && context.verification_hints.is_empty())
        || needs_task_metadata;

    if needs_verification {
        let question = if needs_task_metadata {
            "What outcomes and verification commands should be added for each task tracker step to prove completion?"
        } else {
            "What exact commands or manual checks should prove the implementation is complete and guard against regressions?"
        };
        questions.push(build_fallback_question(
            "verification",
            "Verify",
            question,
            context,
        ));
    }

    if questions.is_empty() {
        return None;
    }

    Some(json!({ "questions": questions.into_iter().take(3).collect::<Vec<_>>() }))
}

fn extract_plan_validation(response_text: Option<&str>) -> Option<PlanValidationReport> {
    let text = response_text?;
    let extracted = extract_any_plan(text);
    let plan_text = extracted.plan_text?;
    Some(validate_plan_content(&plan_text))
}

pub(super) fn build_fallback_question(
    id: &str,
    header: &str,
    question: &str,
    context: &InterviewResearchContext,
) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("id".to_string(), Value::String(id.to_string()));
    payload.insert("header".to_string(), Value::String(header.to_string()));
    let question_text = append_context_hint(question, context);
    payload.insert("question".to_string(), Value::String(question_text));
    payload.insert(
        "options".to_string(),
        Value::Array(fallback_options_for_question(question, context)),
    );
    if let Some(focus_area) = infer_focus_area_hint(question, context) {
        payload.insert("focus_area".to_string(), Value::String(focus_area));
    }
    let analysis_hints = build_analysis_hints(context);
    if !analysis_hints.is_empty() {
        payload.insert(
            "analysis_hints".to_string(),
            Value::Array(analysis_hints.into_iter().map(Value::String).collect()),
        );
    }
    Value::Object(payload)
}

fn append_context_hint(question: &str, context: &InterviewResearchContext) -> String {
    let base = question.to_string();
    let hint = context
        .open_decision_hints
        .first()
        .or_else(|| context.recent_targets.first())
        .map(|value| single_line(value))
        .map(|value| truncate_hint(&value, 72));
    match hint {
        Some(value) if !value.is_empty() => format!("{base} (Focus: {value})"),
        _ => base,
    }
}

fn truncate_hint(value: &str, max_len: usize) -> String {
    let trimmed = value.trim();
    if trimmed.len() <= max_len {
        return trimmed.to_string();
    }
    let mut out = trimmed
        .chars()
        .take(max_len.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}