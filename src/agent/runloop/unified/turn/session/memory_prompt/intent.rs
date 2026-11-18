use super::*;

pub(super) fn detect_memory_prompt_intent(input: &str) -> Option<MemoryPromptIntent> {
    let normalized = normalize_prompt_clause(input)?;
    if normalized.ends_with('?') {
        if normalized.starts_with("what do you remember")
            || normalized.starts_with("show memory")
            || normalized.starts_with("open memory")
            || normalized.starts_with("list memory")
        {
            return Some(MemoryPromptIntent::Show);
        }
        return None;
    }

    let lowered = normalized.to_ascii_lowercase();
    if matches_show_memory(&lowered) {
        return Some(MemoryPromptIntent::Show);
    }
    if extract_memory_clause(&lowered, &normalized, remember_markers()).is_some() {
        return Some(MemoryPromptIntent::Remember { request: normalized.clone() });
    }
    extract_memory_clause(&lowered, &normalized, forget_markers())
        .map(|_| MemoryPromptIntent::Forget { request: normalized })
}

fn matches_show_memory(lowered: &str) -> bool {
    [
        "show memory",
        "open memory",
        "list memory",
        "browse memory",
        "what do you remember",
        "what's in memory",
    ]
    .iter()
    .any(|pattern| lowered.starts_with(pattern))
}

pub(super) fn remember_markers() -> &'static [&'static str] {
    &[
        "save to memory,",
        "save to memory:",
        "save to memory ",
        "save this to memory ",
        "store this in memory ",
        "store in memory ",
        "remember that ",
        "remember ",
        "add to memory ",
        "note this for memory ",
    ]
}

fn forget_markers() -> &'static [&'static str] {
    &[
        "forget that ",
        "forget about ",
        "forget ",
        "remove from memory ",
        "delete from memory ",
        "drop from memory ",
        "don't remember ",
        "stop remembering ",
    ]
}

pub(super) fn extract_memory_clause(lowered: &str, original: &str, markers: &[&str]) -> Option<String> {
    markers.iter().find_map(|marker| {
        let index = lowered.find(marker)?;
        let start = index + marker.len();
        let suffix = original.get(start..)?.trim();
        normalize_prompt_clause(strip_leading_memory_markers(suffix))
    })
}

fn strip_leading_memory_markers(input: &str) -> &str {
    let trimmed = input.trim_start_matches([',', ':', '-', ' ']).trim();
    let lowered = trimmed.to_ascii_lowercase();
    for marker in remember_markers().iter().chain(forget_markers()) {
        if lowered.starts_with(marker) {
            return trimmed[marker.len()..].trim();
        }
    }
    trimmed
}

pub(super) fn normalize_prompt_clause(input: &str) -> Option<String> {
    let mut text = input.trim();
    while let Some(stripped) = [
        "please ",
        "can you ",
        "could you ",
        "would you ",
        "vt code, ",
        "vt code ",
    ]
    .iter()
    .find_map(|prefix| text.to_ascii_lowercase().starts_with(prefix).then(|| &text[prefix.len()..]))
    {
        text = stripped.trim_start();
    }

    let normalized = vtcode_commons::formatting::collapse_whitespace(text);
    (!normalized.is_empty()).then_some(normalized)
}
