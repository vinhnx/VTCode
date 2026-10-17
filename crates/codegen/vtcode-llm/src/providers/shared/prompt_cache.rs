//! Stable prompt-prefix splitting and session cache lineage.

/// Whole-line section headers that start provider-uncached dynamic suffix
/// content. Single source of truth for the vtcode-llm wire split; keep in
/// sync with `DYNAMIC_HEADERS` in vtcode-core
/// (`core/agent/hash_utils.rs::stable_system_prefix_hash`): the core stable
/// hash and the wire split must agree or cache identity diverges from the
/// wire. Prompt-caching discipline: static content first, dynamic last.
pub(crate) const DYNAMIC_PROMPT_SECTION_HEADERS: &[&str] = &[
    "## Active Tools",
    "## Environment",
    "## Active Primary Agent Runtime State",
    "# PLANNING WORKFLOW (READ-ONLY)",
    "# FULL-AUTO: Complete task autonomously until done or blocked.",
    "# FULL-AUTO (PLANNING WORKFLOW): Work autonomously within planning workflow constraints.",
    "[Harness Limits]",
    "[Runtime Tool Catalog]",
    "[Deferred Tools]",
    "[Runtime Context]",
    "[History Directives]",
    "[Context]",
    "[Recovery Mode]",
];

/// Split prompt text at the earliest dynamic section header.
///
/// Returns `(stable, dynamic)`: `stable` is the cacheable prefix (trimmed,
/// possibly empty when the text starts with a dynamic header), `dynamic` is
/// `None` when no header matches. Whole-line match semantics mirror the
/// Anthropic wire split and the core stable hash.
pub(crate) fn split_dynamic_prompt_suffix(text: &str) -> (String, Option<String>) {
    let mut offset = 0usize;
    let mut split_at = None;
    for line in text.split_inclusive('\n') {
        let line_end = offset + line.len();
        let content_end = line_end.saturating_sub(usize::from(line.ends_with('\n')));
        if DYNAMIC_PROMPT_SECTION_HEADERS.contains(&text[offset..content_end].trim()) {
            split_at = Some(offset);
            break;
        }
        offset = line_end;
    }
    match split_at {
        None => (text.trim().to_string(), None),
        Some(at) => {
            let (stable, dynamic) = text.split_at(at);
            let stable = stable.trim().to_string();
            let dynamic = dynamic.trim_start_matches('\n').trim().to_string();
            (stable, (!dynamic.is_empty()).then_some(dynamic))
        }
    }
}

/// Stable session lineage from a VT Code `prompt_cache_key`.
///
/// Strips `vtcode:` provider namespaces and any residual `-{16 hex}` prefix-hash
/// suffix so cache-routing identity does not rotate when the system prefix or
/// tool catalog changes. Blank keys yield `None`.
pub(crate) fn session_lineage_from_prompt_cache_key(prompt_cache_key: Option<&str>) -> Option<String> {
    let key = prompt_cache_key?.trim();
    if key.is_empty() {
        return None;
    }
    let id = key
        .strip_prefix("vtcode:merge:")
        .or_else(|| key.strip_prefix("vtcode:openai:"))
        .or_else(|| key.strip_prefix("vtcode:openrouter:"))
        .or_else(|| key.strip_prefix("vtcode:xai:"))
        .unwrap_or(key)
        .trim();
    let id = strip_prefix_hash_suffix(id);
    (!id.is_empty()).then(|| id.to_string())
}

/// Drop a trailing `-{16 hex}` suffix used by legacy cache-key assembly.
fn strip_prefix_hash_suffix(id: &str) -> &str {
    if let Some((head, tail)) = id.rsplit_once('-')
        && tail.len() == 16
        && tail.chars().all(|c| c.is_ascii_hexdigit())
    {
        return head;
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_prompt_split_cuts_at_earliest_header() {
        for header in DYNAMIC_PROMPT_SECTION_HEADERS {
            let (stable, dynamic) = split_dynamic_prompt_suffix(&format!("stable instructions\n{header}\n- detail: 1"));
            assert_eq!(stable, "stable instructions", "header must split: {header}");
            assert_eq!(
                dynamic.as_deref(),
                Some(format!("{header}\n- detail: 1").as_str()),
                "header line stays with the dynamic part: {header}"
            );
        }
    }

    #[test]
    fn dynamic_prompt_split_keeps_static_prompts_whole() {
        let (stable, dynamic) = split_dynamic_prompt_suffix("stable instructions\nno headers here");
        assert_eq!(stable, "stable instructions\nno headers here");
        assert_eq!(dynamic, None);
    }

    #[test]
    fn dynamic_prompt_split_treats_body_mentions_as_stable() {
        // A header name inside a body line must not split: only whole lines count.
        let text = "stable instructions mentioning [Harness Limits] inline\nmore stable";
        let (stable, dynamic) = split_dynamic_prompt_suffix(text);
        assert_eq!(stable, text);
        assert_eq!(dynamic, None);
    }

    #[test]
    fn session_lineage_strips_namespaces_and_hex_suffix() {
        assert_eq!(
            session_lineage_from_prompt_cache_key(Some("vtcode:merge:lineage-abc")),
            Some("lineage-abc".to_string())
        );
        assert_eq!(
            session_lineage_from_prompt_cache_key(Some("vtcode:openai:lineage-abc-deadbeef01234567")),
            Some("lineage-abc".to_string())
        );
        assert_eq!(
            session_lineage_from_prompt_cache_key(Some("vtcode:openrouter:session-1")),
            Some("session-1".to_string())
        );
        assert_eq!(session_lineage_from_prompt_cache_key(Some("vtcode:xai:session-2")), Some("session-2".to_string()));
        assert_eq!(session_lineage_from_prompt_cache_key(Some("  ")), None);
        assert_eq!(session_lineage_from_prompt_cache_key(None), None);
        assert_eq!(session_lineage_from_prompt_cache_key(Some("plain-id")), Some("plain-id".to_string()));
    }
}
