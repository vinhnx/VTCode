//! Archived history-picker prompt cleaning: harness filtering and wrapper stripping.
//!
//! Extracted from `session_bootstrap.rs` so bootstrap/thread setup stays
//! separate from user-typed text recovery. No behavior change; history order
//! and asymmetric wrapper handling are preserved and covered by
//! `session_bootstrap/tests.rs`.

use crate::agent::runloop::unified::turn::is_internal_harness_follow_up;
use crate::agent::runloop::unified::webmcp::{
    TURN_HANDOFF_DIFF_OPEN, TURN_HANDOFF_DIFF_TAG_OPEN, TURN_HANDOFF_PREFIX, TURN_HANDOFF_REQUEST_HEADER,
    is_turn_handoff,
};

/// Map one archived message body to a history-picker entry.
///
/// Returns `None` for empty, harness-generated, or wrapper-only content so
/// reverse history only surfaces genuine user-typed prompts.
pub(super) fn archived_prompt_content(trimmed: &str) -> Option<String> {
    if trimmed.is_empty() {
        return None;
    }
    if is_internal_harness_follow_up(trimmed) {
        return None;
    }
    let cleaned = clean_archived_user_content(trimmed);
    let cleaned_trimmed = cleaned.trim();
    if cleaned_trimmed.is_empty() {
        return None;
    }
    if is_internal_harness_follow_up(cleaned_trimmed) {
        return None;
    }
    Some(cleaned)
}

/// Restore the user-typed text from an archived `User`-role message.
///
/// Archived messages hold the model-facing (refined/enriched) prompt, not the
/// raw composer text. This strips the known wrappers so reverse history shows
/// the typed message instead of harness phrasing:
///
/// - `User request: <original>` vibe-enrichment wrapper (keeps `<original>`,
///   see `EnrichedPrompt::to_llm_prompt` in `agent/runloop/prompt.rs`)
/// - `VT Code WebMCP handoff.` browser-handoff wrapper (keeps the embedded
///   [`TURN_HANDOFF_REQUEST_HEADER`] request section, see `build_turn_prompt`
///   in `agent/runloop/unified/webmcp.rs`)
/// - trailing `[file_reference_metadata]` / `[agent_reference_metadata]`
///   blocks appended at submit time (see `build_file_reference_metadata` /
///   `append_agent_reference_metadata` in
///   `turn/session/interaction_loop_runner/support.rs`)
/// - trailing `Note: For diagnostics, ...` refinement suffix (see
///   `finalize_refined_prompt` in `agent/runloop/prompt.rs`)
/// - trailing `Please interpret the user's request using this context.`
///   (enrichment tail in `agent/runloop/prompt.rs`)
fn clean_archived_user_content(trimmed: &str) -> String {
    let mut text = trimmed;

    // Browser-handoff wrapper: recover the embedded typed request. `is_turn_handoff`
    // proves the authoritative-diff block is present, so a user-typed line that
    // merely starts with the prefix still shows verbatim.
    if text.starts_with(TURN_HANDOFF_PREFIX) && is_turn_handoff(text) {
        if let Some(start) = text.find(TURN_HANDOFF_REQUEST_HEADER) {
            let after = &text[start + TURN_HANDOFF_REQUEST_HEADER.len()..];
            let end = after
                .find(TURN_HANDOFF_DIFF_OPEN)
                .or_else(|| after.find(TURN_HANDOFF_DIFF_TAG_OPEN))
                .unwrap_or(after.len());
            text = after[..end].trim();
        } else {
            // Machine handoff with no recoverable request: drop the wrapper
            // rather than showing it as reverse history.
            return String::new();
        }
    }

    if let Some(rest) = text.strip_prefix("User request: ") {
        // Enriched format is `User request: {original}\n\n<sections>`.
        // Keep everything up to the first blank-line section boundary.
        if let Some(end) = rest.find("\n\n") {
            let candidate = rest[..end].trim();
            text = if candidate.is_empty() { rest.trim() } else { candidate };
        } else {
            text = rest.trim();
        }
    } else if let Some(rest) = text.strip_prefix("User request:") {
        text = rest.trim();
    }

    for marker in ["\n\n[file_reference_metadata]", "\n\n[agent_reference_metadata]"] {
        if let Some(idx) = text.find(marker) {
            text = text[..idx].trim_end();
        }
    }

    if let Some(idx) = text.find("\n\nNote: For diagnostics, inspect files") {
        text = text[..idx].trim_end();
    }

    if let Some(stripped) = text.strip_suffix("Please interpret the user's request using this context.") {
        text = stripped.trim_end();
    }

    text.trim().to_string()
}
