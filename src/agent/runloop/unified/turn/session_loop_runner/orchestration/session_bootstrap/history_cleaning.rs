//! Archived history-picker prompt cleaning: harness filtering and wrapper stripping.
//!
//! Extracted from `session_bootstrap.rs` so bootstrap/thread setup stays
//! separate from user-typed text recovery. History order and asymmetric
//! wrapper handling are preserved and covered by `session_bootstrap/tests.rs`.

use crate::agent::runloop::unified::turn::is_internal_harness_follow_up;
use crate::agent::runloop::unified::webmcp::{
    TURN_HANDOFF_DIFF_OPEN, TURN_HANDOFF_DIFF_TAG_OPEN, TURN_HANDOFF_PREFIX, TURN_HANDOFF_REQUEST_HEADER,
    is_turn_handoff,
};
use crate::cli::exec::{
    EXEC_TASK_INSTRUCTIONS_DRY_RUN_PREFIX, EXEC_TASK_INSTRUCTIONS_PREFIX, REVIEW_TASK_INSTRUCTIONS_PREFIX,
};
use vtcode_commons::message_metadata::MessageMetadata;

/// Stable openings of harness-authored prompts that are stored as ordinary
/// `User`-role turns but were never typed by the user.
///
/// These cover AgentRunner recovery injections that legacy archives did not
/// tag as synthetic. Exec task scaffolding is structural (see
/// [`looks_like_exec_task_prompt`]) so a user prompt that merely starts with
/// "Task:" is never dropped.
const HARNESS_PROMPT_PREFIXES: &[&str] = &[
    EXEC_TASK_INSTRUCTIONS_PREFIX,
    EXEC_TASK_INSTRUCTIONS_DRY_RUN_PREFIX,
    REVIEW_TASK_INSTRUCTIONS_PREFIX,
    "It looks like you're repeating the same response.",
    "No tool calls or completion for ",
    "You have reached the tool-call iteration limit of ",
    "Evaluator rejected the candidate implementation in round ",
    "The following tool calls were blocked by the safety escalation gate:",
    "I've tried multiple approaches but the safety escalation gate keeps blocking my tool calls:",
    "Durable matrix results: ",
    "This is the final planning turn within the configured budget.",
    "Task tracker steps still open: ",
    "The harness task tracker could not be loaded",
    "Run a verifier and observe its terminal exit before completing changed code.",
    "Verification failed: ",
    "Unverified: ",
];

/// True for the headless exec/review/eval task frame
/// (`Task: <title>` followed by `Description: <prompt>`).
///
/// The two-line shape is the structural signature produced by
/// `core::agent::conversation::build_conversation`; matching the shape instead
/// of the bare `Task: ` prefix keeps genuine prompts that start with "Task:"
/// in reverse history.
fn looks_like_exec_task_prompt(trimmed: &str) -> bool {
    let mut lines = trimmed.lines();
    let Some(first) = lines.next() else {
        return false;
    };
    first.starts_with("Task: ") && lines.next().is_some_and(|line| line.starts_with("Description: "))
}

/// True when an archived `User`-role message is machine-authored.
///
/// Non-`user_input` provenance (`synthetic`, `llm_response`, `tool_result`,
/// `system`) is authoritative and rejected. `user_input` still runs the stable
/// opening checks: legacy archives tagged some harness recovery notices as
/// `user_input`, so the prefix list remains the safety net for them. The
/// structural exec-task frame is skipped only for trusted `user_input`
/// metadata, because legacy task frames carry no metadata at all.
fn is_harness_authored_prompt(trimmed: &str, metadata: Option<&MessageMetadata>) -> bool {
    let source = metadata.and_then(MessageMetadata::source);
    if source.is_some_and(|source| source != "user_input") {
        return true;
    }
    let trusted_user_input = source == Some("user_input");
    if !trusted_user_input && looks_like_exec_task_prompt(trimmed) {
        return true;
    }
    HARNESS_PROMPT_PREFIXES.iter().any(|prefix| trimmed.starts_with(prefix))
}

/// Map one archived message body to a history-picker entry.
///
/// Returns `None` for empty, harness-generated, or wrapper-only content so
/// reverse history only surfaces genuine user-typed prompts.
#[cfg(test)]
pub(super) fn archived_prompt_content(trimmed: &str) -> Option<String> {
    archived_prompt_content_with_metadata(trimmed, None)
}

/// `archived_prompt_content` with archived provenance metadata.
pub(super) fn archived_prompt_content_with_metadata(
    trimmed: &str,
    metadata: Option<&MessageMetadata>,
) -> Option<String> {
    if trimmed.is_empty() {
        return None;
    }
    if is_internal_harness_follow_up(trimmed) || is_harness_authored_prompt(trimmed, metadata) {
        return None;
    }
    let cleaned = clean_archived_user_content(trimmed);
    let cleaned_trimmed = cleaned.trim();
    if cleaned_trimmed.is_empty() {
        return None;
    }
    if is_internal_harness_follow_up(cleaned_trimmed) || is_harness_authored_prompt(cleaned_trimmed, metadata) {
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
