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

    // Capture the empty-output facts before the planning salvage below can
    // move `reasoning_text` into `final_text`; a dropped tool-call pass
    // re-nulls `final_text` later, so the reasoning text is kept here for the
    // bounded violation-repair salvage path.
    let dropped_native_tool_calls =
        planning_active && !allow_tool_calls && response.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty());
    let salvaged_reasoning = if dropped_native_tool_calls {
        vtcode_commons::formatting::truncate_byte_budget(&reasoning_text, 8192, "…")
    } else {
        String::new()
    };

    // Some providers put the completed planning synthesis in the reasoning
    // channel and return an empty assistant content field. That previously
    // made a successful planning turn look empty and left the user stuck after
    // the model said it was ready to present the plan. Preserve this only for
    // planning mode, and only when there is no normal response content.
    if planning_active
        && final_text.as_deref().is_none_or(|text| text.trim().is_empty())
        && !reasoning_text.trim().is_empty()
    {
        final_text = Some(reasoning_text);
    }

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
        let plan_shape_is_valid = !strict_recovery_plan_shape || has_exactly_one_proposed_plan_block(text);
        // The plan is rendered once by the approval flow below. Keep only the
        // non-plan prose in the normal assistant response; retaining the plan
        // body here makes the transcript render it a second time before the
        // approval heading is emitted.
        let stripped_text = if strict_recovery_plan_shape && !plan_shape_is_valid {
            // Keep malformed/duplicate/alternate plan markup available to the
            // recovery handoff. The handler stores it as a bounded rejected
            // draft instead of silently reducing the failure to "no answer".
            text.to_string()
        } else if extraction.plan_text.is_some() {
            strip_plan_persistence_policy_line(&extraction.stripped_text)
        } else {
            extraction.stripped_text
        };
        proposed_plan = if plan_shape_is_valid {
            extraction.plan_text
        } else {
            None
        };
        final_text = Some(stripped_text);
    }

    // A provider response that includes native tool calls is not a valid
    // tool-free synthesis, even when it also contains a seemingly complete
    // plan. Do not silently accept the plan while dropping the forbidden
    // calls; the recovery handler must produce the resumable blocked handoff.
    if planning_active && !allow_tool_calls && response.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty()) {
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
            let tool_display = crate::agent::runloop::unified::tool_summary::humanize_tool_name(&name);
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
                format!("Detected {} request", crate::agent::runloop::unified::tool_summary::humanize_tool_name(&name))
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
        salvaged_reasoning,
        dropped_tool_calls: dropped_native_tool_calls,
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
mod tests {
    use super::*;
    use vtcode_core::config::constants::tools;
    use vtcode_core::llm::provider::{FinishReason, LLMResponse, ToolCall, Usage};

    #[test]
    fn attempted_draft_detection_preserves_questions_native_tools_and_execution_prose() {
        let draft = "## Implementation Steps\n1. Inspect README\n2. Refine wording";
        for (planning, native_tools, question) in [(true, true, false), (true, false, true), (false, false, false)] {
            let text = if question {
                format!("{draft}\nWhich audience should this target?")
            } else {
                draft.to_string()
            };
            let mut response = LLMResponse::new("test", &text);
            if native_tools {
                response.tool_calls = Some(vec![ToolCall::function(
                    "read".to_string(),
                    "read_file".to_string(),
                    r#"{"path":"README.md"}"#.to_string(),
                )]);
            }
            let mut renderer = AnsiRenderer::stdout();
            let result =
                process_llm_response(&response, &mut renderer, 0, planning, false, false, true, None, None).unwrap();
            if native_tools {
                let TurnProcessingResult::ToolCalls { tool_calls, .. } = result else {
                    panic!("native inspection must be preserved");
                };
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].tool_name(), "read_file");
            } else {
                assert!(
                    matches!(result, TurnProcessingResult::TextResponse { proposed_plan: None, text: actual, .. } if actual == text)
                );
            }
        }
    }

    #[test]
    fn prepare_tool_calls_reuses_parsed_arguments_and_metadata() {
        let tool_calls = vec![
            ToolCall::function(
                "call_search".to_string(),
                tools::CODE_SEARCH.to_string(),
                r#"{"query":"TurnProcessingResult"}"#.to_string(),
            ),
            ToolCall::function(
                "call_exec".to_string(),
                tools::UNIFIED_EXEC.to_string(),
                r#"{"action":"run","command":["cargo","check"]}"#.to_string(),
            ),
        ];

        let prepared = prepare_tool_calls(tool_calls);

        assert_eq!(prepared.len(), 2);
        assert_eq!(prepared[0].call_id(), "call_search");
        assert_eq!(prepared[0].tool_name(), tools::CODE_SEARCH);
        assert_eq!(prepared[0].args(), Some(&serde_json::json!({"query":"TurnProcessingResult"})));
        assert!(prepared[0].is_parallel_safe());
        assert!(!prepared[0].is_command_execution());

        assert_eq!(prepared[1].call_id(), "call_exec");
        assert_eq!(prepared[1].tool_name(), tools::UNIFIED_EXEC);
        assert_eq!(prepared[1].args(), Some(&serde_json::json!({"action":"run","command":["cargo","check"]})));
        assert!(!prepared[1].is_parallel_safe());
        assert!(prepared[1].is_command_execution());
    }

    #[test]
    fn prepare_tool_calls_records_invalid_json_without_reparsing() {
        let tool_calls = vec![ToolCall::function(
            "call_invalid".to_string(),
            tools::CODE_SEARCH.to_string(),
            "{not-json}".to_string(),
        )];

        let prepared = prepare_tool_calls(tool_calls);

        assert_eq!(prepared.len(), 1);
        assert_eq!(prepared[0].call_id(), "call_invalid");
        assert_eq!(prepared[0].tool_name(), tools::CODE_SEARCH);
        assert!(prepared[0].args().is_none());
        assert!(prepared[0].args_error().is_some());
        assert!(!prepared[0].is_parallel_safe());
        assert!(!prepared[0].is_command_execution());
    }

    #[test]
    fn prepare_tool_calls_keeps_custom_tool_payload_as_raw_string() {
        let tool_calls = vec![ToolCall::custom(
            "call_patch".to_string(),
            tools::APPLY_PATCH.to_string(),
            "*** Begin Patch\n*** End Patch\n".to_string(),
        )];

        let prepared = prepare_tool_calls(tool_calls);

        assert_eq!(prepared.len(), 1);
        assert_eq!(prepared[0].call_id(), "call_patch");
        assert_eq!(prepared[0].tool_name(), tools::APPLY_PATCH);
        assert_eq!(prepared[0].args(), Some(&serde_json::json!("*** Begin Patch\n*** End Patch\n")));
        assert!(prepared[0].args_error().is_none());
        assert!(!prepared[0].is_parallel_safe());
        assert!(!prepared[0].is_command_execution());
    }

    #[test]
    fn extract_interview_questions_from_numbered_lines() {
        let text = "1. First question?\n2) Second question?\n3. Third question?";
        let questions = extract_interview_questions(text);
        assert_eq!(questions.len(), 3);
        assert_eq!(questions[0], "First question?");
        assert_eq!(questions[1], "Second question?");
        assert_eq!(questions[2], "Third question?");
    }

    #[test]
    fn extract_interview_questions_from_bullets() {
        let text = "- Should we do X?\n- Should we do Y?";
        let questions = extract_interview_questions(text);
        assert_eq!(questions.len(), 2);
        assert_eq!(questions[0], "Should we do X?");
    }

    #[test]
    fn process_llm_response_turns_questions_into_tool_call() {
        let response = LLMResponse {
            content: Some("1. First question?\n2. Second question?".to_string()),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, false, true, true, true, None, None)
            .expect("processing should succeed");

        match result {
            TurnProcessingResult::ToolCalls { tool_calls, .. } => {
                assert_eq!(tool_calls.len(), 1);
            }
            _ => panic!("Expected tool calls"),
        }
    }

    fn refusal_response(content: Option<&str>, reasoning_details: Option<Vec<String>>) -> LLMResponse {
        LLMResponse {
            content: content.map(str::to_string),
            finish_reason: FinishReason::Refusal,
            reasoning_details,
            ..Default::default()
        }
    }

    fn usage_with_completion_tokens(completion_tokens: u32) -> Usage {
        Usage {
            prompt_tokens: 1,
            completion_tokens,
            total_tokens: completion_tokens + 1,
            ..Default::default()
        }
    }

    #[test]
    fn process_llm_response_captures_completion_tokens_without_content() {
        // The observed failure shape: the provider billed tokens but surfaced
        // no final content, so the turn must record that evidence instead of
        // claiming a plain empty response.
        let response = LLMResponse {
            usage: Some(usage_with_completion_tokens(477)),
            ..Default::default()
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, true, false, true, false, None, None)
            .expect("processing should succeed");

        let TurnProcessingResult::Empty { facts } = result else {
            panic!("tokens without content must classify as empty with captured facts");
        };
        assert_eq!(facts.completion_tokens, 477);
        assert!(facts.has_completion_tokens());
        assert!(!facts.dropped_tool_calls);
        assert!(facts.salvaged_reasoning.is_empty(), "no tool calls were dropped, so nothing is salvageable");
    }

    #[test]
    fn process_llm_response_captures_dropped_tool_calls_and_keeps_reasoning() {
        // A tool-free pass that still emits native tool calls drops them; the
        // reasoning text those calls displaced must survive for repair instead
        // of being reported as an empty synthesis. Reasoning models deliver
        // that text inside `<thinking>` tags on the wire.
        let plan_body = "## Summary\nPlan to change the parser.\n## Implementation Steps\n1. Edit the parser -> files: [src/a.rs] -> verify: [cargo check]";
        let response = LLMResponse {
            content: None,
            reasoning: Some(format!("<thinking>{plan_body}</thinking>")),
            tool_calls: Some(vec![ToolCall::function(
                "call_1".to_string(),
                "grep".to_string(),
                "{}".to_string(),
            )]),
            usage: Some(usage_with_completion_tokens(210)),
            ..Default::default()
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, true, false, true, false, None, None)
            .expect("processing should succeed");

        let TurnProcessingResult::Empty { facts } = result else {
            panic!("dropped tool calls must classify as empty with captured facts");
        };
        assert!(facts.dropped_tool_calls);
        assert_eq!(facts.completion_tokens, 210);
        assert_eq!(
            facts.salvaged_reasoning.trim(),
            plan_body,
            "the reasoning displaced by the dropped calls must be preserved verbatim for repair"
        );
    }

    #[test]
    fn process_llm_response_total_absence_captures_no_evidence() {
        // Asymmetric pair for the tokens case: a genuinely empty response must
        // not fabricate salvage or a token count it has no evidence for.
        let response = LLMResponse::default();

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, true, false, true, false, None, None)
            .expect("processing should succeed");

        let TurnProcessingResult::Empty { facts } = result else {
            panic!("a response with nothing in it must classify as empty");
        };
        assert_eq!(facts.completion_tokens, 0);
        assert!(!facts.has_completion_tokens());
        assert!(!facts.dropped_tool_calls);
        assert!(facts.salvaged_reasoning.is_empty());
    }

    #[test]
    fn process_llm_response_ends_refused_turn_with_category_and_explanation() {
        let detail = serde_json::json!({
            "type": "stop_details",
            "category": "cyber",
            "explanation": "Request resembles malware development",
        })
        .to_string();
        // Partial output cut off by the refusal must not become the answer.
        let response = refusal_response(Some("Sure, here is"), Some(vec![detail]));

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, false, true, true, true, None, None)
            .expect("processing should succeed");

        let TurnProcessingResult::Refusal { reason } = result else {
            panic!("refusal should end the turn");
        };
        assert_eq!(
            reason,
            "The model declined this request (category: cyber): request resembles malware development. \
             The request was not retried; rephrase it or switch models."
        );
        assert!(!reason.contains("Sure, here is"), "{reason}");
        assert!(!reason.contains("fallback"), "no fallback ran, so none is claimed: {reason}");
    }

    #[test]
    fn process_llm_response_uses_refusal_content_when_stop_details_are_absent() {
        let response = refusal_response(Some("  I can't help with that.  "), None);

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, false, true, true, true, None, None)
            .expect("processing should succeed");

        let TurnProcessingResult::Refusal { reason } = result else {
            panic!("refusal should end the turn");
        };
        assert_eq!(
            reason,
            "The model declined this request: I can't help with that. \
             The request was not retried; rephrase it or switch models."
        );
    }

    #[test]
    fn process_llm_response_reports_refusal_without_stop_details() {
        let response = refusal_response(None, None);

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, false, true, true, true, None, None)
            .expect("processing should succeed");

        let TurnProcessingResult::Refusal { reason } = result else {
            panic!("refusal should not fall through to empty-response recovery");
        };
        assert!(reason.starts_with("The model declined this request."), "{reason}");
        assert!(!reason.contains("category"), "{reason}");
        assert!(!reason.contains("fallback"), "{reason}");
    }

    #[tokio::test]
    async fn process_llm_response_rejects_textual_exec_command_without_command() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let tool_registry = vtcode_core::tools::ToolRegistry::new(temp.path().to_path_buf()).await;
        let response = LLMResponse {
            content: Some("run()".to_string()),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result =
            process_llm_response(&response, &mut renderer, 7, false, false, true, true, None, Some(&tool_registry))
                .expect("processing should succeed");

        match result {
            TurnProcessingResult::TextResponse { text, .. } => {
                assert_eq!(text, "run()");
            }
            _ => panic!("Expected textual invalid exec_command to stay a text response"),
        }
    }

    #[test]
    fn process_llm_response_extracts_execution_replan_block() {
        let response = LLMResponse {
            content: Some(
                "Replanning: the approved paths no longer exist.\n<proposed_plan>\n## Summary\nRevised scope.\n</proposed_plan>"
                    .to_string(),
            ),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        // planning_active=false: execution turns must still surface an
        // explicit `<proposed_plan>` replan instead of leaking it as text.
        let result = process_llm_response(&response, &mut renderer, 0, false, false, true, true, None, None)
            .expect("processing should succeed");

        match result {
            TurnProcessingResult::TextResponse { text, proposed_plan, .. } => {
                assert!(proposed_plan.is_some_and(|plan| plan.contains("Revised scope")));
                assert!(!text.contains("<proposed_plan>"));
                assert!(text.contains("Replanning: the approved paths no longer exist."));
            }
            _ => panic!("Expected text response with extracted execution replan"),
        }
    }

    #[test]
    fn process_llm_response_keeps_execution_replan_with_tool_calls_unextracted() {
        let response = LLMResponse {
            content: Some("Continuing work.\n<proposed_plan>\n## Summary\nDraft only.\n</proposed_plan>".to_string()),
            tool_calls: Some(vec![ToolCall::function(
                "call_1".to_string(),
                "code_search".to_string(),
                r#"{"query":"x"}"#.to_string(),
            )]),
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, false, false, true, true, None, None)
            .expect("processing should succeed");

        match result {
            TurnProcessingResult::ToolCalls { tool_calls, assistant_text, .. } => {
                assert_eq!(tool_calls.len(), 1);
                assert!(assistant_text.contains("<proposed_plan>"));
            }
            _ => panic!("Expected tool-call dispatch when the response carries native calls"),
        }
    }

    #[test]
    fn process_llm_response_parses_spaced_dsml_before_stripping_markup() {
        let response = LLMResponse {
            content: Some(concat!(
                "<\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} invoke name=\"exec_command\">\n",
                "<\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} parameter name=\"cmd\" string=\"true\">true</\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} parameter>\n",
                "</\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} invoke>",
            ).to_string()),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, false, false, true, true, None, None)
            .expect("processing should succeed");

        match result {
            TurnProcessingResult::ToolCalls { tool_calls, .. } => {
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].tool_name(), tools::EXEC_COMMAND);
                assert_eq!(tool_calls[0].args(), Some(&serde_json::json!({"cmd": "true", "action": "run"})));
            }
            TurnProcessingResult::TextResponse { text, .. } => {
                panic!("spaced DSML leaked as text: {text}");
            }
            TurnProcessingResult::Empty { .. } | TurnProcessingResult::Refusal { .. } => {
                panic!("spaced DSML should produce a tool call")
            }
        }
    }

    #[test]
    fn process_llm_response_skips_questions_when_interview_not_ready() {
        let response = LLMResponse {
            content: Some("1. First question?\n2. Second question?".to_string()),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, false, false, true, true, None, None)
            .expect("processing should succeed");

        match result {
            TurnProcessingResult::TextResponse { text, .. } => {
                assert!(text.contains("First question"));
            }
            _ => panic!("Expected text response without tool calls"),
        }
    }

    #[test]
    fn process_llm_response_removes_plan_from_duplicate_assistant_text() {
        let response = LLMResponse {
            content: Some("Intro\n<proposed_plan>\n- Step 1\n</proposed_plan>\nOutro".to_string()),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, true, false, true, true, None, None)
            .expect("processing should succeed");

        match result {
            TurnProcessingResult::TextResponse { text, proposed_plan, .. } => {
                assert_eq!(text, "Intro\n\nOutro");
                assert!(!text.contains("<proposed_plan>"));
                assert_eq!(proposed_plan.as_deref(), Some("- Step 1"));
            }
            _ => panic!("Expected text response with visible proposed plan"),
        }
    }

    #[test]
    fn process_llm_response_extracts_plan_block_in_planning_workflow() {
        let response = LLMResponse {
            content: Some("Intro\n<plan>\n- Step 1\n</plan>\nOutro".to_string()),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, true, false, true, true, None, None)
            .expect("processing should succeed");

        match result {
            TurnProcessingResult::TextResponse { text, proposed_plan, .. } => {
                assert_eq!(proposed_plan.as_deref(), Some("- Step 1"));
                assert!(!text.contains("<plan>"));
                assert!(!text.contains("</plan>"));
            }
            _ => panic!("Expected text response with extracted <plan> block"),
        }
    }

    #[test]
    fn process_llm_response_removes_echoed_plan_policy_text() {
        let policy = vtcode_core::prompts::system::PLANNING_WORKFLOW_PLAN_PERSISTENCE_POLICY_LINE;
        let response = LLMResponse {
            content: Some(format!("{policy}\n\n<plan>\n- Step 1\n</plan>")),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, true, false, true, true, None, None)
            .expect("response processing should succeed");

        match result {
            TurnProcessingResult::TextResponse { text, proposed_plan, .. } => {
                assert!(text.trim().is_empty(), "policy echo must not remain visible: {text:?}");
                assert_eq!(proposed_plan.as_deref(), Some("- Step 1"));
            }
            _ => panic!("expected the alternate plan block to remain actionable"),
        }
    }

    #[test]
    fn process_llm_response_extracts_canonical_stream_handoff_plan() {
        let response = LLMResponse {
            content: Some("Intro\n\nOutro\n\n<proposed_plan>\n- Streamed step\n</proposed_plan>".to_string()),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, true, false, true, true, None, None)
            .expect("response processing should succeed");

        match result {
            TurnProcessingResult::TextResponse { text, proposed_plan, .. } => {
                assert_eq!(proposed_plan.as_deref(), Some("- Streamed step"));
                assert_eq!(text.trim_end(), "Intro\n\nOutro");
                assert!(!text.contains("<proposed_plan>"));
            }
            _ => panic!("expected the canonical stream handoff to produce a text response"),
        }
    }

    #[test]
    fn process_llm_response_drops_tool_calls_attached_to_a_complete_plan() {
        let response = LLMResponse {
            content: Some(
                "# Attached plan\n\n## Summary\nKeep approval ahead of tool execution.\n\n## Steps\n1. Gate the plan -> files: [src/agent/runloop/unified/turn/turn_processing/response_processing.rs] -> verify: cargo check\n\n## Validation\n1. Run the planning regression tests.\n\n## Assumptions\n1. Preserve the existing planning policy.\n".to_string(),
            ),
            tool_calls: Some(vec![ToolCall::function(
                "call_after_plan".to_string(),
                "code_search".to_string(),
                r#"{"query":"should-not-run"}"#.to_string(),
            )]),
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::ToolCalls,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, true, false, true, true, None, None)
            .expect("processing should succeed");

        match result {
            TurnProcessingResult::TextResponse { text, proposed_plan, .. } => {
                assert!(text.is_empty());
                assert!(proposed_plan.is_some());
            }
            TurnProcessingResult::ToolCalls { .. } => panic!("attached calls must not bypass plan approval"),
            TurnProcessingResult::Empty { .. } | TurnProcessingResult::Refusal { .. } => {
                panic!("complete plan should remain actionable")
            }
        }
    }

    #[test]
    fn process_llm_response_extracts_sparse_proposed_plan_blocks() {
        let response = LLMResponse {
            content: Some(
                r#"Intro
Next open decision: none.
<proposed_plan>
# Apply Slate-Style Prompting

## Summary
Keep the default runtime prompt sparse and consistent with Planning workflow.

## Implementation Steps
1. Update prompt constants -> files: [crates/codegen/vtcode-core/src/prompts/system.rs] -> verify: [cargo check]
2. Update plan scaffold -> files: [crates/codegen/vtcode-core/src/tools/handlers/planning_workflow.rs] -> verify: [cargo test -p vtcode-core test_start_planning -- --nocapture]
3. Update parser tests -> files: [src/agent/runloop/unified/turn/turn_processing/response_processing.rs] -> verify: [cargo test -p vtcode process_llm_response_extracts_sparse_proposed_plan_blocks -- --nocapture]

## Test Cases and Validation
1. Build and lint: [project build and lint command(s) based on detected toolchain]
2. Tests: [project test command(s) based on detected toolchain]
3. Targeted behavior checks: planning workflow transcript extraction

## Assumptions and Defaults
1. `Next open decision` remains the only explicit reopen marker.
2. The proposed plan stays decision-complete without rigid extra sections.
</proposed_plan>
Outro"#
                    .to_string(),
            ),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, true, false, true, true, None, None)
            .expect("processing should succeed");

        match result {
            TurnProcessingResult::TextResponse { text, proposed_plan, .. } => {
                let plan = proposed_plan.expect("expected extracted proposed plan");
                for required_section in [
                    "## Summary",
                    "## Implementation Steps",
                    "## Test Cases and Validation",
                    "## Assumptions and Defaults",
                ] {
                    assert!(
                        plan.contains(required_section),
                        "proposed_plan missing required section: {required_section}"
                    );
                }
                assert!(!plan.contains("## Scope Locked"));
                assert!(!plan.contains("## Public API / Interface Changes"));
                assert!(!text.contains("<proposed_plan>"));
                assert!(text.contains("Intro"));
                assert!(text.contains("Outro"));
            }
            _ => panic!("Expected text response with extracted proposed plan"),
        }
    }

    #[test]
    fn tool_free_planning_recovery_accepts_only_one_canonical_plan_block() {
        let valid_plan = "<proposed_plan>\n## Summary\nKeep recovery deterministic.\n\n## Implementation Steps\n1. Update src/lib.rs -> files: [src/lib.rs] -> verify: [cargo check --locked]\n\n## Test Cases and Validation\n- Run the focused nextest suite.\n\n## Assumptions and Defaults\n- Preserve the existing event contract.\n</proposed_plan>";
        let response = |content: &str| LLMResponse::new("test", content);

        let mut renderer = AnsiRenderer::stdout();
        let valid =
            process_llm_response(&response(valid_plan), &mut renderer, 0, true, false, false, false, None, None)
                .expect("valid recovery plan should process");
        assert!(matches!(
            valid,
            TurnProcessingResult::TextResponse { proposed_plan: Some(_), text, .. } if text.trim().is_empty()
        ));

        let mut renderer = AnsiRenderer::stdout();
        let alternate = process_llm_response(
            &response(&valid_plan.replace("proposed_plan", "plan")),
            &mut renderer,
            0,
            true,
            false,
            false,
            false,
            None,
            None,
        )
        .expect("alternate marker should be handled");
        assert!(matches!(
            alternate,
            TurnProcessingResult::TextResponse { proposed_plan: None, text, .. } if text.contains("<plan>")
        ));

        let mut renderer = AnsiRenderer::stdout();
        let duplicate = process_llm_response(
            &response(&format!("{valid_plan}\n{valid_plan}")),
            &mut renderer,
            0,
            true,
            false,
            false,
            false,
            None,
            None,
        )
        .expect("duplicate marker should be handled");
        assert!(matches!(
            duplicate,
            TurnProcessingResult::TextResponse { proposed_plan: None, text, .. } if text.matches("<proposed_plan>").count() == 2
        ));
    }

    #[test]
    fn tool_free_planning_recovery_rejects_native_tool_calls_even_with_a_plan() {
        let mut response = LLMResponse::new(
            "test",
            "<proposed_plan>\n## Summary\nDo the work.\n\n## Implementation Steps\n1. Update src/lib.rs -> files: [src/lib.rs] -> verify: [cargo check --locked]\n\n## Test Cases and Validation\n- Run nextest.\n\n## Assumptions and Defaults\n- Preserve the contract.\n</proposed_plan>",
        );
        response.tool_calls = Some(vec![ToolCall::function(
            "call-forbidden".to_string(),
            "code_search".to_string(),
            r#"{"query":"must not execute"}"#.to_string(),
        )]);

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, true, false, false, false, None, None)
            .expect("forbidden tool call should be handled as an empty recovery result");
        assert!(matches!(result, TurnProcessingResult::Empty { .. }));
    }

    #[test]
    fn extract_interview_questions_strips_markdown_wrapping() {
        let text = "**How should we proceed?**";
        let questions = extract_interview_questions(text);
        assert_eq!(questions, vec!["How should we proceed?".to_string()]);
    }

    #[test]
    fn extract_interview_questions_handles_bold_bullets() {
        let text = "- **Should we do X?**";
        let questions = extract_interview_questions(text);
        assert_eq!(questions, vec!["Should we do X?".to_string()]);
    }

    #[test]
    fn build_interview_args_synthesizes_alignment_question_with_hints() {
        let text = r#"
I've analyzed the current system prompt architecture.
The plan is drafted. I need clarification on 3 key decisions before finalizing the implementation approach.
Key findings:
• Redundancy exists between prompt variants (tool guidance, bias for action warnings)
• Missing explicit guidance for common failure patterns (patch failures, circular deps)
• Harness integration is good but could be strengthened with more specific doc refs
Open questions for alignment:
"#;

        let args = build_interview_args_from_text(text).expect("expected synthesized interview args");
        let questions = args["questions"].as_array().expect("questions should be an array");
        assert_eq!(questions.len(), 1);

        let first = &questions[0];
        let question_text = first["question"].as_str().expect("question should be a string");
        assert!(question_text.contains("prioritize"));
        assert_eq!(first["focus_area"].as_str(), Some("system_prompt"));

        let hints = first["analysis_hints"].as_array().expect("analysis_hints should exist");
        assert!(!hints.is_empty(), "expected extracted hints");
    }

    #[test]
    fn process_llm_response_turns_alignment_request_into_tool_call() {
        let response = LLMResponse {
            content: Some("Need clarification before finalizing.\nOpen questions for alignment:".to_string()),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 1, true, true, true, true, None, None)
            .expect("processing should succeed");

        match result {
            TurnProcessingResult::ToolCalls { tool_calls, .. } => {
                let name = tool_calls.first().map(|call| call.tool_name()).expect("function name expected");
                assert_eq!(name, tools::REQUEST_USER_INPUT);
            }
            _ => panic!("Expected tool calls"),
        }
    }

    #[test]
    fn process_llm_response_preserves_reasoning_details_for_tool_calls() {
        let response = LLMResponse {
            content: Some("".to_string()),
            tool_calls: Some(vec![ToolCall::function(
                "call_1".to_string(),
                "code_search".to_string(),
                r#"{"query":"x"}"#.to_string(),
            )]),
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::ToolCalls,
            reasoning: None,
            reasoning_details: Some(vec![r#"{"type":"reasoning_content","text":"trace"}"#.to_string()]),
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, false, false, true, true, None, None)
            .expect("processing should succeed");

        match result {
            TurnProcessingResult::ToolCalls { tool_calls, reasoning_details, .. } => {
                assert_eq!(reasoning_details, Some(vec![r#"{"type":"reasoning_content","text":"trace"}"#.to_string()]));
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].call_id(), "call_1");
                assert!(tool_calls[0].is_parallel_safe());
                assert!(!tool_calls[0].is_command_execution());
            }
            _ => panic!("Expected tool calls"),
        }
    }

    #[test]
    fn process_llm_response_keeps_textual_tool_request_as_text_when_tool_calls_disabled() {
        let response = LLMResponse {
            content: Some("code_search({\"query\":\"Widget\",\"path\":\"src\"})".to_string()),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, false, false, true, false, None, None)
            .expect("processing should succeed");

        match result {
            TurnProcessingResult::TextResponse { text, .. } => {
                assert_eq!(text, "code_search({\"query\":\"Widget\",\"path\":\"src\"})");
            }
            _ => panic!("Expected text response when tool calls are disabled"),
        }
    }

    #[test]
    fn process_llm_response_ignores_structured_tool_calls_when_tool_calls_disabled() {
        let response = LLMResponse {
            content: Some("Final synthesis only.".to_string()),
            tool_calls: Some(vec![ToolCall::function(
                "call_1".to_string(),
                "code_search".to_string(),
                r#"{"query":"Widget","path":"src","result_types":["path"]}"#.to_string(),
            )]),
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::ToolCalls,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            compaction: None,
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result = process_llm_response(&response, &mut renderer, 0, false, false, true, false, None, None)
            .expect("processing should succeed");

        match result {
            TurnProcessingResult::TextResponse { text, .. } => {
                assert_eq!(text, "Final synthesis only.");
            }
            _ => panic!("Expected text response when tool calls are disabled"),
        }
    }

    #[test]
    fn looks_like_structured_plan_detects_plain_text_plan_without_tags() {
        let text = r#"
 •   Summary
     Focus on the startup configuration/validation path.

     1. Measure baseline
       - Action: Add a high-resolution startup timer.
       - Files/symbols: src/main.rs:main.
       - Verify: `cargo run --release` prints phase durations.

     2. Trim binary size
       - Action: Audit Cargo.toml for large crates.
       - Files/symbols: Cargo.toml.
       - Verify: `cargo build --locked --release` size drops.

      Validation

      •   Build: cargo check --locked
      •   Tests: targeted tests in src/startup/.

      Assumptions

      •   Keep the existing startup entry point and output format.
        "#;
        assert!(looks_like_structured_plan(text));
    }

    #[test]
    fn looks_like_structured_plan_rejects_plain_conversation() {
        let text = "Here is a quick update: I searched the codebase and found nothing relevant.";
        assert!(!looks_like_structured_plan(text));

        let generic_fallback = r#"Summary
The current context is sufficient.

Steps
1. Review the relevant code.
2. Make the required changes.

Validation
Run the usual checks.
"#;
        assert!(!looks_like_structured_plan(generic_fallback));
    }
}
