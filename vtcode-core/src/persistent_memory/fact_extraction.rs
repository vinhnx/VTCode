use super::*;

pub fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().fold(String::new(), |mut acc, s| {
        if !acc.is_empty() {
            acc.push(' ');
        }
        acc.push_str(s);
        acc
    })
}

pub fn truncate_for_fact(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    format!(
        "{}...",
        trimmed
            .chars()
            .take(max_chars.saturating_sub(3))
            .collect::<String>()
    )
}

pub fn maybe_extract_tool_fact(message: &Message) -> Option<GroundedFactRecord> {
    if message.role != MessageRole::Tool {
        return None;
    }
    let tool_name = message.origin_tool.as_deref().unwrap_or("tool");
    let text = message.content.as_text();
    let raw = text.trim();
    if raw.is_empty() {
        return None;
    }

    let candidate = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| {
            if value.get("error").is_some()
                || value.get("success") == Some(&serde_json::Value::Bool(false))
            {
                return None;
            }
            for key in ["summary", "message", "result", "output", "stdout"] {
                if let Some(v) = value.get(key) {
                    if let Some(text) = v.as_str() {
                        let normalized = normalize_whitespace(text);
                        if !normalized.is_empty() {
                            return Some(normalized);
                        }
                    } else if !v.is_null() {
                        let normalized = normalize_whitespace(&v.to_string());
                        if !normalized.is_empty() {
                            return Some(normalized);
                        }
                    }
                }
            }
            let compact = normalize_whitespace(&value.to_string());
            (!compact.is_empty()).then_some(compact)
        })
        .or_else(|| {
            let lowered = raw.to_ascii_lowercase();
            if lowered.contains("error")
                || lowered.contains("failed")
                || lowered.contains("denied")
                || lowered.contains("timeout")
            {
                return None;
            }
            Some(normalize_whitespace(raw))
        })?;

    Some(GroundedFactRecord {
        fact: truncate_for_fact(&candidate, 180),
        source: format!("tool:{tool_name}"),
    })
}

pub fn maybe_extract_user_fact(message: &Message) -> Option<GroundedFactRecord> {
    if message.role != MessageRole::User {
        return None;
    }
    let text = normalize_whitespace(message.content.as_text().as_ref());
    if text.is_empty() {
        return None;
    }
    let candidate_text = strip_user_memory_candidate_prefixes(&text);
    let (candidate_text, looks_authored_note) = strip_user_memory_note_marker(candidate_text)
        .map(|fact| (fact, true))
        .unwrap_or((candidate_text, false));
    let looks_durable_self_fact = SELF_FACT_PREFIXES
        .iter()
        .any(|p| candidate_text.to_ascii_lowercase().starts_with(*p));
    (looks_authored_note || looks_durable_self_fact).then(|| GroundedFactRecord {
        fact: truncate_for_fact(candidate_text, 180),
        source: "user_assertion".to_string(),
    })
}

fn strip_user_memory_candidate_prefixes(text: &str) -> &str {
    let mut trimmed = text.trim();
    loop {
        let lowered = trimmed.to_ascii_lowercase();
        let Some(prefix) = STRIP_PREFIXES.iter().find(|p| lowered.starts_with(**p)) else {
            return trimmed;
        };
        trimmed = trimmed
            .get(prefix.len()..)
            .unwrap_or("")
            .trim_start_matches([',', ':', '-', ' '])
            .trim_start();
    }
}

fn strip_user_memory_note_marker(text: &str) -> Option<&str> {
    let lowered = text.to_ascii_lowercase();
    CLEANUP_NOTE_PREFIXES.iter().find_map(|prefix| {
        lowered.starts_with(prefix).then(|| {
            text.get(prefix.len()..)
                .unwrap_or("")
                .trim_start_matches([',', ':', '-', ' '])
                .trim_start()
        })
    })
}

pub fn dedup_latest_facts(history: &[Message], limit: usize) -> Vec<GroundedFactRecord> {
    let mut facts = Vec::new();
    for message in history {
        if let Some(fact) =
            maybe_extract_tool_fact(message).or_else(|| maybe_extract_user_fact(message))
        {
            let normalized = normalize_whitespace(&fact.fact).to_ascii_lowercase();
            if let Some(existing_idx) = facts.iter().position(|entry: &GroundedFactRecord| {
                normalize_whitespace(&entry.fact).to_ascii_lowercase() == normalized
            }) {
                facts.remove(existing_idx);
            }
            facts.push(fact);
        }
    }

    let keep_from = facts.len().saturating_sub(limit);
    facts.into_iter().skip(keep_from).collect()
}
