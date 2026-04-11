use vtcode_config::SubagentSpec;

use super::constants::VAGUE_SUBAGENT_PROMPTS;
use super::types::SubagentInputItem;

// ─── Delegation Detection ───────────────────────────────────────────────────

#[must_use]
pub fn delegated_task_requires_clarification(prompt: &str) -> bool {
    let normalized = prompt
        .trim()
        .trim_matches(|ch: char| matches!(ch, '"' | '\'' | '.' | ',' | '!' | '?' | ':' | ';'))
        .to_ascii_lowercase();
    if normalized.is_empty() {
        return true;
    }
    if VAGUE_SUBAGENT_PROMPTS
        .iter()
        .any(|candidate| normalized == *candidate)
    {
        return true;
    }
    normalized.split_whitespace().count() == 1
}

// ─── Agent Mention Extraction ──────────────────────────────────────────────

pub fn extract_explicit_agent_mentions(input: &str, specs: &[SubagentSpec]) -> Vec<String> {
    let mut mentions = Vec::new();
    for direct in extract_direct_agent_mentions(input) {
        let canonical = specs
            .iter()
            .find(|spec| spec.matches_name(direct.as_str()))
            .map(|spec| spec.name.clone())
            .unwrap_or(direct);
        push_unique_agent_mention(&mut mentions, &canonical);
    }

    let lower = input.to_ascii_lowercase();
    for spec in specs {
        if !matches_explicit_named_agent_selection(lower.as_str(), spec) {
            continue;
        }
        push_unique_agent_mention(&mut mentions, &spec.name);
    }

    mentions
}

fn extract_direct_agent_mentions(input: &str) -> Vec<String> {
    input
        .split_whitespace()
        .filter_map(|token| {
            let trimmed = token.trim_matches(|ch: char| {
                matches!(
                    ch,
                    '"' | '\'' | ',' | '.' | ':' | ';' | '!' | '?' | ')' | '('
                )
            });
            trimmed
                .strip_prefix("@agent-")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .collect()
}

fn push_unique_agent_mention(mentions: &mut Vec<String>, candidate: &str) {
    if mentions
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(candidate))
    {
        return;
    }
    mentions.push(candidate.to_string());
}

fn matches_explicit_named_agent_selection(input: &str, spec: &SubagentSpec) -> bool {
    std::iter::once(spec.name.as_str())
        .chain(spec.aliases.iter().map(String::as_str))
        .any(|candidate| contains_explicit_named_agent_selection(input, candidate))
}

fn contains_explicit_named_agent_selection(input: &str, candidate: &str) -> bool {
    let candidate = candidate.trim().to_ascii_lowercase();
    if candidate.is_empty() {
        return false;
    }

    let direct_match = [
        format!("use {candidate} agent"),
        format!("use the {candidate} agent"),
        format!("use {candidate} subagent"),
        format!("use the {candidate} subagent"),
        format!("run {candidate} agent"),
        format!("run the {candidate} agent"),
        format!("run {candidate} subagent"),
        format!("run the {candidate} subagent"),
        format!("delegate to {candidate}"),
        format!("delegate this to {candidate}"),
        format!("delegate the task to {candidate}"),
        format!("spawn {candidate}"),
        format!("spawn the {candidate}"),
        format!("ask {candidate} to"),
    ]
    .iter()
    .any(|pattern| input.contains(pattern.as_str()));
    if direct_match {
        return true;
    }

    [
        format!("use {candidate} and"),
        format!("use the {candidate} and"),
    ]
    .iter()
    .any(|pattern| input.contains(pattern.as_str()))
        && (input.contains(" agent") || input.contains(" subagent"))
}

pub fn contains_explicit_delegation_request(input: &str, explicit_mentions: &[String]) -> bool {
    let lower = input.to_ascii_lowercase();
    !explicit_mentions.is_empty()
        || lower.contains(" run in parallel")
        || lower.contains(" spawn ")
        || lower.starts_with("spawn ")
        || lower.contains(" delegate ")
        || lower.starts_with("delegate ")
        || lower.contains(" background subagent")
        || lower.contains(" background agent")
        || (lower.contains(" use the ")
            && (lower.contains(" agent") || lower.contains(" subagent")))
        || (lower.starts_with("use ") && (lower.contains(" agent") || lower.contains(" subagent")))
}

pub fn contains_explicit_model_request(input: &str, requested_model: &str) -> bool {
    let requested = requested_model.trim();
    if requested.is_empty() {
        return false;
    }

    let lower_input = input.to_ascii_lowercase();
    let lower_requested = requested.to_ascii_lowercase();

    match lower_requested.as_str() {
        "small" => {
            lower_input.contains("small model")
                || lower_input.contains("smaller model")
                || lower_input.contains("lightweight model")
                || lower_input.contains("cheap model")
        }
        "haiku" | "sonnet" | "opus" | "inherit" => {
            contains_bounded_term(&lower_input, &lower_requested)
                || lower_input.contains(&format!("use {lower_requested}"))
                || lower_input.contains(&format!("using {lower_requested}"))
                || lower_input.contains(&format!("with {lower_requested}"))
                || lower_input.contains(&format!("run on {lower_requested}"))
                || lower_input.contains(&format!("{lower_requested} model"))
                || lower_input.contains(&format!("model {lower_requested}"))
        }
        _ => contains_bounded_term(&lower_input, &lower_requested),
    }
}

pub fn normalize_requested_model_override(
    raw: Option<String>,
    current_input: &str,
) -> Option<String> {
    let requested = raw?.trim().to_string();
    if requested.is_empty() || requested.eq_ignore_ascii_case("default") {
        return None;
    }
    if requested.eq_ignore_ascii_case("inherit")
        && !contains_explicit_model_request(current_input, requested.as_str())
    {
        return None;
    }
    Some(requested)
}

fn contains_bounded_term(input: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }

    input.match_indices(needle).any(|(start, matched)| {
        let end = start + matched.len();
        let leading_ok = start == 0
            || !input[..start]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_alphanumeric());
        let trailing_ok = end == input.len()
            || !input[end..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphanumeric());
        leading_ok && trailing_ok
    })
}

// ─── Input Sanitization ─────────────────────────────────────────────────────

pub fn sanitize_subagent_input_items(items: &mut Vec<SubagentInputItem>) {
    let mut sanitized = Vec::with_capacity(items.len());
    for mut item in items.drain(..) {
        item.item_type = trim_optional_field(item.item_type.take());
        item.text = trim_optional_field(item.text.take());
        item.path = trim_optional_field(item.path.take());
        item.name = trim_optional_field(item.name.take());
        item.image_url = trim_optional_field(item.image_url.take());
        if item.text.is_none()
            && item.path.is_none()
            && item.name.is_none()
            && item.image_url.is_none()
        {
            continue;
        }
        sanitized.push(item);
    }
    *items = sanitized;
}

fn trim_optional_field(value: Option<String>) -> Option<String> {
    let trimmed = value?.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

// ─── Request Prompt Extraction ──────────────────────────────────────────────

pub fn request_prompt(message: &Option<String>, items: &[SubagentInputItem]) -> Option<String> {
    if let Some(message) = message
        && !message.trim().is_empty()
    {
        return Some(message.trim().to_string());
    }

    let segments = items
        .iter()
        .filter_map(item_prompt_segment)
        .collect::<Vec<_>>();
    if segments.is_empty() {
        None
    } else {
        Some(segments.join("\n"))
    }
}

fn item_prompt_segment(item: &SubagentInputItem) -> Option<String> {
    if let Some(text) = item.text.as_ref()
        && !text.trim().is_empty()
    {
        return Some(text.trim().to_string());
    }
    if let Some(path) = item.path.as_ref()
        && !path.trim().is_empty()
    {
        return Some(format!("Reference: {}", path.trim()));
    }
    if let Some(name) = item.name.as_ref()
        && !name.trim().is_empty()
    {
        return Some(name.trim().to_string());
    }
    if let Some(image_url) = item.image_url.as_ref()
        && !image_url.trim().is_empty()
    {
        return Some(format!("Image: {}", image_url.trim()));
    }
    None
}
