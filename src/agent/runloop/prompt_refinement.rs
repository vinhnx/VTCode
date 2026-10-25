use hashbrown::HashSet;
use std::sync::Arc;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::llm::provider::{FinishReason, LLMProvider, LLMRequest, LLMResponse, Message, ToolChoice};

use super::{
    MAX_REFINED_WORD_MULTIPLIER, MIN_KEYWORD_LENGTH, MIN_KEYWORD_OVERLAP_RATIO, MIN_PROMPT_LENGTH_FOR_REFINEMENT,
    MIN_PROMPT_WORDS_FOR_REFINEMENT, SHORT_PROMPT_WORD_THRESHOLD,
};

const REFINEMENT_SYSTEM_PROMPT: &str = "Rewrite the supplied request for clarity. Do not answer or execute it. Preserve its intent, language, negations, authorization limits, paths, identifiers, and requested deliverable. Add no requirements or facts. Leave unresolved references unresolved when context is unavailable. If the request is already clear, return it unchanged. Return only the rewritten request, without a preamble or Markdown fences.\n\nExample input: Review the parser; do not change files.\nExample output: Review the parser and report your findings without changing files.";

pub(super) const REFINEMENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub(super) fn refinement_request(
    provider: &dyn LLMProvider,
    raw: &str,
    model: &str,
    supported_efforts: &[&str],
    temperature: f32,
) -> LLMRequest {
    // This auxiliary rewrite does not need the primary task's reasoning budget.
    let reasoning_effort = supported_efforts.contains(&"low").then_some(ReasoningEffortLevel::Low);
    let output_budget = vtcode_commons::estimate_tokens(raw)
        .saturating_mul(2)
        .saturating_add(2048)
        .clamp(2048, 8192);
    LLMRequest {
        messages: Arc::new(vec![Message::user(raw.to_string())]),
        system_prompt: Some(Arc::from(REFINEMENT_SYSTEM_PROMPT)),
        model: model.to_string(),
        max_tokens: Some(output_budget as u32),
        // Anthropic Messages (API 2026-10-21) rejects sampling; a model that
        // advertises no effort level would otherwise keep this value.
        temperature: (supported_efforts.is_empty()
            && !provider.sampling_overrides(model).drops_sampling_params(false, false))
        .then_some(temperature),
        tool_choice: Some(ToolChoice::none()),
        reasoning_effort,
        ..Default::default()
    }
}

pub(super) fn accepted_refinement(raw: &str, response: LLMResponse) -> Option<String> {
    if response.finish_reason != FinishReason::Stop
        || response.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty())
    {
        return None;
    }
    let text = response.content?;
    should_accept_refinement(raw, &text).then_some(text)
}

pub(super) fn should_attempt_refinement(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }

    let char_len = trimmed.chars().count();
    let word_count = trimmed.split_whitespace().count();

    char_len >= MIN_PROMPT_LENGTH_FOR_REFINEMENT && word_count >= MIN_PROMPT_WORDS_FOR_REFINEMENT
}

pub(super) fn should_accept_refinement(raw: &str, refined: &str) -> bool {
    let trimmed = refined.trim();
    if trimmed.is_empty() {
        return false;
    }

    if trimmed.eq_ignore_ascii_case(raw.trim()) {
        return true;
    }

    // Avoid allocating Vecs when we only need word counts for early checks.
    let raw_word_count = raw.split_whitespace().count();
    if raw_word_count < MIN_PROMPT_WORDS_FOR_REFINEMENT {
        return false;
    }

    let refined_word_count = trimmed.split_whitespace().count();
    if raw_word_count <= SHORT_PROMPT_WORD_THRESHOLD
        && refined_word_count > raw_word_count * MAX_REFINED_WORD_MULTIPLIER
    {
        return false;
    }

    let refined_lower = trimmed.to_lowercase();
    let suspicious_prefixes = ["hello", "hi", "hey", "greetings", "i'm", "i am"];
    if suspicious_prefixes.iter().any(|prefix| refined_lower.starts_with(prefix)) {
        return false;
    }
    let suspicious_phrases = ["how can i help you", "i'm here to", "let me know if"];
    if suspicious_phrases.iter().any(|phrase| refined_lower.contains(phrase)) {
        return false;
    }

    let raw_keywords = keyword_set(raw);
    if raw_keywords.is_empty() {
        return true;
    }
    let refined_keywords = keyword_set(trimmed);
    let overlap = raw_keywords.intersection(&refined_keywords).count() as f32;
    let ratio = overlap / raw_keywords.len() as f32;
    ratio >= MIN_KEYWORD_OVERLAP_RATIO
}

fn keyword_set(text: &str) -> HashSet<String> {
    text.split_whitespace()
        .map(|token| token.trim_matches(|ch: char| !ch.is_alphanumeric()))
        .filter(|token| token.len() >= MIN_KEYWORD_LENGTH)
        .map(|token| token.to_ascii_lowercase())
        .collect()
}
