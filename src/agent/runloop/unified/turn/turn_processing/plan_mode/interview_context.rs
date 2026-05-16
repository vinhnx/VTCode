//! Plan Mode interview context and draft-loading helpers.

use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::handlers::plan_mode::{
    PlanModeState, PlanValidationReport, tracker_file_for_plan_file, validate_plan_content,
};

use super::{
    CUSTOM_NOTE_POLICY, MAX_PLAN_DRAFT_CHARS, MAX_RESEARCH_SNIPPETS_PER_BUCKET,
    MAX_TASK_TRACKER_CHARS, PLAN_TRACKER_END, PLAN_TRACKER_START,
};
use crate::agent::runloop::unified::plan_blocks::extract_any_plan;

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct InterviewResearchContext {
    pub(super) discovery_tools_used: Vec<String>,
    pub(super) recent_targets: Vec<String>,
    pub(super) risk_hints: Vec<String>,
    pub(super) open_decision_hints: Vec<String>,
    pub(super) goal_hints: Vec<String>,
    pub(super) verification_hints: Vec<String>,
    pub(super) custom_note_policy: String,
    pub(super) plan_draft_excerpt: Option<String>,
    pub(super) plan_draft_path: Option<String>,
    pub(super) plan_validation: Option<PlanValidationSnapshot>,
    pub(super) task_tracker_excerpt: Option<String>,
    pub(super) task_tracker_path: Option<String>,
    pub(super) task_tracker_summary: Option<TaskTrackerSummary>,
}

#[derive(Debug, Clone)]
pub(super) struct PlanDraftContext {
    pub(super) plan_path: Option<String>,
    pub(super) plan_excerpt: Option<String>,
    pub(super) plan_validation: Option<PlanValidationReport>,
    pub(super) tracker_path: Option<String>,
    pub(super) tracker_excerpt: Option<String>,
    pub(super) tracker_summary: Option<TaskTrackerSummary>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct PlanValidationSnapshot {
    pub(super) missing_sections: Vec<String>,
    pub(super) placeholder_tokens: Vec<String>,
    pub(super) open_decisions: Vec<String>,
    pub(super) implementation_step_count: usize,
    pub(super) validation_item_count: usize,
    pub(super) assumption_count: usize,
    pub(super) summary_present: bool,
}

impl From<&PlanValidationReport> for PlanValidationSnapshot {
    fn from(report: &PlanValidationReport) -> Self {
        Self {
            missing_sections: report.missing_sections.clone(),
            placeholder_tokens: report.placeholder_tokens.clone(),
            open_decisions: report.open_decisions.clone(),
            implementation_step_count: report.implementation_step_count,
            validation_item_count: report.validation_item_count,
            assumption_count: report.assumption_count,
            summary_present: report.summary_present,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct TaskTrackerSummary {
    pub(super) item_count: usize,
    pub(super) has_outcome: bool,
    pub(super) has_verify: bool,
}

pub(super) fn collect_interview_research_context(
    working_history: &[uni::Message],
    response_text: Option<&str>,
    session_stats: &crate::agent::runloop::unified::state::SessionStats,
    plan_context: Option<&PlanDraftContext>,
) -> InterviewResearchContext {
    let discovery_tools_used = session_stats
        .sorted_tools()
        .into_iter()
        .filter(|tool| {
            matches!(
                tool.as_str(),
                tools::READ_FILE | tools::LIST_FILES | tools::GREP_FILE | tools::UNIFIED_SEARCH
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

            if let Some(hint) = extract_path_or_symbol_hint(trimmed)
                && recent_targets.len() < MAX_RESEARCH_SNIPPETS_PER_BUCKET
            {
                push_unique_case_insensitive(&mut recent_targets, hint);
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

    let extracted_plan = response_text
        .and_then(|text| extract_any_plan(text).plan_text)
        .filter(|text| !text.trim().is_empty());
    let extracted_plan_excerpt = extracted_plan
        .as_deref()
        .map(|content| truncate_for_context(content, MAX_PLAN_DRAFT_CHARS));
    let extracted_plan_validation = extracted_plan.as_deref().map(validate_plan_content);
    let preferred_validation = select_best_plan_validation(
        plan_context.and_then(|ctx| ctx.plan_validation.as_ref()),
        extracted_plan_validation.as_ref(),
    );
    let prefer_extracted_plan = match (
        plan_context.and_then(|ctx| ctx.plan_validation.as_ref()),
        extracted_plan_validation.as_ref(),
    ) {
        (Some(existing), Some(candidate)) => is_validation_better(candidate, existing),
        (None, Some(_)) => true,
        _ => false,
    };

    let mut open_decision_hints = response_text.map(extract_open_decision_hints).unwrap_or_default();
    if let Some(plan_validation) = preferred_validation.as_ref() {
        for decision in plan_validation
            .open_decisions
            .iter()
            .take(MAX_RESEARCH_SNIPPETS_PER_BUCKET)
        {
            push_unique_case_insensitive(&mut open_decision_hints, decision.to_string());
        }
    }

    let extracted_plan_snapshot = extracted_plan_validation
        .as_ref()
        .map(PlanValidationSnapshot::from);

    let (
        plan_draft_excerpt,
        plan_draft_path,
        plan_validation,
        task_tracker_excerpt,
        task_tracker_path,
        task_tracker_summary,
    ) = if let Some(plan_context) = plan_context {
        let plan_excerpt = if prefer_extracted_plan {
            extracted_plan_excerpt
                .clone()
                .or_else(|| plan_context.plan_excerpt.clone())
        } else {
            plan_context
                .plan_excerpt
                .clone()
                .or_else(|| extracted_plan_excerpt.clone())
        };
        (
            plan_excerpt,
            plan_context.plan_path.clone(),
            preferred_validation
                .as_ref()
                .map(PlanValidationSnapshot::from)
                .or(extracted_plan_snapshot.clone()),
            plan_context.tracker_excerpt.clone(),
            plan_context.tracker_path.clone(),
            plan_context.tracker_summary.clone(),
        )
    } else {
        (
            extracted_plan_excerpt,
            None,
            preferred_validation
                .as_ref()
                .map(PlanValidationSnapshot::from)
                .or(extracted_plan_snapshot),
            None,
            None,
            None,
        )
    };

    InterviewResearchContext {
        discovery_tools_used,
        recent_targets,
        risk_hints,
        open_decision_hints,
        goal_hints,
        verification_hints,
        custom_note_policy: CUSTOM_NOTE_POLICY.to_string(),
        plan_draft_excerpt,
        plan_draft_path,
        plan_validation,
        task_tracker_excerpt,
        task_tracker_path,
        task_tracker_summary,
    }
}

pub(super) async fn load_plan_draft_context(
    plan_state: Option<PlanModeState>,
) -> Option<PlanDraftContext> {
    let plan_state = plan_state?;
    let plan_file = plan_state.get_plan_file().await?;
    let plan_path = Some(plan_file.display().to_string());

    let plan_content = if plan_file.exists() {
        tokio::fs::read_to_string(&plan_file).await.ok()
    } else {
        None
    };

    let plan_excerpt = plan_content
        .as_deref()
        .map(strip_embedded_tracker)
        .filter(|content| !content.trim().is_empty())
        .map(|content| truncate_for_context(&content, MAX_PLAN_DRAFT_CHARS));
    let plan_validation = plan_content
        .as_deref()
        .filter(|content| !content.trim().is_empty())
        .map(validate_plan_content);

    let tracker_path = tracker_file_for_plan_file(&plan_file);
    let (tracker_path, tracker_content) = match tracker_path {
        Some(path) if path.exists() => {
            let content = tokio::fs::read_to_string(&path).await.ok();
            (Some(path.display().to_string()), content)
        }
        Some(path) => {
            let content = plan_content.as_deref().and_then(extract_embedded_tracker);
            (Some(path.display().to_string()), content)
        }
        None => {
            let content = plan_content.as_deref().and_then(extract_embedded_tracker);
            (None, content)
        }
    };

    let tracker_excerpt = tracker_content
        .as_deref()
        .filter(|content| !content.trim().is_empty())
        .map(|content| truncate_for_context(content, MAX_TASK_TRACKER_CHARS));
    let tracker_summary = tracker_content
        .as_deref()
        .filter(|content| !content.trim().is_empty())
        .map(summarize_task_tracker);

    Some(PlanDraftContext {
        plan_path,
        plan_excerpt,
        plan_validation,
        tracker_path,
        tracker_excerpt,
        tracker_summary,
    })
}

fn truncate_for_context(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    const MARKER: &str = " ... [truncated] ...";
    let marker_chars = MARKER.chars().count();
    if max_chars <= marker_chars {
        return text.chars().take(max_chars).collect();
    }

    let take_chars = max_chars.saturating_sub(marker_chars);
    let mut truncated = text.chars().take(take_chars).collect::<String>();
    truncated.push_str(MARKER);
    truncated
}

fn extract_embedded_tracker(plan_content: &str) -> Option<String> {
    let start = plan_content.find(PLAN_TRACKER_START)?;
    let end = plan_content.find(PLAN_TRACKER_END)?;
    if end <= start {
        return None;
    }
    let content = plan_content[start + PLAN_TRACKER_START.len()..end].trim();
    if content.is_empty() {
        None
    } else {
        Some(content.to_string())
    }
}

fn strip_embedded_tracker(plan_content: &str) -> String {
    let Some(start) = plan_content.find(PLAN_TRACKER_START) else {
        return plan_content.trim().to_string();
    };
    let end = plan_content[start..]
        .find(PLAN_TRACKER_END)
        .map(|offset| start + offset + PLAN_TRACKER_END.len())
        .unwrap_or(plan_content.len());
    let mut merged = String::new();
    merged.push_str(plan_content[..start].trim_end());
    if !merged.is_empty() && !plan_content[end..].trim().is_empty() {
        merged.push_str("\n\n");
    }
    merged.push_str(plan_content[end..].trim_start());
    merged.trim().to_string()
}

fn summarize_task_tracker(content: &str) -> TaskTrackerSummary {
    let mut item_count = 0;
    let mut has_outcome = false;
    let mut has_verify = false;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- [") {
            item_count += 1;
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("outcome:") {
            has_outcome = true;
        }
        if lower.starts_with("verify:") {
            has_verify = true;
        }
    }

    TaskTrackerSummary {
        item_count,
        has_outcome,
        has_verify,
    }
}

fn extract_path_or_symbol_hint(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut rest = trimmed;
    let needle = "\"text\":\"";
    while let Some(idx) = rest.find(needle) {
        let after = &rest[idx + needle.len()..];
        if let Some((value, remaining)) = parse_json_string(after) {
            if is_safe_hint(&value) && looks_like_path_or_file(&value) {
                return Some(value);
            }
            rest = remaining;
        } else {
            break;
        }
    }

    for raw in trimmed.split_whitespace() {
        let candidate = trim_hint_token(raw);
        if candidate.is_empty() || !is_safe_hint(candidate) {
            continue;
        }
        if looks_like_path_or_file(candidate) || candidate.contains("::") {
            return Some(candidate.to_string());
        }
    }

    None
}

fn parse_json_string(input: &str) -> Option<(String, &str)> {
    let mut out = String::new();
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let escaped = chars.next()?;
            match escaped {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                other => out.push(other),
            }
            continue;
        }

        if ch == '"' {
            return Some((out, chars.as_str()));
        }

        out.push(ch);
    }

    None
}

fn looks_like_path_or_file(value: &str) -> bool {
    value.contains('/') || value.contains(".rs") || value.contains(".toml") || value.contains(".md")
}

fn is_safe_hint(value: &str) -> bool {
    !value.contains('{') && !value.contains('}') && !value.contains('"')
}

fn trim_hint_token(token: &str) -> &str {
    token.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | ';' | ')' | ']' | '(' | '[' | '>' | '<' | '\'' | '"'
        )
    })
}

pub(super) fn push_unique_case_insensitive(target: &mut Vec<String>, value: String) {
    if target
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(value.as_str()))
    {
        return;
    }
    target.push(value);
}

pub(super) fn has_open_decision_markers(text: &str) -> bool {
    text.lines().any(line_has_open_decision_marker)
}

pub(super) fn line_has_open_decision_marker(line: &str) -> bool {
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

pub(super) fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

pub(super) fn select_best_plan_validation(
    current: Option<&PlanValidationReport>,
    candidate: Option<&PlanValidationReport>,
) -> Option<PlanValidationReport> {
    match (current, candidate) {
        (None, None) => None,
        (Some(current), None) => Some(current.clone()),
        (None, Some(candidate)) => Some(candidate.clone()),
        (Some(current), Some(candidate)) => {
            if is_validation_better(candidate, current) {
                Some(candidate.clone())
            } else {
                Some(current.clone())
            }
        }
    }
}

fn is_validation_better(candidate: &PlanValidationReport, current: &PlanValidationReport) -> bool {
    if candidate.is_ready() && !current.is_ready() {
        return true;
    }
    if current.is_ready() && !candidate.is_ready() {
        return false;
    }

    let candidate_missing = candidate.missing_sections.len();
    let current_missing = current.missing_sections.len();
    if candidate_missing != current_missing {
        return candidate_missing < current_missing;
    }

    let candidate_placeholders = candidate.placeholder_tokens.len();
    let current_placeholders = current.placeholder_tokens.len();
    if candidate_placeholders != current_placeholders {
        return candidate_placeholders < current_placeholders;
    }

    let candidate_open = candidate.open_decisions.len();
    let current_open = current.open_decisions.len();
    if candidate_open != current_open {
        return candidate_open < current_open;
    }

    if candidate.summary_present != current.summary_present {
        return candidate.summary_present;
    }

    if candidate.implementation_step_count != current.implementation_step_count {
        return candidate.implementation_step_count > current.implementation_step_count;
    }

    if candidate.validation_item_count != current.validation_item_count {
        return candidate.validation_item_count > current.validation_item_count;
    }

    candidate.assumption_count > current.assumption_count
}