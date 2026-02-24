use anyhow::Result;
use vtcode_core::llm::providers::split_reasoning_from_text;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::plan_blocks::extract_proposed_plan;
use crate::agent::runloop::unified::turn::context::TurnProcessingResult;
use crate::agent::runloop::unified::turn::guards::validate_tool_args_security;

/// Process an LLM response and return a `TurnProcessingResult` describing whether
/// there are tool calls to run, a textual assistant response, or nothing.
pub(crate) fn process_llm_response(
    response: &vtcode_core::llm::provider::LLMResponse,
    renderer: &mut AnsiRenderer,
    conversation_len: usize,
    plan_mode_active: bool,
    allow_plan_interview: bool,
    request_user_input_enabled: bool,
    validation_cache: Option<
        &std::sync::Arc<vtcode_core::tools::validation_cache::ValidationCache>,
    >,
    tool_registry: Option<&vtcode_core::tools::ToolRegistry>,
) -> Result<TurnProcessingResult> {
    use crate::agent::runloop::unified::turn::harmony::strip_harmony_syntax;
    use vtcode_core::config::constants::tools;
    use vtcode_core::llm::provider as uni;

    let mut final_text = response.content.clone();
    let mut proposed_plan: Option<String> = None;
    let mut tool_calls = response.tool_calls.clone().unwrap_or_default();
    let mut interpreted_textual_call = false;
    let mut is_harmony = false;

    if let Some(ref text) = final_text
        && (text.contains("<|start|>") || text.contains("<|channel|>") || text.contains("<|call|>"))
    {
        is_harmony = true;
        let cleaned = strip_harmony_syntax(text);
        if !cleaned.trim().is_empty() {
            final_text = Some(cleaned);
        } else {
            final_text = Some("".to_string());
        }
    }

    if plan_mode_active
        && tool_calls.is_empty()
        && let Some(ref text) = final_text
    {
        let extraction = extract_proposed_plan(text);
        final_text = Some(extraction.stripped_text);
        proposed_plan = extraction.plan_text;
    }

    if tool_calls.is_empty()
        && let Some(text) = final_text.clone()
        && !text.trim().is_empty()
        && let Some((name, args)) =
            crate::agent::runloop::text_tools::detect_textual_tool_call(&text)
    {
        if let Some(validation_failures) =
            validate_tool_args_security(&name, &args, validation_cache, tool_registry)
        {
            let tool_display =
                crate::agent::runloop::unified::tool_summary::humanize_tool_name(&name);
            let failures_list = validation_failures.join("; ");
            crate::agent::runloop::unified::turn::turn_helpers::display_status(
                renderer,
                &format!(
                    "Detected {} but validation failed: {}",
                    tool_display, failures_list
                ),
            )?;
        } else {
            let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
            let code_blocks = crate::agent::runloop::text_tools::extract_code_fence_blocks(&text);
            if !code_blocks.is_empty() {
                crate::agent::runloop::tool_output::render_code_fence_blocks(
                    renderer,
                    &code_blocks,
                )?;
                renderer.line(MessageStyle::Output, "")?;
            }
            let (headline, _) =
                crate::agent::runloop::unified::tool_summary::describe_tool_action(&name, &args);
            let notice = if headline.is_empty() {
                format!(
                    "Detected {} request",
                    crate::agent::runloop::unified::tool_summary::humanize_tool_name(&name)
                )
            } else {
                format!("Detected {headline}")
            };
            crate::agent::runloop::unified::turn::turn_helpers::display_status(renderer, &notice)?;
            let call_id = format!("call_textual_{}", conversation_len);
            tool_calls.push(uni::ToolCall::function(
                call_id.clone(),
                name.clone(),
                args_json.clone(),
            ));
            interpreted_textual_call = true;
            final_text = None;
        }
    }

    if !interpreted_textual_call
        && allow_plan_interview
        && request_user_input_enabled
        && tool_calls.is_empty()
        && let Some(text) = final_text.clone()
        && let Some(args) = build_interview_args_from_text(&text)
    {
        let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
        let call_id = format!("call_interview_{}", conversation_len);
        tool_calls.push(uni::ToolCall::function(
            call_id.clone(),
            tools::REQUEST_USER_INPUT.to_string(),
            args_json,
        ));
        interpreted_textual_call = true;
        final_text = None;
    }

    if !tool_calls.is_empty() {
        return Ok(TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text: if interpreted_textual_call {
                String::new()
            } else {
                final_text.clone().unwrap_or_default()
            },
            reasoning: split_reasoning_from_text(response.reasoning.as_deref().unwrap_or("")).0,
        });
    }

    if let Some(text) = final_text
        && (!text.trim().is_empty() || is_harmony || proposed_plan.is_some())
    {
        return Ok(TurnProcessingResult::TextResponse {
            text,
            reasoning: split_reasoning_from_text(response.reasoning.as_deref().unwrap_or("")).0,
            proposed_plan,
        });
    }

    Ok(TurnProcessingResult::Empty)
}

fn build_interview_args_from_text(text: &str) -> Option<serde_json::Value> {
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
            entry.insert(
                "id".to_string(),
                serde_json::json!(format!("question_{}", index + 1)),
            );
            entry.insert(
                "header".to_string(),
                serde_json::json!(format!("Q{}", index + 1)),
            );
            entry.insert("question".to_string(), serde_json::json!(question));
            if let Some(focus_area) = focus_area {
                entry.insert("focus_area".to_string(), serde_json::json!(focus_area));
            }
            if !analysis_hints.is_empty() {
                entry.insert(
                    "analysis_hints".to_string(),
                    serde_json::json!(analysis_hints),
                );
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

    if contains_any(
        &lower,
        &["system prompt", "prompt architecture", "prompt variants"],
    ) {
        return Some(
            "Which system prompt improvement area should we prioritize first?".to_string(),
        );
    }

    if lower.contains("plan mode") {
        return Some("Which plan mode improvement area should we prioritize first?".to_string());
    }

    Some("Which improvement area should we prioritize first?".to_string())
}

fn infer_focus_area(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    if contains_any(
        &lower,
        &["system prompt", "prompt architecture", "prompt variants"],
    ) {
        return Some("system_prompt");
    }
    if lower.contains("plan mode") {
        return Some("plan_mode");
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

        if hints
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&normalized))
        {
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
    use vtcode_core::llm::provider::{FinishReason, LLMResponse};

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
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result =
            process_llm_response(&response, &mut renderer, 0, false, true, true, None, None)
                .expect("processing should succeed");

        match result {
            TurnProcessingResult::ToolCalls { tool_calls, .. } => {
                assert_eq!(tool_calls.len(), 1);
            }
            _ => panic!("Expected tool calls"),
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
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result =
            process_llm_response(&response, &mut renderer, 0, false, false, true, None, None)
                .expect("processing should succeed");

        match result {
            TurnProcessingResult::TextResponse { text, .. } => {
                assert!(text.contains("First question"));
            }
            _ => panic!("Expected text response without tool calls"),
        }
    }

    #[test]
    fn process_llm_response_strips_proposed_plan_in_plan_mode() {
        let response = LLMResponse {
            content: Some("Intro\n<proposed_plan>\n- Step 1\n</proposed_plan>\nOutro".to_string()),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result =
            process_llm_response(&response, &mut renderer, 0, true, false, true, None, None)
                .expect("processing should succeed");

        match result {
            TurnProcessingResult::TextResponse {
                text,
                proposed_plan,
                ..
            } => {
                assert_eq!(text, "Intro\n\nOutro");
                assert_eq!(proposed_plan.as_deref(), Some("- Step 1"));
            }
            _ => panic!("Expected stripped text response with proposed plan"),
        }
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

        let args =
            build_interview_args_from_text(text).expect("expected synthesized interview args");
        let questions = args["questions"]
            .as_array()
            .expect("questions should be an array");
        assert_eq!(questions.len(), 1);

        let first = &questions[0];
        let question_text = first["question"]
            .as_str()
            .expect("question should be a string");
        assert!(question_text.contains("prioritize"));
        assert_eq!(first["focus_area"].as_str(), Some("system_prompt"));

        let hints = first["analysis_hints"]
            .as_array()
            .expect("analysis_hints should exist");
        assert!(!hints.is_empty(), "expected extracted hints");
    }

    #[test]
    fn process_llm_response_turns_alignment_request_into_tool_call() {
        let response = LLMResponse {
            content: Some(
                "Need clarification before finalizing.\nOpen questions for alignment:".to_string(),
            ),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result =
            process_llm_response(&response, &mut renderer, 1, true, true, true, None, None)
                .expect("processing should succeed");

        match result {
            TurnProcessingResult::ToolCalls { tool_calls, .. } => {
                let name = tool_calls
                    .first()
                    .and_then(|call| call.function.as_ref())
                    .map(|f| f.name.as_str())
                    .expect("function name expected");
                assert_eq!(
                    name,
                    vtcode_core::config::constants::tools::REQUEST_USER_INPUT
                );
            }
            _ => panic!("Expected tool calls"),
        }
    }
}
