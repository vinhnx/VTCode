use std::collections::HashSet;

use super::{
    MAX_REFINED_WORD_MULTIPLIER, MIN_KEYWORD_LENGTH, MIN_KEYWORD_OVERLAP_RATIO,
    MIN_PROMPT_LENGTH_FOR_REFINEMENT, MIN_PROMPT_WORDS_FOR_REFINEMENT, SHORT_PROMPT_WORD_THRESHOLD,
};

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
    if suspicious_prefixes
        .iter()
        .any(|prefix| refined_lower.starts_with(prefix))
    {
        return false;
    }
    let suspicious_phrases = ["how can i help you", "i'm here to", "let me know if"];
    if suspicious_phrases
        .iter()
        .any(|phrase| refined_lower.contains(phrase))
    {
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
