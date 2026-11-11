use anyhow::Result;
use vtcode_core::core::agent::refusal;
use vtcode_core::llm::providers::split_reasoning_from_text;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::plan_blocks::{
    extract_any_plan, has_exactly_one_proposed_plan_block, looks_like_attempted_plan, looks_like_clarifying_question,
    strip_plan_persistence_policy_line,
};
use crate::agent::runloop::unified::planning_workflow::validate_plan_content;
use crate::agent::runloop::unified::turn::context::{NoOutputFacts, PreparedAssistantToolCall, TurnProcessingResult};
use crate::agent::runloop::unified::turn::guards::validate_tool_args_security;

/// Process an LLM response and return a `TurnProcessingResult` describing whether
/// there are tool calls to run, a textual assistant response, or nothing.
pub(crate) fn process_llm_response(
    response: &vtcode_core::llm::provider::LLMResponse,
    renderer: &mut AnsiRenderer,
    conversation_len: usize,
    planning_active: bool,
    allow_plan_interview: bool,
    request_user_input_enabled: bool,
    allow_tool_calls: bool,
    validation_cache: Option<&std::sync::Arc<vtcode_core::tools::validation_cache::ValidationCache>>,
    tool_registry: Option<&vtcode_core::tools::ToolRegistry>,
) -> Result<TurnProcessingResult> {
    use crate::agent::runloop::unified::turn::harmony::{contains_harmony_marker, strip_harmony_syntax};
    use crate::agent::runloop::unified::turn::provider_noise::strip_provider_noise;
    use vtcode_core::config::constants::tools;
    use vtcode_core::llm::provider as uni;

    // A refusal is terminal for this prompt: empty-response recovery would
    // resend it and be refused again, and any partial output was cut off by
    // the provider, so it must not be committed as an answer or executed.
    if refusal::is_refusal(response) {
        return Ok(TurnProcessingResult::Refusal { reason: refusal::refusal_reason(response) });
    }

    let reasoning = split_reasoning_from_text(response.reasoning.as_deref().unwrap_or("")).0;
    let reasoning_text = reasoning
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let reasoning_details = response.reasoning_details.clone();
    let mut final_text = response.content.clone();

    let dropped_native_tool_calls =
        planning_active && !allow_tool_calls && response.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty());
    let reasoning_only = response.content.as_deref().is_none_or(|text| text.trim().is_empty())
        && (!reasoning_text.trim().is_empty()
            || response.reasoning_details.as_ref().is_some_and(|details| !details.is_empty()));

    // Strip provider noise (e.g. MiniMax `]<]minimax[>[`) from non-streamed
    // response content before any downstream processing. This prevents noise
    // from leaking into `working_history` or corrupting harmony/tool-call
    // parsing. Streamed responses are sanitized live by `StreamSanitizer`.
    if let Some(ref text) = final_text {
        let cleaned = strip_provider_noise(text);
        if cleaned != *text {
            final_text = Some(cleaned);
        }
    }

    let mut proposed_plan: Option<String> = None;
    let mut tool_calls = if allow_tool_calls {
        response.tool_calls.clone().unwrap_or_default()
    } else {
        Vec::new()
    };
    let mut interpreted_textual_call = false;
    let mut is_harmony = false;
    let reasoning_segment_count = reasoning.len();
    let reasoning_details_count = reasoning_details.as_ref().map_or(0, Vec::len);

    if let Some(ref text) = final_text
        && contains_harmony_marker(text)
    {
        is_harmony = true;
        let cleaned = strip_harmony_syntax(text);
        if !cleaned.trim().is_empty() {
            final_text = Some(cleaned);
        } else {
            final_text = Some("".to_string());
        }
    }

    if planning_active && let Some(ref text) = final_text {
        let extraction = extract_any_plan(text);
        let strict_recovery_plan_shape = !allow_tool_calls;
        let complete_unwrapped_plan = extraction.plan_text.is_none()
            && crate::agent::runloop::unified::plan_blocks::strip_all_plan_tags(text) == text.trim()
            && validate_plan_content(text).is_ready();
        let plan_shape_is_valid =
            !strict_recovery_plan_shape || has_exactly_one_proposed_plan_block(text) || complete_unwrapped_plan;
        // The plan is rendered once by the approval flow below. Keep only the
        // non-plan prose in the normal assistant response; retaining the plan
        // body here makes the transcript render it a second time before the
        // approval heading is emitted.
        let stripped_text = if strict_recovery_plan_shape && !plan_shape_is_valid {
            // Keep malformed/duplicate/alternate plan markup available to the
            // recovery handoff. The handler stores it as a bounded rejected
            // draft instead of silently reducing the failure to "no answer".
            text.to_string()
        } else if complete_unwrapped_plan {
            String::new()
        } else if extraction.plan_text.is_some() {
            strip_plan_persistence_policy_line(&extraction.stripped_text)
        } else {
            extraction.stripped_text
        };
        proposed_plan = if plan_shape_is_valid {
            extraction
                .plan_text
                .or_else(|| complete_unwrapped_plan.then(|| text.trim().to_owned()))
        } else {
            None
        };
        final_text = Some(stripped_text);
    }

    // A provider response that includes native tool calls is not a valid
    // tool-free synthesis, even when it also contains a seemingly complete
    // plan. Do not silently accept the plan while dropping the forbidden
    // calls; the recovery handler repairs salvageable prose within its
    // bounded budget and otherwise produces the resumable blocked handoff.
    // Preserve the voided plan/prose for bounded salvage first: the extracted
    // plan body when one is present, else surrounding prose. Repair still
    // requires validation, a clean text-only re-emit, and user approval.
    let mut voided_text: Option<String> = None;
    if planning_active && !allow_tool_calls && response.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty()) {
        let non_empty = |text: &str| !text.trim().is_empty();
        if proposed_plan.as_deref().is_some_and(non_empty) {
            voided_text = proposed_plan.clone();
        } else if final_text.as_deref().is_some_and(non_empty) {
            voided_text = final_text.clone();
        }
        final_text = None;
        proposed_plan = None;
    }

    // A write-capable execution agent can emit a bounded replan when the plan
    // it is executing no longer matches the repository. Outside planning only
    // the canonical tagged block counts — ordinary prose is never mistaken for
    // a plan — and the response must be text-only so tool work in flight is
    // not discarded. Extracting here lets the response handler persist and
    // route the revision instead of ending the turn behind plain-text output.
    if !planning_active
        && allow_tool_calls
        && tool_calls.is_empty()
        && proposed_plan.is_none()
        && let Some(ref text) = final_text
    {
        let extraction = extract_any_plan(text);
        if extraction.plan_text.is_some() {
            proposed_plan = extraction.plan_text;
            final_text = Some(strip_plan_persistence_policy_line(&extraction.stripped_text));
        }
    }

    // Providers occasionally omit the plan tags while still returning a
    // structured synthesis. Preserve both validated structured responses and
    // text-only attempted drafts before interview forcing can replace them.
    // Invalid candidates must reach bounded validation repair, not an unrelated
    // interview followed by a planless completion (session 20261015T161453Z).
    if planning_active
        && proposed_plan.is_none()
        && let Some(text) = final_text.as_deref()
        && (looks_like_structured_plan(text)
            || (tool_calls.is_empty() && !looks_like_clarifying_question(text) && looks_like_attempted_plan(text)))
        && allow_tool_calls
    {
        proposed_plan = Some(text.trim().to_string());
        // Untagged structured responses are themselves the plan. They are
        // rendered by the approval flow, so do not also append them as a
        // normal assistant response.
        final_text = Some(String::new());
    }

    // A completed plan is a terminal planning response. Providers sometimes
    // attach exploratory tool calls after the plan text; executing them would
    // bypass the confirmation gate and continue the turn. Drop those calls so
    // the user sees the plan and can explicitly approve it.
    if planning_active && proposed_plan.is_some() {
        tool_calls.clear();
    }

    if allow_tool_calls
        && tool_calls.is_empty()
        && let Some(text) = final_text.as_deref()
        && !text.trim().is_empty()
        && let Some((name, args)) = crate::agent::runloop::text_tools::detect_textual_tool_call(text)
    {
        if let Some(validation_failures) = validate_tool_args_security(&name, &args, validation_cache, tool_registry) {
            let tool_display = vtcode_commons::tool_headlines::verb_callout_headline(&name, &args, None);
            let failures_list = validation_failures.join("; ");
            crate::agent::runloop::unified::turn::turn_helpers::display_status(
                renderer,
                &format!("Detected {tool_display} but validation failed: {failures_list}"),
            )?;
        } else {
            let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
            let code_blocks = crate::agent::runloop::text_tools::extract_code_fence_blocks(text);
            if !code_blocks.is_empty() {
                crate::agent::runloop::tool_output::render_code_fence_blocks(renderer, &code_blocks)?;
                renderer.line(MessageStyle::Output, "")?;
            }
            let (headline, _) = crate::agent::runloop::unified::tool_summary::describe_tool_action(&name, &args, None);
            let notice = if headline.is_empty() {
                format!(
                    "Detected {} request",
                    vtcode_commons::tool_headlines::verb_callout_headline(&name, &args, None)
                )
            } else {
                format!("Detected {headline}")
            };
            crate::agent::runloop::unified::turn::turn_helpers::display_status(renderer, &notice)?;
            let call_id = format!("call_textual_{conversation_len}");
            tool_calls.push(uni::ToolCall::function(call_id, name, args_json));
            interpreted_textual_call = true;
            final_text = None;
        }
    }

    // Strip DSML markup only after textual tool-call detection. If cleanup runs
    // first, a valid DSML call loses its tags and is rendered as raw argument
    // text instead of being dispatched as a tool call.
    if let Some(ref text) = final_text
        && crate::agent::runloop::text_tools::contains_dsml_markup(text)
    {
        final_text = Some(crate::agent::runloop::text_tools::strip_dsml_markup(text));
    }

    if allow_tool_calls
        && !interpreted_textual_call
        && allow_plan_interview
        && request_user_input_enabled
        && tool_calls.is_empty()
        && let Some(text) = final_text.as_deref()
        && let Some(args) = build_interview_args_from_text(text)
    {
        let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
        let call_id = format!("call_interview_{conversation_len}");
        tool_calls.push(uni::ToolCall::function(call_id.clone(), tools::REQUEST_USER_INPUT.to_string(), args_json));
        interpreted_textual_call = true;
        final_text = None;
    }

    if !tool_calls.is_empty() {
        tracing::info!(
            target: "vtcode.turn.metrics",
            metric = "llm_response_parsed",
            kind = "tool_calls",
            tool_calls = tool_calls.len(),
            interpreted_textual_call,
            reasoning_segments = reasoning_segment_count,
            reasoning_details = reasoning_details_count,
            content_len = final_text.as_ref().map_or(0, |text| text.len()),
            is_harmony,
            planning_active,
            allow_plan_interview,
            request_user_input_enabled,
            proposed_plan = proposed_plan.is_some(),
            "turn metric"
        );
        return Ok(TurnProcessingResult::ToolCalls {
            tool_calls: prepare_tool_calls(tool_calls),
            assistant_text: if interpreted_textual_call {
                String::new()
            } else {
                final_text.unwrap_or_default()
            },
            reasoning,
            reasoning_details,
        });
    }

    if let Some(text) = final_text
        && (!text.trim().is_empty() || is_harmony || proposed_plan.is_some())
    {
        tracing::info!(
            target: "vtcode.turn.metrics",
            metric = "llm_response_parsed",
            kind = "text",
            tool_calls = 0,
            interpreted_textual_call,
            reasoning_segments = reasoning_segment_count,
            reasoning_details = reasoning_details_count,
            content_len = text.len(),
            is_harmony,
            planning_active,
            allow_plan_interview,
            request_user_input_enabled,
            proposed_plan = proposed_plan.is_some(),
            "turn metric"
        );
        return Ok(TurnProcessingResult::TextResponse { text, reasoning, reasoning_details, proposed_plan });
    }

    // Classify the empty shape from the response's own data so later recovery
    // steps can distinguish a true no-output response from tokens that never
    // surfaced as final content. Native tool calls on a tool-free passthrough
    // keep their reasoning text in the capture above (see the drop site).
    let facts = NoOutputFacts {
        completion_tokens: response.usage.as_ref().map_or(0, |usage| usage.completion_tokens),
        reasoning_only,
        dropped_tool_calls: dropped_native_tool_calls,
        voided_text,
    };

    tracing::info!(
        target: "vtcode.turn.metrics",
        metric = "llm_response_parsed",
        kind = "empty",
        tool_calls = 0,
        interpreted_textual_call,
        reasoning_segments = reasoning_segment_count,
        reasoning_details = reasoning_details_count,
        content_len = 0,
        completion_tokens = facts.completion_tokens,
        had_dropped_tool_calls = facts.dropped_tool_calls,
        is_harmony,
        planning_active,
        allow_plan_interview,
        request_user_input_enabled,
        proposed_plan = proposed_plan.is_some(),
        "turn metric"
    );

    Ok(TurnProcessingResult::Empty { facts })
}

fn looks_like_structured_plan(text: &str) -> bool {
    const SUMMARY_HEADERS: &[&str] = &["Summary"];
    const VALIDATION_HEADERS: &[&str] = &["Test Cases and Validation", "Validation"];

    let has_section = |headers: &[&str]| {
        text.lines().map(str::trim).any(|line| {
            let mut normalized = line.trim_start_matches('>').trim_start();
            while let Some(stripped) = normalized.strip_prefix('#') {
                normalized = stripped.trim_start();
            }
            for marker in ["- ", "* ", "• "] {
                if let Some(stripped) = normalized.strip_prefix(marker) {
                    normalized = stripped.trim_start();
                    break;
                }
            }
            headers.iter().any(|header| {
                normalized.eq_ignore_ascii_case(header)
                    || normalized
                        .get(..header.len() + 1)
                        .and_then(|prefix| prefix.strip_suffix(':'))
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(header))
            })
        })
    };

    let report = validate_plan_content(text);
    has_section(SUMMARY_HEADERS) && has_section(VALIDATION_HEADERS) && report.is_ready()
}
pub(crate) fn prepare_tool_calls(
    tool_calls: Vec<vtcode_core::llm::provider::ToolCall>,
) -> Vec<PreparedAssistantToolCall> {
    tool_calls.into_iter().map(PreparedAssistantToolCall::new).collect()
}

pub(crate) fn build_interview_args_from_text(text: &str) -> Option<serde_json::Value> {
    let mut questions = extract_interview_questions(text);
    if questions.is_empty()
        && let Some(synthesized) = synthesize_alignment_question(text)
    {
        questions.push(synthesized);
    }
    if questions.is_empty() {
        return None;
    }

    let focus_area = infer_focus_area(text);
    let analysis_hints = extract_analysis_hints(text);

    let payload = questions
        .iter()
        .enumerate()
        .map(|(index, question)| {
            let mut entry = serde_json::Map::new();
            entry.insert("id".to_string(), serde_json::json!(format!("question_{}", index + 1)));
            entry.insert("header".to_string(), serde_json::json!(format!("Q{}", index + 1)));
            entry.insert("question".to_string(), serde_json::json!(question));
            if let Some(focus_area) = focus_area {
                entry.insert("focus_area".to_string(), serde_json::json!(focus_area));
            }
            if !analysis_hints.is_empty() {
                entry.insert("analysis_hints".to_string(), serde_json::json!(analysis_hints));
            }
            serde_json::Value::Object(entry)
        })
        .collect::<Vec<_>>();

    Some(serde_json::json!({ "questions": payload }))
}

pub(crate) fn extract_interview_questions(text: &str) -> Vec<String> {
    let mut questions = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(question) = parse_numbered_question(trimmed) {
            questions.push(question);
            continue;
        }
        if let Some(question) = parse_bullet_question(trimmed) {
            questions.push(question);
        }
    }

    if questions.is_empty() {
        let trimmed = text.trim();
        let normalized = normalize_question_line(trimmed);
        if !normalized.is_empty() && normalized.contains('?') && normalized.len() <= 200 {
            questions.push(normalized);
        }
    }

    questions.truncate(3);
    questions
}

fn parse_numbered_question(line: &str) -> Option<String> {
    let mut digits_len = 0usize;
    for ch in line.chars() {
        if ch.is_ascii_digit() {
            digits_len += ch.len_utf8();
        } else {
            break;
        }
    }
    if digits_len == 0 {
        return None;
    }

    let rest = line[digits_len..].trim_start();
    let mut chars = rest.chars();
    let punct = chars.next()?;
    if punct != '.' && punct != ')' {
        return None;
    }
    let remainder = chars.as_str().trim_start();
    let normalized = normalize_question_line(remainder);
    if normalized.contains('?') {
        Some(normalized)
    } else {
        None
    }
}

fn parse_bullet_question(line: &str) -> Option<String> {
    for prefix in ["- ", "* ", "• "] {
        if let Some(stripped) = line.strip_prefix(prefix) {
            let candidate = normalize_question_line(stripped.trim());
            if candidate.contains('?') {
                return Some(candidate);
            }
        }
    }
    None
}

fn normalize_question_line(line: &str) -> String {
    let mut current = line.trim();

    if let Some(stripped) = current.strip_prefix('>') {
        current = stripped.trim_start();
    }

    let mut changed = true;
    while changed {
        changed = false;
        if let Some(stripped) = strip_wrapping(current, "**", "**") {
            current = stripped;
            changed = true;
        } else if let Some(stripped) = strip_wrapping(current, "__", "__") {
            current = stripped;
            changed = true;
        } else if let Some(stripped) = strip_wrapping(current, "`", "`") {
            current = stripped;
            changed = true;
        } else if let Some(stripped) = strip_wrapping(current, "*", "*") {
            current = stripped;
            changed = true;
        } else if let Some(stripped) = strip_wrapping(current, "_", "_") {
            current = stripped;
            changed = true;
        } else if let Some(stripped) = strip_wrapping(current, "\"", "\"") {
            current = stripped;
            changed = true;
        } else if let Some(stripped) = strip_wrapping(current, "'", "'") {
            current = stripped;
            changed = true;
        }
    }

    current.trim().to_string()
}

fn synthesize_alignment_question(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    if !contains_any(
        &lower,
        &[
            "need clarification",
            "need your input",
            "before finalizing",
            "before finalising",
            "open questions",
            "for alignment",
            "key decisions",
            "decision points",
        ],
    ) {
        return None;
    }

    if contains_any(&lower, &["system prompt", "prompt architecture", "prompt variants"]) {
        return Some("Which system prompt improvement area should we prioritize first?".to_string());
    }

    if lower.contains("planning workflow") {
        return Some("Which planning workflow improvement area should we prioritize first?".to_string());
    }

    Some("Which improvement area should we prioritize first?".to_string())
}

fn infer_focus_area(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    if contains_any(&lower, &["system prompt", "prompt architecture", "prompt variants"]) {
        return Some("system_prompt");
    }
    if lower.contains("planning workflow") {
        return Some("planning_workflow");
    }
    if contains_any(&lower, &["verification", "test coverage", "validation"]) {
        return Some("verification");
    }
    None
}

fn extract_analysis_hints(text: &str) -> Vec<String> {
    let mut hints = Vec::new();

    for line in text.lines() {
        if hints.len() >= 8 {
            break;
        }

        let normalized = normalize_hint_line(line);
        if normalized.len() < 12 || normalized.contains('?') {
            continue;
        }

        let lower = normalized.to_lowercase();
        if !contains_any(
            &lower,
            &[
                "redundan",
                "overlap",
                "missing",
                "failure",
                "timeout",
                "fallback",
                "loop",
                "optimiz",
                "token",
                "prompt",
                "harness",
                "doc",
                "verification",
                "test",
                "quality",
                "risk",
                "constraint",
                "circular",
            ],
        ) {
            continue;
        }

        if hints.iter().any(|existing: &String| existing.eq_ignore_ascii_case(&normalized)) {
            continue;
        }

        hints.push(normalized);
    }

    hints
}

fn normalize_hint_line(line: &str) -> String {
    let mut current = line.trim();

    for prefix in ["- ", "* ", "• "] {
        if let Some(stripped) = current.strip_prefix(prefix) {
            current = stripped.trim_start();
            break;
        }
    }

    let mut digit_len = 0usize;
    for ch in current.chars() {
        if ch.is_ascii_digit() {
            digit_len += ch.len_utf8();
        } else {
            break;
        }
    }
    if digit_len > 0 {
        let rest = current[digit_len..].trim_start();
        if let Some(stripped) = rest.strip_prefix('.') {
            current = stripped.trim_start();
        } else if let Some(stripped) = rest.strip_prefix(')') {
            current = stripped.trim_start();
        }
    }

    normalize_question_line(current)
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn strip_wrapping<'a>(line: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    if line.len() <= prefix.len() + suffix.len() {
        return None;
    }
    if !line.starts_with(prefix) || !line.ends_with(suffix) {
        return None;
    }
    Some(line[prefix.len()..line.len() - suffix.len()].trim())
}

#[cfg(test)]
mod salvage_tests;
#[cfg(test)]
mod tests;
